#![cfg(any(target_os = "macos", target_os = "linux"))]
use itero::watch::{self, Kind};
use std::io::Write;
use std::time::{Duration, Instant};

/// The kinds seen within `secs`, or until one of `kind` comes.
fn until(b: &mut Box<dyn watch::Backend>, kind: Kind, secs: u64) -> Vec<Kind> {
    let end = Instant::now() + Duration::from_secs(secs);
    let mut seen = vec![];
    while Instant::now() < end {
        for c in b.wait(Some(Duration::from_millis(200))).unwrap() {
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
fn a_file_changing_in_a_folder_is_a_change() {
    // names with a composed letter: FSEvents compares them decomposed
    let d = tempfile::tempdir().unwrap();
    let folder = d.path().join("gelen kutusü");
    std::fs::create_dir(&folder).unwrap();
    let f = folder.join("fatüra.txt");
    append(&f, "ilk");
    let mut b = watch::open(&folder).unwrap();
    append(&f, "ikinci");
    let seen = until(&mut b, Kind::Changed, 3);
    assert!(seen.contains(&Kind::Changed), "{seen:?}");
}

#[test]
fn a_file_changing_in_a_subfolder_is_no_change() {
    let d = tempfile::tempdir().unwrap();
    let folder = d.path().join("gelen kutusu");
    let sub = folder.join("eski");
    std::fs::create_dir_all(&sub).unwrap();
    let f = sub.join("makbuz.txt");
    append(&f, "ilk");
    let mut b = watch::open(&folder).unwrap();
    append(&f, "ikinci");
    let seen = until(&mut b, Kind::Changed, 1);
    assert!(seen.is_empty(), "{seen:?}");
}

#[test]
fn a_path_through_a_link_is_watched() {
    let d = tempfile::tempdir().unwrap();
    let real = d.path().join("gerçek");
    std::fs::create_dir(&real).unwrap();
    let link = d.path().join("bağ");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let f = link.join("not.txt");
    append(&f, "ilk");
    let mut b = watch::open(&f).unwrap();
    append(&f, "ikinci");
    let seen = until(&mut b, Kind::Changed, 3);
    assert!(seen.contains(&Kind::Changed), "{seen:?}");
}

#[test]
fn nothing_happening_is_no_change() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("sessiz");
    append(&f, "ı");
    let mut b = watch::open(&f).unwrap();
    assert!(b.wait(Some(Duration::from_millis(300))).unwrap().is_empty());
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

/// The paths changes were seen for within `secs`, until all of `want`
/// (none: all of `secs`).
fn paths_until(
    b: &mut Box<dyn watch::Backend>,
    want: &[std::path::PathBuf],
    secs: u64,
) -> std::collections::BTreeSet<std::path::PathBuf> {
    let end = Instant::now() + Duration::from_secs(secs);
    let mut seen = std::collections::BTreeSet::new();
    while Instant::now() < end && (want.is_empty() || !want.iter().all(|p| seen.contains(p))) {
        for c in b.wait(Some(Duration::from_millis(200))).unwrap() {
            seen.insert(c.path);
        }
    }
    seen
}

#[test]
fn one_backend_watches_many_paths_and_takes_new_ones() {
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("bir.txt");
    let sub = d.path().join("klasör");
    std::fs::create_dir(&sub).unwrap();
    let b2 = sub.join("iki.txt");
    let c = d.path().join("üç.txt");
    for f in [&a, &b2, &c] {
        append(f, "ş");
    }
    let mut b = watch::open_many(&[a.clone(), b2.clone(), c.clone()]).unwrap();
    for f in [&a, &b2, &c] {
        append(f, "ğ");
    }
    let all = [a.clone(), b2.clone(), c.clone()];
    let seen = paths_until(&mut b, &all, 3);
    assert_eq!(
        seen,
        all.iter().cloned().collect(),
        "each path, by its own name"
    );
    // a path set later is watched; one dropped is not
    let later = d.path().join("sonra.txt");
    b.set(&[a.clone(), later.clone()]).unwrap();
    append(&later, "é");
    append(&c, "ü");
    let seen = paths_until(&mut b, std::slice::from_ref(&later), 3);
    assert!(seen.contains(&later), "{seen:?}");
    let seen = paths_until(&mut b, &[], 1);
    assert!(!seen.contains(&c), "{seen:?}");
    append(&a, "ı");
    let seen = paths_until(&mut b, std::slice::from_ref(&a), 3);
    assert!(seen.contains(&a), "the one kept is still watched: {seen:?}");
}
