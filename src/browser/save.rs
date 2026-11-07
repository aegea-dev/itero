//! `save`: a page as it is, to one MHTML file Chrome opens offline, or to
//! a folder of every file it loaded, each as it came. Files come from
//! Chrome's own copies. A page (a frame's document) the site marked not to
//! be kept may be gone from them: it is fetched again through the page,
//! with its cookies, and said so.
use super::cdp::Cdp;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// What saving to a folder did.
pub struct Saved {
    pub files: usize,
    /// The URLs Chrome had nothing for.
    pub failed: Vec<String>,
    /// The pages Chrome had no copy of, fetched again.
    pub fetched_again: Vec<String>,
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// The page, with its files inside, as one MHTML file.
pub fn mhtml(cdp: &mut Cdp, path: &Path) -> Result<(), String> {
    let r = cdp.send("Page.captureSnapshot", json!({ "format": "mhtml" }))?;
    let data = r["data"].as_str().ok_or("Chrome gave no snapshot")?;
    write(path, data.as_bytes())
}

/// Every file of the page and its frames to `folder`, by `file_name`.
pub fn files(cdp: &mut Cdp, folder: &Path) -> Result<Saved, String> {
    let tree = cdp.send("Page.getResourceTree", json!({}))?;
    let mut wanted: Vec<(String, String, bool)> = vec![];
    collect(&tree["frameTree"], &mut wanted);
    let mut saved = Saved {
        files: 0,
        failed: vec![],
        fetched_again: vec![],
    };
    let mut seen = std::collections::HashSet::new();
    for (frame, url, page) in wanted {
        if !(url.starts_with("http://") || url.starts_with("https://")) || !seen.insert(url.clone())
        {
            continue;
        }
        let got = cdp.send(
            "Page.getResourceContent",
            json!({ "frameId": frame, "url": url }),
        );
        let bytes = got.and_then(|c| {
            let body = c["content"].as_str().unwrap_or_default();
            if c["base64Encoded"] == true {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD
                    .decode(body)
                    .map_err(|e| e.to_string())
            } else {
                Ok(body.as_bytes().to_vec())
            }
        });
        let bytes = match bytes {
            Err(_) if page => match fetch_again(cdp, &url) {
                Ok(b) => {
                    saved.fetched_again.push(url.clone());
                    Ok(b)
                }
                Err(e) => Err(e),
            },
            other => other,
        };
        match bytes {
            Ok(b) => {
                write(&folder.join(file_name(&url)), &b)?;
                saved.files += 1;
            }
            Err(_) => saved.failed.push(url),
        }
    }
    Ok(saved)
}

/// `url` fetched again from the page, with its cookies, byte for byte.
fn fetch_again(cdp: &mut Cdp, url: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let got = cdp.eval(&format!(
        "fetch({}, {{ credentials: 'include' }}).then(async r => {{
  if (!r.ok) throw new Error('status ' + r.status);
  const b = new Uint8Array(await r.arrayBuffer());
  let s = ''; for (let i = 0; i < b.length; i += 32768) s += String.fromCharCode(...b.subarray(i, i + 32768));
  return btoa(s); }})",
        json!(url)
    ))?;
    base64::engine::general_purpose::STANDARD
        .decode(got.as_str().unwrap_or_default())
        .map_err(|e| e.to_string())
}

/// The frame's document and resources, then its child frames', as
/// (frame id, url, whether it is a frame's document).
fn collect(node: &Value, out: &mut Vec<(String, String, bool)>) {
    let frame = node["frame"]["id"].as_str().unwrap_or_default().to_string();
    if let Some(u) = node["frame"]["url"].as_str() {
        out.push((frame.clone(), u.to_string(), true));
    }
    for r in node["resources"].as_array().into_iter().flatten() {
        if let Some(u) = r["url"].as_str() {
            out.push((frame.clone(), u.to_string(), false));
        }
    }
    for child in node["childFrames"].as_array().into_iter().flatten() {
        collect(child, out);
    }
}

/// Where `url` is saved in a folder: `host[:port]/path`, percent-decoded
/// so it reads as the site's own names; `index.html` for a path ending in
/// `/`; a query becomes `-<8 hex of its hash>` before the extension, so
/// two queries make two files. `.` and `..` never leave the folder.
pub fn file_name(url: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let rest = rest.split('#').next().unwrap_or_default();
    let (rest, query) = match rest.split_once('?') {
        Some((r, q)) => (r, Some(q)),
        None => (rest, None),
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let mut parts: Vec<String> = path
        .split('/')
        .map(decode)
        .filter(|p| !p.is_empty() && p != "." && p != "..")
        .map(|p| p.replace(['/', '\\'], "_"))
        .collect();
    if path.is_empty() || path.ends_with('/') {
        parts.push("index.html".into());
    }
    if let (Some(q), Some(last)) = (query, parts.last_mut()) {
        let hash: String = Sha256::digest(q.as_bytes())[..4]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        *last = match last.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => format!("{stem}-{hash}.{ext}"),
            _ => format!("{last}-{hash}"),
        };
    }
    let mut p = PathBuf::from(host.replace(['/', '\\'], "_"));
    for part in parts {
        p.push(part);
    }
    p
}

/// `%C3%A7` as `ç`.
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

#[cfg(test)]
mod tests {
    use super::file_name;
    use std::path::PathBuf;

    #[test]
    fn a_url_is_a_readable_path_inside_the_folder() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(file_name("https://a.com/"), p("a.com/index.html"));
        assert_eq!(file_name("https://a.com"), p("a.com/index.html"));
        assert_eq!(
            file_name("http://127.0.0.1:8080/x/"),
            p("127.0.0.1:8080/x/index.html")
        );
        assert_eq!(
            file_name("https://a.com/%C3%A7/%C5%9F.css"),
            p("a.com/ç/ş.css")
        );
        assert_eq!(file_name("https://a.com/x/../../y.js#b"), p("a.com/x/y.js"));
        assert_eq!(file_name("https://a.com/a%2Fb.js"), p("a.com/a_b.js"));
        let v2 = file_name("https://a.com/x/y.css?v=2");
        let v3 = file_name("https://a.com/x/y.css?v=3");
        assert_ne!(v2, v3);
        let name = v2.file_name().unwrap().to_str().unwrap().to_string();
        assert!(
            name.starts_with("y-")
                && name.ends_with(".css")
                && name.len() == "y-12345678.css".len(),
            "{name}"
        );
    }
}
