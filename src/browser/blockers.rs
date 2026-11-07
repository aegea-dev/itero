//! What stands between a step and its page. The cookie banners of common
//! consent tools are rejected, as a person would click "reject"; any other
//! banner is shown to the user, and the step goes on.
use super::cdp::Cdp;
use super::{page, target};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// A consent tool: `shown` is there and visible while its banner is up;
/// `reject` is the button that says no, when itero may click it. A part
/// in a shadow root is written `host >>> inner`.
pub struct Tool {
    pub name: &'static str,
    pub shown: &'static str,
    pub reject: Option<&'static str>,
}

/// The tools itero knows, each checked on a live site that uses it on
/// 2026-11-07: onetrust.com, cookiebot.com, didomi.io, usercentrics.com,
/// google.de, reddit.com, theguardian.com. These change: a tool whose
/// button is no longer there is shown to the user as an unknown banner.
pub const CONSENT: [Tool; 7] = [
    Tool {
        name: "OneTrust",
        shown: "#onetrust-reject-all-handler",
        reject: Some("#onetrust-reject-all-handler"),
    },
    Tool {
        name: "Cookiebot",
        shown: "#CybotCookiebotDialogBodyButtonDecline",
        reject: Some("#CybotCookiebotDialogBodyButtonDecline"),
    },
    Tool {
        name: "Didomi",
        shown: "#didomi-notice-disagree-button",
        reject: Some("#didomi-notice-disagree-button"),
    },
    Tool {
        name: "Usercentrics",
        shown: "#usercentrics-cmp-ui >>> button.uc-deny-button",
        reject: Some("#usercentrics-cmp-ui >>> button.uc-deny-button"),
    },
    Tool {
        name: "Google",
        shown: "#W0wltc",
        reject: Some("#W0wltc"),
    },
    Tool {
        name: "Reddit",
        shown: "#data-protection-consent-dialog button[slot=\"secondary-button\"]",
        reject: Some("#data-protection-consent-dialog button[slot=\"secondary-button\"]"),
    },
    // its "reject" is each publisher's own button in a frame, and at times
    // a paid subscription: never clicked
    Tool {
        name: "Sourcepoint",
        shown: "iframe[id^=\"sp_message_iframe\"]",
        reject: None,
    },
];

/// How long a rejected banner is given to go.
const GOES: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq)]
pub enum Blocker {
    /// A cookie banner: of a known tool (`tool`), or not; `cleared` when
    /// itero rejected it.
    Consent {
        tool: Option<&'static str>,
        cleared: bool,
    },
}

/// The page's helpers: an element by a `CONSENT` selector, shadow roots
/// included, and whether it is visible.
const FIND: &str = "const find = s => { let root = document, el = null;
  for (const part of s.split(' >>> ')) { el = root.querySelector(part); if (!el) return null; root = el.shadowRoot || el; }
  return el; };
const shown = el => { if (!el) return false; const r = el.getBoundingClientRect(), st = getComputedStyle(el);
  return r.width > 0 && r.height > 0 && st.visibility !== 'hidden' && st.display !== 'none'; };";

/// Words of a cookie banner, in the languages a banner is most often in.
const WORDS: &str = "cookie|çerez|consent|einwilligung|consentement|galleta|privacidad";

/// Words of its answers: a banner asks for one, a page about cookies
/// (a consent tool's own site) does not.
const ANSWERS: &str = "accept|agree|allow|reject|decline|deny|refuse|got it|ok|okay|kabul|reddet|tamam|izin ver|akzeptieren|zustimmen|ablehnen|einverstanden|accepter|refuser|aceptar|rechazar|accetta|rifiuta";

/// Which of `CONSENT` shows a banner (its index), else whether another
/// banner covers part of the page: an element fixed over it, or a dialog,
/// with words of cookies and a button that answers them.
fn look(cdp: &mut Cdp) -> Result<Value, String> {
    let shown: Vec<&str> = CONSENT.iter().map(|t| t.shown).collect();
    cdp.eval(&format!(
        "(() => {{ {FIND}
  const tools = {tools};
  const i = tools.findIndex(s => shown(find(s)));
  if (i >= 0) return {{ tool: i }};
  const words = /{WORDS}/i, answers = /\\b({ANSWERS})\\b/i;
  const answer = el => [...el.querySelectorAll('button,[role=button],a[href],input[type=submit],input[type=button]')]
    .some(b => {{ const t = b.innerText || b.value || ''; return t.length < 60 && answers.test(t); }});
  for (const el of document.querySelectorAll('body *')) {{
    const st = getComputedStyle(el);
    const over = st.position === 'fixed' || st.position === 'sticky' || el.matches('[role=dialog],[aria-modal=true],dialog[open]');
    if (!over || !shown(el)) continue;
    const text = el.innerText || '';
    if (text.length < 2000 && words.test(text) && answer(el))
      return {{ banner: true }};
  }}
  return {{}};
}})()",
        tools = json!(shown)
    ))
}

/// The page's blockers, a known tool's banner rejected first; a page the
/// reject sends elsewhere is waited for until `until`.
pub fn check(cdp: &mut Cdp, until: Instant) -> Result<Vec<Blocker>, String> {
    let seen = look(cdp)?;
    if seen["banner"] == true {
        return Ok(vec![Blocker::Consent {
            tool: None,
            cleared: false,
        }]);
    }
    let Some(i) = seen["tool"].as_u64() else {
        return Ok(vec![]);
    };
    let tool = &CONSENT[i as usize];
    let cleared = match tool.reject {
        Some(reject) => reject_with(cdp, tool.shown, reject, until)?,
        None => false,
    };
    Ok(vec![Blocker::Consent {
        tool: Some(tool.name),
        cleared,
    }])
}

/// Clicks `reject` as a person would, and waits for `shown` to go; false
/// when it cannot be clicked or stays.
fn reject_with(cdp: &mut Cdp, shown: &str, reject: &str, until: Instant) -> Result<bool, String> {
    let r = cdp.send(
        "Runtime.evaluate",
        json!({ "expression": format!("(() => {{ {FIND} return find({}); }})()", json!(reject)) }),
    )?;
    let Some(id) = r["result"]["objectId"].as_str() else {
        return Ok(false);
    };
    let d = cdp.send("DOM.describeNode", json!({ "objectId": id }))?;
    let Some(backend) = d["node"]["backendNodeId"].as_i64() else {
        return Ok(false);
    };
    let Ok((x, y)) = target::point(cdp, backend) else {
        return Ok(false);
    };
    page::click(cdp, x, y)?;
    // some send the page again, with the answer kept
    page::follow(cdp, until)?;
    let end = Instant::now() + GOES;
    let still = format!(
        "(() => {{ {FIND} return shown(find({})); }})()",
        json!(shown)
    );
    while Instant::now() < end {
        if cdp.eval(&still)? == false {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(false)
}
