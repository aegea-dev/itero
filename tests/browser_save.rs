//! `save`: a page as it is, its files to a folder or one MHTML file.
#[path = "common/browser.rs"]
mod browser;
#[path = "common/chrome.rs"]
mod test_chrome;
#[path = "common/web.rs"]
mod web;
use base64::Engine;
use browser::{step, tab};
use serde_json::json;

const PAGE: &str = r#"<!doctype html><title>Sipariş</title>
<link rel="stylesheet" href="/stil.css?v=2">
<h1 id="baş">Işık</h1><img src="/resim.png" width="1" height="1">
<img src="data:image/png;base64,iVBORw0KGgo=">
<p style="font-family: Deneme">Gönder</p>
<script src="/uygulama.js"></script>"#;

/// A 1 by 1 PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

fn site() -> (web::Web, Vec<u8>, Vec<u8>) {
    let png = base64::engine::general_purpose::STANDARD
        .decode(PNG)
        .unwrap();
    let font: Vec<u8> = (0u8..=255).cycle().take(600).collect();
    let w = web::serve(vec![
        web::html("/sipariş", PAGE),
        web::file(
            "/stil.css",
            "text/css",
            "@font-face { font-family: Deneme; src: url(/yazı.woff2) format('woff2') } h1 { color: #E57373 }",
        ),
        web::file(
            "/uygulama.js",
            "text/javascript",
            "document.getElementById('baş').textContent = 'Değişti';",
        ),
        web::bytes("/resim.png", "image/png", &png),
        web::bytes("/yazı.woff2", "font/woff2", &font),
    ]);
    (w, png, font)
}

#[test]
fn a_page_is_saved_as_its_files_as_they_came() {
    let c = test_chrome::start();
    let (w, png, font) = site();
    let t = tab(c.port);
    step(&t, json!({ "open": w.url("/sipariş") })).unwrap();
    // the font is asked for once the text needs it
    step(&t, json!({ "wait": "Değişti" })).unwrap();
    let o = step(&t, json!({ "save": "kayıt" })).unwrap();
    let host = format!("127.0.0.1:{}", w.port);
    let dir = t.root.path().join("kayıt").join(&host);
    assert_eq!(
        o["folder"],
        t.root.path().join("kayıt").display().to_string(),
        "{o}"
    );
    assert_eq!(o["failed"], json!([]), "{o}");
    // the document as it was served, not as its script changed it
    let doc = std::fs::read_to_string(dir.join("sipariş")).unwrap();
    assert_eq!(doc, PAGE);
    let css: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.starts_with("stil-") && n.ends_with(".css"))
        .collect();
    assert_eq!(css.len(), 1, "{css:?}");
    assert!(
        std::fs::read_to_string(dir.join(&css[0]))
            .unwrap()
            .contains("#E57373")
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("uygulama.js")).unwrap(),
        "document.getElementById('baş').textContent = 'Değişti';"
    );
    assert_eq!(std::fs::read(dir.join("resim.png")).unwrap(), png);
    assert_eq!(std::fs::read(dir.join("yazı.woff2")).unwrap(), font);
    assert_eq!(o["files"], 5, "{o}");
}

#[test]
fn a_page_is_saved_as_one_mhtml_file() {
    let c = test_chrome::start();
    let (w, _, _) = site();
    let t = tab(c.port);
    step(&t, json!({ "open": w.url("/sipariş") })).unwrap();
    let o = step(&t, json!({ "save": "kayıt/sayfa.mhtml" })).unwrap();
    let file = t.root.path().join("kayıt/sayfa.mhtml");
    assert_eq!(o["file"], file.display().to_string(), "{o}");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.starts_with("From: <Saved by Blink>"),
        "{}",
        &text[..80]
    );
    // the stylesheet is in it, as Chrome reads it, not byte for byte
    assert!(
        text.contains("Content-Location: http://127.0.0.1:"),
        "{text}"
    );
    assert!(text.contains("h1 { color: rgb(229, 115, 115); }"), "{text}");
}

#[test]
fn save_is_a_path() {
    let e = itero::browser::step::check(&json!({ "save": "" })).unwrap_err();
    assert!(
        e.contains("save is a file ending in .mhtml or a folder"),
        "{e}"
    );
}

#[test]
fn a_page_chrome_kept_no_copy_of_is_fetched_again_and_said() {
    let c = test_chrome::start();
    let mut page = web::html(
        "/taze",
        "<!doctype html><title>Taze</title><p>Her seferinde yeni</p>",
    );
    page.headers
        .push(("Cache-Control".into(), "no-store".into()));
    let w = web::serve(vec![page]);
    let t = tab(c.port);
    step(&t, json!({ "open": w.url("/taze") })).unwrap();
    let o = step(&t, json!({ "save": "kayıt" })).unwrap();
    let file = t
        .root
        .path()
        .join(format!("kayıt/127.0.0.1:{}/taze", w.port));
    assert_eq!(
        std::fs::read_to_string(file).unwrap(),
        "<!doctype html><title>Taze</title><p>Her seferinde yeni</p>"
    );
    assert_eq!(o["failed"], json!([]), "{o}");
    assert_eq!(o["fetched_again"], json!([w.url("/taze")]), "{o}");
}
