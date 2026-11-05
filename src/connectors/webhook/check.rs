//! The checks a webhook's request passes, each named on its `--hook`, never
//! guessed from the request, so a sender cannot pick a weaker one. Every
//! comparison goes through `ring`, in constant time.
use base64::Engine;
use ring::hmac;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Check {
    /// `Authorization: Bearer <secret>`.
    Token,
    /// `X-Hub-Signature-256: sha256=<hex>`, HMAC-SHA256 of the raw body:
    /// GitHub's and Meta's.
    Hmac,
    /// `X-Twilio-Signature: <base64>`, HMAC-SHA1 of the URL, then each
    /// form field's name and value, sorted by name.
    Twilio,
}

impl Check {
    pub fn named(name: &str) -> Option<Check> {
        match name {
            "token" => Some(Check::Token),
            "hmac" => Some(Check::Hmac),
            "twilio" => Some(Check::Twilio),
            _ => None,
        }
    }

    /// The header holding what is checked. Never saved with the event.
    pub fn header(self) -> &'static str {
        match self {
            Check::Token => "authorization",
            Check::Hmac => "x-hub-signature-256",
            Check::Twilio => "x-twilio-signature",
        }
    }
}

/// Whether a request passes: `given` its `Check::header`, `url` the URL it
/// was sent to (only Twilio signs it), `body` its raw body.
pub fn passes(check: Check, secret: &str, given: Option<&str>, url: &str, body: &[u8]) -> bool {
    let Some(given) = given else { return false };
    match check {
        Check::Token => {
            // both as their HMACs: ring compares those in constant time
            let Some(token) = given.strip_prefix("Bearer ") else {
                return false;
            };
            let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
            let want = hmac::sign(&key, secret.as_bytes());
            hmac::verify(&key, token.as_bytes(), want.as_ref()).is_ok()
        }
        Check::Hmac => {
            let Some(tag) = given.strip_prefix("sha256=").and_then(from_hex) else {
                return false;
            };
            let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
            hmac::verify(&key, body, &tag).is_ok()
        }
        Check::Twilio => {
            let engine = base64::engine::general_purpose::STANDARD;
            let Ok(tag) = engine.decode(given) else {
                return false;
            };
            let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret.as_bytes());
            hmac::verify(&key, twilio_text(url, body).as_bytes(), &tag).is_ok()
        }
    }
}

fn twilio_text(url: &str, body: &[u8]) -> String {
    let mut fields: Vec<(String, String)> = form_urlencoded::parse(body).into_owned().collect();
    fields.sort();
    let mut text = url.to_string();
    for (name, value) in fields {
        text.push_str(&name);
        text.push_str(&value);
    }
    text
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.is_ascii() || s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}
