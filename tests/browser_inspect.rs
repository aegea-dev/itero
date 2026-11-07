//! What a page loads: the JSON its scripts fetch (`responses`), and
//! `inspect`, a page's facts.
#[path = "common/browser.rs"]
mod browser;
#[path = "common/chrome.rs"]
mod test_chrome;
#[path = "common/web.rs"]
mod web;
use browser::{step, tab};
use serde_json::json;

/// A page that fetches JSON after it has loaded, and once more on a click.
fn shop() -> web::Web {
    web::serve(vec![
        web::html(
            "/dükkân",
            r#"<!doctype html><title>Dükkân</title><button id="daha">Daha fazla</button>
<script>addEventListener('load', () => {
  fetch('/api/ürünler?sayfa=1'); fetch('/api/başka'); fetch('/api/ürünler.txt');
});
document.getElementById('daha').addEventListener('click', () => fetch('/api/ürünler?sayfa=2'));</script>"#,
        ),
        web::file(
            "/api/ürünler",
            "application/json; charset=utf-8",
            r#"[{"ad": "Işık", "fiyat": 12}]"#,
        ),
        web::file("/api/başka", "application/json", r#"{"başka": true}"#),
        web::file("/api/ürünler.txt", "text/plain", "düz metin"),
    ])
}

#[test]
fn the_json_a_page_loads_is_kept_by_the_text_of_its_url() {
    let (c, w) = (test_chrome::start(), shop());
    let t = tab(c.port);
    let o = step(
        &t,
        json!({ "open": w.url("/dükkân"), "responses": "/api/%C3%BCr%C3%BCnler" }),
    )
    .unwrap();
    assert_eq!(
        o["responses"],
        json!([{
            "url": w.url("/api/%C3%BCr%C3%BCnler?sayfa=1"),
            "status": 200,
            "json": [{"ad": "Işık", "fiyat": 12}]
        }]),
        "{o}"
    );
    // on a click too: only what that step loaded
    let o = step(&t, json!({ "click": "Daha fazla", "responses": "ürünler" })).unwrap();
    let urls: Vec<&str> = o["responses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["url"].as_str().unwrap())
        .collect();
    assert_eq!(urls, [w.url("/api/%C3%BCr%C3%BCnler?sayfa=2")], "{o}");
    // without the arg, none are kept
    let o = step(&t, json!({ "open": w.url("/dükkân") })).unwrap();
    assert!(o.get("responses").is_none(), "{o}");
}

#[test]
fn responses_goes_with_a_step_that_loads_and_is_a_text() {
    let e = itero::browser::step::check(&json!({ "read": "text", "responses": 3 })).unwrap_err();
    for says in [
        "responses goes with open, wait, click or key",
        "responses is a text",
    ] {
        assert!(e.contains(says), "{says}: {e}");
    }
}
