//! `itero ai models add`: a model from Hugging Face into itero's own
//! folder. Which file comes by the same rule a step's name does; the
//! download resumes after a break and is checked against the SHA-256
//! Hugging Face publishes.
use anyhow::{Context, Result, bail};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Hugging Face, or `$HF_ENDPOINT` (Hugging Face's own variable: a mirror,
/// or a test's server).
pub fn endpoint() -> String {
    std::env::var("HF_ENDPOINT")
        .unwrap_or_else(|_| "https://huggingface.co".into())
        .trim_end_matches('/')
        .to_string()
}

/// One file to download: its path in the repo, size and SHA-256.
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// What `add` will download.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    pub repo: String,
    pub quant: String,
    /// The model's file, or its parts in order.
    pub files: Vec<File>,
    /// The repo's commit.
    pub revision: String,
    /// `unknown` when the repo names none.
    pub licence: String,
}

impl Planned {
    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

fn get(url: &str, token: Option<&str>) -> Result<Value> {
    let c = crate::net::client(Duration::from_secs(60))?;
    let mut r = c.get(url);
    if let Some(t) = token {
        r = r.bearer_auth(t);
    }
    let resp = r.send().with_context(|| url.to_string())?;
    match resp.status().as_u16() {
        200 => Ok(resp.json()?),
        401 | 403 => bail!(
            "{url}: the repo needs a Hugging Face token, one whose account accepted its terms: \
             itero secret HF_TOKEN"
        ),
        404 => bail!("no such repo on Hugging Face: {url}"),
        s => bail!("{url}: {s}"),
    }
}

/// The files of `name` (`<org>/<repo>[:<QUANT>]`) to download, picked as a
/// step's name picks: the quantization named, else the repo's only model,
/// else `Q4_K_M`, else an error naming them. A model in parts is all of
/// them.
pub fn plan(name: &str, token: Option<&str>) -> Result<Planned> {
    let (repo, _) = name.rsplit_once(':').unwrap_or((name, ""));
    if repo.split('/').count() != 2 {
        bail!("a model is named <org>/<repo>, with :<QUANT> or without: not {name}");
    }
    let api = endpoint();
    let info = get(&format!("{api}/api/models/{repo}"), token)?;
    let revision = info["sha"]
        .as_str()
        .context("the repo has no revision")?
        .to_string();
    let licence = info["cardData"]["license"]
        .as_str()
        .map(String::from)
        .or_else(|| {
            info["tags"]
                .as_array()?
                .iter()
                .find_map(|t| t.as_str()?.strip_prefix("license:").map(String::from))
        })
        .unwrap_or_else(|| "unknown".into());
    let tree = get(
        &format!("{api}/api/models/{repo}/tree/{revision}?recursive=true"),
        token,
    )?;
    let mut models: Vec<(String, Vec<File>)> = vec![];
    for f in tree.as_array().into_iter().flatten() {
        let Some(path) = f["path"].as_str().filter(|p| p.ends_with(".gguf")) else {
            continue;
        };
        let file = path.rsplit('/').next().unwrap_or(path);
        if file.to_ascii_lowercase().starts_with("mmproj") {
            continue;
        }
        let Some(quant) = super::find::quant_of(file) else {
            continue;
        };
        let size = f["lfs"]["size"]
            .as_u64()
            .or(f["size"].as_u64())
            .unwrap_or(0);
        let sha256 = f["lfs"]["oid"]
            .as_str()
            .context(format!("{path}: Hugging Face gives no SHA-256 for it"))?
            .to_string();
        let file = File {
            path: path.to_string(),
            size,
            sha256,
        };
        match models.iter_mut().find(|(q, _)| *q == quant) {
            Some((_, parts)) => parts.push(file),
            None => models.push((quant, vec![file])),
        }
    }
    // the same rule as a step's name, over the repo's models
    let ix = itero_ai_index::Index {
        models: models
            .iter()
            .map(|(q, _)| itero_ai_index::Entry {
                name: format!("{repo}:{q}"),
                repo: repo.to_string(),
                quant: q.clone(),
                paths: vec![],
                maker: String::new(),
                params: None,
                architecture: String::new(),
                context: None,
                bytes: 0,
                licence: String::new(),
                chat: false,
                runs: false,
                thinking: false,
                modified: 0,
                downloaded: None,
                removed: None,
            })
            .collect(),
    };
    let picked = itero_ai_index::pick(&ix, name).map_err(|e| match e {
        itero_ai_index::ResolveError::NotFound(_) if models.is_empty() => {
            anyhow::anyhow!("{repo} has no GGUF model files")
        }
        e => anyhow::anyhow!("{e}"),
    })?;
    let mut files = models
        .into_iter()
        .find(|(q, _)| *q == picked.quant)
        .map(|(_, f)| f)
        .expect("picked from them");
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Planned {
        repo: repo.to_string(),
        quant: picked.quant.clone(),
        files,
        revision,
        licence,
    })
}

/// Free bytes where `dir` is.
pub fn free_space(dir: &Path) -> Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes())?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: statvfs writes only into `s`, whose size it is given by type
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        bail!("{}: {}", dir.display(), std::io::Error::last_os_error());
    }
    Ok(s.f_bavail as u64 * s.f_frsize as u64)
}

/// Downloads `p` into `into/<org>/<repo>/`, each file to `<file>.part`
/// first: a break resumes from there (HTTP ranges), the whole is checked
/// against its SHA-256 (a mismatch deletes it), then renamed into place.
/// `progress` gets the bytes done and the total. Gives the files' paths.
pub fn fetch(
    p: &Planned,
    token: Option<&str>,
    into: &Path,
    progress: &dyn Fn(u64, u64),
) -> Result<Vec<PathBuf>> {
    let dir = into.join(&p.repo);
    std::fs::create_dir_all(&dir).with_context(|| dir.display().to_string())?;
    let api = endpoint();
    let c = crate::net::download_client(Duration::from_secs(30))?;
    let (total, mut done) = (p.size(), 0u64);
    let mut out = vec![];
    for f in &p.files {
        let name = f.path.rsplit('/').next().unwrap_or(&f.path);
        let target = dir.join(name);
        if target.is_file() {
            done += f.size;
            out.push(target);
            continue;
        }
        let part = dir.join(format!("{name}.part"));
        let mut hash = Sha256::new();
        let mut have = 0u64;
        if let Ok(mut old) = std::fs::File::open(&part) {
            let mut buf = vec![0; 1 << 20];
            loop {
                let n = old.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hash.update(&buf[..n]);
                have += n as u64;
            }
        }
        let url = format!("{api}/{}/resolve/{}/{}", p.repo, p.revision, f.path);
        let mut r = c.get(&url);
        if let Some(t) = token {
            r = r.bearer_auth(t);
        }
        if have > 0 {
            r = r.header("Range", format!("bytes={have}-"));
        }
        let mut resp = r.send().with_context(|| url.clone())?;
        let status = resp.status().as_u16();
        if have > 0 && status == 200 {
            // the server sent it whole: start again
            hash = Sha256::new();
            have = 0;
        } else if status != 200 && status != 206 {
            bail!("{url}: {status}");
        }
        let mut w = std::fs::OpenOptions::new()
            .create(true)
            .append(have > 0)
            .write(true)
            .truncate(have == 0)
            .open(&part)
            .with_context(|| part.display().to_string())?;
        done += have;
        let mut buf = vec![0; 1 << 20];
        loop {
            let n = resp
                .read(&mut buf)
                .with_context(|| format!("{url}: the download broke; run it again to resume"))?;
            if n == 0 {
                break;
            }
            w.write_all(&buf[..n])?;
            hash.update(&buf[..n]);
            done += n as u64;
            progress(done, total);
        }
        w.flush()?;
        let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if got != f.sha256 {
            let _ = std::fs::remove_file(&part);
            bail!(
                "{name}: the download does not match its SHA-256 ({got}, not {}); deleted",
                f.sha256
            );
        }
        std::fs::rename(&part, &target)?;
        out.push(target);
    }
    Ok(out)
}
