//! What a page loads while a step runs, from Chrome's Network events: the
//! JSON its scripts fetch (`responses`).
use super::cdp::Cdp;
use serde_json::{Value, json};
use std::time::Instant;

/// Turns on Chrome's Network events for the tab, keeping response bodies
/// until they are read.
pub fn start(cdp: &mut Cdp) -> Result<(), String> {
    cdp.send(
        "Network.enable",
        json!({ "maxTotalBufferSize": 64 << 20, "maxResourceBufferSize": 16 << 20 }),
    )
    .map(|_| ())
}

/// Whether `url`, as it came or percent-decoded, contains `text`.
pub fn matches(url: &str, text: &str) -> bool {
    url.contains(text) || decode(url).contains(text)
}

/// `%C3%BC` as `ü`.
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether a response's MIME type is JSON's.
pub fn is_json(mime: &str) -> bool {
    let m = mime.to_ascii_lowercase();
    m == "application/json" || m == "text/json" || m.ends_with("+json")
}

/// The JSON responses whose URL contains `text`, of the Network events
/// kept since `start`, in the order they were asked for: `url`, `status`,
/// and `json` (or `error` when the body is not JSON after all). A request
/// the page has sent and not had its answer to yet is waited for until
/// `until`, its answer then kept if it is JSON.
pub fn responses(cdp: &mut Cdp, text: &str, until: Instant) -> Result<Vec<Value>, String> {
    let mut events = cdp.events("Network.");
    let of = |events: &[Value], method: &str, id: &str| {
        events
            .iter()
            .find(|e| e["method"] == method && e["params"]["requestId"] == id)
            .cloned()
    };
    let ended = |e: &Value, id: &str| {
        matches!(
            e["method"].as_str(),
            Some("Network.loadingFinished" | "Network.loadingFailed")
        ) && e["params"]["requestId"] == id
    };
    // every request the text matches, once each, in the order sent
    let mut asked: Vec<String> = vec![];
    for e in events
        .iter()
        .filter(|e| e["method"] == "Network.requestWillBeSent")
    {
        let url = e["params"]["request"]["url"].as_str().unwrap_or_default();
        let id = e["params"]["requestId"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if matches(url, text) && !asked.contains(&id) {
            asked.push(id);
        }
    }
    let mut out = vec![];
    for id in asked {
        if !events.iter().any(|e| ended(e, &id)) {
            // still on its way: its answer and its end, kept as they come
            let wait = cdp.wait_event(until, &mut |e| ended(e, &id));
            events.extend(cdp.events("Network."));
            if let Ok(e) = wait {
                events.push(e);
            }
        }
        let Some(got) = of(&events, "Network.responseReceived", &id) else {
            continue;
        };
        let r = &got["params"]["response"];
        if !is_json(r["mimeType"].as_str().unwrap_or_default()) {
            continue;
        }
        let mut item = json!({ "url": r["url"], "status": r["status"] });
        match json_body(cdp, &id) {
            Ok(v) => item["json"] = v,
            Err(e) => item["error"] = json!(e),
        }
        out.push(item);
    }
    Ok(out)
}

/// The body of response `id` as JSON, while Chrome still has it.
pub fn json_body(cdp: &mut Cdp, id: &str) -> Result<Value, String> {
    let b = cdp.send("Network.getResponseBody", json!({ "requestId": id }))?;
    let body = b["body"].as_str().unwrap_or_default();
    let text = if b["base64Encoded"] == true {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(body)
            .map(|v| String::from_utf8_lossy(&v).into_owned())
            .map_err(|e| e.to_string())?
    } else {
        body.to_string()
    };
    serde_json::from_str(&text).map_err(|e| format!("not JSON: {e}"))
}
