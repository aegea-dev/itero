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
    for path in ["/temiz", "/menü"] {
        let t = tab(c.port);
        let o = step(&t, json!({ "open": w.url(path) })).unwrap();
        assert!(o.get("blocked").is_none(), "{path}: {o}");
    }
    assert!(
        !notified(w.port)
            .iter()
            .any(|l| l.contains("/temiz") || l.contains("/men"))
    );
}
