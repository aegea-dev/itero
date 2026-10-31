//! Secrets kept, listed, read and removed, in keyring's in-memory store
//! instead of the OS one: the Keychain is never touched. Its own test
//! binary with one test, since the store is set for the whole process.
//! macOS only: on Linux keyring's own start fails without a Secret
//! Service (in Docker, on CI), and no store can then be put in its place.
#![cfg(target_os = "macos")]
use itero::engine::secrets;

/// Puts the in-memory store in place of the OS one, and checks it is.
fn in_memory_store() {
    // keyring sets the OS store on first use: let it, then replace it
    keyring::Entry::store_status().as_ref().unwrap();
    keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
    let e = keyring::Entry::new("itero", "deneme").unwrap();
    assert!(
        e.inner
            .as_any()
            .downcast_ref::<keyring_core::mock::Cred>()
            .is_some(),
        "not the in-memory store: stopping before the Keychain is written"
    );
}

/// The next call on the store's entry `name` fails.
fn fail_next(name: &str) {
    let e = keyring::Entry::new("itero", name).unwrap();
    let cred: &keyring_core::mock::Cred = e.inner.as_any().downcast_ref().unwrap();
    cred.set_error(keyring_core::Error::NoStorageAccess(
        "kilitli".to_string().into(),
    ));
}

/// A workspace in a folder of its own, kept for the whole test, its
/// journal made.
fn workspace_with_journal() -> itero::workspace::Workspace {
    let d = tempfile::tempdir().unwrap().keep();
    std::fs::write(d.join("itero.config.json"), "{}").unwrap();
    let ws = itero::workspace::find(&d).unwrap();
    itero::engine::journal::Journal::open(&ws).unwrap();
    ws
}

#[test]
fn secrets_are_kept_listed_read_and_removed() {
    in_memory_store();
    // SAFETY: this binary's only test, before it starts any thread
    unsafe {
        std::env::remove_var("ITERO_NO_SECRET_STORE");
        std::env::set_var("ORTAM_SIRRI", "ortamdan");
    }
    assert_eq!(secrets::names().unwrap(), Vec::<String>::new());
    assert_eq!(secrets::get("PAROLA"), None);

    secrets::set("PAROLA", "çok gizli şifre").unwrap();
    secrets::set("JETON", "ğüşıöç").unwrap();
    secrets::set("PAROLA", "yeni şifre").unwrap();
    assert_eq!(secrets::names().unwrap(), ["JETON", "PAROLA"]);
    assert_eq!(secrets::get("PAROLA").as_deref(), Some("yeni şifre"));
    // not kept: from the environment
    assert_eq!(secrets::get("ORTAM_SIRRI").as_deref(), Some("ortamdan"));

    // kept and in the environment: the secret is used, and said
    secrets::set("ORTAM_SIRRI", "kasadan").unwrap();
    assert_eq!(secrets::get("ORTAM_SIRRI").as_deref(), Some("kasadan"));
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("itero.config.json"),
        r#"{"action": {"gönder": {"args": {"a": "$$ORTAM_SIRRI", "b": "$$PAROLA"}}}}"#,
    )
    .unwrap();
    let ws = itero::workspace::find(d.path()).unwrap();
    assert_eq!(secrets::shadowed(&ws).unwrap(), ["ORTAM_SIRRI"]);

    // removed: off the list; removing again says it was not there
    assert!(secrets::unset("ORTAM_SIRRI").unwrap());
    assert!(!secrets::unset("ORTAM_SIRRI").unwrap());
    assert_eq!(secrets::names().unwrap(), ["JETON", "PAROLA"]);
    assert_eq!(secrets::get("ORTAM_SIRRI").as_deref(), Some("ortamdan"));

    // a name that is no variable's is refused before the store is asked
    let bad = secrets::set("1ADI", "x").unwrap_err().to_string();
    assert!(bad.contains("a letter or _"), "{bad}");
    assert!(secrets::unset("ad-soyad").is_err());

    // a store that cannot be read: get falls back to the environment
    fail_next("ORTAM_SIRRI");
    assert_eq!(secrets::get("ORTAM_SIRRI").as_deref(), Some("ortamdan"));
    // ... and set, unset and names say why
    fail_next("YENI");
    let e = secrets::set("YENI", "x").unwrap_err().to_string();
    assert!(e.contains("the OS secret store"), "{e}");
    fail_next("PAROLA");
    let e = secrets::unset("PAROLA").unwrap_err().to_string();
    assert!(e.contains("the OS secret store"), "{e}");
    fail_next("itero.names");
    let e = secrets::names().unwrap_err().to_string();
    assert!(e.contains("the OS secret store"), "{e}");
    // a secret kept while the list cannot be written is kept, not listed
    fail_next("itero.names");
    let e = secrets::set("LISTESIZ", "x").unwrap_err().to_string();
    assert!(e.contains("is kept, but not listed"), "{e}");
    // a list that is not JSON is said to be unreadable
    keyring::Entry::new("itero", "itero.names")
        .unwrap()
        .set_password("bozuk")
        .unwrap();
    let e = secrets::names().unwrap_err().to_string();
    assert!(e.contains("unreadable"), "{e}");

    keyring::Entry::new("itero", "itero.names")
        .unwrap()
        .set_password("[]")
        .unwrap();

    // a workspace's own secret: before the machine's, for it alone, and
    // under its id, so a new name keeps it
    let bir = workspace_with_journal();
    let iki = workspace_with_journal();
    std::fs::write(bir.root.join(".env"), "POSTA=dosyadan\n").unwrap();
    assert_eq!(secrets::get_in(&bir, "POSTA").as_deref(), Some("dosyadan"));
    secrets::set("POSTA", "makineden").unwrap();
    assert_eq!(secrets::get_in(&bir, "POSTA").as_deref(), Some("makineden"));
    secrets::set_for(&bir.id().unwrap(), "POSTA", "çalışma alanından").unwrap();
    assert_eq!(
        secrets::get_in(&bir, "POSTA").as_deref(),
        Some("çalışma alanından")
    );
    assert_eq!(secrets::get_in(&iki, "POSTA").as_deref(), Some("makineden"));
    assert_eq!(secrets::get("POSTA").as_deref(), Some("makineden"));
    itero::engine::journal::Journal::open(&bir)
        .unwrap()
        .rename_workspace("yeni-ad")
        .unwrap();
    let bir_again = itero::workspace::find(&bir.root).unwrap();
    assert_eq!(
        secrets::get_in(&bir_again, "POSTA").as_deref(),
        Some("çalışma alanından")
    );
    assert!(
        secrets::names()
            .unwrap()
            .contains(&format!("{}.POSTA", bir.id().unwrap()))
    );
    assert!(secrets::set_for(&bir.id().unwrap(), "ad-soyad", "x").is_err());
    // the warning sees a workspace's own secret too
    secrets::set_for(&bir.id().unwrap(), "ORTAM_SIRRI", "kendi").unwrap();
    std::fs::write(
        bir.root.join("itero.config.json"),
        r#"{"action": {"gönder": {"args": {"a": "$$ORTAM_SIRRI"}}}}"#,
    )
    .unwrap();
    let bir_used = itero::workspace::find(&bir.root).unwrap();
    assert_eq!(secrets::shadowed(&bir_used).unwrap(), ["ORTAM_SIRRI"]);

    // with the store off, nothing is kept or listed, and nothing shadows
    // SAFETY: as above, no other thread
    unsafe { std::env::set_var("ITERO_NO_SECRET_STORE", "1") };
    assert!(secrets::names().unwrap_err().to_string().contains("off"));
    assert!(secrets::set("PAROLA", "x").is_err());
    assert_eq!(secrets::shadowed(&ws).unwrap(), Vec::<String>::new());
    assert_eq!(secrets::get("PAROLA"), None);
}
