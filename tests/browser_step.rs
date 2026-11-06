//! The built-in `#browser`, run as the worker runs it, on a tab of a
//! headless Chrome the test starts, against pages served here.
#[path = "common/chrome.rs"]
mod test_chrome;
#[path = "common/web.rs"]
mod web;
use itero::browser::cdp;
use itero::engine::worker::{self, Outcome, StepRun};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

const SIPARIS: &str = r#"<!doctype html><title>Sipariş</title>
<h1>Sipariş</h1><p id="kargo">Kargo <b>yarın</b> gelir.</p>
<a href="/çıkış">Çıkış</a>
<a href="/yeni" target="_blank">Yeni sekme</a>
<button>Gönder</button><button>Gönder</button>
<form action="/ara"><label>Ara <input name="q"></label></form>
<style>#alt{display:none}#menü:hover #alt{display:block}</style>
<div id="menü" style="width:200px">Menü<div id="alt"><a href="/x">Alt menü</a></div></div>
<a href="/tartışma" id="yerinde">Tartışma</a>
<script>document.getElementById('yerinde').addEventListener('click', e => {
  e.preventDefault(); history.pushState({}, '', '/tartışma');
  setTimeout(() => { document.title = 'Tartışma'; document.body.innerHTML = '<p>Yorumlar</p>' }, 100) })</script>
<script>setTimeout(() => { const p = document.createElement('p'); p.textContent = 'Sonuç'; document.body.append(p) }, 1000)</script>
"#;

fn site() -> web::Web {
    web::serve(vec![
        web::html("/sipariş", SIPARIS),
        web::html(
            "/çıkış",
            "<!doctype html><title>Çıkış</title><p>Güle güle</p>",
        ),
        web::html(
            "/yeni",
            "<!doctype html><title>Yeni</title><p>Yeni sayfa</p>",
        ),
        web::html("/ara", "<!doctype html><title>Arama</title><p>Sonuçlar</p>"),
    ])
}

struct Tab {
    ws: String,
    root: tempfile::TempDir,
}

fn tab(c: &test_chrome::TestChrome) -> Tab {
    Tab {
        ws: cdp::new_tab(c.port, "about:blank").unwrap().ws,
        root: tempfile::tempdir().unwrap(),
    }
}

fn step(t: &Tab, args: Value) -> Result<Value, String> {
    let mut input = Map::new();
    input.insert("args".into(), args);
    let r = StepRun {
        file: PathBuf::from(worker::BROWSER),
        root: t.root.path().to_path_buf(),
        workflow: "tarayıcı".into(),
        env: [
            ("ITERO_BROWSER_TAB", t.ws.as_str()),
            ("ITERO_BROWSER_MODE", "headless"),
            ("ITERO_BROWSER_RUN", "koşu"),
            ("ITERO_BROWSER_STEP", "2"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect(),
        input,
        memory: t.root.path().join("hafıza"),
        waiting_since: None,
        quiet: 60,
        most: 60,
        now: String::new(),
    };
    match worker::run_step(&r) {
        Ok(Outcome::Done(v)) => Ok(v),
        Ok(other) => panic!("{other:?}"),
        Err(e) => Err(e.to_string()),
    }
}

#[test]
fn open_wait_find_and_read() {
    let (c, w) = (test_chrome::start(), site());
    let t = tab(&c);
    let o = step(&t, json!({ "open": w.url("/sipariş") })).unwrap();
    assert_eq!(o["title"], "Sipariş");
    assert_eq!(o["mode"], "headless");
    assert_eq!(o["url"], w.url("/sipari%C5%9F"));

    let o = step(&t, json!({ "wait": "Sonuç" })).unwrap();
    assert_eq!(o["found"], true);
    let o = step(&t, json!({ "find": "Gönder" })).unwrap();
    assert_eq!(
        (o["found"].clone(), o["count"].clone(), o["text"].clone()),
        (json!(true), json!(2), json!("Gönder"))
    );
    let o = step(&t, json!({ "find": "Yok" })).unwrap();
    assert_eq!(
        (o["found"].clone(), o["count"].clone()),
        (json!(false), json!(0))
    );

    let o = step(&t, json!({ "read": "text" })).unwrap();
    let text = o["text"].as_str().unwrap();
    assert_eq!(
        text,
        "# Sipariş\nKargo yarın gelir.\n[Çıkış]\n[Yeni sekme]\n[button: Gönder]\nAra [Ara: ___]\nMenü\n[Tartışma]\nSonuç"
    );
    let o = step(&t, json!({ "read": "html" })).unwrap();
    assert!(
        o["html"].as_str().unwrap().contains("<h1>Sipariş</h1>"),
        "{o}"
    );
    let o = step(&t, json!({ "read": "html", "in": { "css": "#kargo" } })).unwrap();
    assert_eq!(o["html"], r#"<p id="kargo">Kargo <b>yarın</b> gelir.</p>"#);
}

#[test]
fn what_is_not_there_fails_naming_it_with_a_screenshot() {
    let (c, w) = (test_chrome::start(), site());
    let t = tab(&c);
    step(&t, json!({ "open": w.url("/sipariş") })).unwrap();
    let started = std::time::Instant::now();
    let e = step(&t, json!({ "wait": "Yok", "timeout": 1 })).unwrap_err();
    assert!(started.elapsed().as_secs_f64() < 3.0);
    assert!(e.contains("\"Yok\" is not on the page after 1 s"), "{e}");
    assert!(e.contains("on \"Sipariş\""), "{e}");
    let shot = Path::new(".itero/browser/koşu-2.png");
    assert!(e.contains(&shot.display().to_string()), "{e}");
    let png = std::fs::read(t.root.path().join(shot)).unwrap();
    assert_eq!(&png[1..4], b"PNG");
}

#[test]
fn click_hover_type_and_key() {
    let (c, w) = (test_chrome::start(), site());
    let t = tab(&c);
    step(&t, json!({ "open": w.url("/sipariş") })).unwrap();
    let o = step(&t, json!({ "hover": "Menü" })).unwrap();
    assert_eq!(o["title"], "Sipariş");
    assert_eq!(
        step(&t, json!({ "find": "Alt menü" })).unwrap()["found"],
        true
    );

    let o = step(&t, json!({ "click": "Çıkış" })).unwrap();
    assert_eq!(o["title"], "Çıkış");

    step(&t, json!({ "open": w.url("/sipariş") })).unwrap();
    step(&t, json!({ "type": "ışık", "into": "Ara" })).unwrap();
    let o = step(&t, json!({ "key": "Enter" })).unwrap();
    assert_eq!(o["title"], "Arama");
    assert!(
        o["url"].as_str().unwrap().contains("q=%C4%B1%C5%9F%C4%B1k"),
        "{o}"
    );
}

#[test]
fn a_click_that_changes_the_page_in_place_does_not_wait_for_a_load() {
    let (c, w) = (test_chrome::start(), site());
    let t = tab(&c);
    step(&t, json!({ "open": w.url("/sipariş") })).unwrap();
    let started = std::time::Instant::now();
    let o = step(&t, json!({ "click": "Tartışma", "timeout": 5 })).unwrap();
    assert!(
        started.elapsed().as_secs_f64() < 4.0,
        "{:?}",
        started.elapsed()
    );
    assert!(
        o["url"].as_str().unwrap().ends_with("/tart%C4%B1%C5%9Fma"),
        "{o}"
    );
    assert_eq!(
        step(&t, json!({ "wait": "Yorumlar" })).unwrap()["found"],
        true
    );
}

#[test]
fn a_click_that_opens_a_tab_is_followed() {
    let (c, w) = (test_chrome::start(), site());
    let state = tempfile::tempdir().unwrap();
    // SAFETY: set before any step of this test reads it; the other tests
    // never open a tab with a click
    unsafe { std::env::set_var("XDG_STATE_HOME", state.path()) };
    let t = tab(&c);
    step(&t, json!({ "open": w.url("/sipariş") })).unwrap();
    let o = step(&t, json!({ "click": "Yeni sekme" })).unwrap();
    assert_eq!(
        (o["new_tab"].clone(), o["title"].clone()),
        (json!(true), json!("Yeni"))
    );
    // left for the daemon, which gives the run's next step that tab
    let left = std::fs::read_to_string(state.path().join("itero/browser/followed/koşu")).unwrap();
    assert!(left.contains("\"ws\""), "{left}");
}

#[test]
fn every_problem_with_the_args_is_said_at_once() {
    let e = itero::browser::step::check(&json!({
        "open": "ftp://x", "click": "Gönder", "timeout": "x", "key": "Uç", "read": "pdf", "in": 3
    }))
    .unwrap_err();
    for says in [
        "one action per step, not open and read and click and key",
        "open is an http or https address",
        "in: a target is a name",
        "read is \"text\" or \"html\", not \"pdf\"",
        "key is one of Enter",
        "timeout is seconds",
    ] {
        assert!(e.contains(says), "{says}: {e}");
    }
    let e = itero::browser::step::check(&json!({ "type": "x" })).unwrap_err();
    assert!(e.contains("type needs into"), "{e}");
    let e = itero::browser::step::check(&json!({})).unwrap_err();
    assert!(
        e.contains("a step does one of open, wait, find, read, click, hover, type or key"),
        "{e}"
    );
    // known only in a run
    itero::browser::step::check(&json!({ "open": "$output.url" })).unwrap();
}
