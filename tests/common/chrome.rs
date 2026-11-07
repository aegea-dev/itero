//! A headless Chrome for a test, on its own profile, stopped however the
//! test ends.
use itero::browser::{Mode, chrome};

pub struct TestChrome {
    pub child: std::process::Child,
    pub port: u16,
    _profile: tempfile::TempDir,
}

pub fn start() -> TestChrome {
    // SAFETY: the same value from every test; a test Chrome never reaches
    // the internet, whatever a fixture page points at
    unsafe { std::env::set_var("ITERO_TEST_BROWSER_OFFLINE", "1") };
    let exe = chrome::find().expect("these tests need Chrome");
    let profile = tempfile::tempdir().unwrap();
    let l = chrome::launch(&exe, Mode::Headless, profile.path()).unwrap();
    TestChrome {
        child: l.child,
        port: l.port,
        _profile: profile,
    }
}

impl Drop for TestChrome {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
