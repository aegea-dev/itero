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
fn matches(url: &str, text: &str) -> bool {
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

fn is_json(mime: &str) -> bool {
    let m = mime.to_ascii_lowercase();
    m == "application/json" || m == "text/json" || m.ends_with("+json")
}

/// The JSON responses whose URL contains `text`, of the Network events
/// kept since `start`, in the order they came: `url`, `status`, and
/// `json` (or `error` when the body is not JSON after all). One not
/// finished yet is waited for until `until`.
pub fn responses(cdp: &mut Cdp, text: &str, until: Instant) -> Result<Vec<Value>, String> {
    let events = cdp.events("Network.");
    let wanted: Vec<(String, String, Value)> = events
        .iter()
        .filter(|e| e["method"] == "Network.responseReceived")
        .filter_map(|e| {
            let r = &e["params"]["response"];
            let url = r["url"].as_str()?;
            (matches(url, text) && is_json(r["mimeType"].as_str().unwrap_or_default())).then(|| {
                (
                    e["params"]["requestId"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    url.to_string(),
                    r["status"].clone(),
                )
            })
        })
        .collect();
    let finished = |id: &str| {
        events.iter().any(|e| {
            matches!(
                e["method"].as_str(),
                Some("Network.loadingFinished" | "Network.loadingFailed")
            ) && e["params"]["requestId"] == id
        })
    };
    let mut out = vec![];
    for (id, url, status) in wanted {
        if !finished(&id) {
            let _ = cdp.wait_event(until, &mut |e| {
                matches!(
                    e["method"].as_str(),
                    Some("Network.loadingFinished" | "Network.loadingFailed")
                ) && e["params"]["requestId"] == id.as_str()
            });
        }
        let mut item = json!({ "url": url, "status": status });
        match cdp.send("Network.getResponseBody", json!({ "requestId": id })) {
            Ok(b) => {
                let body = b["body"].as_str().unwrap_or_default();
                let text = if b["base64Encoded"] == true {
                    use base64::Engine;
                    base64::engine::general_purpose::STANDARD
                        .decode(body)
                        .map(|v| String::from_utf8_lossy(&v).into_owned())
                        .map_err(|e| e.to_string())
                } else {
                    Ok(body.to_string())
                };
                match text
                    .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
                {
                    Ok(v) => item["json"] = v,
                    Err(e) => item["error"] = json!(format!("not JSON: {e}")),
                }
            }
            Err(e) => item["error"] = json!(e),
        }
        out.push(item);
    }
    Ok(out)
}
