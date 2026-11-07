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
        "responses goes with open, wait, click, key or inspect",
        "responses is a text",
    ] {
        assert!(e.contains(says), "{says}: {e}");
    }
}

const HOME: &str = r#"<!doctype html><html lang="tr"><head><title>Işık Dükkânı</title>
<meta name="description" content="Lambalar ve ışıklar">
<link rel="canonical" href="https://dukkan.example/">
<meta property="og:title" content="Işık">
<meta name="twitter:card" content="summary">
<script type="application/ld+json">{"@type": "Store", "name": "Işık"}</script>
<link rel="stylesheet" href="/stil.css">
<script src="/uygulama.js"></script></head>
<body><h1>Işık</h1><script>console.error("hata"); console.warn("uyarı"); console.log("sessiz")</script></body></html>"#;

fn site() -> web::Web {
    let mut home = web::html("/", HOME);
    home.headers.extend([
        ("X-Deneme".into(), "ışık".into()),
        (
            "Set-Cookie".into(),
            "oturum=gizli123; HttpOnly; Path=/".into(),
        ),
        ("Set-Cookie".into(), "tema=koyu; Path=/".into()),
    ]);
    web::serve(vec![
        web::Page {
            path: "/eski".into(),
            status: 301,
            headers: vec![("Location".into(), "/".into())],
            body: String::new(),
        },
        home,
        web::file("/stil.css", "text/css", "h1 { color: #E57373 }"),
        web::file(
            "/uygulama.js",
            "text/javascript",
            "window.jQuery = { fn: { jquery: '3.7.1' } };",
        ),
        web::file(
            "/robots.txt",
            "text/plain",
            "User-agent: *\nDisallow: /gizli\n",
        ),
        web::file(
            "/sitemap.xml",
            "application/xml",
            "<urlset><url><loc>/a</loc></url><url><loc>/b</loc></url><url><loc>/c</loc></url></urlset>",
        ),
        web::file("/llms.txt", "text/plain", "# Işık Dükkânı\n"),
    ])
}

#[test]
fn inspect_gives_a_pages_facts() {
    let (c, w) = (test_chrome::start(), site());
    let t = tab(c.port);
    let o = step(&t, json!({ "inspect": w.url("/eski") })).unwrap();
    assert_eq!(o["url"], w.url("/"), "{o}");
    let i = &o["inspect"];

    let d = &i["document"];
    assert_eq!(d["status"], 200, "{d}");
    assert_eq!(d["redirects"], json!([w.url("/eski")]), "{d}");
    assert_eq!(d["response_headers"]["X-Deneme"], "ışık", "{d}");
    assert!(d["request_headers"].is_object(), "{d}");

    let files = i["files"].as_array().unwrap();
    for (path, kind) in [
        ("/", "document"),
        ("/stil.css", "stylesheet"),
        ("/uygulama.js", "script"),
    ] {
        let f = files
            .iter()
            .find(|f| f["url"] == w.url(path))
            .unwrap_or_else(|| panic!("{path} not in {files:?}"));
        assert_eq!(f["type"], kind, "{f}");
        assert_eq!(f["status"], 200, "{f}");
        assert!(f["size"].as_u64().unwrap() > 0, "{f}");
        assert!(f["ms"].is_number(), "{f}");
        assert!(f["mime"].is_string(), "{f}");
    }

    let cookies = i["cookies"].as_array().unwrap();
    let named = |n: &str| {
        cookies
            .iter()
            .find(|c| c["name"] == n)
            .unwrap_or_else(|| panic!("{n}: {cookies:?}"))
    };
    assert_eq!(named("oturum")["http_only"], true);
    assert_eq!(named("tema")["http_only"], false);
    assert!(
        cookies.iter().all(|c| c.get("value").is_none()),
        "{cookies:?}"
    );
    assert_eq!(named("tema")["third_party"], false);

    let m = &i["meta"];
    assert_eq!(m["title"], "Işık Dükkânı");
    assert_eq!(m["description"], "Lambalar ve ışıklar");
    assert_eq!(m["canonical"], "https://dukkan.example/");
    assert_eq!(m["language"], "tr");
    assert_eq!(m["og"]["title"], "Işık");
    assert_eq!(m["twitter"]["card"], "summary");
    assert_eq!(m["json_ld"], json!([{"@type": "Store", "name": "Işık"}]));

    assert_eq!(
        i["libraries"],
        json!([{"name": "jQuery", "version": "3.7.1"}])
    );
    assert_eq!(i["tls"], json!(null));
    for k in [
        "first_byte_ms",
        "dom_ready_ms",
        "load_ms",
        "first_contentful_paint_ms",
    ] {
        assert!(i["timing"][k].is_number(), "{k}: {}", i["timing"]);
    }
    let console = i["console"].as_array().unwrap();
    assert!(
        console
            .iter()
            .any(|c| c["level"] == "error" && c["text"] == "hata"),
        "{console:?}"
    );
    assert!(
        console
            .iter()
            .any(|c| c["level"] == "warning" && c["text"] == "uyarı"),
        "{console:?}"
    );
    assert!(
        !console.iter().any(|c| c["text"] == "sessiz"),
        "{console:?}"
    );

    let s = &i["site"];
    assert_eq!(s["robots"]["found"], true, "{s}");
    assert_eq!(s["robots"]["text"], "User-agent: *\nDisallow: /gizli\n");
    assert_eq!(s["sitemap"]["urls"], 3, "{s}");
    assert_eq!(s["llms"]["found"], true, "{s}");

    // the values only when asked for
    let t = tab(c.port);
    let o = step(&t, json!({ "inspect": w.url("/"), "cookie_values": true })).unwrap();
    let cookies = o["inspect"]["cookies"].as_array().unwrap();
    assert!(
        cookies
            .iter()
            .any(|c| c["name"] == "oturum" && c["value"] == "gizli123"),
        "{cookies:?}"
    );
}

#[test]
fn a_site_with_none_of_its_files_says_so() {
    let (c, w) = (
        test_chrome::start(),
        web::serve(vec![web::html("/", "<title>Boş</title>")]),
    );
    let t = tab(c.port);
    let o = step(&t, json!({ "inspect": w.url("/") })).unwrap();
    let s = &o["inspect"]["site"];
    for f in ["robots", "sitemap", "llms"] {
        assert_eq!(s[f]["found"], false, "{f}: {s}");
    }
    assert_eq!(o["inspect"]["libraries"], json!([]));
}

#[test]
fn inspect_args_are_checked() {
    let e = itero::browser::step::check(&json!({ "inspect": "ftp://x", "cookie_values": "evet" }))
        .unwrap_err();
    for says in [
        "inspect is an http or https address",
        "cookie_values is true or false",
    ] {
        assert!(e.contains(says), "{says}: {e}");
    }
    let e = itero::browser::step::check(&json!({ "open": "https://a.b", "cookie_values": true }))
        .unwrap_err();
    assert!(e.contains("cookie_values goes with inspect"), "{e}");
}
