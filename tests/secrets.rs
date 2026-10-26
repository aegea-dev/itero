//! `$$NAME` given both by a secret and by an environment variable: the
//! secret wins, and itero says so. The store is never touched here: which
//! names are kept where is given to the function that decides.
use itero::engine::secrets::{shadowing, used};

#[test]
fn the_names_a_workspace_uses_come_from_workflows_and_args() {
    let d = tempfile::tempdir().unwrap();
    let write = |rel: &str, text: &str| {
        let f = d.path().join(rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    };
    write(
        "itero.config.json",
        r#"{"action": {"gönder": {"args": [{"parola": "$$POSTA_PAROLA"}]}},
            "connector": {"posta.geldi": {"args": {"kullanıcı": {"ad": "$$POSTA_KULLANICI"}}}}}"#,
    );
    write(
        "workflows/kapı.json",
        r##"{ "on": [{ "event": "kapı.çal" }, { "has": "$$KAPI_VAR" }],
              "steps": [{ "#aç": { "args": { "anahtar": "$$KAPI_ANAHTARI" } } }] }"##,
    );
    let ws = itero::workspace::find(d.path()).unwrap();
    assert_eq!(
        used(&ws).unwrap(),
        [
            "KAPI_ANAHTARI",
            "KAPI_VAR",
            "POSTA_KULLANICI",
            "POSTA_PAROLA"
        ]
    );
}

#[test]
fn a_name_kept_in_both_places_is_shadowed() {
    let names: Vec<String> = ["İKİSİ", "SIR", "ORTAM", "HİÇ"].map(String::from).into();
    let in_store = |n: &str| ["İKİSİ", "SIR"].contains(&n);
    let in_env = |n: &str| ["İKİSİ", "ORTAM"].contains(&n);
    assert_eq!(shadowing(&names, in_store, in_env), ["İKİSİ"]);
}
