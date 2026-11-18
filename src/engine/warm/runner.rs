//! A warm copy's worker side: itero's runner (runner.py for Python,
//! runner.mjs for Node), started
//! once in the action's Python, and one request at a time given to it. A
//! request ends as a fresh process's run would (`worker::outcome_of`).
use super::super::worker::{self, Failure, Outcome, StepRun};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};

const RUNNER: &str = include_str!("runner.py");
const NODE_RUNNER: &str = include_str!("runner.mjs");

/// The runner of one action's file, kept between requests.
pub struct Runner {
    child: Child,
    channel: BufReader<UnixStream>,
    out: Option<ChildStdout>,
    err: Option<ChildStderr>,
}

/// How a request ended: its outcome, and whether the copy may take another.
#[derive(Debug)]
pub struct End {
    pub outcome: Result<Outcome, Failure>,
    pub keep: bool,
    /// Why the copy is not kept, when it is not.
    pub why_not: Option<String>,
}

impl Runner {
    /// Starts the runner for `step`'s action, in the workspace, with the
    /// worker's environment: a request brings the step's own.
    pub fn start(step: &StepRun) -> Result<Runner, String> {
        let first = worker::first_line(&step.file).unwrap_or_default();
        let runtime = super::runtime(&first)?;
        let program = super::interpreter(&runtime, &step.file, &step.root)?;
        let (ours, theirs) = UnixStream::pair().map_err(|e| format!("warm: {e}"))?;
        let mut c = Command::new(&program);
        match runtime {
            super::Runtime::Node(_) => c.args(["--input-type=module", "--eval", NODE_RUNNER]),
            _ => c.args(["-c", RUNNER]),
        };
        c.arg(&step.file)
            .current_dir(&step.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let raw = std::os::fd::AsRawFd::as_raw_fd(&theirs);
        // SAFETY: between fork and exec only dup2 and fcntl, both
        // async-signal-safe, put the runner's end of the channel at fd 3,
        // open across exec
        unsafe {
            std::os::unix::process::CommandExt::pre_exec(&mut c, move || {
                if raw == 3 {
                    let flags = libc::fcntl(3, libc::F_GETFD);
                    if flags < 0 || libc::fcntl(3, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                } else if libc::dup2(raw, 3) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = c
            .spawn()
            .map_err(|e| format!("warm: cannot start {}: {e}", program.display()))?;
        drop(theirs);
        Ok(Runner {
            out: child.stdout.take(),
            err: child.stderr.take(),
            child,
            channel: BufReader::new(ours),
        })
    }

    /// The runner's process id.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Runs one request: the step's environment and stdin, the file run
    /// again, read back as a fresh process's end would be.
    pub fn request(&mut self, step: &StepRun, told: &(dyn Fn(Option<&str>) + Sync)) -> End {
        if let Some(dir) = step.memory.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            return End {
                outcome: Err(Failure::Error(format!(
                    "cannot make {}: {e}",
                    dir.display()
                ))),
                keep: true,
                why_not: None,
            };
        }
        let mark = format!("itero-mark-{:016x}", random());
        let mut env: serde_json::Map<String, serde_json::Value> = step
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone().into()))
            .collect();
        env.insert(
            "ITERO_MEMORY".into(),
            step.memory.display().to_string().into(),
        );
        if let Some(since) = &step.waiting_since {
            env.insert("ITERO_WAITING_SINCE".into(), since.clone().into());
        }
        let req = serde_json::json!({
            "env": env,
            "stdin": format!("{}\n", serde_json::Value::Object(step.input.clone())),
            "mark": mark,
        });
        if let Err(e) = writeln!(self.channel.get_mut(), "{req}") {
            return self.gone(format!("warm: the runner is gone: {e}"));
        }
        let end = format!("\n{mark}\n").into_bytes();
        let (out, err) = (self.out.take(), self.err.take());
        let on_stderr = worker::stderr_telling(told);
        let ((stdout, out), (stderr, err)) = std::thread::scope(|s| {
            let o = s.spawn(|| until_mark(out, &end, &|_| told(None)));
            let e = s.spawn(|| until_mark(err, &end, &on_stderr));
            (
                o.join().unwrap_or((vec![], None)),
                e.join().unwrap_or((vec![], None)),
            )
        });
        self.out = out;
        self.err = err;
        let mut line = String::new();
        match self.channel.read_line(&mut line) {
            Ok(n) if n > 0 => {
                let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
                let code = v["exit"].as_i64().and_then(|c| i32::try_from(c).ok());
                let left = v["left"].as_str().map(str::to_string);
                End {
                    outcome: worker::outcome_of(
                        code,
                        "the runner gave no exit code",
                        &stdout,
                        &stderr,
                    ),
                    keep: left.is_none() && self.out.is_some() && self.err.is_some(),
                    why_not: left,
                }
            }
            // the runner ended (os._exit, a crash): its exit code says how,
            // as a fresh process's would
            _ => {
                let status = self.child.wait();
                let code = status.as_ref().ok().and_then(|s| s.code());
                let killed = match &status {
                    Ok(s) => format!("killed ({s})"),
                    Err(e) => format!("killed ({e})"),
                };
                End {
                    outcome: worker::outcome_of(code, &killed, &stdout, &stderr),
                    keep: false,
                    why_not: Some(match code {
                        Some(c) => format!("exited {c}"),
                        None => "killed".into(),
                    }),
                }
            }
        }
    }

    fn gone(&mut self, why: String) -> End {
        let _ = self.child.kill();
        let _ = self.child.wait();
        End {
            outcome: Err(Failure::Error(why.clone())),
            keep: false,
            why_not: Some(why),
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Reads a pipe until `end` (the request's mark), calling `printed` with
/// each piece; returns what came before the mark, and the pipe for the next
/// request (`None` once it closed: the runner ended).
fn until_mark<R: Read>(
    pipe: Option<R>,
    end: &[u8],
    printed: &(dyn Fn(&[u8]) + Sync),
) -> (Vec<u8>, Option<R>) {
    let Some(mut pipe) = pipe else {
        return (vec![], None);
    };
    let mut all = vec![];
    let mut buf = [0; 8192];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => return (all, None),
            Ok(n) => {
                // only the new bytes, and the mark's length before them, can
                // hold the mark's start
                let from = all.len().saturating_sub(end.len());
                all.extend_from_slice(&buf[..n]);
                printed(&buf[..n]);
                if let Some(i) = all[from..].windows(end.len()).position(|w| w == end) {
                    all.truncate(from + i);
                    return (all, Some(pipe));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return (all, None),
        }
    }
}

/// A random number for the mark: std's hasher keys come from the system's
/// randomness, new for each `RandomState`.
fn random() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish()
}
