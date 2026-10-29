//! itero-ai, the local model's server: where it is, its socket, itero's
//! own folder of models, and starting it when a step needs it (or using
//! the one that answers).
pub mod find;
pub mod keeper;
pub mod step;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a new itero-ai may take to answer. Its first start on a
/// machine compiles llama.cpp's GPU kernels: 15 s on an M1 Max.
const START: Duration = Duration::from_secs(60);

/// itero's cache, the same on every OS, as Hugging Face's
/// `~/.cache/huggingface`: `$XDG_CACHE_HOME/itero`, else `~/.cache/itero`.
pub fn cache_dir() -> Result<PathBuf> {
    if let Some(x) = std::env::var_os("XDG_CACHE_HOME").filter(|x| !x.is_empty()) {
        return Ok(PathBuf::from(x).join("itero"));
    }
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".cache").join("itero"))
}

/// The index of the machine's models, which itero writes and itero-ai reads.
pub fn index_path() -> Result<PathBuf> {
    Ok(cache_dir()?.join("models.json"))
}

/// Where `itero ai models add` puts what it downloads.
pub fn downloads_dir() -> Result<PathBuf> {
    Ok(cache_dir()?.join("models"))
}

/// The socket of a workspace's itero-ai: short, whatever the workspace's
/// path, for the 104 bytes a socket's path may have.
pub fn socket_for(root: &Path) -> PathBuf {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(root.as_os_str().as_encoded_bytes());
    let name: String = h.iter().take(4).map(|b| format!("{b:02x}")).collect();
    run_dir().join(format!("{name}.sock"))
}

/// The user's own folder for sockets: `$XDG_RUNTIME_DIR/itero-ai` on
/// Linux, else `/tmp/itero-ai-<uid>` (macOS's `$TMPDIR` is too long).
fn run_dir() -> PathBuf {
    if cfg!(target_os = "linux")
        && let Some(x) = std::env::var_os("XDG_RUNTIME_DIR").filter(|x| !x.is_empty())
    {
        return PathBuf::from(x).join("itero-ai");
    }
    // SAFETY: getuid has no preconditions and cannot fail
    PathBuf::from(format!("/tmp/itero-ai-{}", unsafe { libc::getuid() }))
}

/// Makes `dir` this user's own (`0700`), or refuses one someone else made.
fn own_dir(dir: &Path) -> Result<()> {
    match std::fs::create_dir_all(dir) {
        Ok(()) => {}
        Err(e) => return Err(e).with_context(|| dir.display().to_string()),
    }
    let m = std::fs::symlink_metadata(dir).with_context(|| dir.display().to_string())?;
    // SAFETY: getuid has no preconditions and cannot fail
    let me = unsafe { libc::getuid() };
    if !m.is_dir() || m.uid() != me {
        bail!(
            "{} is not this user's own folder: itero-ai's socket goes only there",
            dir.display()
        );
    }
    if m.mode() & 0o077 != 0 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .with_context(|| dir.display().to_string())?;
    }
    Ok(())
}

/// itero-ai's program: `ITERO_AI_BIN` when set (a build, or a test's),
/// else beside the running itero (where Homebrew links both), else on
/// `PATH`.
pub fn binary() -> Result<PathBuf> {
    if let Some(b) = std::env::var_os("ITERO_AI_BIN") {
        return Ok(b.into());
    }
    let beside = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("itero-ai")));
    let on_path: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("itero-ai"))
                .collect()
        })
        .unwrap_or_default();
    beside
        .into_iter()
        .chain(on_path)
        .find(|p| p.is_file())
        .context(
            "itero-ai is not installed: brew install aegea-dev/tap/itero-ai, \
             or its archive from the release page",
        )
}

/// Whether an itero-ai answers on `socket`.
pub fn answers(socket: &Path) -> bool {
    crate::net::unix_client(socket, Duration::from_millis(500))
        .and_then(|c| c.get("http://itero-ai/health").send())
        .is_ok_and(|r| r.status().is_success())
}

/// An itero-ai to talk to: its socket, and the process when this started
/// it (none when one answered already).
pub struct Started {
    pub socket: PathBuf,
    pub child: Option<Child>,
    /// The last line it wrote on stderr: why, when it ends badly.
    pub said: Arc<Mutex<String>>,
}

/// The workspace's itero-ai: the one answering on its socket, else one
/// started now, which exits after `idle` with nothing asked. `on_line`
/// gets each thing it tells (`{"event": "started"}`, `model.loaded`).
/// With `ITERO_AI_SOCKET` set in itero's environment, that socket is
/// used and nothing is started (a test's fake server, or one run by hand).
pub fn ensure(
    root: &Path,
    idle: Duration,
    on_line: impl Fn(Value) + Send + 'static,
) -> Result<Started> {
    if let Some(s) = std::env::var_os("ITERO_AI_SOCKET") {
        return Ok(Started {
            socket: s.into(),
            child: None,
            said: Arc::new(Mutex::new(String::new())),
        });
    }
    ensure_at(socket_for(root), idle, on_line)
}

/// `ensure` on a given socket.
pub fn ensure_at(
    socket: PathBuf,
    idle: Duration,
    on_line: impl Fn(Value) + Send + 'static,
) -> Result<Started> {
    let said = Arc::new(Mutex::new(String::new()));
    if answers(&socket) {
        return Ok(Started {
            socket,
            child: None,
            said,
        });
    }
    own_dir(socket.parent().expect("a socket is in a folder"))?;
    let bin = binary()?;
    let mut child = Command::new(&bin)
        .env("ITERO_AI_SOCKET", &socket)
        .env("ITERO_AI_IDLE", idle.as_secs().to_string())
        .env("ITERO_AI_INDEX", index_path()?)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("cannot start {}", bin.display()))?;
    let out = child.stdout.take().expect("piped");
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                on_line(v);
            }
        }
    });
    let err = child.stderr.take().expect("piped");
    let last = said.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(err).lines().map_while(Result::ok) {
            if !line.trim().is_empty() {
                *last.lock().unwrap_or_else(|e| e.into_inner()) = line;
            }
        }
    });
    let since = Instant::now();
    while since.elapsed() < START {
        if answers(&socket) {
            return Ok(Started {
                socket,
                child: Some(child),
                said,
            });
        }
        if let Some(status) = child.try_wait()? {
            // let the reader take its last line
            std::thread::sleep(Duration::from_millis(50));
            let why = said.lock().unwrap_or_else(|e| e.into_inner()).clone();
            bail!("itero-ai ended ({status}) before it answered: {why}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    let why = said.lock().unwrap_or_else(|e| e.into_inner()).clone();
    bail!(
        "itero-ai did not answer within {} s: {why}",
        START.as_secs()
    )
}
