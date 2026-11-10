//! Secrets: the values `$$NAME` stands for. They are kept in the OS secret
//! store (macOS Keychain, Windows Credential Manager, the Secret Service on
//! Linux) under the service `itero`: for every workspace under the name
//! itself, for one workspace under `<its id>.<name>` (a `.` is never in a
//! name, so the two cannot clash). itero never writes one to a file.
use crate::workspace::Workspace;
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

/// A secret's value: from the store, else from the environment.
pub fn get(name: &str) -> Option<String> {
    from_store(name).or_else(|| std::env::var(name).ok())
}

/// A `$$NAME` in the workspace: its own secret, else the machine's, else
/// its `.env`, else the secrets folder's (its own file, then the
/// machine's), else the environment.
pub fn get_in(ws: &Workspace, name: &str) -> Option<String> {
    own_key(ws, name)
        .and_then(|key| from_store(&key))
        .or_else(|| from_store(name))
        .or_else(|| dot_env(&ws.root, name))
        .or_else(|| from_folder(ws, name))
        .or_else(|| std::env::var(name).ok())
}

/// The variable the machine's daemon names its secrets folder in
/// (server.json's `secrets`), for itself and every process it starts.
pub const FOLDER: &str = "ITERO_SECRETS";

/// A secret from the folder of secrets the platform mounts, a file each:
/// the workspace's own (`<its name on the list>.NAME`), else `NAME`; its
/// last newline left out. A file there that cannot be read is said once.
fn from_folder(ws: &Workspace, name: &str) -> Option<String> {
    let dir = std::path::PathBuf::from(std::env::var_os(FOLDER)?);
    let listed = crate::machine::list_path()
        .and_then(|p| crate::machine::List::read(&p))
        .ok()
        .and_then(|l| {
            l.workspaces
                .into_iter()
                .find(|e| e.folder == ws.root)
                .map(|e| e.name)
        });
    let files = listed
        .map(|w| dir.join(format!("{w}.{name}")))
        .into_iter()
        .chain([dir.join(name)]);
    for f in files {
        match std::fs::read_to_string(&f) {
            Ok(t) => {
                let t = t.strip_suffix('\n').unwrap_or(&t);
                return Some(t.strip_suffix('\r').unwrap_or(t).to_string());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                static REPORTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
                if REPORTED.set(()).is_ok() {
                    eprintln!("itero: {}: {e}", f.display());
                }
            }
        }
    }
    None
}

/// What a server without a store answers to a secret sent to it.
pub fn none_kept(name: &str) -> String {
    format!(
        "this server keeps no secrets: set {name} in its environment, or a file {name} in its secrets folder (server.json's secrets)"
    )
}

/// A workspace's own secret's name in the store.
fn key_for(id: &str, name: &str) -> String {
    format!("{id}.{name}")
}

/// `key_for` the workspace; none with the store off. A workspace whose id
/// cannot be read is reported once (each `$$` lookup asks again): its own
/// secrets are passed over.
fn own_key(ws: &Workspace, name: &str) -> Option<String> {
    if store_off() {
        return None;
    }
    match ws.id() {
        Ok(id) => Some(key_for(&id, name)),
        Err(e) => {
            static REPORTED: std::sync::Mutex<std::collections::BTreeSet<std::path::PathBuf>> =
                std::sync::Mutex::new(std::collections::BTreeSet::new());
            let first = REPORTED
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(ws.root.clone());
            if first {
                eprintln!(
                    "itero: {}: its own secrets cannot be read: {e:#}",
                    ws.root.display()
                );
            }
            None
        }
    }
}

/// A secret from the store. A store that cannot be read is reported once
/// (to stderr, the daemon's log) and then left aside, so a machine without
/// one still runs on `.env` and its environment.
fn from_store(name: &str) -> Option<String> {
    if store_off() {
        return None;
    }
    match lookup(name) {
        Ok(v) => v,
        Err(e) => {
            static REPORTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
            if REPORTED.set(()).is_ok() {
                eprintln!(
                    "itero: the OS secret store cannot be read ({e}); values come from .env and the environment"
                );
            }
            None
        }
    }
}

/// A value of the workspace's `.env`: `NAME=value` lines, the user's own
/// values (never a secret's place). `#` starts a comment line; a value in
/// quotes is the text between them.
pub fn dot_env(root: &std::path::Path, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(root.join(".env")).ok()?;
    text.lines().find_map(|l| {
        let l = l.trim();
        let (k, v) = l.strip_prefix("export ").unwrap_or(l).split_once('=')?;
        if l.starts_with('#') || k.trim() != name {
            return None;
        }
        let v = v.trim();
        let unquoted = ['"', '\'']
            .iter()
            .find_map(|q| v.strip_prefix(*q).and_then(|s| s.strip_suffix(*q)));
        Some(unquoted.unwrap_or(v).to_string())
    })
}

/// The `$$` names a workspace uses: in its workflows' steps and
/// conditions, and in its actions' and connectors' args. Sorted, once each.
pub fn used(ws: &Workspace) -> Result<Vec<String>> {
    fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
            serde_json::Value::Object(m) => m.values().for_each(|i| walk(i, out)),
            v => out.extend(crate::flow::secret_ref(v).map(str::to_string)),
        }
    }
    let mut out = vec![];
    for (_, f) in ws.flows()? {
        out.extend(f.secret_names());
        out.extend(f.defined_names());
        // the args its on entries give their connectors
        for e in &f.on.events {
            walk(&serde_json::Value::Object(e.args.clone()), &mut out);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Of `names`, those both a secret (`in_store`) and an environment
/// variable (`in_env`): `$$NAME` takes the secret, and the variable is
/// passed over, which a person may not expect.
pub fn shadowing(
    names: &[String],
    in_store: impl Fn(&str) -> bool,
    in_env: impl Fn(&str) -> bool,
) -> Vec<String> {
    names
        .iter()
        .filter(|n| in_store(n) && in_env(n))
        .cloned()
        .collect()
}

/// The names the workspace uses that are both a secret (its own or the
/// machine's) and an environment variable here; none with the store off. A
/// store that cannot be read counts as holding none.
pub fn shadowed(ws: &Workspace) -> Result<Vec<String>> {
    if store_off() {
        return Ok(vec![]);
    }
    Ok(shadowing(
        &used(ws)?,
        |n| {
            let kept = |key: &str| matches!(lookup(key), Ok(Some(_)));
            kept(n) || own_key(ws, n).is_some_and(|key| kept(&key))
        },
        |n| std::env::var_os(n).is_some(),
    ))
}

/// What itero says of a name `shadowed` found.
pub fn shadow_warning(name: &str) -> String {
    format!("$${name} is a secret and an environment variable: the secret is used")
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
    keep(name, value)
}

/// `set`, for the workspace with this id alone.
pub fn set_for(id: &str, name: &str, value: &str) -> Result<()> {
    ready(name)?;
    keep(&key_for(id, name), value)
}

fn keep(key: &str, value: &str) -> Result<()> {
    entry(key)?
        .set_password(value)
        .map_err(|e| anyhow!("the OS secret store: {e}"))?;
    let listed = names().and_then(|mut all| {
        all.push(key.to_string());
        save_names(all)
    });
    listed.map_err(|e| anyhow!("{key} is kept, but not listed: {e:#}"))
}

/// Removes a secret from the store; false when it was not there.
pub fn unset(name: &str) -> Result<bool> {
    ready(name)?;
    forget(name)
}

/// `unset`, for the workspace with this id alone.
pub fn unset_for(id: &str, name: &str) -> Result<bool> {
    ready(name)?;
    forget(&key_for(id, name))
}

fn forget(key: &str) -> Result<bool> {
    let had = match entry(key)?.delete_credential() {
        Ok(()) => true,
        Err(keyring::Error::NoEntry) => false,
        Err(e) => return Err(anyhow!("the OS secret store: {e}")),
    };
    let all: Vec<String> = names()?.into_iter().filter(|n| n != key).collect();
    save_names(all)?;
    Ok(had)
}
