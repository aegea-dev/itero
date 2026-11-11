//! `inspect`: a page's facts, as it loads: its document and every file,
//! cookies, meta tags, known libraries, TLS, timing, what it logged, and
//! the site's `robots.txt`, `sitemap.xml` and `llms.txt`.
use super::cdp::Cdp;
use super::{network, page};
use serde_json::{Map, Value, json};
use std::time::{Duration, Instant};

/// Opens `url` recording what it loads and logs; the page is then left
/// for the step's blockers, as after `open`.
pub fn load(cdp: &mut Cdp, url: &str, until: Instant) -> Result<(), String> {
    network::start(cdp)?;
    cdp.send("Runtime.enable", json!({}))?;
    cdp.send("Log.enable", json!({}))?;
    page::load(cdp, url, until)
}

/// The facts of the page `load` opened. Cookie values only with `values`.
pub fn facts(cdp: &mut Cdp, values: bool) -> Result<Value, String> {
    let net = cdp.kept("Network.");
    let (url, _) = page::about(cdp)?;
    let document = document(&net, &url);
    let files = files(&net);
    let urls: Vec<&str> = files.iter().filter_map(|f| f["url"].as_str()).collect();
    let cookies = cookies(cdp, &url, &urls, values)?;
    let in_page = cdp.eval(PAGE_FACTS)?;
    let tls = net
        .iter()
        .find(|e| {
            e["method"] == "Network.responseReceived"
                && e["params"]["type"] == "Document"
                && e["params"]["response"]["url"] == url.as_str()
        })
        .map(|e| tls(&e["params"]["response"]["securityDetails"]))
        .unwrap_or(Value::Null);
    let console = console(&cdp.events("Runtime."), &cdp.events("Log."));
    Ok(json!({
        "document": document,
        "files": files,
        "cookies": cookies,
        "meta": in_page["meta"],
        "libraries": libraries(&in_page, &urls),
        "tls": tls,
        "timing": in_page["timing"],
        "console": console,
        "site": site(&url),
    }))
}

/// The main document: the request that ended at `url`, its status and
/// headers, and the addresses it was sent on from (`redirects`).
fn document(net: &[Value], url: &str) -> Value {
    let Some(sent) = net.iter().rev().find(|e| {
        e["method"] == "Network.requestWillBeSent"
            && e["params"]["type"] == "Document"
            && e["params"]["request"]["url"] == url
    }) else {
        return Value::Null;
    };
    let id = &sent["params"]["requestId"];
    let redirects: Vec<Value> = net
        .iter()
        .filter(|e| {
            e["method"] == "Network.requestWillBeSent"
                && e["params"]["requestId"] == *id
                && e["params"].get("redirectResponse").is_some()
        })
        .map(|e| e["params"]["redirectResponse"]["url"].clone())
        .collect();
    let response = net
        .iter()
        .find(|e| e["method"] == "Network.responseReceived" && e["params"]["requestId"] == *id)
        .map(|e| &e["params"]["response"]);
    json!({
        "url": url,
        "status": response.map_or(Value::Null, |r| r["status"].clone()),
        "request_headers": sent["params"]["request"]["headers"],
        "response_headers": response.map_or(Value::Null, |r| r["headers"].clone()),
        "redirects": redirects,
    })
}

/// Every file the page loaded: url, type, status, size (bytes over the
/// wire), ms (from asking to the last byte), mime.
fn files(net: &[Value]) -> Vec<Value> {
    let of = |method: &str, id: &Value| {
        net.iter()
            .find(|e| e["method"] == method && e["params"]["requestId"] == *id)
    };
    net.iter()
        .filter(|e| e["method"] == "Network.responseReceived")
        .map(|e| {
            let p = &e["params"];
            let id = &p["requestId"];
            let done = of("Network.loadingFinished", id);
            // a redirect's hops share the id: the last request is this one
            let asked = net
                .iter()
                .rev()
                .find(|s| s["method"] == "Network.requestWillBeSent" && s["params"]["requestId"] == *id);
            let ms = match (asked, done) {
                (Some(a), Some(d)) => match (a["params"]["timestamp"].as_f64(), d["params"]["timestamp"].as_f64()) {
                    (Some(a), Some(d)) => json!(((d - a) * 1000.0).round()),
                    _ => Value::Null,
                },
                _ => Value::Null,
            };
            json!({
                "url": p["response"]["url"],
                "type": p["type"].as_str().map(str::to_lowercase),
                "status": p["response"]["status"],
                "size": done.map_or(Value::Null, |d| json!(d["params"]["encodedDataLength"].as_f64().map(|n| n as u64))),
                "ms": ms,
                "mime": p["response"]["mimeType"],
            })
        })
        .collect()
}

/// The cookies of the page and its files; a value only with `values`.
/// Third party: set for a site other than the page's.
fn cookies(cdp: &mut Cdp, page: &str, urls: &[&str], values: bool) -> Result<Vec<Value>, String> {
    let mut all: Vec<&str> = vec![page];
    all.extend(urls.iter().filter(|u| u.starts_with("http")));
    let r = cdp.send("Network.getCookies", json!({ "urls": all }))?;
    let host = page
        .split('/')
        .nth(2)
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .to_string();
    Ok(r["cookies"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let domain = c["domain"]
                .as_str()
                .unwrap_or_default()
                .trim_start_matches('.');
            let first = host == domain
                || host.ends_with(&format!(".{domain}"))
                || domain.ends_with(&format!(".{host}"));
            let mut o = json!({
                "name": c["name"],
                "domain": c["domain"],
                "path": c["path"],
                "http_only": c["httpOnly"],
                "secure": c["secure"],
                "same_site": c.get("sameSite").cloned().unwrap_or(Value::Null),
                "expires": if c["session"] == true { Value::Null } else { c["expires"].clone() },
                "third_party": !first,
            });
            if values {
                o["value"] = c["value"].clone();
            }
            o
        })
        .collect())
}

/// The main response's TLS, from Chrome's `securityDetails`: none over
/// plain HTTP.
pub fn tls(details: &Value) -> Value {
    if !details.is_object() {
        return Value::Null;
    }
    let until = details["validTo"]
        .as_f64()
        .and_then(|t| chrono::DateTime::from_timestamp(t as i64, 0))
        .map(|t| t.to_rfc3339());
    json!({
        "protocol": details["protocol"],
        "issuer": details["issuer"],
        "subject": details["subjectName"],
        "valid_until": until,
    })
}

/// Errors and warnings the page logged, from `console` and from Chrome.
fn console(runtime: &[Value], log: &[Value]) -> Vec<Value> {
    let mut out = vec![];
    for e in runtime {
        let p = &e["params"];
        match e["method"].as_str() {
            Some("Runtime.consoleAPICalled") => {
                let level = match p["type"].as_str() {
                    Some("error" | "assert") => "error",
                    Some("warning" | "warn") => "warning",
                    _ => continue,
                };
                let text: Vec<String> = p["args"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|a| match &a["value"] {
                        Value::String(s) => s.clone(),
                        Value::Null => a["description"].as_str().unwrap_or_default().to_string(),
                        v => v.to_string(),
                    })
                    .collect();
                out.push(json!({ "level": level, "text": text.join(" ") }));
            }
            Some("Runtime.exceptionThrown") => {
                let d = &p["exceptionDetails"];
                let text = d["exception"]["description"]
                    .as_str()
                    .or(d["text"].as_str())
                    .unwrap_or_default();
                out.push(
                    json!({ "level": "error", "text": text.lines().next().unwrap_or_default() }),
                );
            }
            _ => {}
        }
    }
    for e in log.iter().filter(|e| e["method"] == "Log.entryAdded") {
        let entry = &e["params"]["entry"];
        if let Some(level @ ("error" | "warning")) = entry["level"].as_str() {
            out.push(json!({ "level": level, "text": entry["text"], "url": entry["url"] }));
        }
    }
    out
}

/// Libraries known by their globals (from the page) or their scripts'
/// addresses, with a version when one says it. Others are not guessed.
fn libraries(in_page: &Value, urls: &[&str]) -> Vec<Value> {
    let mut found: Vec<Value> = in_page["libraries"].as_array().cloned().unwrap_or_default();
    let by_url: [(&str, &str); 4] = [
        ("Google Tag Manager", "googletagmanager.com/gtm.js"),
        ("Google Analytics", "google-analytics.com/"),
        ("Google Analytics", "googletagmanager.com/gtag/js"),
        ("Bootstrap", "bootstrap"),
    ];
    for (name, part) in by_url {
        let known = found.iter().any(|l| l["name"] == name);
        if !known && urls.iter().any(|u| u.contains(part)) {
            found.push(json!({ "name": name, "version": null }));
        }
    }
    found
}

/// What the page itself knows: meta tags, libraries by their globals,
/// timing from its `performance` entries (paints and layout shift read
/// back with `buffered`, the first contentful paint waited for).
const PAGE_FACTS: &str = r#"(async () => {
  const attr = (sel, a) => document.querySelector(sel)?.getAttribute(a) ?? null;
  const tags = prefix => Object.fromEntries([...document.querySelectorAll(`meta[property^="${prefix}:"],meta[name^="${prefix}:"]`)]
    .map(m => [(m.getAttribute('property') || m.getAttribute('name')).slice(prefix.length + 1), m.getAttribute('content')]));
  const jsonLd = [...document.querySelectorAll('script[type="application/ld+json"]')].map(s => {
    try { return JSON.parse(s.textContent); } catch (e) { return { error: String(e) }; } });
  const meta = { title: document.title, description: attr('meta[name="description"]', 'content'),
    canonical: document.querySelector('link[rel="canonical"]')?.href ?? null,
    language: document.documentElement.getAttribute('lang'), og: tags('og'), twitter: tags('twitter'), json_ld: jsonLd };
  const w = window, libs = [];
  const add = (name, version) => libs.push({ name, version: version ?? null });
  if (w.jQuery?.fn?.jquery) add('jQuery', w.jQuery.fn.jquery);
  const reactHook = w.__REACT_DEVTOOLS_GLOBAL_HOOK__?.renderers;
  if (w.React?.version) add('React', w.React.version);
  else if (reactHook && reactHook.size) add('React', [...reactHook.values()][0]?.version);
  if (w.Vue?.version) add('Vue', w.Vue.version); else if (w.__VUE__) add('Vue');
  const ngv = attr('[ng-version]', 'ng-version');
  if (w.ng?.version?.full) add('Angular', w.ng.version.full); else if (ngv) add('Angular', ngv);
  if (w.__NEXT_DATA__) add('Next.js', w.next?.version);
  if (w.__NUXT__ || w.$nuxt) add('Nuxt', w.$nuxt?.$root?.constructor?.version);
  if (w.__svelte) add('Svelte', w.__svelte.v ? [...w.__svelte.v][0] : null);
  if (w.bootstrap?.Tooltip?.VERSION) add('Bootstrap', w.bootstrap.Tooltip.VERSION);
  if (typeof w.gtag === 'function' || typeof w.ga === 'function') add('Google Analytics');
  if (w.google_tag_manager) add('Google Tag Manager');
  const nav = performance.getEntriesByType('navigation')[0];
  // a page painted after its load (a slow machine, content shown late)
  // is waited for, up to 3 s: one that paints no content has none
  const fcp = await new Promise(r => {
    try { new PerformanceObserver(l => { const e = l.getEntries().find(e => e.name === 'first-contentful-paint'); if (e) r(e); })
      .observe({ type: 'paint', buffered: true }); } catch (e) { r(null); }
    setTimeout(() => r(null), 3000); });
  const buffered = type => new Promise(r => {
    try { new PerformanceObserver(l => r(l.getEntries())).observe({ type, buffered: true }); } catch (e) { r([]); }
    setTimeout(() => r([]), 200); });
  const lcp = await buffered('largest-contentful-paint'), shifts = await buffered('layout-shift');
  const ms = v => v == null ? null : Math.round(v);
  const timing = { first_byte_ms: ms(nav?.responseStart), dom_ready_ms: ms(nav?.domContentLoadedEventEnd),
    load_ms: ms(nav?.loadEventEnd), first_contentful_paint_ms: ms(fcp?.startTime),
    largest_contentful_paint_ms: ms(lcp.at(-1)?.startTime),
    layout_shift: Math.round(shifts.filter(s => !s.hadRecentInput).reduce((a, s) => a + s.value, 0) * 1000) / 1000 };
  return { meta, libraries: libs, timing };
})()"#;

/// `robots.txt`, `sitemap.xml` and `llms.txt` at the page's origin, over
/// plain HTTP with no cookies: public files. The text of robots and llms;
/// the sitemap's number of URLs.
fn site(page: &str) -> Value {
    let origin: String = page.splitn(4, '/').take(3).collect::<Vec<_>>().join("/");
    let get = |path: &str| -> Value {
        let got = crate::net::client(Duration::from_secs(10))
            .map_err(|e| crate::net::cause(&e))
            .and_then(|c| {
                c.get(format!("{origin}{path}"))
                    .send()
                    .map_err(|e| crate::net::cause(&e))
            })
            .and_then(|r| {
                let ok = r.status().is_success();
                r.text().map(|t| (ok, t)).map_err(|e| crate::net::cause(&e))
            });
        match got {
            Ok((true, text)) => json!({ "found": true, "size": text.len(), "text": text }),
            Ok((false, _)) => json!({ "found": false }),
            Err(e) => json!({ "found": false, "error": e }),
        }
    };
    let mut sitemap = get("/sitemap.xml");
    if let Some(text) = sitemap.get("text").and_then(Value::as_str) {
        let urls = text.matches("<loc>").count();
        let m: &mut Map<String, Value> = sitemap.as_object_mut().expect("an object");
        m.remove("text");
        m.insert("urls".into(), json!(urls));
    }
    json!({ "robots": get("/robots.txt"), "sitemap": sitemap, "llms": get("/llms.txt") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_is_read_from_chromes_security_details() {
        let d = json!({ "protocol": "TLS 1.3", "issuer": "Deneme CA", "subjectName": "ışık.example",
                        "validTo": 1_893_456_000.0, "cipher": "AES_128_GCM" });
        assert_eq!(
            tls(&d),
            json!({ "protocol": "TLS 1.3", "issuer": "Deneme CA", "subject": "ışık.example",
                    "valid_until": "2030-01-01T00:00:00+00:00" })
        );
        assert_eq!(tls(&Value::Null), Value::Null);
    }
}
