//! Secrets: the values `${env.NAME}` stands for. They are kept in the OS
//! secret store (macOS Keychain, Windows Credential Manager, the Secret
//! Service on Linux) under the service `itero`, and looked up there first,
//! then in the daemon's own environment. itero never writes one to a file.
use anyhow::{Result, anyhow, bail};

const SERVICE: &str = "itero";
/// The names of the secrets itero keeps: stores cannot list their entries.
const INDEX: &str = "itero.names";

/// A secret's name is an environment variable's: a letter or `_`, then
/// letters, digits or `_`.
pub fn valid_name(name: &str) -> bool {
    let mut cs = name.chars();
    cs.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `ITERO_NO_SECRET_STORE` keeps itero away from the OS secret store.
pub fn store_off() -> bool {
    std::env::var_os("ITERO_NO_SECRET_STORE").is_some()
}

fn entry(name: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, name).map_err(|e| anyhow!("the OS secret store: {e}"))
}

/// Whether a secret can be kept or removed: a valid name, and the store on.
pub fn ready(name: &str) -> Result<()> {
    if !valid_name(name) {
        bail!("{name}: a secret's name is a letter or _, then letters, digits or _");
    }
    if store_off() {
        bail!("the OS secret store is off (ITERO_NO_SECRET_STORE is set)");
    }
    Ok(())
}

/// A secret's value from the store: none when the store has no such entry,
/// an error when the store cannot be read (locked, denied, not running).
fn lookup(name: &str) -> keyring::Result<Option<String>> {
    match keyring::Entry::new(SERVICE, name)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e),
    }
}

/// A secret's value: from the store, else from the environment. A store
/// that cannot be read is reported once (to stderr, the daemon's log) and
/// then left aside, so a machine without one still runs on its environment.
pub fn get(name: &str) -> Option<String> {
    if !store_off() {
        match lookup(name) {
            Ok(Some(v)) => return Some(v),
            Ok(None) => {}
            Err(e) => {
                static REPORTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
                if REPORTED.set(()).is_ok() {
                    eprintln!(
                        "itero: the OS secret store cannot be read ({e}); secrets come from the environment"
                    );
                }
            }
        }
    }
    std::env::var(name).ok()
}

/// The names of the secrets in the store, sorted.
pub fn names() -> Result<Vec<String>> {
    if store_off() {
        bail!("the OS secret store is off (ITERO_NO_SECRET_STORE is set)");
    }
    match entry(INDEX)?.get_password() {
        Ok(text) => serde_json::from_str(&text).map_err(|e| {
            anyhow!("the list of secret names ({SERVICE}/{INDEX}) is unreadable: {e}")
        }),
        Err(keyring::Error::NoEntry) => Ok(vec![]),
        Err(e) => Err(anyhow!("the OS secret store: {e}")),
    }
}

fn save_names(mut names: Vec<String>) -> Result<()> {
    names.sort();
    names.dedup();
    entry(INDEX)?
        .set_password(&serde_json::to_string(&names)?)
        .map_err(|e| anyhow!("the OS secret store: {e}"))
}

/// Keeps a secret in the store, then adds its name to the list. If the list
/// cannot be written, the secret is kept but not listed, and the error says
/// so; setting it again lists it. Two `set`s at the very same moment could
/// each miss the other's name in the list: secrets are set by a person, one
/// at a time, so the list takes no lock.
pub fn set(name: &str, value: &str) -> Result<()> {
    ready(name)?;
    entry(name)?
        .set_password(value)
        .map_err(|e| anyhow!("the OS secret store: {e}"))?;
    let listed = names().and_then(|mut all| {
        all.push(name.to_string());
        save_names(all)
    });
    listed.map_err(|e| anyhow!("{name} is kept, but not listed: {e:#}"))
}

/// Removes a secret from the store; false when it was not there.
pub fn unset(name: &str) -> Result<bool> {
    ready(name)?;
    let had = match entry(name)?.delete_credential() {
        Ok(()) => true,
        Err(keyring::Error::NoEntry) => false,
        Err(e) => return Err(anyhow!("the OS secret store: {e}")),
    };
    let all: Vec<String> = names()?.into_iter().filter(|n| n != name).collect();
    save_names(all)?;
    Ok(had)
}
