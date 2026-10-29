//! The built-in `#find-models`: the GGUF files already on the machine, in
//! LM Studio's folder, Hugging Face's cache and itero's own downloads,
//! named by where they are, and written to the index itero-ai reads. Each
//! new or changed file's header is read by itero-ai (`/itero/inspect`).
use crate::engine::worker::{Failure, Outcome, StepRun};
use itero_ai_index::{Entry, Index};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A folder of models, by how it is laid out.
#[derive(Debug, Clone)]
pub enum Root {
    /// `<root>/<org>/<repo>/.../<file>.gguf`: LM Studio's, and itero's own.
    LmStudio(PathBuf),
    /// `<root>/models--<org>--<repo>/snapshots/<revision>/.../<file>.gguf`.
    HfCache(PathBuf),
}

/// A model file found, before its header is read.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// `<org>/<repo>`.
    pub repo: String,
    /// The quantization its name says, if it says one.
    pub quant_hint: Option<String>,
    pub path: PathBuf,
}

/// The folders a machine keeps models in: LM Studio's, Hugging Face's
/// cache (`$HF_HUB_CACHE`, else `$HF_HOME/hub`, else
/// `~/.cache/huggingface/hub`), and what `itero ai models add` downloaded.
pub fn roots() -> anyhow::Result<Vec<Root>> {
    let home =
        PathBuf::from(std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME is not set"))?);
    let hf = match (
        std::env::var_os("HF_HUB_CACHE"),
        std::env::var_os("HF_HOME"),
    ) {
        (Some(c), _) => PathBuf::from(c),
        (None, Some(h)) => PathBuf::from(h).join("hub"),
        (None, None) => home.join(".cache/huggingface/hub"),
    };
    Ok(vec![
        Root::LmStudio(home.join(".lmstudio/models")),
        Root::HfCache(hf),
        Root::LmStudio(super::downloads_dir()?),
    ])
}

/// Every `.gguf` file under `dir`, at any depth.
fn gguf_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        // a link (Hugging Face's snapshots are links) is followed
        if p.is_dir() {
            gguf_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "gguf") {
            out.push(p);
        }
    }
}

/// The quantization a file's name says: its last part like `Q4_K_M`,
/// `IQ4_XS`, `F16`, `BF16` or `F32`, with Unsloth's `UD-` (Dynamic)
/// before it kept, as llama.cpp's `-hf` names it: `UD-Q4_K_XL`.
pub fn quant_of(file: &str) -> Option<String> {
    let stem = file.strip_suffix(".gguf").unwrap_or(file);
    let parts: Vec<String> = stem
        .split(['-', '.'])
        .map(str::to_ascii_uppercase)
        .collect();
    let i = parts.iter().rposition(|t| {
        let digit_after = |p: &str| {
            t.strip_prefix(p)
                .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
        };
        digit_after("Q") || digit_after("IQ") || ["F16", "BF16", "F32"].contains(&t.as_str())
    })?;
    Some(match i.checked_sub(1).map(|j| parts[j].as_str()) {
        Some("UD") => format!("UD-{}", parts[i]),
        _ => parts[i].clone(),
    })
}

/// A part of a model in parts other than its first: `-00002-of-00003`.
fn later_part(file: &str) -> bool {
    let stem = file.strip_suffix(".gguf").unwrap_or(file);
    let parts: Vec<&str> = stem.rsplitn(4, '-').collect();
    matches!(parts.as_slice(), [total, "of", n, _]
        if total.chars().all(|c| c.is_ascii_digit())
            && n.chars().all(|c| c.is_ascii_digit())
            && n.trim_start_matches('0') != "1")
}

/// The model files under `roots`, named `<org>/<repo>` by where they are.
/// Vision projectors (`mmproj`) are not models, and a model in parts is
/// its first part; a file not under an `<org>/<repo>` is skipped.
pub fn model_files(roots: &[Root]) -> Vec<Found> {
    let mut out = vec![];
    for root in roots {
        let (base, hf) = match root {
            Root::LmStudio(b) => (b, false),
            Root::HfCache(b) => (b, true),
        };
        let mut files = vec![];
        gguf_files(base, &mut files);
        for path in files {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.to_ascii_lowercase().starts_with("mmproj") || later_part(name) {
                continue;
            }
            let Ok(rel) = path.strip_prefix(base) else {
                continue;
            };
            let parts: Vec<&str> = rel.iter().filter_map(|c| c.to_str()).collect();
            let repo = if hf {
                // models--<org>--<repo>/snapshots/<revision>/...
                match parts.first().and_then(|d| d.strip_prefix("models--")) {
                    Some(d) if parts.len() >= 4 => d.splitn(2, "--").collect::<Vec<_>>().join("/"),
                    _ => continue,
                }
            } else if parts.len() >= 3 {
                format!("{}/{}", parts[0], parts[1])
            } else {
                continue;
            };
            if !repo.contains('/') {
                continue;
            }
            out.push(Found {
                repo,
                quant_hint: quant_of(name),
                path,
            });
        }
    }
    out
}

fn modified(p: &Path) -> (u64, u64) {
    let m = std::fs::metadata(p).ok();
    let secs = m
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    (secs, m.map_or(0, |m| m.len()))
}

/// A file's quantization: its name says it, as Hugging Face and
/// llama.cpp's -hf name it; its header only when the name does not.
fn quant(f: &Found, header: Option<&str>) -> String {
    match (&f.quant_hint, header) {
        (Some(q), _) => q.clone(),
        (None, Some(q)) => q.to_string(),
        (None, None) => "unknown".into(),
    }
}

/// The entry for a file whose header itero-ai read.
fn entry(f: &Found, v: &Value, modified: u64) -> Entry {
    let quant = quant(f, v["quant"].as_str());
    Entry {
        name: format!("{}:{quant}", f.repo),
        repo: f.repo.clone(),
        quant,
        paths: vec![f.path.clone()],
        maker: f.repo.split('/').next().unwrap_or("").to_string(),
        params: v["params"].as_str().map(String::from),
        architecture: v["architecture"].as_str().unwrap_or("unknown").to_string(),
        context: v["context"].as_u64(),
        bytes: v["bytes"].as_u64().unwrap_or(0),
        licence: v["licence"].as_str().unwrap_or("unknown").to_string(),
        chat: v["chat"].as_bool().unwrap_or(false),
        runs: v["runs"].as_bool().unwrap_or(false),
        thinking: v["thinking"].as_bool().unwrap_or(false),
        modified,
        downloaded: None,
        removed: None,
    }
}

/// The index, built again from what is on disk: an unchanged file keeps
/// its entry, a new or changed one is inspected with `inspect`, a file
/// gone takes its entry with it, and the marks (`downloaded`, `removed`)
/// are carried over by name. A file `inspect` refuses (`None`: not a GGUF
/// file, one cut short) is left out. Gives the new index and how many
/// files were inspected.
pub fn rebuild(
    old: &Index,
    found: &[Found],
    inspect: &mut dyn FnMut(&Path) -> Result<Option<Value>, String>,
) -> Result<(Index, usize), String> {
    let mut inspected = 0;
    let mut new: Vec<Entry> = vec![];
    for f in found {
        let (secs, bytes) = modified(&f.path);
        let known = old
            .models
            .iter()
            .find(|e| e.paths.first() == Some(&f.path) && e.modified == secs && e.bytes == bytes);
        let mut e = match known {
            // named again: an index from an older itero may name it otherwise
            Some(e) => {
                let quant = quant(f, Some(&e.quant));
                Entry {
                    name: format!("{}:{quant}", e.repo),
                    quant,
                    paths: vec![f.path.clone()],
                    ..e.clone()
                }
            }
            None => {
                inspected += 1;
                let Some(v) = inspect(&f.path)? else {
                    continue;
                };
                if v["architecture"] == "clip" {
                    continue;
                }
                entry(f, &v, secs)
            }
        };
        if let Some(old) = old.models.iter().find(|o| o.name == e.name) {
            e.downloaded = old.downloaded.clone();
            e.removed = old.removed.clone();
        }
        match new.iter_mut().find(|n| n.name == e.name) {
            // one model in two places: one entry with both paths
            Some(n) => n.paths.push(f.path.clone()),
            None => new.push(e),
        }
    }
    new.sort_by(|a, b| a.name.cmp(&b.name));
    Ok((Index { models: new }, inspected))
}

/// Runs `#find-models`. Only when a file is new or changed does it start
/// an itero-ai, its own on its own socket, beside the daemon's, and stops
/// it when done: a machine with no model never runs one. itero-ai not
/// installed: nothing can use a model, and it ends well, saying so.
pub fn run(r: &StepRun) -> Result<Outcome, Failure> {
    scan(&r.root)
        .map(Outcome::Done)
        .map_err(|e| Failure::Error(format!("#find-models: {e}")))
}

/// Builds the index again from what is on disk and writes it; `root` is
/// the workspace whose own short-lived itero-ai reads new headers. Gives
/// how many models there are and how many files were inspected.
pub fn scan(root: &Path) -> Result<Value, String> {
    let fail = |e: String| e;
    let index = super::index_path().map_err(|e| fail(format!("{e:#}")))?;
    let old = Index::read(&index).map_err(|e| fail(format!("{e:#}")))?;
    let found = model_files(&roots().map_err(|e| fail(format!("{e:#}")))?);
    let socket = std::env::var_os("ITERO_AI_SOCKET").map(PathBuf::from);
    let mut started = None;
    let mut client = None;
    let mut inspect = |path: &Path| -> Result<Option<Value>, String> {
        if client.is_none() {
            let sock = match &socket {
                Some(s) => s.clone(),
                None => {
                    let own = super::socket_for(root).with_extension("find.sock");
                    let s = super::ensure_at(own, Duration::from_secs(60), |_| ())
                        .map_err(|e| format!("{e:#}"))?;
                    let sock = s.socket.clone();
                    started = Some(s);
                    sock
                }
            };
            client = Some((
                sock.clone(),
                crate::net::unix_client(&sock, Duration::from_secs(60))
                    .map_err(|e| e.to_string())?,
            ));
        }
        let (_, c) = client.as_ref().expect("made above");
        let resp = c
            .post("http://itero-ai/itero/inspect")
            .json(&json!({ "path": path }))
            .send()
            .map_err(|e| format!("itero-ai did not answer: {e}"))?;
        let status = resp.status().as_u16();
        let v: Value = resp.json().map_err(|e| e.to_string())?;
        match status {
            200 => Ok(Some(v)),
            // a file that is no model, or gone since: left out
            400 | 404 => Ok(None),
            _ => Err(v["error"]["message"]
                .as_str()
                .unwrap_or("itero-ai refused")
                .to_string()),
        }
    };
    let needs_inspecting = found.iter().any(|f| {
        let (secs, bytes) = modified(&f.path);
        !old.models
            .iter()
            .any(|e| e.paths.first() == Some(&f.path) && e.modified == secs && e.bytes == bytes)
    });
    if needs_inspecting && socket.is_none() && super::binary().is_err() {
        return Ok(json!({
            "models": 0, "inspected": 0,
            "skipped": "itero-ai is not installed: no model can be used",
        }));
    }
    let (ix, inspected) = rebuild(&old, &found, &mut inspect).map_err(fail)?;
    if let Some(mut s) = started
        && let Some(mut child) = s.child.take()
    {
        let _ = child.kill();
        let _ = child.wait();
    }
    if ix != old {
        ix.write(&index).map_err(|e| fail(format!("{e:#}")))?;
    }
    Ok(json!({ "models": ix.models.len(), "inspected": inspected }))
}
