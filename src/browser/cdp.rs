//! A small blocking CDP client: one WebSocket to one target (a tab, or
//! the browser), commands answered in order, events kept for whoever
//! waits on them.
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::net::TcpStream;
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

/// A tab: its id and the address of its WebSocket.
#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    pub id: String,
    pub ws: String,
}

pub struct Cdp {
    ws: WebSocket<TcpStream>,
    /// How long a command's answer is waited for.
    timeout: Duration,
    next: u64,
    queued: VecDeque<Value>,
}

enum Read {
    Timeout,
    Failed(String),
}

impl Cdp {
    /// Connects to `url` (`ws://127.0.0.1:<port>/devtools/...`); each
    /// command's answer is waited for up to `timeout`.
    pub fn connect(url: &str, timeout: Duration) -> Result<Cdp, String> {
        let rest = url
            .strip_prefix("ws://")
            .ok_or_else(|| format!("not a ws:// address: {url}"))?;
        let host = rest.split('/').next().unwrap_or_default();
        let tcp = TcpStream::connect(host).map_err(|e| format!("the browser at {host}: {e}"))?;
        tcp.set_read_timeout(Some(timeout))
            .map_err(|e| e.to_string())?;
        let (ws, _) = tungstenite::client(url, tcp).map_err(|e| format!("the browser: {e}"))?;
        Ok(Cdp {
            ws,
            timeout,
            next: 0,
            queued: VecDeque::new(),
        })
    }

    fn read(&mut self, until: Instant) -> Result<Value, Read> {
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Read::Timeout);
            }
            self.ws
                .get_ref()
                .set_read_timeout(Some(left))
                .map_err(|e| Read::Failed(e.to_string()))?;
            match self.ws.read() {
                Ok(Message::Text(t)) => {
                    return serde_json::from_str(&t)
                        .map_err(|e| Read::Failed(format!("the browser said: {e}")));
                }
                Ok(Message::Close(_)) => {
                    return Err(Read::Failed("the browser closed the tab".into()));
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Err(Read::Timeout);
                }
                Err(e) => return Err(Read::Failed(format!("the browser: {e}"))),
            }
        }
    }

    /// Sends a command and waits for its answer: the result, or the
    /// error's message after the method's name. Events read meanwhile are
    /// kept.
    pub fn send(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        let msg = json!({ "id": id, "method": method, "params": params });
        self.ws
            .send(Message::text(msg.to_string()))
            .map_err(|e| format!("the browser: {e}"))?;
        let until = Instant::now() + self.timeout;
        loop {
            let m = match self.read(until) {
                Ok(m) => m,
                Err(Read::Timeout) => {
                    return Err(format!(
                        "{method}: no answer in {} s",
                        self.timeout.as_secs()
                    ));
                }
                Err(Read::Failed(why)) => return Err(why),
            };
            if m["id"] == id {
                if let Some(e) = m.get("error") {
                    let why = e["message"].as_str().unwrap_or("failed");
                    return Err(format!("{method}: {why}"));
                }
                return Ok(m["result"].clone());
            }
            if m.get("method").is_some() {
                self.queued.push_back(m);
            }
        }
    }

    /// The first event, kept or new, that `pick` takes, before `until`;
    /// else "timed out". The events it does not take are kept.
    pub fn wait_event(
        &mut self,
        until: Instant,
        pick: &mut dyn FnMut(&Value) -> bool,
    ) -> Result<Value, String> {
        if let Some(i) = self.queued.iter().position(&mut *pick) {
            return Ok(self.queued.remove(i).expect("found"));
        }
        loop {
            match self.read(until) {
                Ok(m) if m.get("method").is_some() => {
                    if pick(&m) {
                        return Ok(m);
                    }
                    self.queued.push_back(m);
                }
                Ok(_) => {}
                Err(Read::Timeout) => return Err("timed out".into()),
                Err(Read::Failed(why)) => return Err(why),
            }
        }
    }

    /// A copy of the events kept so far of a domain (`prefix`), left kept.
    pub fn kept(&self, prefix: &str) -> Vec<Value> {
        self.queued
            .iter()
            .filter(|e| e["method"].as_str().is_some_and(|m| m.starts_with(prefix)))
            .cloned()
            .collect()
    }

    /// The events kept so far of a domain (`prefix`, as `"Page."`), taken;
    /// the others stay kept.
    pub fn events(&mut self, prefix: &str) -> Vec<Value> {
        let (taken, kept): (VecDeque<Value>, VecDeque<Value>) = self
            .queued
            .drain(..)
            .partition(|e| e["method"].as_str().is_some_and(|m| m.starts_with(prefix)));
        self.queued = kept;
        taken.into()
    }

    /// The value of a JavaScript expression in the page, its promise
    /// awaited; an exception is an error with its first line.
    pub fn eval(&mut self, expr: &str) -> Result<Value, String> {
        let r = self.send(
            "Runtime.evaluate",
            json!({ "expression": expr, "returnByValue": true, "awaitPromise": true }),
        )?;
        if let Some(x) = r.get("exceptionDetails") {
            let why = x["exception"]["description"]
                .as_str()
                .or(x["text"].as_str())
                .unwrap_or("an exception");
            return Err(why.lines().next().unwrap_or(why).to_string());
        }
        Ok(r["result"]["value"].clone())
    }
}

fn http(method: reqwest::Method, port: u16, path: &str) -> Result<String, String> {
    let c = crate::net::client(Duration::from_secs(10)).map_err(|e| e.to_string())?;
    let r = c
        .request(method, format!("http://127.0.0.1:{port}{path}"))
        .send()
        .map_err(|e| format!("the browser: {}", crate::net::cause(&e)))?;
    let ok = r.status().is_success();
    let body = r
        .text()
        .map_err(|e| format!("the browser: {}", crate::net::cause(&e)))?;
    if ok {
        Ok(body)
    } else {
        Err(format!("the browser: {}", body.trim()))
    }
}

/// The browser's own connection, for the Chrome on `port`.
pub fn browser(port: u16) -> Result<Cdp, String> {
    let body = http(reqwest::Method::GET, port, "/json/version")?;
    let v: Value = serde_json::from_str(&body).map_err(|e| format!("the browser said: {e}"))?;
    let ws = v["webSocketDebuggerUrl"]
        .as_str()
        .ok_or("the browser gave no address")?;
    Cdp::connect(ws, Duration::from_secs(10))
}

/// Opens `url` in a new window of the Chrome on `port`, so the page is in
/// front and drawn: a page behind another tab is not, and a click on it
/// lands on an old layout or nowhere.
pub fn new_window(port: u16, url: &str) -> Result<Tab, String> {
    let r = browser(port)?.send(
        "Target.createTarget",
        json!({ "url": url, "newWindow": true }),
    )?;
    let id = r["targetId"].as_str().ok_or("the browser gave no tab id")?;
    Ok(Tab {
        id: id.into(),
        ws: format!("ws://127.0.0.1:{port}/devtools/page/{id}"),
    })
}

/// Closes tab `id` in the Chrome on `port`.
pub fn close_tab(port: u16, id: &str) -> Result<(), String> {
    http(reqwest::Method::GET, port, &format!("/json/close/{id}")).map(|_| ())
}
