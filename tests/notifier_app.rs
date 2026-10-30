//! Itero.app's stable copy: kept in one place, brought up to date in place
//! from the Itero.app itero ships with, so macOS's record of it survives
//! upgrades. Fake bundles: an Info.plist with a version, a script as the
//! notifier.
#![cfg(unix)]
use itero::engine::notifier_app::{stable, up_to_date, version};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// A fake Itero.app of `v` at `at`, with a file `extra` of its own.
fn app(at: &Path, v: u64, extra: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let c = at.join("Contents");
    std::fs::create_dir_all(c.join("MacOS")).unwrap();
    std::fs::write(
        c.join("Info.plist"),
        format!(
            "<plist><dict>\n  <key>CFBundleVersion</key>\n  <string>{v}</string>\n</dict></plist>\n"
        ),
    )
    .unwrap();
    let n = c.join("MacOS/itero-notifier");
    std::fs::write(&n, format!("#!/bin/sh\necho sürüm {v}\n")).unwrap();
    std::fs::set_permissions(&n, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(c.join(extra), "kendi").unwrap();
    at.to_path_buf()
}

fn run(program: &Path) -> String {
    let o = std::process::Command::new(program).output().unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

#[test]
fn the_stable_copy_is_made_and_kept_up_to_date_in_place() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("ev");
    let st = stable(&home);
    assert_eq!(st, home.join("Library/Application Support/itero/Itero.app"));
    let lock = d.path().join("durum/notifier.lock");
    // missing: copied
    let v20 = app(&d.path().join("Cellar/20/Itero.app"), 20, "eski");
    let p = up_to_date(&v20, &st, &lock).unwrap();
    assert_eq!(p, st.join("Contents/MacOS/itero-notifier"));
    assert_eq!(version(&st), Some(20));
    assert_eq!(run(&p), "sürüm 20");
    let ino = std::fs::metadata(&st).unwrap().ino();
    // older: replaced in the same folder, its own files gone
    let v21 = app(&d.path().join("Cellar/21/Itero.app"), 21, "yeni");
    let p = up_to_date(&v21, &st, &lock).unwrap();
    assert_eq!(version(&st), Some(21));
    assert_eq!(run(&p), "sürüm 21");
    assert_eq!(
        std::fs::metadata(&st).unwrap().ino(),
        ino,
        "the same folder"
    );
    assert!(st.join("Contents/yeni").is_file());
    assert!(!st.join("Contents/eski").exists());
    assert!(!st.join("Contents.new").exists() && !st.join("Contents.old").exists());
    // equal or newer: untouched
    let before = std::fs::metadata(st.join("Contents/yeni"))
        .unwrap()
        .mtime_nsec();
    up_to_date(&v21, &st, &lock).unwrap();
    up_to_date(&v20, &st, &lock).unwrap();
    assert_eq!(version(&st), Some(21));
    assert_eq!(
        std::fs::metadata(st.join("Contents/yeni"))
            .unwrap()
            .mtime_nsec(),
        before
    );
}

#[test]
fn a_copy_cut_short_is_cleaned_and_made_again() {
    let d = tempfile::tempdir().unwrap();
    let st = stable(&d.path().join("ev"));
    let lock = d.path().join("durum/notifier.lock");
    let v20 = app(&d.path().join("Cellar/20/Itero.app"), 20, "eski");
    up_to_date(&v20, &st, &lock).unwrap();
    // an earlier copy stopped half way
    std::fs::create_dir_all(st.join("Contents.new/MacOS")).unwrap();
    std::fs::create_dir_all(st.join("Contents.old")).unwrap();
    let v22 = app(&d.path().join("Cellar/22/Itero.app"), 22, "yeni");
    let p = up_to_date(&v22, &st, &lock).unwrap();
    assert_eq!(run(&p), "sürüm 22");
    assert!(!st.join("Contents.new").exists() && !st.join("Contents.old").exists());
}

#[test]
fn steps_side_by_side_copy_once_and_both_get_a_whole_copy() {
    let d = tempfile::tempdir().unwrap();
    let st = stable(&d.path().join("ev"));
    let lock = d.path().join("durum/notifier.lock");
    let v30 = app(&d.path().join("Cellar/30/Itero.app"), 30, "otuz");
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let (v30, st, lock) = (v30.clone(), st.clone(), lock.clone());
            std::thread::spawn(move || up_to_date(&v30, &st, &lock))
        })
        .collect();
    for t in threads {
        let p = t.join().unwrap().unwrap();
        assert_eq!(run(&p), "sürüm 30");
    }
    assert_eq!(version(&st), Some(30));
    assert!(!st.join("Contents.new").exists() && !st.join("Contents.old").exists());
}

#[test]
fn a_shipped_app_with_no_version_is_an_error_naming_it() {
    let d = tempfile::tempdir().unwrap();
    let bad = d.path().join("Kırık.app");
    std::fs::create_dir_all(bad.join("Contents")).unwrap();
    let err = up_to_date(&bad, &stable(&d.path().join("ev")), &d.path().join("k")).unwrap_err();
    assert!(err.contains("Kırık.app"), "{err}");
}

#[test]
fn a_copy_waits_for_the_lock_another_holds_and_keeps_links_as_links() {
    let d = tempfile::tempdir().unwrap();
    let st = stable(&d.path().join("ev"));
    let lock = d.path().join("durum/notifier.lock");
    let v40 = app(&d.path().join("Cellar/40/Itero.app"), 40, "kırk");
    // a framework's links, as a real bundle has them
    std::os::unix::fs::symlink("MacOS/itero-notifier", v40.join("Contents/kısayol")).unwrap();
    // another step copying: this one waits for it
    let held = itero::engine::lock::take_file(&lock, "başka adım")
        .ok()
        .unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        drop(held);
    });
    let p = up_to_date(&v40, &st, &lock).unwrap();
    release.join().unwrap();
    assert_eq!(run(&p), "sürüm 40");
    assert_eq!(
        std::fs::read_link(st.join("Contents/kısayol")).unwrap(),
        Path::new("MacOS/itero-notifier")
    );
    // a lock that cannot be made: said, nothing copied
    let file = d.path().join("dosya");
    std::fs::write(&file, "").unwrap();
    let st2 = stable(&d.path().join("ev2"));
    let e = up_to_date(&v40, &st2, &file.join("notifier.lock")).unwrap_err();
    assert!(e.contains("dosya"), "{e}");
    assert!(!st2.exists());
}
