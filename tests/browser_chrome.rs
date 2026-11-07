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

#[test]
fn a_chrome_left_running_on_the_profile_is_stopped_first() {
    let exe = chrome::find().expect("these tests need Chrome");
    let d = tempfile::tempdir().unwrap();
    let profile = d.path().join("profil");
    let mut first = chrome::launch(&exe, Mode::Headless, &profile).unwrap();
    // as a daemon that ended without stopping it leaves it
    let mut second = chrome::launch(&exe, Mode::Headless, &profile).unwrap();
    assert!(first.child.wait().is_ok(), "the first one ended");
    assert_ne!(first.port, second.port);
    second.child.kill().unwrap();
    second.child.wait().unwrap();
}

fn prefs(profile: &std::path::Path) -> serde_json::Value {
    let text = std::fs::read_to_string(profile.join("Default/Preferences")).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn assert_quiet(p: &serde_json::Value) {
    assert_eq!(p["translate"]["enabled"], false, "{p}");
    assert_eq!(p["credentials_enable_service"], false, "{p}");
    assert_eq!(p["profile"]["password_manager_enabled"], false, "{p}");
    assert_eq!(
        p["profile"]["default_content_setting_values"]["notifications"], 2,
        "{p}"
    );
}

#[test]
fn chrome_starts_with_no_translate_password_or_notification_prompts() {
    let exe = chrome::find().expect("these tests need Chrome");
    let d = tempfile::tempdir().unwrap();
    let profile = d.path().join("profil");
    let mut c = chrome::launch(&exe, Mode::Headless, &profile).unwrap();
    assert_quiet(&prefs(&profile));
    c.child.kill().unwrap();
    c.child.wait().unwrap();
}

#[test]
fn the_profile_keeps_its_other_preferences() {
    let d = tempfile::tempdir().unwrap();
    let profile = d.path().join("profil");
    std::fs::create_dir_all(profile.join("Default")).unwrap();
    std::fs::write(
        profile.join("Default/Preferences"),
        r#"{"translate": {"enabled": true, "ışık": 1}, "profile": {"name": "Gönder"}, "çerez": [1, 2]}"#,
    )
    .unwrap();
    chrome::write_prefs(&profile).unwrap();
    let p = prefs(&profile);
    assert_quiet(&p);
    assert_eq!(p["translate"]["ışık"], 1, "{p}");
    assert_eq!(p["profile"]["name"], "Gönder", "{p}");
    assert_eq!(p["çerez"], serde_json::json!([1, 2]), "{p}");
}

#[test]
fn preferences_that_are_not_json_are_an_error_naming_the_file() {
    let d = tempfile::tempdir().unwrap();
    let profile = d.path().join("profil");
    std::fs::create_dir_all(profile.join("Default")).unwrap();
    std::fs::write(profile.join("Default/Preferences"), "{ yarım").unwrap();
    let e = chrome::write_prefs(&profile).unwrap_err();
    assert!(e.contains("Default/Preferences"), "{e}");
}
