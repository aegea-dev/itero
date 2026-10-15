//! The worker: runs one job and reports its output. It is dumb: the HTTP
//! request, the command, or the program, as the job says; nothing else.
//! For `python@` and `bun@` actions it first prepares the interpreter
//! (once, and again when the requirements or packages change).
use super::{Interpreter, Job};
use serde_json::{Map, Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// The Python shim: import the module, call the function with the job
/// (awaiting it when async), print the result.
const PYTHON_SHIM: &str = r#"import asyncio, importlib, inspect, json, os, sys
sys.path.insert(0, os.getcwd())
job = json.loads(sys.stdin.readline() or "{}")
f = getattr(importlib.import_module(os.environ["ITERO_MODULE"]), os.environ["ITERO_CALLABLE"])
out = f(job)
if inspect.isawaitable(out):
    async def wait():
        return await out
    out = asyncio.run(wait())
sys.stdout.write(json.dumps(out))
"#;

/// The Bun shim: the same, for a TypeScript or JavaScript file's export.
const BUN_SHIM: &str = r#"const line = (await Bun.stdin.text()).split("\n")[0] || "{}";
const m = await import(`${process.cwd()}/${process.env.ITERO_FILE}`);
const f = m[process.env.ITERO_EXPORT];
if (typeof f !== "function") {
  throw new Error(`${process.env.ITERO_FILE} exports no function ${process.env.ITERO_EXPORT}`);
}
const out = await f(JSON.parse(line));
process.stdout.write(JSON.stringify(out ?? null));
"#;

const UV: &str = "python@ actions need it (https://docs.astral.sh/uv/)";
const BUN: &str = "bun@ actions need it (https://bun.sh)";

/// The most an HTTP response may be, unless `with.max_bytes` says otherwise.
const MAX_BYTES: u64 = 10 * 1024 * 1024;

fn sha(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Runs a setup command; a missing program is named with where to get it.
fn setup(program: &str, args: &[&str], folder: &Path, hint: &str) -> Result<(), String> {
    let out = Command::new(program)
        .args(args)
        .current_dir(folder)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => format!("{program} is not installed: {hint}"),
            _ => format!("cannot run {program}: {e}"),
        })?;
    if !out.status.success() {
        return Err(format!(
            "{program} {}: {}",
            args.first().copied().unwrap_or(""),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// Holds the action's setup lock until dropped: workers running side by
/// side prepare its environment one at a time.
fn setup_lock(folder: &Path) -> Result<std::fs::File, String> {
    use fs2::FileExt;
    let f = std::fs::File::create(folder.join(".itero-setup.lock"))
        .map_err(|e| format!("cannot create the setup lock: {e}"))?;
    f.lock_exclusive()
        .map_err(|e| format!("cannot take the setup lock: {e}"))?;
    Ok(f)
}

/// The Python in the action's virtual environment, made with `uv` the
/// first time and given its requirements whenever they change.
fn python(
    folder: &Path,
    version: &str,
    home: &str,
    requirements: Option<&str>,
) -> Result<PathBuf, String> {
    let _setup = setup_lock(folder)?;
    let home = folder.join(home);
    let python = if cfg!(windows) {
        home.join("Scripts").join("python.exe")
    } else {
        home.join("bin").join("python")
    };
    if !python.exists() {
        let h = home.to_string_lossy().to_string();
        setup(
            "uv",
            &["venv", &h, "--python", version, "--quiet"],
            folder,
            UV,
        )?;
    }
    if let Some(r) = requirements {
        let text = std::fs::read(folder.join(r)).map_err(|e| format!("cannot read {r}: {e}"))?;
        let mark = home.join(".itero-requirements");
        let want = sha(&text);
        if std::fs::read_to_string(&mark).ok().as_deref() != Some(want.as_str()) {
            let py = python.to_string_lossy().to_string();
            setup(
                "uv",
                &["pip", "install", "--python", &py, "-r", r, "--quiet"],
                folder,
                UV,
            )?;
            std::fs::write(&mark, want).map_err(|e| e.to_string())?;
        }
    }
    Ok(python)
}

/// `bun install` the first time, and whenever `package.json` or `bun.lock`
/// change.
fn bun_packages(folder: &Path) -> Result<(), String> {
    if !folder.join("package.json").is_file() {
        return Ok(());
    }
    let _setup = setup_lock(folder)?;
    let mut both = std::fs::read(folder.join("package.json")).unwrap_or_default();
    both.extend(std::fs::read(folder.join("bun.lock")).unwrap_or_default());
    let mark = folder.join("node_modules").join(".itero-packages");
    let want = sha(&both);
    if std::fs::read_to_string(&mark).ok().as_deref() != Some(want.as_str()) {
        setup("bun", &["install", "--silent"], folder, BUN)?;
        std::fs::create_dir_all(folder.join("node_modules")).map_err(|e| e.to_string())?;
        std::fs::write(&mark, want).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Why a job gave no output.
#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    /// It ran and failed, or could not run: the action fails.
    Error(String),
    /// It ended without an answer (killed by a signal): it runs again.
    Lost(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Failure::Error(why) | Failure::Lost(why) => f.write_str(why),
        }
    }
}

impl From<String> for Failure {
    fn from(why: String) -> Failure {
        Failure::Error(why)
    }
}

impl Failure {
    fn context(self, what: &str) -> Failure {
        match self {
            Failure::Error(why) => Failure::Error(format!("{what}: {why}")),
            Failure::Lost(why) => Failure::Lost(format!("{what}: {why}")),
        }
    }
}

/// The exit code of a worker whose job was lost, so the daemon runs it
/// again (EX_TEMPFAIL).
pub const LOST_EXIT: i32 = 75;

/// Runs a job: its output, or why it gave none.
pub fn run(job: &Job) -> Result<Value, Failure> {
    let a = job.action.as_str();
    match a {
        "http.request@1" => http(job).map_err(|e| Failure::Error(format!("{a}: {e}"))),
        "command@1" => command(job).map_err(|e| Failure::Error(format!("{a}: {e}"))),
        "sh@1" => sh(job).map_err(|e| Failure::Error(format!("{a}: {e}"))),
        "notify@1" => notify(job).map_err(|e| Failure::Error(format!("{a}: {e}"))),
        _ if job.program.is_some() => program(job).map_err(|e| e.context(a)),
        _ => Err(Failure::Error(format!("{a} does not run in a worker"))),
    }
}

/// Stops this worker's process group, its job with it, once the process
/// that started it has gone: a daemon that stopped leaves no job running
/// that its next start would run again.
#[cfg(unix)]
pub fn stop_with_parent() {
    // SAFETY: getppid, getpid and getpgrp only read this process's ids
    let parent = unsafe { libc::getppid() };
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(500));
            if unsafe { libc::getppid() } != parent {
                // SAFETY: signals only this worker's own group when it leads
                // one (the daemon starts it so), else only itself
                unsafe {
                    if libc::getpgrp() == libc::getpid() {
                        libc::killpg(libc::getpgrp(), libc::SIGKILL);
                    }
                    libc::kill(libc::getpid(), libc::SIGKILL);
                }
            }
        }
    });
}

fn text(job: &Job, k: &str) -> Option<String> {
    job.with.get(k).and_then(Value::as_str).map(str::to_string)
}

fn http(job: &Job) -> Result<Value, String> {
    let url = text(job, "url").ok_or("needs with.url")?;
    let method = text(job, "method")
        .unwrap_or_else(|| "GET".into())
        .to_uppercase();
    let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(job.timeout))
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client.request(method, &url);
    let headers = job
        .with
        .get("headers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut has_key = false;
    for (k, v) in &headers {
        has_key |= k.eq_ignore_ascii_case("idempotency-key");
        let v = v
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| v.to_string());
        req = req.header(k.as_str(), v);
    }
    if !has_key {
        req = req.header("Idempotency-Key", &job.key);
    }
    if let Some(j) = job.with.get("json") {
        req = req.json(j);
    } else if let Some(b) = job.with.get("body") {
        req = req.body(
            b.as_str()
                .map(str::to_string)
                .unwrap_or_else(|| b.to_string()),
        );
    }
    let max = job
        .with
        .get("max_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(MAX_BYTES);
    let too_big = || format!("the response is larger than {max} bytes (with.max_bytes)");
    let res = req.send().map_err(|e| e.to_string())?;
    if res.content_length().is_some_and(|n| n > max) {
        return Err(too_big());
    }
    let status = res.status().as_u16();
    let headers: Map<String, Value> = res
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), json!(v.to_str().unwrap_or(""))))
        .collect();
    // read one byte past the cap: more than that is too big, whatever the
    // response said its length was
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(res, max + 1), &mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max {
        return Err(too_big());
    }
    let body = String::from_utf8_lossy(&bytes).into_owned();
    let mut out = json!({"status": status, "headers": headers, "text": body});
    if let Ok(j) = serde_json::from_str::<Value>(&body) {
        out["json"] = j;
    }
    Ok(out)
}

fn command(job: &Job) -> Result<Value, String> {
    let run: Vec<String> = job
        .with
        .get("run")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let (program, args) = run.split_first().ok_or("needs with.run")?;
    let out = Command::new(program)
        .args(args)
        .current_dir(&job.root)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    Ok(output(&out))
}

/// What a program printed, as an action's output: its exit code, stdout
/// and stderr as text, stdout's lines, and stdout as `json` when it parses.
fn output(out: &std::process::Output) -> Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut o = json!({
        "exit": out.status.code().unwrap_or(-1),
        "stdout": stdout,
        "stderr": String::from_utf8_lossy(&out.stderr),
        "lines": stdout.lines().collect::<Vec<_>>(),
    });
    if !stdout.trim().is_empty()
        && let Ok(j) = serde_json::from_str::<Value>(&stdout)
    {
        o["json"] = j;
    }
    o
}

/// A shell line, `sh -c`, in the workspace. Values reach it only through
/// its environment (`with.env`), never pasted into its text.
fn sh(job: &Job) -> Result<Value, String> {
    let run = text(job, "run").ok_or("needs with.run")?;
    let mut c = Command::new("sh");
    c.arg("-c")
        .arg(&run)
        .current_dir(&job.root)
        .stdin(Stdio::null());
    if let Some(env) = job.with.get("env").and_then(Value::as_object) {
        for (k, v) in env {
            match v {
                Value::String(s) => c.env(k, s),
                other => c.env(k, other.to_string()),
            };
        }
    }
    let out = c.output().map_err(|e| format!("cannot run sh: {e}"))?;
    Ok(output(&out))
}

/// A desktop notification: osascript on macOS, notify-send elsewhere, the
/// title and text as arguments (never in a script's text). With
/// `ITERO_NOTIFY_LOG` set, a line is appended to that file instead.
fn notify(job: &Job) -> Result<Value, String> {
    let title = text(job, "title").ok_or("needs with.title")?;
    let body = text(job, "text").ok_or("needs with.text")?;
    if let Some(log) = std::env::var_os("ITERO_NOTIFY_LOG") {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .map_err(|e| format!("cannot open {}: {e}", PathBuf::from(&log).display()))?;
        writeln!(f, "{title}\t{body}").map_err(|e| e.to_string())?;
        return Ok(json!({"shown": true}));
    }
    let mut c = if cfg!(target_os = "macos") {
        let mut c = Command::new("osascript");
        c.args([
            "-e",
            "on run argv",
            "-e",
            "display notification (item 2 of argv) with title (item 1 of argv)",
            "-e",
            "end run",
        ]);
        c
    } else {
        Command::new("notify-send")
    };
    let out = c
        .arg(&title)
        .arg(&body)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot show a notification: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cannot show a notification: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(json!({"shown": true}))
}

fn program(job: &Job) -> Result<Value, Failure> {
    let p = job.program.as_ref().expect("checked");
    let mut env = p.environment.clone();
    let (exe, args): (PathBuf, Vec<String>) = match &p.interpreter {
        None => (
            if p.executable.contains('/') {
                p.folder.join(&p.executable)
            } else {
                p.executable.clone().into()
            },
            p.arguments.clone(),
        ),
        Some(Interpreter::Python {
            version,
            home,
            requirements,
            module,
            callable,
        }) => {
            let py = python(&p.folder, version, home, requirements.as_deref())?;
            env.insert("ITERO_MODULE".into(), module.clone());
            env.insert("ITERO_CALLABLE".into(), callable.clone());
            (py, vec!["-c".into(), PYTHON_SHIM.into()])
        }
        Some(Interpreter::Bun { file, export }) => {
            bun_packages(&p.folder)?;
            env.insert("ITERO_FILE".into(), file.clone());
            env.insert("ITERO_EXPORT".into(), export.clone());
            ("bun".into(), vec!["-e".into(), BUN_SHIM.into()])
        }
    };
    let mut c = Command::new(&exe)
        .args(&args)
        .current_dir(&p.folder)
        .envs(&env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| match (e.kind(), &p.interpreter) {
            (std::io::ErrorKind::NotFound, Some(Interpreter::Bun { .. })) => {
                format!("bun is not installed: {BUN}")
            }
            _ => format!("cannot run {}: {e}", exe.display()),
        })?;
    let given = json!({
        "with": job.with,
        "input": job.input,
        "prev": job.prev,
        "run": job.run,
        "memory": job.memory,
        "args": job.args,
        "key": job.key,
    });
    let mut stdin = c.stdin.take().expect("piped");
    // a program that reads nothing closes its end early: not an error
    let _ = writeln!(stdin, "{given}");
    drop(stdin);
    let out = c.wait_with_output().map_err(|e| e.to_string())?;
    let Some(exit) = out.status.code() else {
        return Err(Failure::Lost(format!("killed ({})", out.status)));
    };
    if exit != 0 {
        // the end of stderr: where a traceback says what went wrong
        let all = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let why: String = all
            .chars()
            .rev()
            .take(500)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return Err(Failure::Error(if why.is_empty() {
            format!("exit {exit}")
        } else {
            format!("exit {exit}: {why}")
        }));
    }
    Ok(output(&out))
}
