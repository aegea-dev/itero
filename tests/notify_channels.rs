//! `#notify` on a machine with `daemon.json`'s `notify` channels: each
//! format POSTed, a channel's secret url never shown, every channel
//! tried, and what a channel cannot do refused. Its own test binary: it
//! sets `ITERO_SECRETS` for the whole process.
#![cfg(unix)]
#[path = "common/web.rs"]
mod web;
use itero::engine::worker::{self, Outcome, StepRun};
use serde_json::{Map, Value, json};
use std::path::PathBuf;

fn step(root: &std::path::Path, channels: &Value, args: &[(&str, &str)]) -> StepRun {
    let mut env: std::collections::BTreeMap<String, String> = args
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    env.insert("ITERO_NOTIFY_CHANNELS".into(), channels.to_string());
    StepRun {
        file: PathBuf::from(worker::NOTIFY),
        root: root.to_path_buf(),
        workflow: "sipariş".into(),
        env,
        input: Map::new(),
        memory: root.join("hafıza"),
        waiting_since: None,
        quiet: 10,
        most: 10,
        now: String::new(),
    }
}

fn posted(w: &web::Web, path: &str) -> Vec<Value> {
    w.bodies
        .lock()
        .unwrap()
        .iter()
        .filter(|(p, _)| p == path)
        .map(|(_, b)| serde_json::from_str(b).unwrap())
        .collect()
}

#[test]
fn notify_sends_to_each_channel_and_says_which_did_not_take_it() {
    let d = tempfile::tempdir().unwrap();
    let ok = |p: &str| web::file(p, "text/plain", "tamam");
    let mut broken = ok("/bozuk-gizli");
    broken.status = 500;
    let w = web::serve(vec![
        ok("/kanca"),
        ok("/ntfy/"),
        ok("/slack"),
        ok("/discord"),
        ok("/gizli-yol"),
        broken,
    ]);
    // the machine's secrets folder, as itero daemon passes it on
    let secrets = d.path().join("sırlar");
    std::fs::create_dir_all(&secrets).unwrap();
    std::fs::write(secrets.join("KANAL_ADRESI"), w.url("/gizli-yol") + "\n").unwrap();
    std::fs::write(secrets.join("BOZUK_KANAL"), w.url("/bozuk-gizli")).unwrap();
    // SAFETY: the only test in this binary, before any thread reads it.
    // A notification that went to the desktop instead would land in the
    // log, never on the screen of the machine running the tests
    let shown = d.path().join("gösterilen.log");
    unsafe {
        std::env::set_var("ITERO_SECRETS", &secrets);
        std::env::set_var("ITERO_NOTIFY_LOG", &shown);
    }
    let args = [
        ("title", "Sipariş"),
        ("subtitle", "depo 2"),
        ("text", "43 kargoda"),
        ("url", "https://örnek.test/s/43"),
        ("sound", "Basso"),
    ];

    // every format, and a url from a secret
    let all = json!([
        { "url": w.url("/kanca"), "format": "json" },
        { "url": w.url("/ntfy/uyarılar"), "format": "ntfy" },
        { "url": w.url("/slack"), "format": "slack" },
        { "url": w.url("/discord"), "format": "discord" },
        { "url": "$$KANAL_ADRESI", "format": "json" },
    ]);
    assert_eq!(
        worker::run_step(&step(d.path(), &all, &args)).unwrap(),
        Outcome::Done(json!(""))
    );
    assert_eq!(
        posted(&w, "/kanca"),
        vec![
            json!({"title": "Sipariş", "subtitle": "depo 2", "text": "43 kargoda",
                    "url": "https://örnek.test/s/43", "workflow": "sipariş"})
        ]
    );
    assert_eq!(posted(&w, "/ntfy/")[0]["topic"], "uyarılar");
    assert_eq!(
        posted(&w, "/slack")[0]["text"],
        "*Sipariş*\ndepo 2\n43 kargoda\nhttps://örnek.test/s/43"
    );
    assert_eq!(posted(&w, "/discord")[0]["embeds"][0]["title"], "Sipariş");
    assert_eq!(posted(&w, "/gizli-yol").len(), 1, "the secret's url");

    // a channel answering 500, a secret one too, and one nobody listens
    // on: the one that works still gets it; each other named by its place
    // and host, never its path
    let gone = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let nobody = format!(
        "http://127.0.0.1:{}/kimse",
        gone.local_addr().unwrap().port()
    );
    drop(gone);
    let mixed = json!([
        { "url": "$$BOZUK_KANAL" },
        { "url": w.url("/kanca") },
        { "url": nobody },
    ]);
    let e = worker::run_step(&step(d.path(), &mixed, &args))
        .unwrap_err()
        .to_string();
    assert_eq!(posted(&w, "/kanca").len(), 2, "the working one: {e}");
    assert!(e.contains("notify[0] (127.0.0.1): 500"), "{e}");
    assert!(e.contains("notify[2] (127.0.0.1): "), "{e}");
    assert!(!e.contains("notify[1]"), "{e}");
    for path in ["bozuk-gizli", "kimse", "gizli-yol"] {
        assert!(!e.contains(path), "{path} in {e}");
    }

    // a secret not there
    let e = worker::run_step(&step(d.path(), &json!([{ "url": "$$YOK" }]), &args))
        .unwrap_err()
        .to_string();
    assert!(e.contains("notify[0]: no secret YOK"), "{e}");

    // waiting for an answer: refused, nothing sent
    let before = w.bodies.lock().unwrap().len();
    for extra in [("buttons", r#"["Evet", "Hayır"]"#), ("wait", "true")] {
        let mut a = args.to_vec();
        a.push(extra);
        let e = worker::run_step(&step(d.path(), &json!([{ "url": w.url("/kanca") }]), &a))
            .unwrap_err()
            .to_string();
        assert!(e.contains("waits for an answer"), "{extra:?}: {e}");
    }
    assert_eq!(w.bodies.lock().unwrap().len(), before, "nothing sent");

    // a url to open that is not http
    let mut a = args.to_vec();
    a[3] = ("url", "file:///etc/passwd");
    let e = worker::run_step(&step(d.path(), &json!([{ "url": w.url("/kanca") }]), &a))
        .unwrap_err()
        .to_string();
    assert!(e.contains("http or https"), "{e}");

    assert!(!shown.exists(), "nothing went to the desktop");

    // the daemon's own mistake, said
    let mut s = step(d.path(), &json!([]), &args);
    s.env.insert("ITERO_NOTIFY_CHANNELS".into(), "{".into());
    let e = worker::run_step(&s).unwrap_err().to_string();
    assert!(e.contains("ITERO_NOTIFY_CHANNELS"), "{e}");
}
