//! Blockers: cookie banners rejected or shown to the user, human checks.
//! Each fixture is shaped as the consent tool's live markup.
#[path = "common/browser.rs"]
mod browser;
#[path = "common/chrome.rs"]
mod test_chrome;
#[path = "common/web.rs"]
mod web;
use browser::{logged, step, tab};
use serde_json::json;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Every notification of this binary goes to one file; each test knows
/// its own by its server's address.
fn notified(port: u16) -> Vec<String> {
    let text = std::fs::read_to_string(notify_log()).unwrap_or_default();
    let host = format!("127.0.0.1:{port}");
    text.lines()
        .filter(|l| l.contains(&host))
        .map(String::from)
        .collect()
}

fn notify_log() -> &'static PathBuf {
    static LOG: OnceLock<PathBuf> = OnceLock::new();
    LOG.get_or_init(|| {
        let d = tempfile::tempdir().unwrap().keep();
        let log = d.join("bildirimler.log");
        // SAFETY: set once, before any step of this binary shows one
        unsafe { std::env::set_var("ITERO_NOTIFY_LOG", &log) };
        log
    })
}

const PAGE: &str = "<!doctype html><title>Haber</title><h1>Haber</h1><p>Bugün hava ışıklı.</p>";

/// A banner fixed over the page's bottom; its reject button removes it.
fn banner(inner: &str, remove: &str) -> String {
    format!(
        r#"{PAGE}{inner}<script>document.addEventListener('click', e => {{
  if (e.target.matches('.ret')) document.querySelector('{remove}').remove() }})</script>"#
    )
}

const FIXED: &str = "position:fixed;left:0;right:0;bottom:0;height:200px;background:#fff";

#[test]
fn the_banners_of_common_consent_tools_are_rejected_and_logged() {
    notify_log();
    let tools = [
        (
            "OneTrust",
            banner(
                &format!(
                    r#"<div id="onetrust-consent-sdk"><div id="onetrust-banner-sdk" style="{FIXED}">
<p>Çerezleri kullanıyoruz</p><button id="onetrust-accept-btn-handler">Tümünü kabul et</button>
<button id="onetrust-reject-all-handler" class="ret">Yalnızca gerekli olanlar</button></div></div>"#
                ),
                "#onetrust-consent-sdk",
            ),
        ),
        (
            "Cookiebot",
            banner(
                &format!(
                    r#"<div id="CybotCookiebotDialog" style="{FIXED}"><p>Bu site çerez kullanır</p>
<button id="CybotCookiebotDialogBodyButtonDecline" class="ret">Reddet</button>
<button id="CybotCookiebotDialogBodyLevelButtonLevelOptinAllowAll">İzin ver</button></div>"#
                ),
                "#CybotCookiebotDialog",
            ),
        ),
        (
            "Didomi",
            banner(
                &format!(
                    r#"<div id="didomi-host"><div id="didomi-popup" style="{FIXED}"><p>Çerezlere izin veriyor musunuz?</p>
<button id="didomi-notice-agree-button">Kabul ediyorum</button>
<button id="didomi-notice-disagree-button" class="ret">Kabul etmiyorum</button></div></div>"#
                ),
                "#didomi-host",
            ),
        ),
        (
            "Usercentrics",
            format!(
                r#"{PAGE}<aside id="usercentrics-cmp-ui"></aside><script>
const host = document.getElementById('usercentrics-cmp-ui');
const root = host.attachShadow({{ mode: 'open' }});
root.innerHTML = '<div style="{FIXED}"><p>Çerez ayarları</p><button id="accept" class="accept uc-accept-button">Kabul et</button><button id="deny" class="deny uc-deny-button">Reddet</button></div>';
root.getElementById('deny').addEventListener('click', () => host.remove());</script>"#
            ),
        ),
        (
            "Google",
            banner(
                &format!(
                    r#"<div id="google" role="dialog" style="{FIXED}"><p>Google'a devam etmeden önce</p>
<button id="W0wltc" class="ret">Tümünü reddet</button><button id="L2AGLb">Tümünü kabul et</button></div>"#
                ),
                "#google",
            ),
        ),
        (
            "Reddit",
            banner(
                &format!(
                    r#"<shreddit-app><div id="data-protection-consent-dialog" style="{FIXED}"><rpl-modal-card>
<p>Çerez bildirimi</p><button slot="primary-button">Tümünü kabul et</button>
<button slot="secondary-button" class="ret">İsteğe bağlı çerezleri reddet</button></rpl-modal-card></div></shreddit-app>"#
                ),
                "#data-protection-consent-dialog",
            ),
        ),
    ];
    let pages: Vec<web::Page> = tools
        .iter()
        .map(|(tool, html)| web::html(&format!("/{tool}"), html))
        .collect();
    let (c, w) = (test_chrome::start(), web::serve(pages));
    for (tool, _) in &tools {
        let t = tab(c.port);
        let o = step(&t, json!({ "open": w.url(&format!("/{tool}")) })).unwrap();
        assert!(o.get("blocked").is_none(), "{tool}: {o}");
        let text = step(&t, json!({ "read": "text" })).unwrap()["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(text, "# Haber\nBugün hava ışıklı.", "{tool}");
        let lines = logged(&t, "browser.consent");
        assert_eq!(lines.len(), 1, "{tool}: {lines:?}");
        assert_eq!(lines[0]["tool"], *tool);
        assert_eq!(lines[0]["workflow"], "tarayıcı");
    }
    assert_eq!(notified(w.port), Vec::<String>::new());
}

#[test]
fn a_banner_itero_cannot_reject_is_notified_and_said_in_the_output() {
    notify_log();
    let unknown = format!(
        r#"{PAGE}<div id="çerez" style="{FIXED}"><p>Çerezleri kullanıyoruz</p><button>Tümünü kabul et</button></div>"#
    );
    // Sourcepoint's reject is the publisher's own, at times a paid
    // subscription: never clicked
    let sourcepoint = format!(
        r#"{PAGE}<iframe id="sp_message_iframe_1482251" title="SP Consent Message" src="about:blank" style="{FIXED};border:0"></iframe>"#
    );
    let (c, w) = (
        test_chrome::start(),
        web::serve(vec![
            web::html("/bilinmeyen", &unknown),
            web::html("/sourcepoint", &sourcepoint),
            web::html("/temiz", PAGE),
            // a banner already answered, slid out of the view: not on top
            web::html(
                "/kapalı",
                &format!(
                    r#"{PAGE}<div style="{FIXED};transform:translateY(300%)"><p>Çerezleri kullanıyoruz</p><button>Kabul et</button></div>"#
                ),
            ),
            // a consent tool's own site: its sticky menu speaks of cookies,
            // with no answer to give
            web::html(
                "/menü",
                &format!(
                    r#"{PAGE}<nav style="position:sticky;top:0"><p>Çerez yönetimi platformu</p><a href="/x">Daha fazla</a> <button>Demo iste</button></nav>"#
                ),
            ),
        ]),
    );
    for (path, tool) in [("/bilinmeyen", None), ("/sourcepoint", Some("Sourcepoint"))] {
        let t = tab(c.port);
        let o = step(&t, json!({ "open": w.url(path) })).unwrap();
        assert_eq!(o["blocked"], json!(["consent"]), "{path}: {o}");
        assert!(logged(&t, "browser.consent").is_empty(), "{path}");
        let n = notified(w.port);
        let shown = n.iter().filter(|l| l.contains(&w.url(path))).count();
        assert_eq!(shown, 1, "{path}: {n:?}");
        if let Some(tool) = tool {
            assert!(n.iter().any(|l| l.contains(tool)), "{n:?}");
        }
    }
    for path in ["/temiz", "/menü", "/kapalı"] {
        let t = tab(c.port);
        let o = step(&t, json!({ "open": w.url(path) })).unwrap();
        assert!(o.get("blocked").is_none(), "{path}: {o}");
    }
    assert!(
        !notified(w.port)
            .iter()
            .any(|l| l.contains("/temiz") || l.contains("/men") || l.contains("/kapal"))
    );
}

/// A page with a human check's frame, pointing where a real one does: the
/// test Chrome resolves no host but 127.0.0.1, so nothing is fetched.
fn frame(src: &str, style: &str) -> String {
    format!(r#"<iframe src="{src}" title="reCAPTCHA" style="{style}"></iframe>"#)
}

const POPUP: &str = "https://www.google.com/recaptcha/api2/bframe?hl=tr&k=deneme";
const SHOWN: &str = "position:fixed;top:50px;left:50px;width:400px;height:580px;border:0";

#[test]
fn a_human_check_fails_a_headless_step_at_once_and_is_notified() {
    notify_log();
    let (c, w) = (
        test_chrome::start(),
        web::serve(vec![
            web::html(
                "/insan",
                "<!doctype html><title>Prove your humanity</title><p>Devam etmek için doğrulayın</p>",
            ),
            web::html("/açılır", &format!("{PAGE}{}", frame(POPUP, SHOWN))),
            web::html("/bağlantı", &format!(r#"{PAGE}<a href="/insan">Giriş</a>"#)),
        ]),
    );
    for path in ["/insan", "/açılır"] {
        let t = tab(c.port);
        let started = std::time::Instant::now();
        let e = step(&t, json!({ "open": w.url(path) })).unwrap_err();
        assert!(started.elapsed().as_secs() < 10, "{path}: at once");
        assert!(e.contains("cannot be solved headless"), "{path}: {e}");
        assert!(e.contains(&format!("127.0.0.1:{}", w.port)), "{e}");
    }
    // after a click too
    let t = tab(c.port);
    step(&t, json!({ "open": w.url("/bağlantı") })).unwrap();
    let e = step(&t, json!({ "click": "Giriş" })).unwrap_err();
    assert!(e.contains("cannot be solved headless"), "{e}");
    let n = notified(w.port);
    assert_eq!(n.len(), 3, "{n:?}");
    assert!(n.iter().all(|l| l.contains("human check")), "{n:?}");
}

#[test]
fn a_human_check_in_a_window_waits_until_it_is_gone() {
    notify_log();
    let page = format!("{PAGE}{}", frame(POPUP, SHOWN));
    let (c, w) = (
        test_chrome::start(),
        web::serve(vec![web::html("/bekle", &page)]),
    );
    let t = tab(c.port);
    let said = std::sync::Mutex::new(vec![]);
    let o = browser::step_as(&t, "window", json!({ "open": w.url("/bekle") }), &|s| {
        if let Some(s) = s {
            said.lock().unwrap().push(s.to_string());
            // once the step waits, the check goes, as when a person has
            // solved it
            let mut page =
                itero::browser::cdp::Cdp::connect(&t.ws, std::time::Duration::from_secs(5))
                    .unwrap();
            page.eval("document.querySelector('iframe')?.remove(), 1")
                .unwrap();
        }
    })
    .unwrap();
    assert!(o.get("blocked").is_none(), "{o}");
    let said = said.into_inner().unwrap();
    assert!(
        said.iter().any(|s| s.contains(&format!(
            "waiting for a human check on 127.0.0.1:{}",
            w.port
        ))),
        "{said:?}"
    );
    let n = notified(w.port);
    assert_eq!(n.len(), 1, "{n:?}");
    assert!(n[0].contains("human check"), "{n:?}");
}

#[test]
fn a_check_that_sits_in_a_form_is_said_and_the_step_goes_on() {
    notify_log();
    let widgets = [
        (
            "reCAPTCHA",
            "https://www.google.com/recaptcha/api2/anchor?ar=1&k=deneme&size=normal",
        ),
        (
            "hCaptcha",
            "https://newassets.hcaptcha.com/captcha/v1/abc/static/hcaptcha.html#frame=checkbox&id=0",
        ),
        (
            "Turnstile",
            "https://challenges.cloudflare.com/cdn-cgi/challenge-platform/h/b/turnstile/if/ov2/av0/rcv0/0/abc/light/normal",
        ),
    ];
    let pages: Vec<web::Page> = widgets
        .iter()
        .map(|(kind, src)| {
            web::html(
                &format!("/{kind}"),
                &format!(
                    r#"{PAGE}<form><label>E-posta <input name="e"></label>{}<button>Gönder</button></form>"#,
                    frame(src, "width:300px;height:78px;border:0")
                ),
            )
        })
        .chain([web::html(
            "/görünmez",
            &format!(
                "{PAGE}{}",
                frame(
                    "https://www.google.com/recaptcha/api2/anchor?ar=1&k=deneme&size=invisible",
                    "width:256px;height:60px;border:0"
                )
            ),
        )])
        .collect();
    let (c, w) = (test_chrome::start(), web::serve(pages));
    for (kind, _) in widgets {
        let t = tab(c.port);
        let o = step(&t, json!({ "open": w.url(&format!("/{kind}")) })).unwrap();
        assert_eq!(o["captcha"], kind, "{o}");
    }
    let t = tab(c.port);
    let o = step(&t, json!({ "open": w.url("/görünmez") })).unwrap();
    assert!(o.get("captcha").is_none(), "{o}");
    assert_eq!(notified(w.port), Vec::<String>::new());
}
