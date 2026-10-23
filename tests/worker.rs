//! The worker: one action file per step.

#[cfg(unix)]
mod steps {
    use itero::engine::worker::{self, Failure, Outcome, StepRun};
    use serde_json::{Map, Value, json};
    use std::path::{Path, PathBuf};

    /// An executable shell action in `dir`.
    fn action(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let f = dir.join(name);
        std::fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        f
    }

    fn step(dir: &Path, file: PathBuf, env: &[(&str, &str)], state: Value) -> StepRun {
        StepRun {
            file,
            root: dir.to_path_buf(),
            workflow: "deneme".into(),
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            state: state.as_object().cloned().unwrap_or_default(),
            memory: dir.join(".itero/memory/sipariş"),
            waiting_since: None,
            quiet: 10,
            most: 10,
            now: String::new(),
        }
    }

    fn done(o: Result<Outcome, Failure>) -> Map<String, Value> {
        match o {
            Ok(Outcome::Done(m)) => m,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_action_reads_its_values_and_the_state_and_prints_json() {
        let d = tempfile::tempdir().unwrap();
        let f = action(
            d.path(),
            "selam",
            r#"read -r state
printf '{"host": "%s", "state": %s, "where": "%s"}' "$host" "$state" "$(pwd)""#,
        );
        let out = done(worker::run_step(&step(
            d.path(),
            f,
            &[("host", "posta.örnek.test")],
            json!({"sipariş": 43}),
        )));
        assert_eq!(out["host"], "posta.örnek.test");
        assert_eq!(out["state"], json!({"sipariş": 43}));
        let here = std::fs::canonicalize(d.path()).unwrap();
        assert_eq!(out["where"], here.display().to_string());
    }

    #[test]
    fn printing_nothing_is_an_empty_output_and_anything_but_an_object_an_error() {
        let d = tempfile::tempdir().unwrap();
        let quiet = action(d.path(), "sessiz", "exit 0");
        assert_eq!(
            done(worker::run_step(&step(d.path(), quiet, &[], json!({})))),
            Map::new()
        );
        for (name, body) in [("liste", "echo '[1]'"), ("yazı", "echo 'çay hazır'")] {
            let f = action(d.path(), name, body);
            let e = worker::run_step(&step(d.path(), f, &[], json!({}))).unwrap_err();
            assert!(e.to_string().contains("not a JSON object"), "{name}: {e}");
        }
    }

    #[test]
    fn exit_75_is_not_yet_and_keeps_what_it_printed() {
        let d = tempfile::tempdir().unwrap();
        let f = action(d.path(), "ödeme", r#"echo '{"ref": "ödeme-1"}'; exit 75"#);
        match worker::run_step(&step(d.path(), f, &[], json!({}))) {
            Ok(Outcome::NotYet(m)) => assert_eq!(m["ref"], "ödeme-1"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn another_exit_is_an_error_with_the_end_of_stderr() {
        let d = tempfile::tempdir().unwrap();
        let f = action(d.path(), "bozuk", "echo 'bağlantı yok' >&2; exit 3");
        match worker::run_step(&step(d.path(), f, &[], json!({}))) {
            Err(Failure::Error(why)) => assert_eq!(why, "exit 3: bağlantı yok"),
            other => panic!("{other:?}"),
        }
        let missing = d.path().join("yok");
        let e = worker::run_step(&step(d.path(), missing, &[], json!({}))).unwrap_err();
        assert!(e.to_string().contains("cannot run"), "{e}");
    }

    #[test]
    fn memory_is_kept_between_runs_and_waiting_since_is_given() {
        let d = tempfile::tempdir().unwrap();
        let f = action(
            d.path(),
            "sayaç",
            r#"n=$(cat "$ITERO_MEMORY" 2>/dev/null || echo 0)
n=$((n + 1)); echo "$n" > "$ITERO_MEMORY"
printf '{"n": %s, "since": "%s"}' "$n" "$ITERO_WAITING_SINCE""#,
        );
        let s = step(d.path(), f, &[], json!({}));
        assert_eq!(done(worker::run_step(&s))["n"], 1);
        let mut again = s.clone();
        again.waiting_since = Some("2026-10-21T10:00:00Z".into());
        let out = done(worker::run_step(&again));
        assert_eq!(out["n"], 2);
        assert_eq!(out["since"], "2026-10-21T10:00:00Z");
    }
}
