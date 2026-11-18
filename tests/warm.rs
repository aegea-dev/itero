//! Warm actions: which Python, a file's identity, the runner.
#![cfg(unix)]
use itero::engine::warm::{self, Python};
use std::path::Path;

fn file(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
    let f = dir.join(name);
    std::fs::write(&f, text).unwrap();
    f
}

#[test]
fn which_python_comes_from_the_first_line() {
    assert_eq!(
        warm::python("#!/usr/bin/env python3").unwrap(),
        Python::Plain("python3".into())
    );
    assert_eq!(
        warm::python("#!/usr/bin/python3.12").unwrap(),
        Python::Plain("/usr/bin/python3.12".into())
    );
    assert_eq!(
        warm::python("#!/usr/bin/env -S uv run --script").unwrap(),
        Python::UvScript
    );
    assert_eq!(
        warm::python("#!/usr/bin/env -S uv run --project actions/py/güzel python").unwrap(),
        Python::UvProject("actions/py/güzel".into())
    );
    for (first, runs) in [
        ("#!/bin/sh", "sh"),
        ("#!/usr/bin/env node", "node"),
        ("#!/usr/bin/env -S bun --install=force", "bun"),
    ] {
        assert_eq!(
            warm::python(first).unwrap_err(),
            format!("warm: only Python actions can be warm: its first line runs {runs}")
        );
    }
}

#[test]
fn a_changed_file_has_another_identity() {
    let d = tempfile::tempdir().unwrap();
    let f = file(d.path(), "şeftali", "#!/usr/bin/env python3\n");
    let before = warm::identity(&f).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&f, "#!/usr/bin/env python3\nprint('ılık')\n").unwrap();
    assert_ne!(before, warm::identity(&f).unwrap());
}

#[test]
fn copies_are_at_most_the_cpus_and_the_steps() {
    let cpus = std::thread::available_parallelism().unwrap().get();
    assert_eq!(warm::most_copies(1), 1);
    assert_eq!(warm::most_copies(10_000), cpus);
}
