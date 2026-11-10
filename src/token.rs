//! The tokens a server lets in: `itero token create` shows one once and
//! keeps only its SHA-256 hash, with its name, in the state folder's
//! `tokens.json`. A token reaches the whole server.
use anyhow::{Context as _, Result, bail};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What every token starts with: a leaked one is easy to find.
pub const PREFIX: &str = "itero_";

/// A token as kept: never the token itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    pub name: String,
    pub id: String,
    /// SHA-256 of the token, hex.
    pub hash: String,
    pub created: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    tokens: Vec<Kept>,
}

/// `<state folder>/tokens.json`.
pub fn path() -> Result<PathBuf> {
    Ok(crate::machine::state_dir()?.join("tokens.json"))
}

fn read(p: &Path) -> Result<File> {
    match std::fs::read_to_string(p) {
        Ok(t) => serde_json::from_str(&t).with_context(|| p.display().to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(File::default()),
        Err(e) => Err(e).with_context(|| p.display().to_string()),
    }
}

fn write(p: &Path, f: &File) -> Result<()> {
    crate::machine::write_private(p, &(serde_json::to_string_pretty(f)? + "\n"))
}

/// SHA-256 of `token`, hex.
fn hash(token: &str) -> String {
    ring::digest::digest(&ring::digest::SHA256, token.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A new token named `name`, kept as its hash: its text, to show once.
pub fn create(name: &str) -> Result<String> {
    if !crate::names::plain(name) {
        bail!("{name}: {}", crate::names::PLAIN);
    }
    let p = path()?;
    let mut f = read(&p)?;
    if f.tokens.iter().any(|t| t.name == name) {
        bail!("a token named {name} is there: itero token revoke {name} first, or another name");
    }
    let mut bytes = [0u8; 32];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes)
        .map_err(|_| anyhow::anyhow!("no random bytes from the system"))?;
    let token = format!(
        "{PREFIX}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    );
    f.tokens.push(Kept {
        name: name.to_string(),
        id: uuid::Uuid::now_v7().to_string(),
        hash: hash(&token),
        created: crate::engine::daemon::stamp(chrono::Utc::now()),
    });
    write(&p, &f)?;
    Ok(token)
}

/// The tokens kept, in the order they were made.
pub fn list() -> Result<Vec<Kept>> {
    Ok(read(&path()?)?.tokens)
}

/// Takes the token named `name` out: it lets nobody in from then on.
pub fn revoke(name: &str) -> Result<()> {
    let p = path()?;
    let mut f = read(&p)?;
    let before = f.tokens.len();
    f.tokens.retain(|t| t.name != name);
    if f.tokens.len() == before {
        bail!("no token {name}: itero token list lists them");
    }
    write(&p, &f)
}

/// The name of the token `presented` is, if it is one, as `tokens.json`
/// says now: read at each check (a few hundred bytes), so a token revoked
/// lets nobody in at the next request.
pub fn who(presented: &str) -> Result<Option<String>> {
    Ok(find(&read(&path()?)?.tokens, presented))
}

/// Whether no token is kept: an opened server would let nobody in.
pub fn none() -> Result<bool> {
    Ok(read(&path()?)?.tokens.is_empty())
}

/// The kept token whose hash is `presented`'s. Every kept hash is
/// compared, every byte of each, whichever matches. What is compared is a
/// SHA-256 of 32 random bytes, so even a timing difference would tell
/// nothing that helps find a token; the fold keeps it from stopping early
/// all the same.
fn find(kept: &[Kept], presented: &str) -> Option<String> {
    let h = hash(presented);
    let mut found = None;
    for t in kept {
        let differ = h.len() != t.hash.len()
            || h.bytes()
                .zip(t.hash.bytes())
                .fold(0u8, |d, (a, b)| d | (a ^ b))
                != 0;
        if !differ {
            found = Some(t.name.clone());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(name: &str, token: &str) -> Kept {
        Kept {
            name: name.into(),
            id: "1".into(),
            hash: hash(token),
            created: "2026-11-10T00:00:00Z".into(),
        }
    }

    #[test]
    fn the_hash_is_sha256_hex() {
        assert_eq!(
            hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_token_is_found_by_its_hash_and_no_other_is() {
        let kept = vec![kept("dizüstü", "itero_bir"), kept("sunucu", "itero_iki")];
        assert_eq!(find(&kept, "itero_iki").as_deref(), Some("sunucu"));
        assert_eq!(find(&kept, "itero_bir").as_deref(), Some("dizüstü"));
        assert_eq!(find(&kept, "itero_yanlış"), None);
        assert_eq!(find(&[], "itero_bir"), None);
    }
}
