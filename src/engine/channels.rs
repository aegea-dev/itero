//! Where `#notify` sends on a machine whose `daemon.json` lists `notify`
//! channels: each channel's body, and sending to them.
use crate::engine::worker::Note;
use crate::settings::Format;
use serde_json::{Map, Value, json};

/// Where to POST `n` for a channel at `url` taking `format`, and what:
/// `workspace` (its name on the machine's list) and `workflow` say where
/// it came from. A url is never in an error, only its host.
pub fn body(
    format: Format,
    url: &str,
    n: &Note,
    workspace: Option<&str>,
    workflow: &str,
) -> Result<(String, Value), String> {
    // the subtitle, then the text: one message for those with no subtitle
    let message = match &n.subtitle {
        Some(sub) => format!("{sub}\n{}", n.text),
        None => n.text.clone(),
    };
    let mut b = Map::new();
    let mut put = |k: &str, v: Option<&str>| {
        if let Some(v) = v {
            b.insert(k.into(), Value::String(v.into()));
        }
    };
    match format {
        Format::Json => {
            put("title", Some(&n.title));
            put("subtitle", n.subtitle.as_deref());
            put("text", Some(&n.text));
            put("url", n.url.as_deref());
            put("workspace", workspace);
            put("workflow", Some(workflow));
            Ok((url.into(), Value::Object(b)))
        }
        Format::Ntfy => {
            let (to, topic) = ntfy_topic(url)?;
            put("topic", Some(&topic));
            put("title", Some(&n.title));
            put("message", Some(&message));
            if let Some(u) = &n.url {
                b.insert(
                    "actions".into(),
                    json!([{ "action": "view", "label": "Open", "url": u }]),
                );
            }
            Ok((to, Value::Object(b)))
        }
        Format::Slack => {
            let shown = format!("*{}*\n{message}", n.title);
            let mut blocks =
                vec![json!({ "type": "section", "text": { "type": "mrkdwn", "text": shown } })];
            let mut text = shown.clone();
            if let Some(u) = &n.url {
                text.push_str(&format!("\n{u}"));
                blocks.push(json!({ "type": "actions", "elements": [
                    { "type": "button", "text": { "type": "plain_text", "text": "Open" }, "url": u }
                ] }));
            }
            Ok((url.into(), json!({ "text": text, "blocks": blocks })))
        }
        Format::Discord => {
            put("title", Some(&n.title));
            put("description", Some(&message));
            put("url", n.url.as_deref());
            Ok((url.into(), json!({ "embeds": [Value::Object(b)] })))
        }
    }
}

/// The host of `url`, to name a channel by: never its path or query,
/// where a webhook's secret is.
pub fn host(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| "an address that is not a URL".into())
}

/// An ntfy topic's url split: where to publish (the url without its last
/// segment) and the topic (that segment).
fn ntfy_topic(url: &str) -> Result<(String, String), String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| format!("{}: not a URL", host(url)))?;
    let none = || {
        format!(
            "an ntfy channel's url ends in its topic (https://{}/<topic>)",
            host(url)
        )
    };
    if parsed.path().trim_matches('/').is_empty() {
        return Err(none());
    }
    let trimmed = url.trim_end_matches('/');
    let (to, topic) = trimmed.rsplit_once('/').ok_or_else(none)?;
    Ok((format!("{to}/"), topic.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note() -> Note {
        Note {
            title: "Sipariş".into(),
            subtitle: Some("depo 2".into()),
            text: "43 kargoda".into(),
            url: Some("https://örnek.test/s/43".into()),
            ..Default::default()
        }
    }

    fn bare() -> Note {
        Note {
            title: "itero".into(),
            text: "yalnız metin".into(),
            ..Default::default()
        }
    }

    #[test]
    fn json_has_the_four_and_where_it_came_from() {
        let (to, b) = body(
            Format::Json,
            "https://ops.örnek.test/kanca",
            &note(),
            Some("sakin-nehir"),
            "sipariş",
        )
        .unwrap();
        assert_eq!(to, "https://ops.örnek.test/kanca");
        assert_eq!(
            b,
            json!({"title": "Sipariş", "subtitle": "depo 2", "text": "43 kargoda",
                   "url": "https://örnek.test/s/43", "workspace": "sakin-nehir",
                   "workflow": "sipariş"})
        );
        let (_, b) = body(Format::Json, "https://a.test/", &bare(), None, "w").unwrap();
        assert_eq!(
            b,
            json!({"title": "itero", "text": "yalnız metin", "workflow": "w"})
        );
    }

    #[test]
    fn ntfy_publishes_json_to_the_topics_parent() {
        let (to, b) = body(Format::Ntfy, "https://ntfy.sh/uyarılar", &note(), None, "w").unwrap();
        assert_eq!(to, "https://ntfy.sh/");
        assert_eq!(
            b,
            json!({"topic": "uyarılar", "title": "Sipariş", "message": "depo 2\n43 kargoda",
                   "actions": [{"action": "view", "label": "Open",
                                "url": "https://örnek.test/s/43"}]})
        );
        // a server of its own, under a path
        let (to, _) = body(
            Format::Ntfy,
            "https://örnek.test/ntfy/iş/",
            &bare(),
            None,
            "w",
        )
        .unwrap();
        assert_eq!(to, "https://örnek.test/ntfy/");
        let (_, b) = body(Format::Ntfy, "https://ntfy.sh/a", &bare(), None, "w").unwrap();
        assert_eq!(
            b,
            json!({"topic": "a", "title": "itero", "message": "yalnız metin"})
        );
        let e = body(Format::Ntfy, "https://ntfy.sh/", &note(), None, "w").unwrap_err();
        assert!(e.contains("topic") && e.contains("ntfy.sh"), "{e}");
    }

    #[test]
    fn slack_has_its_text_for_others_and_a_button() {
        let (to, b) = body(
            Format::Slack,
            "https://hooks.slack.com/x",
            &note(),
            None,
            "w",
        )
        .unwrap();
        assert_eq!(to, "https://hooks.slack.com/x");
        assert_eq!(
            b,
            json!({"text": "*Sipariş*\ndepo 2\n43 kargoda\nhttps://örnek.test/s/43",
                   "blocks": [
                     {"type": "section",
                      "text": {"type": "mrkdwn", "text": "*Sipariş*\ndepo 2\n43 kargoda"}},
                     {"type": "actions", "elements": [
                       {"type": "button", "text": {"type": "plain_text", "text": "Open"},
                        "url": "https://örnek.test/s/43"}]}]})
        );
        let (_, b) = body(
            Format::Slack,
            "https://hooks.slack.com/x",
            &bare(),
            None,
            "w",
        )
        .unwrap();
        assert_eq!(b["text"], "*itero*\nyalnız metin");
        assert_eq!(
            b["blocks"].as_array().unwrap().len(),
            1,
            "no url, no button"
        );
    }

    #[test]
    fn discord_links_its_title() {
        let (_, b) = body(
            Format::Discord,
            "https://discord.com/api/webhooks/x",
            &note(),
            None,
            "w",
        )
        .unwrap();
        assert_eq!(
            b,
            json!({"embeds": [{"title": "Sipariş", "description": "depo 2\n43 kargoda",
                               "url": "https://örnek.test/s/43"}]})
        );
        let (_, b) = body(Format::Discord, "https://discord.com/x", &bare(), None, "w").unwrap();
        assert_eq!(
            b,
            json!({"embeds": [{"title": "itero", "description": "yalnız metin"}]})
        );
    }
}
