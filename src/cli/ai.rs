//! `itero ai`: the models a machine has, and one model file's header.
use super::out::{show, table, text};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn here() -> Result<PathBuf> {
    Ok(std::env::current_dir()?)
}

fn size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1e9)
    } else {
        format!("{:.0} MB", bytes as f64 / 1e6)
    }
}

fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// `itero ai models list`: the index, looked at again first.
pub fn list(json: bool, all: bool) -> Result<()> {
    crate::ai::find::scan(&here()?).map_err(|e| anyhow::anyhow!(e))?;
    let ix = itero_ai_index::Index::read(&crate::ai::index_path()?)?;
    let models: Vec<&itero_ai_index::Entry> = ix
        .models
        .iter()
        .filter(|e| all || e.removed.is_none())
        .collect();
    let v = json!(models);
    show(json, &v, |_| {
        if models.is_empty() {
            println!(
                "no models found yet: itero looks in LM Studio's folder, Hugging Face's cache \
                 and ~/.cache/itero/models (itero ai models add <org>/<repo> downloads one)"
            );
            return;
        }
        let mut rows = vec![
            [
                "MODEL", "PARAMS", "ARCH", "QUANT", "CONTEXT", "SIZE", "LICENCE", "CHAT", "RUNS",
                "THINKS",
            ]
            .map(String::from)
            .to_vec(),
        ];
        for e in &models {
            let name = match &e.removed {
                Some(_) => format!("{} (removed)", e.name),
                None => e.name.clone(),
            };
            rows.push(vec![
                name,
                e.params.clone().unwrap_or_default(),
                e.architecture.clone(),
                e.quant.clone(),
                e.context.map(|c| c.to_string()).unwrap_or_default(),
                size(e.bytes),
                e.licence.clone(),
                yes(e.chat).into(),
                yes(e.runs).into(),
                yes(e.thinking).into(),
            ]);
        }
        table(&rows);
    })
}

/// `itero ai inspect <file>`: one GGUF file's header, read by an itero-ai
/// of its own, stopped after.
pub fn inspect(file: &Path, json: bool) -> Result<()> {
    let file =
        std::fs::canonicalize(file).map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))?;
    let own = crate::ai::socket_for(&here()?).with_extension("cli.sock");
    let mut s = crate::ai::ensure_at(own, Duration::from_secs(30), |_| ())?;
    let answer = crate::net::unix_client(&s.socket, Duration::from_secs(60))?
        .post("http://itero-ai/itero/inspect")
        .json(&json!({ "path": file }))
        .send();
    if let Some(mut c) = s.child.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
    let resp = answer?;
    let ok = resp.status().is_success();
    let v: Value = resp.json()?;
    if !ok {
        bail!("{}", text(&v["error"]["message"]));
    }
    show(json, &v, |v| {
        for k in ["architecture", "params", "quant", "context", "licence"] {
            println!("{k}: {}", text(&v[k]));
        }
        println!("bytes: {}", size(v["bytes"].as_u64().unwrap_or(0)));
        for k in ["chat", "runs", "thinking"] {
            println!("{k}: {}", yes(v[k].as_bool().unwrap_or(false)));
        }
    })
}

/// `itero ai models add <org>/<repo>[:<QUANT>]`: downloads it from Hugging
/// Face into itero's folder, and puts it in the index.
pub fn add(name: &str, yes_given: bool) -> Result<()> {
    let token = crate::engine::secrets::get("HF_TOKEN");
    let into = crate::ai::downloads_dir()?;
    let ix = itero_ai_index::Index::read(&crate::ai::index_path()?)?;
    // there already: a removed one is only taken back
    if let Ok(e) = itero_ai_index::pick(&ix, name)
        && e.paths.iter().any(|p| p.is_file())
    {
        let mut ix = ix.clone();
        let taken_back = e.removed.is_some();
        let n = e.name.clone();
        for m in ix.models.iter_mut().filter(|m| m.name == n) {
            m.removed = None;
        }
        ix.write(&crate::ai::index_path()?)?;
        println!(
            "{n} is on this machine already{}",
            if taken_back {
                ": taken back into use"
            } else {
                ""
            }
        );
        return Ok(());
    }
    let p = crate::ai::download::plan(name, token.as_deref())?;
    let total = p.size();
    println!(
        "{}:{}  {}  licence: {}",
        p.repo,
        p.quant,
        size(total),
        p.licence
    );
    if !["mit", "apache-2.0"].contains(&p.licence.to_ascii_lowercase().as_str()) {
        println!("licence: {} (itero advises MIT or Apache-2.0)", p.licence);
    }
    std::fs::create_dir_all(&into)?;
    let free = crate::ai::download::free_space(&into)?;
    if total > free {
        bail!(
            "{} needs {}, and {} is free",
            p.repo,
            size(total),
            size(free)
        );
    }
    if !super::out::confirm(
        &format!("download {}?", size(total)),
        yes_given,
        &format!("run it with --yes to download {}", size(total)),
    )? {
        bail!("not downloaded");
    }
    let files = crate::ai::download::fetch(&p, token.as_deref(), &into, &|done, total| {
        eprint!("\r[{} / {}]", size(done), size(total));
    })?;
    eprintln!();
    crate::ai::find::scan(&here()?).map_err(|e| anyhow::anyhow!(e))?;
    let path = crate::ai::index_path()?;
    let mut ix = itero_ai_index::Index::read(&path)?;
    let now = crate::engine::daemon::stamp(chrono::Utc::now());
    let first = &files[0];
    let Some(e) = ix.models.iter_mut().find(|e| e.paths.contains(first)) else {
        bail!(
            "{} was downloaded but is not a model itero can use",
            first.display()
        );
    };
    e.removed = None;
    e.downloaded = Some(itero_ai_index::Downloaded {
        from: format!("{}/{}", crate::ai::download::endpoint(), p.repo),
        file: p.files[0].path.clone(),
        revision: p.revision.clone(),
        sha256: p.files[0].sha256.clone(),
        at: now,
    });
    let n = e.name.clone();
    ix.write(&path)?;
    println!("{n}: added");
    Ok(())
}

/// `itero ai models remove <name>`: out of use; with `delete`, its file too,
/// only when itero downloaded it.
pub fn remove(name: &str, delete: bool) -> Result<()> {
    let path = crate::ai::index_path()?;
    let mut ix = itero_ai_index::Index::read(&path)?;
    let e = itero_ai_index::pick(&ix, name)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .clone();
    if !delete {
        let now = crate::engine::daemon::stamp(chrono::Utc::now());
        for m in ix.models.iter_mut().filter(|m| m.name == e.name) {
            m.removed = Some(now.clone());
        }
        ix.write(&path)?;
        println!(
            "{}: removed from use (its file stays; --delete deletes it)",
            e.name
        );
        return Ok(());
    }
    let ours = crate::ai::downloads_dir()?;
    let theirs: Vec<&PathBuf> = e.paths.iter().filter(|p| !p.starts_with(&ours)).collect();
    if e.downloaded.is_none() || !theirs.is_empty() {
        let p = theirs.first().copied().or(e.paths.first());
        let whose = match p {
            Some(p) if p.to_string_lossy().contains("/.lmstudio/") => "LM Studio's",
            Some(p) if p.to_string_lossy().contains("huggingface") => "Hugging Face's cache's",
            _ => "not one itero downloaded",
        };
        bail!(
            "{} is {whose} file ({}): delete it there",
            e.name,
            p.map(|p| p.display().to_string()).unwrap_or_default()
        );
    }
    for p in &e.paths {
        // a model in parts: every part beside the first
        let stem = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let parts_of = stem.split("-00001-of-").next().filter(|s| *s != stem);
        if let (Some(base), Some(dir)) = (parts_of, p.parent()) {
            for f in std::fs::read_dir(dir)?.flatten() {
                let n = f.file_name();
                if n.to_string_lossy().starts_with(base) {
                    std::fs::remove_file(f.path())?;
                }
            }
        } else if p.exists() {
            std::fs::remove_file(p)?;
        }
    }
    ix.models.retain(|m| m.name != e.name);
    ix.write(&path)?;
    println!("{}: deleted", e.name);
    Ok(())
}
