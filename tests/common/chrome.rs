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
    // SAFETY: the same value from every test; a test Chrome never reaches
    // the internet, whatever a fixture page points at
    unsafe { std::env::set_var("ITERO_TEST_BROWSER_OFFLINE", "1") };
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
            panic!("{e}; Chrome said on stderr:\n{text}");
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
            eprintln!("the test's Chrome ({how}) said on stderr:\n{text}");
        }
        let _ = std::fs::remove_file(&self.said);
    }
}
