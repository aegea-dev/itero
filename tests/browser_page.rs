//! The page as text, and finding what a step names, in a real headless
//! Chrome against pages served here.
#[path = "common/chrome.rs"]
mod test_chrome;
#[path = "common/web.rs"]
mod web;
use itero::browser::cdp::{self, Cdp};
use itero::browser::target::{self, Target};
use itero::browser::text;
use serde_json::json;
use std::time::{Duration, Instant};

const METIN: &str = r#"<!doctype html><html lang="tr"><title>Sipariş</title>
<h1>Sipariş</h1>
<p>Kargo <b>yarın</b>   gelir.</p><p>Kargo yarın gelir.</p>
<a href="/çıkış">Çıkış</a>
<button>Gönder</button>
<label>Ad <input name="ad"></label>
<img src="/yok.png" alt="">
<a href="/boş"></a>
"#;

const HEDEF: &str = r#"<!doctype html><title>Hedef</title>
<button id="bir">Gönder</button>
<button id="iki">Gönder</button>
<div onclick="this.textContent='Eklendi'" style="cursor:pointer">Sepete ekle</div>
<span id="ışık">ışık</span>
"#;

const KAPLI: &str = r#"<!doctype html><title>Kaplı</title>
<style>#perde{position:fixed;inset:0;background:transparent}</style>
<button id="altta">Altta</button>
<div class="kart" style="position:relative;width:300px">
  <a id="başlık" href="/yazı">Yazı başlığı</a>
  <a id="örtü" href="/yazı" style="position:absolute;inset:0"></a>
</div>
<div id="perde"></div>
"#;

const ASAGIDA: &str = r#"<!doctype html><title>Aşağıda</title>
<div style="height:3000px"></div><button>En altta</button>"#;

fn open(c: &test_chrome::TestChrome, url: &str) -> Cdp {
    let tab = cdp::new_window(c.port, "about:blank").unwrap();
    let mut t = Cdp::connect(&tab.ws, Duration::from_secs(10)).unwrap();
    t.send("Page.enable", json!({})).unwrap();
    t.send("Page.navigate", json!({ "url": url })).unwrap();
    t.wait_event(Instant::now() + Duration::from_secs(10), &mut |e| {
        e["method"] == "Page.loadEventFired"
    })
    .unwrap();
    t
}

fn site() -> web::Web {
    web::serve(vec![
        web::html("/metin", METIN),
        web::html("/hedef", HEDEF),
        web::html("/kaplı", KAPLI),
        web::html("/aşağıda", ASAGIDA),
    ])
}

#[test]
fn the_page_reads_as_a_person_sees_it() {
    let (c, w) = (test_chrome::start(), site());
    let mut t = open(&c, &w.url("/metin"));
    let tree = t.send("Accessibility.getFullAXTree", json!({})).unwrap();
    let got = text::from_tree(tree["nodes"].as_array().unwrap());
    assert_eq!(
        got,
        // a paragraph is one line, its bold word in it; a field reads
        // with its label, as a person sees them
        "# Sipariş\nKargo yarın gelir.\n[Çıkış]\n[button: Gönder]\nAd [Ad: ___]"
    );
}

#[test]
fn targets_by_name_by_text_and_by_css() {
    let (c, w) = (test_chrome::start(), site());
    let mut t = open(&c, &w.url("/hedef"));
    let f = target::find(&mut t, &Target::Name("Gönder".into()))
        .unwrap()
        .unwrap();
    assert_eq!((f.count, f.text.as_str()), (2, "Gönder"));
    // the first in reading order
    let id = t
        .send("DOM.describeNode", json!({ "backendNodeId": f.backend }))
        .unwrap();
    assert!(id.to_string().contains("\"bir\""), "{id}");
    let f = target::find(&mut t, &Target::Name("Sepete ekle".into()))
        .unwrap()
        .unwrap();
    assert_eq!(f.text, "Sepete ekle");
    let f = target::find(&mut t, &Target::Css("#ışık".into()))
        .unwrap()
        .unwrap();
    assert_eq!(f.text, "ışık");
    assert!(
        target::find(&mut t, &Target::Name("Yok".into()))
            .unwrap()
            .is_none()
    );
    assert!(
        target::find(&mut t, &Target::Css(".yok".into()))
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_target_is_a_name_or_a_css_selector_and_nothing_else() {
    assert_eq!(
        Target::parse(&json!("Gönder")).unwrap(),
        Target::Name("Gönder".into())
    );
    assert_eq!(
        Target::parse(&json!({"css": "a"})).unwrap(),
        Target::Css("a".into())
    );
    for (v, says) in [
        (json!(""), "empty"),
        (json!(3), "a name or {\"css\""),
        (json!({"css": 3}), "css is a text"),
        (json!({"css": "a", "x": 1}), "only css"),
    ] {
        let e = Target::parse(&v).unwrap_err();
        assert!(e.contains(says), "{v}: {e}");
    }
}

#[test]
fn a_point_is_where_the_target_lies_uncovered() {
    let (c, w) = (test_chrome::start(), site());
    let mut t = open(&c, &w.url("/kaplı"));
    let f = target::find(&mut t, &Target::Css("#altta".into()))
        .unwrap()
        .unwrap();
    let e = target::point(&mut t, f.backend).unwrap_err();
    assert!(e.contains("perde"), "{e}");
    // an overlay link to the same place, as on Reddit's cards, is fine
    t.eval("document.getElementById('perde').remove()").unwrap();
    let f = target::find(&mut t, &Target::Name("Yazı başlığı".into()))
        .unwrap()
        .unwrap();
    target::point(&mut t, f.backend).unwrap();

    let mut t = open(&c, &w.url("/aşağıda"));
    let f = target::find(&mut t, &Target::Name("En altta".into()))
        .unwrap()
        .unwrap();
    let (_, y) = target::point(&mut t, f.backend).unwrap();
    let h = t.eval("innerHeight").unwrap().as_f64().unwrap();
    assert!(y > 0.0 && y < h, "{y} of {h}");
}
