//! What stands between a step and its page. The cookie banners of common
//! consent tools are rejected, as a person would click "reject"; any other
//! banner is shown to the user, and the step goes on. A human check in the
//! way, a challenge page or a challenge over the page, is for a person to
//! solve; one that only sits in a form is said, not waited for.
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

/// The titles of challenge pages: a site's whole page is the check.
pub const CHALLENGE_TITLES: [&str; 3] = [
    // Reddit's
    "Prove your humanity",
    // Cloudflare's
    "Just a moment...",
    "Attention Required! | Cloudflare",
];

#[derive(Debug, Clone, PartialEq)]
pub enum Blocker {
    /// A cookie banner: of a known tool (`tool`), or not; `cleared` when
    /// itero rejected it.
    Consent {
        tool: Option<&'static str>,
        cleared: bool,
    },
    /// A human check (`kind`: reCAPTCHA, hCaptcha, Turnstile, or a
    /// challenge page); `in_the_way` when a person must solve it before
    /// the page can be used, not when it only sits in a form.
    Captcha { kind: String, in_the_way: bool },
}

/// The human check on the page, by its frames' addresses (never their
/// content, from another site) or the page's title: `{kind, in_the_way}`,
/// or null. A challenge is in the way when its frame is on top of the
/// page; a widget only sits in it.
const HUMAN: &str = "const human = () => {
  if (TITLES.some(t => document.title.trim().startsWith(t))) return { kind: 'challenge page', in_the_way: true };
  const recaptcha = /^https:\\/\\/(www\\.)?(google\\.com|recaptcha\\.net)\\/recaptcha\\//,
    hcaptcha = /^https:\\/\\/([a-z0-9-]+\\.)?hcaptcha\\.com\\//,
    turnstile = /^https:\\/\\/challenges\\.cloudflare\\.com\\//;
  let widget = null;
  for (const f of onTop().filter(e => e.tagName === 'IFRAME')) {
    const src = f.src || '';
    if (recaptcha.test(src) && /\\/bframe/.test(src)) return { kind: 'reCAPTCHA', in_the_way: true };
    if (hcaptcha.test(src) && /frame=challenge/.test(src)) return { kind: 'hCaptcha', in_the_way: true };
  }
  for (const f of document.querySelectorAll('iframe')) {
    const src = f.src || '';
    if (!shown(f)) continue;
    if (recaptcha.test(src) && /\\/anchor/.test(src) && !/size=invisible/.test(src)) widget = widget || 'reCAPTCHA';
    if (hcaptcha.test(src) && /frame=checkbox/.test(src)) widget = widget || 'hCaptcha';
    if (turnstile.test(src)) widget = widget || 'Turnstile';
  }
  return widget && { kind: widget, in_the_way: false };
};";

/// The page's helpers: an element by a `CONSENT` selector, shadow roots
/// included; whether it is visible; and what lies on top of the page,
/// asked at points across the view (its edges and corners too, where
/// banners sit), each hit taken up to what holds it over the page (an
/// element fixed or sticky, a dialog), or a frame. What is on top is what
/// a person sees, whatever the z-indexes say.
const FIND: &str = "const find = s => { let root = document, el = null;
  for (const part of s.split(' >>> ')) { el = root.querySelector(part); if (!el) return null; root = el.shadowRoot || el; }
  return el; };
const shown = el => { if (!el) return false; const r = el.getBoundingClientRect(), st = getComputedStyle(el);
  return r.width > 0 && r.height > 0 && st.visibility !== 'hidden' && st.display !== 'none'; };
const onTop = () => { const found = new Set(), at = [0.03, 0.25, 0.5, 0.75, 0.97];
  for (const fx of at) for (const fy of at) {
    const x = innerWidth * fx, y = innerHeight * fy;
    let h = document.elementFromPoint(x, y);
    while (h && h.shadowRoot) { const d = h.shadowRoot.elementFromPoint(x, y); if (!d || d === h) break; h = d; }
    if (!h) continue;
    if (h.tagName === 'IFRAME') { found.add(h); continue; }
    for (let e = h; e && e !== document.body && e !== document.documentElement; e = e.parentElement || e.getRootNode().host) {
      const st = getComputedStyle(e);
      if (st.position === 'fixed' || st.position === 'sticky' || e.matches('[role=dialog],[aria-modal=true],dialog[open]')) { found.add(e); break; }
    }
  }
  return [...found]; };";

/// Words of a cookie banner, in the languages a banner is most often in.
const WORDS: &str = "cookie|çerez|consent|einwilligung|consentement|galleta|privacidad";

/// Words of its answers: a banner asks for one, a page about cookies
/// (a consent tool's own site) does not.
const ANSWERS: &str = "accept|agree|allow|reject|decline|deny|refuse|got it|ok|okay|kabul|reddet|tamam|izin ver|akzeptieren|zustimmen|ablehnen|einverstanden|accepter|refuser|aceptar|rechazar|accetta|rifiuta";

/// The human check on the page (`captcha`, as `HUMAN` says); unless it is
/// in the way, which of `CONSENT` shows a banner (its index), else whether
/// another banner is on top of the page (`FIND`'s `onTop`), with words of
/// cookies and a button that answers them.
fn look(cdp: &mut Cdp) -> Result<Value, String> {
    let shown: Vec<&str> = CONSENT.iter().map(|t| t.shown).collect();
    cdp.eval(&format!(
        "(() => {{ {FIND}
  const TITLES = {titles};
  {HUMAN}
  const captcha = human();
  if (captcha && captcha.in_the_way) return {{ captcha }};
  const found = r => Object.assign(r, captcha ? {{ captcha }} : {{}});
  const tools = {tools};
  const i = tools.findIndex(s => shown(find(s)));
  if (i >= 0) return found({{ tool: i }});
  const words = /{WORDS}/i, answers = /\\b({ANSWERS})\\b/i;
  const answer = el => [...el.querySelectorAll('button,[role=button],a[href],input[type=submit],input[type=button]')]
    .some(b => {{ const t = b.innerText || b.value || ''; return t.length < 60 && answers.test(t); }});
  for (const el of onTop()) {{
    const text = el.tagName === 'IFRAME' ? '' : (el.innerText || '');
    if (text.length < 2000 && words.test(text) && answer(el))
      return found({{ banner: true }});
  }}
  return found({{}});
}})()",
        titles = json!(CHALLENGE_TITLES),
        tools = json!(shown)
    ))
}

/// The human check in the way on the page now, if any: its kind.
pub fn human_check(cdp: &mut Cdp) -> Result<Option<String>, String> {
    let v = cdp.eval(&format!(
        "(() => {{ {FIND}
  const TITLES = {titles};
  {HUMAN}
  const c = human();
  return c && c.in_the_way ? c.kind : null;
}})()",
        titles = json!(CHALLENGE_TITLES)
    ))?;
    Ok(v.as_str().map(String::from))
}

fn captcha(seen: &Value) -> Option<Blocker> {
    let c = &seen["captcha"];
    Some(Blocker::Captcha {
        kind: c["kind"].as_str()?.into(),
        in_the_way: c["in_the_way"] == true,
    })
}

/// The page's blockers: a human check in the way alone, since nothing
/// else can be done before it is solved; else a known tool's banner,
/// rejected first (a page the reject sends elsewhere is waited for until
/// `until`), or another banner, and a check that sits in a form.
pub fn check(cdp: &mut Cdp, until: Instant) -> Result<Vec<Blocker>, String> {
    let seen = look(cdp)?;
    let mut found: Vec<Blocker> = captcha(&seen).into_iter().collect();
    if found.iter().any(|b| {
        matches!(
            b,
            Blocker::Captcha {
                in_the_way: true,
                ..
            }
        )
    }) {
        return Ok(found);
    }
    if seen["banner"] == true {
        found.push(Blocker::Consent {
            tool: None,
            cleared: false,
        });
    } else if let Some(i) = seen["tool"].as_u64() {
        let tool = &CONSENT[i as usize];
        let cleared = match tool.reject {
            Some(reject) => reject_with(cdp, tool.shown, reject, until)?,
            None => false,
        };
        found.push(Blocker::Consent {
            tool: Some(tool.name),
            cleared,
        });
    }
    Ok(found)
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
