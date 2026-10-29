//! `itero ai models list|add|remove`, with a stand-in for Hugging Face on
//! 127.0.0.1 (HF_ENDPOINT) and HOME and the cache in a temporary folder.
#![cfg(unix)]
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Hub {
    body: Arc<Vec<u8>>,
    sha: String,
    licence: &'static str,
    quants: Vec<&'static str>,
    /// Breaks the first whole download after this many bytes.
    break_after: Arc<Mutex<Option<usize>>>,
    ranges: Arc<Mutex<Vec<String>>>,
}

async fn info(State(h): State<Hub>) -> Response {
    axum::Json(json!({ "sha": "abc123", "cardData": { "license": h.licence } })).into_response()
}

async fn tree(State(h): State<Hub>) -> Response {
    let files: Vec<Value> = h
        .quants
        .iter()
        .map(|q| {
            json!({ "type": "file", "path": format!("Küçük-{q}.gguf"),
                         "size": h.body.len(), "lfs": { "oid": h.sha, "size": h.body.len() } })
        })
        .chain([
            json!({ "type": "file", "path": "mmproj-F16.gguf", "size": 1,
                        "lfs": { "oid": "00", "size": 1 } }),
        ])
        .collect();
    axum::Json(json!(files)).into_response()
}

async fn file(
    State(h): State<Hub>,
    headers: HeaderMap,
    UrlPath(_parts): UrlPath<(String, String, String)>,
) -> Response {
    if let Some(r) = headers.get("range").and_then(|v| v.to_str().ok()) {
        h.ranges.lock().unwrap().push(r.to_string());
        let from: usize = r
            .trim_start_matches("bytes=")
            .trim_end_matches('-')
            .parse()
            .unwrap();
        return (StatusCode::PARTIAL_CONTENT, h.body[from..].to_vec()).into_response();
    }
    if let Some(n) = h.break_after.lock().unwrap().take() {
        // a connection that breaks: the length promises more than comes
        let body = axum::body::Body::from_stream(breaking(h.body[..n].to_vec()));
        return Response::builder()
            .header("content-length", h.body.len())
            .body(body)
            .unwrap();
    }
    h.body.to_vec().into_response()
}

/// `v` in pieces of 16 KB, then a broken connection.
fn breaking(v: Vec<u8>) -> impl futures_core::Stream<Item = Result<Vec<u8>, std::io::Error>> {
    struct S(std::collections::VecDeque<Vec<u8>>, bool);
    impl futures_core::Stream for S {
        type Item = Result<Vec<u8>, std::io::Error>;
        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            if let Some(v) = self.0.pop_front() {
                return std::task::Poll::Ready(Some(Ok(v)));
            }
            if !self.1 {
                self.1 = true;
                return std::task::Poll::Ready(Some(Err(std::io::Error::other("koptu"))));
            }
            std::task::Poll::Ready(None)
        }
    }
    S(v.chunks(16 * 1024).map(<[u8]>::to_vec).collect(), false)
}

fn hub(
    licence: &'static str,
    quants: Vec<&'static str>,
    wrong_sha: bool,
    break_after: Option<usize>,
) -> (String, Hub) {
    // a real GGUF file: llama-cpp-2's vocabulary-only one
    let body =
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("itero-ai/tests/data/vocab.gguf"))
            .unwrap();
    let sha = if wrong_sha {
        "0".repeat(64)
    } else {
        Sha256::digest(&body)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    };
    let h = Hub {
        body: Arc::new(body),
        sha,
        licence,
        quants,
        break_after: Arc::new(Mutex::new(break_after)),
        ranges: Arc::new(Mutex::new(vec![])),
    };
    let app = axum::Router::new()
        .route("/api/models/{org}/{repo}", axum::routing::get(info))
        .route(
            "/api/models/{org}/{repo}/tree/{rev}",
            axum::routing::get(tree),
        )
        .route("/{org}/{repo}/resolve/{*rest}", axum::routing::get(file))
        .with_state(h.clone());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                tx.send(l.local_addr().unwrap()).unwrap();
                axum::serve(l, app).await.unwrap();
            });
    });
    (format!("http://{}", rx.recv().unwrap()), h)
}

struct Machine {
    d: tempfile::TempDir,
}

impl Machine {
    fn new() -> Machine {
        Machine {
            d: tempfile::tempdir().unwrap(),
        }
    }
    fn itero(&self, endpoint: &str, args: &[&str]) -> (bool, String) {
        let bin = Path::new(env!("CARGO_BIN_EXE_itero"));
        let out = Command::new(bin)
            .args(args)
            .current_dir(self.d.path())
            .env("HOME", self.d.path().join("ev"))
            .env("XDG_CACHE_HOME", self.d.path().join("önbellek"))
            .env("HF_ENDPOINT", endpoint)
            .env("HF_HUB_CACHE", self.d.path().join("hf"))
            .env("ITERO_AI_BIN", bin.with_file_name("itero-ai"))
            .env("ITERO_NO_SECRET_STORE", "1")
            .env_remove("ITERO_AI_SOCKET")
            .env_remove("HF_TOKEN")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        (out.status.success(), text)
    }
    fn downloaded(&self) -> PathBuf {
        self.d
            .path()
            .join("önbellek/itero/models/örnek/Küçük-GGUF/Küçük-Q4_K_M.gguf")
    }
    fn index(&self) -> itero_ai_index::Index {
        itero_ai_index::Index::read(&self.d.path().join("önbellek/itero/models.json")).unwrap()
    }
}

#[test]
fn add_downloads_resumes_and_checks_it() {
    let m = Machine::new();
    let (url, h) = hub("apache-2.0", vec!["Q3_K_M", "Q4_K_M"], false, Some(500_000));
    // the first try breaks after 500 KB (past what the server buffers before
    // it sends); the second resumes from there
    let (ok, out) = m.itero(&url, &["ai", "models", "add", "örnek/Küçük-GGUF", "--yes"]);
    assert!(!ok && out.contains("run it again to resume"), "{out}");
    let (ok, out) = m.itero(&url, &["ai", "models", "add", "örnek/Küçük-GGUF", "--yes"]);
    assert!(ok, "{out}");
    // one resume, from what came before the break (what the server's
    // buffering let through)
    let ranges = h.ranges.lock().unwrap().clone();
    assert_eq!(ranges.len(), 1, "{ranges:?}");
    let from: usize = ranges[0]
        .trim_start_matches("bytes=")
        .trim_end_matches('-')
        .parse()
        .unwrap();
    assert!(from > 0 && from < h.body.len(), "{ranges:?}");
    assert_eq!(std::fs::read(m.downloaded()).unwrap(), *h.body);
    assert!(out.contains("örnek/Küçük-GGUF:Q4_K_M: added"), "{out}");
    let ix = m.index();
    let e = ix
        .models
        .iter()
        .find(|e| e.repo == "örnek/Küçük-GGUF")
        .unwrap();
    let d = e.downloaded.as_ref().unwrap();
    assert_eq!(
        (d.revision.as_str(), d.sha256.as_str()),
        ("abc123", h.sha.as_str())
    );
    // added again: there already, nothing downloaded
    let (ok, out) = m.itero(&url, &["ai", "models", "add", "örnek/Küçük-GGUF", "--yes"]);
    assert!(ok && out.contains("on this machine already"), "{out}");
}

#[test]
fn a_wrong_sha_is_deleted_and_a_choice_is_named() {
    let m = Machine::new();
    let (url, _) = hub("apache-2.0", vec!["Q4_K_M"], true, None);
    let (ok, out) = m.itero(&url, &["ai", "models", "add", "örnek/Küçük-GGUF", "--yes"]);
    assert!(!ok && out.contains("does not match its SHA-256"), "{out}");
    assert!(!m.downloaded().exists());
    assert!(!m.downloaded().with_extension("gguf.part").exists());
    let (url, _) = hub("apache-2.0", vec!["Q3_K_M", "Q8_0"], false, None);
    let (ok, out) = m.itero(&url, &["ai", "models", "add", "örnek/Küçük-GGUF", "--yes"]);
    assert!(
        !ok && out.contains("Q3_K_M") && out.contains("Q8_0"),
        "{out}"
    );
}

#[test]
fn an_unsloth_dynamic_file_keeps_its_prefix() {
    let m = Machine::new();
    let (url, _) = hub("apache-2.0", vec!["Q4_K_M", "UD-Q4_K_XL"], false, None);
    let (ok, out) = m.itero(
        &url,
        &[
            "ai",
            "models",
            "add",
            "örnek/Küçük-GGUF:UD-Q4_K_XL",
            "--yes",
        ],
    );
    assert!(
        ok && out.contains("örnek/Küçük-GGUF:UD-Q4_K_XL: added"),
        "{out}"
    );
    assert!(
        m.downloaded()
            .with_file_name("Küçük-UD-Q4_K_XL.gguf")
            .exists()
    );
    assert!(!m.downloaded().exists());
}

#[test]
fn a_licence_is_warned_of_and_no_terminal_needs_yes() {
    let m = Machine::new();
    let (url, _) = hub("other", vec!["Q4_K_M"], false, None);
    let (ok, out) = m.itero(&url, &["ai", "models", "add", "örnek/Küçük-GGUF"]);
    assert!(
        !ok && out.contains("licence: other (itero advises MIT or Apache-2.0)"),
        "{out}"
    );
    assert!(out.contains("run it with --yes"), "{out}");
    assert!(!m.downloaded().exists());
}

#[test]
fn list_and_remove_and_delete_only_what_itero_downloaded() {
    let m = Machine::new();
    let vocab = Path::new(env!("CARGO_MANIFEST_DIR")).join("itero-ai/tests/data/vocab.gguf");
    let lm = m.d.path().join("ev/.lmstudio/models/örnek/Sözlük-GGUF");
    std::fs::create_dir_all(&lm).unwrap();
    std::fs::copy(&vocab, lm.join("Sözlük-F16.gguf")).unwrap();
    let (ok, out) = m.itero("http://127.0.0.1:9", &["ai", "models", "list"]);
    assert!(ok, "{out}");
    assert!(
        out.starts_with("MODEL") && out.contains("örnek/Sözlük-GGUF"),
        "{out}"
    );
    let (ok, out) = m.itero(
        "http://127.0.0.1:9",
        &["ai", "models", "remove", "örnek/Sözlük-GGUF", "--delete"],
    );
    assert!(!ok && out.contains("LM Studio's file"), "{out}");
    assert!(lm.join("Sözlük-F16.gguf").exists());
    let (ok, out) = m.itero(
        "http://127.0.0.1:9",
        &["ai", "models", "remove", "örnek/Sözlük-GGUF"],
    );
    assert!(ok, "{out}");
    assert!(m.index().models[0].removed.is_some());
    let (_, out) = m.itero("http://127.0.0.1:9", &["ai", "models", "list"]);
    assert!(out.starts_with("no models found yet"), "{out}");
    let (_, out) = m.itero("http://127.0.0.1:9", &["ai", "models", "list", "--all"]);
    assert!(out.contains("(removed)"), "{out}");
    // itero's own download is deleted with its entry
    let ours = m.d.path().join("önbellek/itero/models/örnek/Kendi-GGUF");
    std::fs::create_dir_all(&ours).unwrap();
    std::fs::copy(&vocab, ours.join("Kendi-F16.gguf")).unwrap();
    m.itero("http://127.0.0.1:9", &["ai", "models", "list"]);
    let mut ix = m.index();
    for e in ix
        .models
        .iter_mut()
        .filter(|e| e.repo == "örnek/Kendi-GGUF")
    {
        e.downloaded = Some(itero_ai_index::Downloaded {
            from: "x".into(),
            file: "Kendi-F16.gguf".into(),
            revision: "r".into(),
            sha256: "s".into(),
            at: "t".into(),
        });
    }
    ix.write(&m.d.path().join("önbellek/itero/models.json"))
        .unwrap();
    let (ok, out) = m.itero(
        "http://127.0.0.1:9",
        &["ai", "models", "remove", "örnek/Kendi-GGUF", "--delete"],
    );
    assert!(ok, "{out}");
    assert!(!ours.join("Kendi-F16.gguf").exists());
    assert!(
        m.index()
            .models
            .iter()
            .all(|e| e.repo != "örnek/Kendi-GGUF")
    );
}

#[test]
fn inspect_reads_one_files_header() {
    let m = Machine::new();
    let vocab = Path::new(env!("CARGO_MANIFEST_DIR")).join("itero-ai/tests/data/vocab.gguf");
    let (ok, out) = m.itero(
        "http://127.0.0.1:9",
        &["ai", "inspect", vocab.to_str().unwrap()],
    );
    assert!(ok, "{out}");
    assert!(
        out.contains("architecture: bert") && out.contains("chat: no"),
        "{out}"
    );
    let (ok, out) = m.itero("http://127.0.0.1:9", &["ai", "inspect", "yok.gguf"]);
    assert!(!ok && out.contains("yok.gguf"), "{out}");
}
