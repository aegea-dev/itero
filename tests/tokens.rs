//! `itero token create`, `list` and `revoke`: the tokens a server lets
//! in, each shown once and kept as its hash in the state folder's
//! `tokens.json`. Every command runs with HOME, XDG_CONFIG_HOME and
//! XDG_STATE_HOME in a temporary folder.
#![cfg(unix)]
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

struct Machine {
    d: tempfile::TempDir,
}

impl Machine {
    fn new() -> Machine {
        Machine {
            d: tempfile::tempdir().unwrap(),
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_itero"))
            .current_dir(self.d.path())
            .env("HOME", self.d.path().join("ev"))
            .env("XDG_CONFIG_HOME", self.d.path().join("ayar"))
            .env("XDG_STATE_HOME", self.d.path().join("durum"))
            .stdin(Stdio::null())
            .args(args)
            .output()
            .unwrap()
    }

    /// `itero <args>`, which must succeed: its stdout.
    fn ok(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).into_owned()
    }

    /// `itero <args>`, which must fail: what it says.
    fn fails(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert!(!o.status.success(), "{args:?} succeeded");
        String::from_utf8_lossy(&o.stderr).into_owned()
    }

    fn file(&self) -> PathBuf {
        self.d.path().join("durum/itero/tokens.json")
    }
}

#[test]
fn create_shows_the_token_once_and_keeps_only_its_hash() {
    let m = Machine::new();
    let token = m.ok(&["token", "create", "dizüstü"]);
    let token = token.trim();
    assert!(token.starts_with("itero_"), "{token}");
    assert_eq!(
        token.len(),
        "itero_".len() + 43,
        "32 bytes, base64url: {token}"
    );
    let kept = std::fs::read_to_string(m.file()).unwrap();
    assert!(!kept.contains(token), "the token itself is never kept");
    let v: Value = serde_json::from_str(&kept).unwrap();
    let t = &v["tokens"][0];
    assert_eq!(t["name"], "dizüstü", "{v}");
    assert_eq!(t["hash"].as_str().unwrap().len(), 64, "SHA-256, hex: {v}");
    let mode = std::fs::metadata(m.file()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "{mode:o}");
    // a second one is another token
    let other = m.ok(&["token", "create", "sunucu"]);
    assert_ne!(other.trim(), token);
}

#[test]
fn list_shows_names_never_a_token_or_a_hash() {
    let m = Machine::new();
    m.ok(&["token", "create", "bir"]);
    m.ok(&["token", "create", "iki"]);
    let v: Value = serde_json::from_str(&m.ok(&["token", "list", "--json"])).unwrap();
    let names: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["bir", "iki"]);
    for t in v.as_array().unwrap() {
        assert!(t.get("hash").is_none() && t.get("token").is_none(), "{v}");
        assert!(t["created"].is_string(), "{v}");
    }
    let text = m.ok(&["token", "list"]);
    assert!(text.contains("bir") && text.contains("iki"), "{text}");
}

#[test]
fn a_name_taken_or_not_plain_is_refused() {
    let m = Machine::new();
    m.ok(&["token", "create", "bir"]);
    let said = m.fails(&["token", "create", "bir"]);
    assert!(said.contains("a token named bir is there"), "{said}");
    let said = m.fails(&["token", "create", "a b"]);
    assert!(
        said.contains("a name is letters, digits, - and _"),
        "{said}"
    );
}

#[test]
fn revoke_takes_it_out() {
    let m = Machine::new();
    m.ok(&["token", "create", "bir"]);
    m.ok(&["token", "create", "iki"]);
    let said = m.ok(&["token", "revoke", "bir"]);
    assert!(said.contains("revoked bir"), "{said}");
    let v: Value = serde_json::from_str(&m.ok(&["token", "list", "--json"])).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1, "{v}");
    assert_eq!(v[0]["name"], "iki");
    let said = m.fails(&["token", "revoke", "bir"]);
    assert!(said.contains("no token bir"), "{said}");
}

#[test]
fn a_token_is_made_on_the_server_never_over_a_context() {
    let m = Machine::new();
    let said = m.fails(&["token", "create", "bir", "--context", "uzak"]);
    assert!(said.contains("works on this machine"), "{said}");
}
