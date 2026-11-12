//! Finding and starting Chrome. Needs Chrome on the machine: without it
//! these fail saying so.
#[path = "common/chrome.rs"]
mod test_chrome;
#[path = "common/web.rs"]
mod web;
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
    chrome::stop(&mut second.child);
}

/// The live processes in process group `group`: zombies (ended, waiting
/// for a parent to reap them) left out.
#[cfg(unix)]
fn alive_in(group: u32) -> Vec<String> {
    let o = std::process::Command::new("pgrep")
        .args(["-g", &group.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter(|pid| {
            let o = std::process::Command::new("ps")
                .args(["-o", "stat=", "-p", pid])
                .output()
                .unwrap();
            let stat = String::from_utf8_lossy(&o.stdout);
            !stat.trim().is_empty() && !stat.trim().starts_with('Z')
        })
        .map(str::to_string)
        .collect()
}

#[cfg(unix)]
#[test]
fn a_stopped_chrome_leaves_none_of_its_processes() {
    let exe = chrome::find().expect("these tests need Chrome");
    let d = tempfile::tempdir().unwrap();
    let mut c = chrome::launch(&exe, Mode::Headless, &d.path().join("profil")).unwrap();
    // its own process group, led by it: its children are in it too
    let group = c.child.id();
    let before = alive_in(group);
    assert!(
        before.len() > 1,
        "Chrome and its children in a group of their own: {before:?}"
    );
    chrome::stop(&mut c.child);
    // the group's processes are killed at once; a moment for the kernel
    let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !alive_in(group).is_empty() && std::time::Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(
        alive_in(group),
        Vec::<String>::new(),
        "left: was {before:?}"
    );
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
    chrome::stop(&mut c.child);
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

#[test]
fn every_window_is_drawn_however_many_are_open() {
    // a window Chrome takes as covered by others is not drawn, and input
    // to its page waits for a frame that never comes
    let c = test_chrome::start();
    let w = web::serve(vec![web::html(
        "/sayfa",
        "<!doctype html><title>Sayfa</title><h1>Işık</h1><p style=\"height:2000px\">Uzun bir sayfa</p>",
    )]);
    for n in 1..=4 {
        let tab = itero::browser::cdp::new_window(c.port, &w.url("/sayfa")).unwrap();
        let mut page =
            itero::browser::cdp::Cdp::connect(&tab.ws, std::time::Duration::from_secs(10)).unwrap();
        // Chrome works out which windows are covered a moment after a
        // page is drawn
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let drawn = page.eval(
            "Promise.race([new Promise(r => requestAnimationFrame(() => r(true))), \
             new Promise(r => setTimeout(() => r(false), 5000))])",
        );
        assert_eq!(drawn, Ok(serde_json::json!(true)), "window {n}");
    }
}

#[test]
fn the_sandbox_is_dropped_as_root_on_linux_or_when_told() {
    use std::ffi::OsStr;
    // unset, empty or on: the rule as it was, root on Linux only
    for v in [None, Some(OsStr::new("")), Some(OsStr::new("on"))] {
        assert_eq!(chrome::no_sandbox(v, false), Ok(false), "{v:?}");
        assert_eq!(chrome::no_sandbox(v, true), Ok(true), "{v:?}");
    }
    // off: a container's user, whom Chrome's sandbox cannot serve
    assert_eq!(chrome::no_sandbox(Some(OsStr::new("off")), false), Ok(true));
    // anything else is a mistake, said with what it takes
    let e = chrome::no_sandbox(Some(OsStr::new("kapalı")), false).unwrap_err();
    assert!(
        e.contains("ITERO_BROWSER_SANDBOX") && e.contains("kapalı"),
        "{e}"
    );
    assert!(e.contains("on or off"), "{e}");
}

#[cfg(unix)]
#[test]
fn a_chrome_that_fails_to_start_says_what_it_printed() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    // a "Chrome" that says why and ends at once, as a broken one does
    let fake = d.path().join("krom");
    // it also says what it was started with, to show the test's Chrome
    // logs verbosely
    std::fs::write(
        &fake,
        "#!/bin/sh\necho \"$@\" >&2\necho 'sandbox açılamadı' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let Err(e) = std::panic::catch_unwind(|| test_chrome::start_with(&fake)) else {
        panic!("a Chrome that ends at once started");
    };
    let said = e.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(said.contains("ended as it started"), "{said}");
    assert!(said.contains("sandbox açılamadı"), "{said}");
    assert!(said.contains("--enable-logging=stderr --v=1"), "{said}");
}

#[cfg(unix)]
#[test]
fn a_chrome_that_gives_no_port_names_its_processes() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    // a "Chrome" that starts a child and never gives a port
    let fake = d.path().join("krom");
    std::fs::write(&fake, "#!/bin/sh\nsleep 37 &\nwait\n").unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let e = chrome::launch_within(
        &fake,
        Mode::Headless,
        &d.path().join("profil"),
        std::process::Stdio::null(),
        std::time::Duration::from_secs(1),
    )
    .err()
    .expect("no port, an error");
    assert!(e.starts_with("Chrome gave no port in 1 s"), "{e}");
    // each of its processes, with its state, kernel wait and CPU time
    assert!(
        e.contains("pid, state, kernel wait, CPU time, command"),
        "{e}"
    );
    assert!(e.contains("sleep 37"), "{e}");
    assert!(e.contains(&fake.display().to_string()), "{e}");
}

#[test]
fn a_long_log_is_told_by_its_start_and_its_end() {
    let text: String = (1..=500).map(|i| format!("satır {i}\n")).collect();
    let told = test_chrome::head_and_tail(&text);
    assert!(
        told.contains("satır 1\n") && told.contains("satır 40\n"),
        "{told}"
    );
    assert!(
        !told.contains("satır 41\n") && !told.contains("satır 380\n"),
        "{told}"
    );
    assert!(told.contains("(340 lines left out)"), "{told}");
    assert!(
        told.contains("satır 381\n") && told.contains("satır 500\n"),
        "{told}"
    );
    assert_eq!(test_chrome::head_and_tail("kısa\n"), "kısa\n");
}

#[test]
fn the_cache_folder_is_listed_with_sizes() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("fontconfig")).unwrap();
    std::fs::write(d.path().join("fontconfig/yazı.cache-9"), vec![0u8; 2048]).unwrap();
    let listed = test_chrome::listing(d.path());
    assert!(listed.contains("fontconfig/yazı.cache-9"), "{listed}");
    assert!(listed.contains("2048"), "{listed}");
    assert_eq!(test_chrome::listing(&d.path().join("yok")), "(not there)");
}

#[test]
fn the_test_web_server_ends_with_its_test() {
    let w = web::serve(vec![web::html("/", "<p>merhaba</p>")]);
    let port = w.port;
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());
    drop(w);
    let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
        assert!(std::time::Instant::now() < end, "port {port} still taken");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[cfg(unix)]
#[test]
fn a_chrome_slow_to_start_on_a_cold_disk_is_waited_for() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    // a "Chrome" that gives its port after 21 s, as a first start on a cold
    // disk did in CI (state D, no CPU time, progressing)
    let fake = d.path().join("krom");
    std::fs::write(
        &fake,
        "#!/bin/sh\nfor a in \"$@\"; do case $a in --user-data-dir=*) p=${a#*=};; esac; done\n\
         sleep 21\nprintf '9222\\n/devtools/browser/yavaş\\n' > \"$p/DevToolsActivePort\"\nsleep 30\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut c = chrome::launch(&fake, Mode::Headless, &d.path().join("profil")).unwrap();
    assert_eq!(c.port, 9222);
    chrome::stop(&mut c.child);
}
