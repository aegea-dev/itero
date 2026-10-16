#![cfg(any(target_os = "macos", target_os = "linux"))]
use itero::watch::{self, Kind};
use std::io::Write;
use std::time::{Duration, Instant};

/// The kinds seen within `secs`, or until one of `kind` comes.
fn until(b: &mut Box<dyn watch::Backend>, kind: Kind, secs: u64) -> Vec<Kind> {
    let end = Instant::now() + Duration::from_secs(secs);
    let mut seen = vec![];
    while Instant::now() < end {
        for c in b.wait(Duration::from_millis(200)).unwrap() {
            seen.push(c.kind);
        }
        if seen.contains(&kind) {
            break;
        }
    }
    seen
}

fn append(p: &std::path::Path, text: &str) {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p)
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}

#[test]
fn an_append_is_a_change() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("posta kutusu");
    append(&f, "From ayşe\n");
    let mut b = watch::open(&f).unwrap();
    append(&f, "From can\n");
    let seen = until(&mut b, Kind::Changed, 3);
    assert!(seen.contains(&Kind::Changed), "{seen:?}");
}

#[test]
fn a_file_that_comes_later_is_seen_and_then_watched() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("henüz yok");
    let mut b = watch::open(&f).unwrap();
    append(&f, "merhaba\n");
    let seen = until(&mut b, Kind::Created, 3);
    assert!(seen.contains(&Kind::Created), "{seen:?}");
    append(&f, "tekrar\n");
    let seen = until(&mut b, Kind::Changed, 3);
    assert!(seen.contains(&Kind::Changed), "{seen:?}");
}

#[test]
fn an_atomic_save_is_seen_and_the_new_file_watched() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("ayarlar.json");
    append(&f, "{}");
    let mut b = watch::open(&f).unwrap();
    let tmp = d.path().join(".ayarlar.json.yeni");
    std::fs::write(&tmp, "{\"dil\": \"tr\"}").unwrap();
    std::fs::rename(&tmp, &f).unwrap();
    let seen = until(&mut b, Kind::Rescan, 3);
    assert!(!seen.is_empty(), "the replace is seen");
    append(&f, " ");
    let seen = until(&mut b, Kind::Changed, 3);
    assert!(seen.contains(&Kind::Changed), "{seen:?}");
}

#[test]
fn a_removal_is_seen() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("silinecek.txt");
    append(&f, "é");
    let mut b = watch::open(&f).unwrap();
    std::fs::remove_file(&f).unwrap();
    let seen = until(&mut b, Kind::Removed, 3);
    assert!(seen.contains(&Kind::Removed), "{seen:?}");
}

#[test]
fn a_folder_reports_entries_coming_and_going() {
    let d = tempfile::tempdir().unwrap();
    let folder = d.path().join("gelen kutusu");
    std::fs::create_dir(&folder).unwrap();
    let mut b = watch::open(&folder).unwrap();
    append(&folder.join("yeni dosya"), "ü");
    let seen = until(&mut b, Kind::Changed, 3);
    assert!(seen.contains(&Kind::Changed), "{seen:?}");
}

#[test]
fn nothing_happening_is_no_change() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("sessiz");
    append(&f, "ı");
    let mut b = watch::open(&f).unwrap();
    assert!(b.wait(Duration::from_millis(300)).unwrap().is_empty());
}

#[test]
fn a_neighbour_changing_is_no_change() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("benim");
    append(&f, "ı");
    let mut b = watch::open(&f).unwrap();
    append(&d.path().join("komşu"), "ğ");
    let seen = until(&mut b, Kind::Changed, 1);
    assert!(seen.is_empty(), "{seen:?}");
}
