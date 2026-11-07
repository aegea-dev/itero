//! A small web server for browser tests: fixed pages on 127.0.0.1, served
//! from a thread until the test ends.
// each test binary uses its own part of it
#![allow(dead_code)]
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Page {
    pub path: String,
    // a header `X-Test-Delay-Ms` makes the server wait that long before
    // it answers; it is not sent
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// An HTML page at `path`.
pub fn html(path: &str, body: &str) -> Page {
    Page {
        path: path.into(),
        status: 200,
        headers: vec![("Content-Type".into(), "text/html; charset=utf-8".into())],
        body: body.as_bytes().to_vec(),
    }
}

/// Any page, with its content type.
pub fn file(path: &str, content_type: &str, body: &str) -> Page {
    bytes(path, content_type, body.as_bytes())
}

/// A file of any bytes (an image, a font), with its content type.
pub fn bytes(path: &str, content_type: &str, body: &[u8]) -> Page {
    Page {
        path: path.into(),
        status: 200,
        headers: vec![("Content-Type".into(), content_type.into())],
        body: body.to_vec(),
    }
}

pub struct Web {
    pub port: u16,
    /// Each request's path and query, as it came.
    pub requests: Arc<Mutex<Vec<String>>>,
}

impl Web {
    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

pub fn serve(pages: Vec<Page>) -> Web {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    std::thread::spawn(move || {
        for s in listener.incoming() {
            let Ok(mut s) = s else { continue };
            let pages = pages.clone();
            let seen = seen.clone();
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut first = String::new();
                if r.read_line(&mut first).is_err() {
                    return;
                }
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                }
                let target = first.split(' ').nth(1).unwrap_or("/").to_string();
                seen.lock().unwrap().push(target.clone());
                let path = target.split('?').next().unwrap_or("/");
                let page = pages
                    .iter()
                    .find(|p| p.path == path || percent_decode(&p.path) == percent_decode(path));
                let (status, mut headers, body) = match page {
                    Some(p) => (p.status, p.headers.clone(), p.body.clone()),
                    None => (404, vec![], b"yok".to_vec()),
                };
                if let Some(at) = headers.iter().position(|h| h.0 == "X-Test-Delay-Ms") {
                    let ms: u64 = headers.remove(at).1.parse().unwrap();
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                }
                let mut out = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (k, v) in headers {
                    out.push_str(&format!("{k}: {v}\r\n"));
                }
                out.push_str("\r\n");
                let mut all = out.into_bytes();
                all.extend_from_slice(&body);
                let _ = s.write_all(&all);
            });
        }
    });
    Web { port, requests }
}

/// `%C4%B1` as `ı`: Chrome asks for non-ASCII paths percent-encoded.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
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
