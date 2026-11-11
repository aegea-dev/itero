//! A headless Chrome for a test, on its own profile, stopped however the
//! test ends; when the test fails, what Chrome said on stderr is printed.
use itero::browser::{Mode, chrome};

pub struct TestChrome {
    pub child: std::process::Child,
    pub port: u16,
    _profile: tempfile::TempDir,
    said: std::path::PathBuf,
}

pub fn start() -> TestChrome {
    // SAFETY: the same value from every test; a test Chrome never reaches
    // the internet, whatever a fixture page points at
    unsafe { std::env::set_var("ITERO_TEST_BROWSER_OFFLINE", "1") };
    let exe = chrome::find().expect("these tests need Chrome");
    let profile = tempfile::tempdir().unwrap();
    // beside the profile, not in it: Chrome's own files stay apart
    let said = profile.path().with_extension("stderr");
    let file = std::fs::File::create(&said).unwrap();
    let l = chrome::launch_with(&exe, Mode::Headless, profile.path(), file.into()).unwrap();
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
        let _ = self.child.kill();
        let _ = self.child.wait();
        if std::thread::panicking() {
            let text = std::fs::read_to_string(&self.said).unwrap_or_default();
            let how = before.map_or("still running".to_string(), |s| format!("had ended, {s}"));
            eprintln!("the test's Chrome ({how}) said on stderr:\n{text}");
        }
        let _ = std::fs::remove_file(&self.said);
    }
}
