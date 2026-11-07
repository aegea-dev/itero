//! The built-in `#browser`, run as the worker runs it, on a tab of a
//! headless Chrome the test starts, against pages served here.
#[path = "common/browser.rs"]
mod browser;
#[path = "common/chrome.rs"]
mod test_chrome;
#[path = "common/web.rs"]
mod web;
use browser::{step, tab};
use serde_json::json;
use std::path::Path;

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

#[test]
fn open_wait_find_and_read() {
    let (c, w) = (test_chrome::start(), site());
    let t = tab(c.port);
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
    let t = tab(c.port);
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
    let t = tab(c.port);
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
    let t = tab(c.port);
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
    let t = tab(c.port);
    step(&t, json!({ "open": w.url("/sipariş") })).unwrap();
    let o = step(&t, json!({ "click": "Yeni sekme" })).unwrap();
    assert_eq!(
        (o["new_tab"].clone(), o["title"].clone()),
        (json!(true), json!("Yeni"))
    );
    // left for the daemon, which gives the run's next step that tab
    let left = std::fs::read_to_string(state.path().join("itero/browser/followed/koşu")).unwrap();
    let left: serde_json::Value = serde_json::from_str(&left).unwrap();
    // in front in the run's window, so the next step's click reaches it
    let mut next = itero::browser::cdp::Cdp::connect(
        left["ws"].as_str().unwrap(),
        std::time::Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(next.eval("document.visibilityState").unwrap(), "visible");
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
        e.contains("a step does one of open, wait, find, read, click, hover, type, key, inspect, screenshot or scroll"),
        "{e}"
    );
    // known only in a run
    itero::browser::step::check(&json!({ "open": "$output.url" })).unwrap();
}

const UZUN: &str = r#"<!doctype html><title>Uzun</title><style>body{margin:0}</style>
<div id="kutu" style="width:120px;height:40px;background:#6C8EEF">Kutu</div>
<div style="height:3000px">Işık</div><p id="dip">Dip</p>"#;

/// A PNG's width and height, from its header.
fn png_size(bytes: &[u8]) -> (u32, u32) {
    assert_eq!(&bytes[1..4], b"PNG");
    let n = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
    (n(16), n(20))
}

/// What the page says now, from a connection of the test's own.
fn ask(t: &browser::Tab, expr: &str) -> serde_json::Value {
    itero::browser::cdp::Cdp::connect(&t.ws, std::time::Duration::from_secs(5))
        .unwrap()
        .eval(expr)
        .unwrap()
}

#[test]
fn screenshots_of_the_view_the_whole_page_or_one_element() {
    let (c, w) = (
        test_chrome::start(),
        web::serve(vec![web::html("/uzun", UZUN)]),
    );
    let t = tab(c.port);
    step(&t, json!({ "open": w.url("/uzun") })).unwrap();
    let (vw, vh) = (
        ask(&t, "innerWidth").as_u64().unwrap() as u32,
        ask(&t, "innerHeight").as_u64().unwrap() as u32,
    );

    let o = step(&t, json!({ "screenshot": "resimler/görünüm.png" })).unwrap();
    let file = t.root.path().join("resimler/görünüm.png");
    assert_eq!(o["file"], file.display().to_string(), "{o}");
    assert_eq!(png_size(&std::fs::read(&file).unwrap()), (vw, vh));

    step(
        &t,
        json!({ "screenshot": "resimler/tüm.png", "full": true }),
    )
    .unwrap();
    let (fw, fh) = png_size(&std::fs::read(t.root.path().join("resimler/tüm.png")).unwrap());
    assert_eq!(fw, vw);
    assert!(fh > 3000, "{fh}");

    step(
        &t,
        json!({ "screenshot": "resimler/kutu.png", "in": { "css": "#kutu" } }),
    )
    .unwrap();
    assert_eq!(
        png_size(&std::fs::read(t.root.path().join("resimler/kutu.png")).unwrap()),
        (120, 40)
    );

    let abs = t.root.path().join("başka/görünüm.jpg");
    step(&t, json!({ "screenshot": abs.display().to_string() })).unwrap();
    assert_eq!(&std::fs::read(&abs).unwrap()[..2], [0xFF, 0xD8]);
}

#[test]
fn scroll_brings_a_target_into_view() {
    let (c, w) = (
        test_chrome::start(),
        web::serve(vec![web::html("/uzun", UZUN)]),
    );
    let t = tab(c.port);
    step(&t, json!({ "open": w.url("/uzun") })).unwrap();
    assert_eq!(ask(&t, "scrollY"), 0);
    step(&t, json!({ "scroll": "Dip" })).unwrap();
    assert!(ask(&t, "scrollY").as_f64().unwrap() > 2000.0);
}

#[test]
fn screenshot_args_are_checked_at_once() {
    let e = itero::browser::step::check(&json!({
        "screenshot": "x.gif", "full": "evet", "in": "Kutu"
    }))
    .unwrap_err();
    for says in [
        "screenshot is a .png, .jpg or .jpeg file, not \"x.gif\"",
        "full is true or false",
        "full and in do not go together",
    ] {
        assert!(e.contains(says), "{says}: {e}");
    }
    let e =
        itero::browser::step::check(&json!({ "open": "https://a.b", "full": true, "in": "Kutu" }))
            .unwrap_err();
    for says in [
        "full goes with screenshot",
        "in goes with read or screenshot",
    ] {
        assert!(e.contains(says), "{says}: {e}");
    }
}

#[test]
fn width_and_height_resize_the_runs_window_first() {
    let (c, w) = (
        test_chrome::start(),
        web::serve(vec![web::html("/uzun", UZUN)]),
    );
    let t = tab(c.port);
    let o = step(
        &t,
        json!({ "open": w.url("/uzun"), "width": 1024, "height": 700 }),
    )
    .unwrap();
    assert_eq!(o["title"], "Uzun");
    assert_eq!(ask(&t, "outerWidth"), 1024);
    assert_eq!(ask(&t, "outerHeight"), 700);
    let inner = ask(&t, "innerWidth").as_u64().unwrap();
    assert!(inner > 900 && inner <= 1024, "{inner}");
}

#[test]
fn width_and_height_are_checked_at_once() {
    let e = itero::browser::step::check(&json!({ "read": "text", "width": 50 })).unwrap_err();
    for says in [
        "width and height go together",
        "width is whole pixels from 200 to 8000, not 50",
    ] {
        assert!(e.contains(says), "{says}: {e}");
    }
}
