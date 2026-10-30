//! `itero ai`: the models a machine has, and one model file's header.
use super::out::{show, table, text};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn here() -> Result<PathBuf> {
    Ok(std::env::current_dir()?)
}

use itero_ai_index::Entry;
use std::cmp::Ordering;

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

/// The column `itero ai models list` sorts by.
#[derive(Debug, Clone, Copy, Default, PartialEq, clap::ValueEnum)]
pub enum Sort {
    #[default]
    Name,
    Size,
    Params,
    Context,
    Licence,
}

/// Which models `itero ai models list` shows, and in what order: every
/// filter given must hold.
#[derive(Debug, Default)]
pub struct Query {
    /// A word in the name, any case.
    pub word: Option<String>,
    pub sort: Sort,
    pub reverse: bool,
    /// Any of these licences, any case.
    pub licences: Vec<String>,
    pub maker: Option<String>,
    pub arch: Option<String>,
    pub thinks: bool,
    pub runs: bool,
    pub max_size: Option<u64>,
    /// In billions.
    pub max_params: Option<f64>,
}

/// A size as the table prints it, `8GB`, `1.5 GB`, `500MB`, in bytes.
pub fn bytes_in(s: &str) -> Result<u64> {
    let t = s.trim().to_ascii_uppercase().replace(' ', "");
    let (n, unit) = match t.strip_suffix("GB") {
        Some(n) => (n, 1e9),
        None => match t.strip_suffix("MB") {
            Some(n) => (n, 1e6),
            None => anyhow::bail!("a size is like 8GB or 500MB, not {s}"),
        },
    };
    let n: f64 = n
        .parse()
        .map_err(|_| anyhow::anyhow!("a size is like 8GB or 500MB, not {s}"))?;
    Ok((n * unit) as u64)
}

/// Parameters as the table prints them, `9B`, `1.5B`, `500M`, in billions;
/// a mixture, `35B-A3B`, counts all of them.
pub fn billions(s: &str) -> Option<f64> {
    let t = s.split('-').next()?.trim().to_ascii_uppercase();
    match t.strip_suffix('B') {
        Some(n) => n.parse().ok(),
        None => t.strip_suffix('M')?.parse::<f64>().ok().map(|m| m / 1000.0),
    }
}

/// The models `q` asks for, in its order; one with no value for the
/// column sorted by comes last, either way.
pub fn pick<'e>(models: Vec<&'e Entry>, q: &Query) -> Vec<&'e Entry> {
    let low = |s: &str| s.to_lowercase();
    let mut out: Vec<&Entry> = models
        .into_iter()
        .filter(|e| {
            q.word
                .as_ref()
                .is_none_or(|w| low(&e.name).contains(&low(w)))
        })
        .filter(|e| q.licences.is_empty() || q.licences.iter().any(|l| low(l) == low(&e.licence)))
        .filter(|e| q.maker.as_ref().is_none_or(|m| low(m) == low(&e.maker)))
        .filter(|e| {
            q.arch
                .as_ref()
                .is_none_or(|a| low(a) == low(&e.architecture))
        })
        .filter(|e| !q.thinks || e.thinking)
        .filter(|e| !q.runs || e.runs)
        .filter(|e| q.max_size.is_none_or(|m| e.bytes <= m))
        .filter(|e| {
            q.max_params.is_none_or(|m| {
                e.params
                    .as_deref()
                    .and_then(billions)
                    .is_some_and(|p| p <= m)
            })
        })
        .collect();
    let number = |e: &Entry| -> Option<f64> {
        match q.sort {
            Sort::Size => Some(e.bytes as f64),
            Sort::Params => e.params.as_deref().and_then(billions),
            Sort::Context => e.context.map(|c| c as f64),
            Sort::Name | Sort::Licence => None,
        }
    };
    let way = |o: Ordering| if q.reverse { o.reverse() } else { o };
    out.sort_by(|a, b| {
        let by = match q.sort {
            Sort::Name => way(a.name.cmp(&b.name)),
            Sort::Licence => way(a.licence.cmp(&b.licence)),
            // one with no number comes last, either way
            _ => match (number(a), number(b)) {
                (Some(x), Some(y)) => way(x.partial_cmp(&y).unwrap_or(Ordering::Equal)),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            },
        };
        by.then_with(|| a.name.cmp(&b.name))
    });
    out
}

/// `itero ai models list`: the index, looked at again first, filtered
/// and sorted as `q` asks.
pub fn list(json: bool, all: bool, q: &Query) -> Result<()> {
    crate::ai::find::scan(&here()?).map_err(|e| anyhow::anyhow!(e))?;
    let ix = itero_ai_index::Index::read(&crate::ai::index_path()?)?;
    let known = ix.models.iter().any(|e| all || e.removed.is_none());
    let models = pick(
        ix.models
            .iter()
            .filter(|e| all || e.removed.is_none())
            .collect(),
        q,
    );
    let v = json!(models);
    show(json, &v, |_| {
        if known && models.is_empty() {
            println!("no model matches");
            return;
        }
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

#[cfg(test)]
mod tests {
    use super::{Query, Sort, billions, bytes_in, pick};
    use itero_ai_index::Entry;

    fn model(name: &str, params: &str, bytes: u64, licence: &str, thinks: bool) -> Entry {
        Entry {
            name: name.into(),
            repo: name.split(':').next().unwrap().into(),
            quant: "Q4_K_M".into(),
            paths: vec![],
            maker: name.split('/').next().unwrap().into(),
            params: (!params.is_empty()).then(|| params.into()),
            architecture: "qwen35".into(),
            context: Some(32768),
            bytes,
            licence: licence.into(),
            chat: true,
            runs: true,
            thinking: thinks,
            modified: 0,
            downloaded: None,
            removed: None,
        }
    }

    fn names(v: &[&Entry]) -> Vec<String> {
        v.iter().map(|e| e.name.clone()).collect()
    }

    #[test]
    fn sizes_and_params_are_read_as_the_table_prints_them() {
        assert_eq!(bytes_in("8GB").unwrap(), 8_000_000_000);
        assert_eq!(bytes_in("1.5 gb").unwrap(), 1_500_000_000);
        assert_eq!(bytes_in("500MB").unwrap(), 500_000_000);
        assert!(bytes_in("sekiz").is_err());
        assert_eq!(billions("9B"), Some(9.0));
        assert_eq!(billions("1.5B"), Some(1.5));
        assert_eq!(billions("35B-A3B"), Some(35.0), "a mixture counts all");
        assert_eq!(billions("500M"), Some(0.5));
        assert_eq!(billions("çok"), None);
    }

    #[test]
    fn filters_all_hold_and_the_sort_is_by_the_column_asked() {
        let all = [
            model(
                "unsloth/Qwen3.5-4B-GGUF:Q4_K_M",
                "4B",
                2_700_000_000,
                "apache-2.0",
                true,
            ),
            model(
                "local/GLM-4-9B-GGUF:Q4_K_M",
                "9B",
                6_200_000_000,
                "mit",
                false,
            ),
            model(
                "local/Büyük-35B-A3B-GGUF:Q4_K_M",
                "35B-A3B",
                21_200_000_000,
                "other",
                true,
            ),
            model(
                "local/Belirsiz-GGUF:Q8_0",
                "",
                900_000_000,
                "unknown",
                false,
            ),
        ];
        let all: Vec<&Entry> = all.iter().collect();
        let q = |f: &dyn Fn(&mut Query)| {
            let mut q = Query::default();
            f(&mut q);
            names(&pick(all.clone(), &q))
        };
        assert_eq!(q(&|_| {}).len(), 4);
        assert_eq!(
            q(&|q| q.word = Some("glm".into())),
            ["local/GLM-4-9B-GGUF:Q4_K_M"]
        );
        assert_eq!(
            q(&|q| q.licences = vec!["MIT".into(), "apache-2.0".into()]),
            [
                "local/GLM-4-9B-GGUF:Q4_K_M",
                "unsloth/Qwen3.5-4B-GGUF:Q4_K_M"
            ]
        );
        assert_eq!(
            q(&|q| {
                q.thinks = true;
                q.maker = Some("local".into());
            }),
            ["local/Büyük-35B-A3B-GGUF:Q4_K_M"]
        );
        assert_eq!(
            q(&|q| q.max_size = Some(3_000_000_000)),
            ["local/Belirsiz-GGUF:Q8_0", "unsloth/Qwen3.5-4B-GGUF:Q4_K_M"]
        );
        // a model whose params are unknown is no match for a limit on them
        assert_eq!(
            q(&|q| q.max_params = Some(9.0)),
            [
                "local/GLM-4-9B-GGUF:Q4_K_M",
                "unsloth/Qwen3.5-4B-GGUF:Q4_K_M"
            ]
        );
        assert_eq!(
            q(&|q| q.sort = Sort::Size),
            [
                "local/Belirsiz-GGUF:Q8_0",
                "unsloth/Qwen3.5-4B-GGUF:Q4_K_M",
                "local/GLM-4-9B-GGUF:Q4_K_M",
                "local/Büyük-35B-A3B-GGUF:Q4_K_M"
            ]
        );
        // unknown params last, either way
        assert_eq!(
            q(&|q| {
                q.sort = Sort::Params;
                q.reverse = true;
            }),
            [
                "local/Büyük-35B-A3B-GGUF:Q4_K_M",
                "local/GLM-4-9B-GGUF:Q4_K_M",
                "unsloth/Qwen3.5-4B-GGUF:Q4_K_M",
                "local/Belirsiz-GGUF:Q8_0"
            ]
        );
    }
}
