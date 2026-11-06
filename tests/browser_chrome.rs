//! Finding and starting Chrome. Needs Chrome on the machine: without it
//! these fail saying so.
#[path = "common/chrome.rs"]
mod test_chrome;
use itero::browser::{Mode, chrome};
use std::path::PathBuf;

#[test]
fn chrome_is_found_and_starts_headless_with_its_own_profile() {
    let c = test_chrome::start();
    let v: serde_json::Value = itero::net::client(std::time::Duration::from_secs(10))
        .unwrap()
        .get(format!("http://127.0.0.1:{}/json/version", c.port))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert!(v["Browser"].as_str().unwrap().contains('/'), "{v}");
    assert!(
        v["User-Agent"].as_str().unwrap().contains("HeadlessChrome"),
        "{v}"
    );
}

#[test]
fn the_mode_follows_the_arg() {
    assert_eq!(chrome::choose(Some(true)), Mode::Headless);
    assert_eq!(chrome::choose(Some(false)), Mode::Window);
}

#[test]
fn not_found_names_every_place_looked() {
    let d = tempfile::tempdir().unwrap();
    let e = chrome::find_in(
        &[d.path().to_path_buf()],
        &[PathBuf::from("/yok/Google Chrome.app")],
    )
    .unwrap_err();
    for looked in [
        "/yok/Google Chrome.app",
        "google-chrome on PATH",
        "chromium-browser on PATH",
    ] {
        assert!(e.contains(looked), "{e}");
    }
}

#[test]
fn chromium_on_path_is_found_when_there_is_no_chrome() {
    let d = tempfile::tempdir().unwrap();
    let exe = d.path().join("chromium");
    std::fs::write(&exe, "").unwrap();
    assert_eq!(
        chrome::find_in(&[d.path().to_path_buf()], &[]).unwrap(),
        exe
    );
}
