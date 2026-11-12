//! A headless Chrome for a test, on its own profile, stopped with every
//! process it started however the test ends; when the test fails, or the
//! Chrome does not start, what Chrome said on stderr is told.
use itero::browser::{Mode, chrome};

pub struct TestChrome {
    pub child: std::process::Child,
    pub port: u16,
    _profile: tempfile::TempDir,
    said: std::path::PathBuf,
}

pub fn start() -> TestChrome {
    start_with(&chrome::find().expect("these tests need Chrome"))
}

pub fn start_with(exe: &std::path::Path) -> TestChrome {
    // SAFETY: the same values from every test. A test Chrome never reaches
    // the internet, whatever a fixture page points at,
    // and logs verbosely, for a test that fails to say what it did
    unsafe {
        std::env::set_var("ITERO_TEST_BROWSER_OFFLINE", "1");
        std::env::set_var("ITERO_TEST_BROWSER_VERBOSE", "1");
    }
    let profile = tempfile::tempdir().unwrap();
    // beside the profile, not in it: Chrome's own files stay apart
    let said = profile.path().with_extension("stderr");
    let file = std::fs::File::create(&said).unwrap();
    // a Chrome that never starts says why too, not only one that stops
    // answering later
    let l = match chrome::launch_with(exe, Mode::Headless, profile.path(), file.into()) {
        Ok(l) => l,
        Err(e) => {
            let text = std::fs::read_to_string(&said).unwrap_or_default();
            let _ = std::fs::remove_file(&said);
            // what a first start may have written there (a font cache)
            let cache = std::env::var_os("XDG_CACHE_HOME").map_or(String::new(), |d| {
                let d = std::path::PathBuf::from(d);
                format!(
                    "\nthe test's XDG_CACHE_HOME ({}):\n{}",
                    d.display(),
                    listing(&d)
                )
            });
            panic!(
                "{e}; Chrome said on stderr:\n{}{cache}",
                head_and_tail(&text)
            );
        }
    };
    TestChrome {
        child: l.child,
        port: l.port,
        _profile: profile,
        said,
    }
}

impl Drop for TestChrome {
    fn drop(&mut self) {
        // asked before the kill: whether Chrome had already ended, and how
        let before = self.child.try_wait().ok().flatten();
        // with every process it started
        chrome::stop(&mut self.child);
        if std::thread::panicking() {
            let text = std::fs::read_to_string(&self.said).unwrap_or_default();
            let how = before.map_or("still running".to_string(), |s| format!("had ended, {s}"));
            eprintln!(
                "the test's Chrome ({how}) said on stderr:\n{}",
                head_and_tail(&text)
            );
        }
        let _ = std::fs::remove_file(&self.said);
    }
}

/// How many of a log's first and last lines are told.
const HEAD: usize = 40;
const TAIL: usize = 120;

/// A log as told: whole when short, else its first `HEAD` and last `TAIL`
/// lines, with how many were left out between.
pub fn head_and_tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= HEAD + TAIL {
        return text.to_string();
    }
    let left = lines.len() - HEAD - TAIL;
    let mut out = String::new();
    for l in &lines[..HEAD] {
        out.push_str(l);
        out.push('\n');
    }
    out.push_str(&format!("... ({left} lines left out) ...\n"));
    for l in &lines[lines.len() - TAIL..] {
        out.push_str(l);
        out.push('\n');
    }
    out
}

/// Every file under `dir`, a line each: its path from `dir`, its size.
pub fn listing(dir: &std::path::Path) -> String {
    fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.path());
        for e in entries {
            let p = e.path();
            match e.metadata() {
                Ok(m) if m.is_dir() => walk(root, &p, out),
                Ok(m) => out.push(format!(
                    "  {} {}",
                    p.strip_prefix(root).unwrap_or(&p).display(),
                    m.len()
                )),
                Err(_) => {}
            }
        }
    }
    if !dir.exists() {
        return "(not there)".into();
    }
    let mut out = vec![];
    walk(dir, dir, &mut out);
    if out.is_empty() {
        return "(empty)".into();
    }
    out.join("\n")
}
