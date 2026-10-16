use itero::engine::journal::{Envelope, Journal, SCHEMA_VERSION};
use serde_json::json;

/// A journal as iteration 1 made it: schema 1, events without `note`.
const VERSION_1: &str = "
CREATE TABLE events (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  id TEXT NOT NULL UNIQUE,
  time TEXT NOT NULL,
  type TEXT NOT NULL,
  source TEXT NOT NULL,
  subject TEXT NOT NULL,
  cause TEXT,
  data TEXT NOT NULL,
  handled TEXT,
  error TEXT
);
CREATE TABLE runs (id TEXT PRIMARY KEY, workflow TEXT NOT NULL, pin TEXT NOT NULL, runtime TEXT NOT NULL, args TEXT NOT NULL, started TEXT NOT NULL, ended TEXT);
CREATE TABLE workflows (pin TEXT PRIMARY KEY, definition TEXT NOT NULL);
CREATE TABLE timers (id INTEGER PRIMARY KEY AUTOINCREMENT, run TEXT NOT NULL, at TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE jobs (seq INTEGER PRIMARY KEY, runs INTEGER NOT NULL, lost TEXT);
CREATE TABLE schedules (key TEXT PRIMARY KEY, last TEXT NOT NULL);
CREATE TABLE memory (workflow TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY (workflow, key));
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
INSERT INTO meta (key, value) VALUES ('schema', '1');
INSERT INTO events (id, time, type, source, subject, data) VALUES ('e-eski', '2026-10-15T14:00:00Z', 'eski.olay', 'outside', 'itero.app', '{\"söz\": \"çay\"}');
";

fn ws(d: &std::path::Path) -> itero::workspace::Workspace {
    std::fs::write(d.join("itero.toml"), "").unwrap();
    itero::workspace::find(d).unwrap()
}

#[test]
fn a_journal_from_an_earlier_itero_is_brought_up_to_date() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    std::fs::create_dir_all(d.path().join(".itero")).unwrap();
    rusqlite::Connection::open(ws.journal())
        .unwrap()
        .execute_batch(VERSION_1)
        .unwrap();
    let j = Journal::open(&ws).unwrap();
    let all = j.events(None).unwrap();
    assert_eq!(all.len(), 1, "what it held is kept");
    assert_eq!(all[0].event.data["söz"], "çay");
    assert_eq!(all[0].note, None);
    j.publish(&Envelope::new(
        "outside",
        "yeni.olay",
        "itero.app",
        "2026-10-15T14:01:00Z",
        None,
        json!({}),
    ))
    .unwrap();
    assert_eq!(j.events(None).unwrap().len(), 2);
    drop(j);
    // opening again changes nothing
    assert_eq!(Journal::open(&ws).unwrap().events(None).unwrap().len(), 2);
    let version: String = rusqlite::Connection::open(ws.journal())
        .unwrap()
        .query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION.to_string());
}

#[test]
fn a_journal_from_a_newer_itero_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    drop(Journal::open(&ws).unwrap());
    rusqlite::Connection::open(ws.journal())
        .unwrap()
        .execute("UPDATE meta SET value = '99' WHERE key = 'schema'", [])
        .unwrap();
    let e = format!("{:#}", Journal::open(&ws).err().unwrap());
    assert!(e.contains("newer itero"), "{e}");
}

use itero::engine::journal::{Handled, Pause, Started};
use serde_json::{Map, Value};

fn ws_with(d: &std::path::Path) -> itero::workspace::Workspace {
    std::fs::write(d.join("itero.toml"), "").unwrap();
    itero::workspace::find(d).unwrap()
}

fn tek(then: &str) -> itero::workflow::File {
    itero::workflow::parse(&format!(
        r#"{{ "name": "tek", "description": "tek seferde bir", "states": ["bekle", "bitti"], "start": "bekle", "end": ["bitti"],
             "concurrency": {{ "max": 1, "then": "{then}" }},
             "transitions": [ {{ "from": "bekle", "to": "bitti", "on": "tamam" }} ] }}"#
    ))
    .unwrap()
}

/// An answer that changes nothing, to fill in with what a test changes.
fn nothing(at: &str) -> Handled<'_> {
    Handled {
        seq: 0,
        at,
        error: None,
        run: None,
        timers: None,
        events: vec![],
        memory: None,
        pause: None,
    }
}

#[test]
fn a_start_beyond_the_maximum_is_queued_and_released_when_a_slot_frees() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&ws_with(d.path())).unwrap();
    let f = tek("queue");
    let args = Map::new();
    let first = j
        .start_run(&f, &args, "2026-10-16T10:00:00Z", None)
        .unwrap();
    let Started::Run(id) = first else {
        panic!("{first:?}")
    };
    assert_eq!(
        j.start_run(&f, &args, "2026-10-16T10:00:01Z", None)
            .unwrap(),
        Started::Queued
    );
    assert!(
        j.release_queued(std::slice::from_ref(&f), "2026-10-16T10:00:02Z")
            .unwrap()
            .is_empty(),
        "still full"
    );
    // the first run ends
    let mut rt = j.run(&id).unwrap().unwrap().runtime;
    rt.state = "bitti".into();
    j.handle(Handled {
        run: Some((&id, &rt, true)),
        ..nothing("2026-10-16T10:00:03Z")
    })
    .unwrap();
    let released = j.release_queued(&[f], "2026-10-16T10:00:04Z").unwrap();
    assert_eq!(released.len(), 1);
    assert_eq!(j.runs().unwrap().len(), 2);
}

#[test]
fn a_start_beyond_the_maximum_is_skipped_when_the_workflow_says_so() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&ws_with(d.path())).unwrap();
    let f = tek("skip");
    j.start_run(&f, &Map::new(), "2026-10-16T10:00:00Z", None)
        .unwrap();
    assert_eq!(
        j.start_run(&f, &Map::new(), "2026-10-16T10:00:01Z", None)
            .unwrap(),
        Started::Skipped
    );
    assert_eq!(j.runs().unwrap().len(), 1);
}

#[test]
fn a_paused_workflow_starts_nothing_until_resumed() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&ws_with(d.path())).unwrap();
    let f = tek("queue");
    j.handle(Handled {
        pause: Some(Pause {
            workflow: "tek",
            pin: "tek@abc",
            why: "5 başarısız".into(),
        }),
        ..nothing("2026-10-16T10:00:00Z")
    })
    .unwrap();
    assert_eq!(j.paused().unwrap()[0].why, "5 başarısız");
    assert_eq!(
        j.start_run(&f, &Map::new(), "2026-10-16T10:00:01Z", None)
            .unwrap(),
        Started::Paused
    );
    assert!(j.resume("tek").unwrap());
    assert!(!j.resume("tek").unwrap(), "not paused any more");
    assert!(matches!(
        j.start_run(&f, &Map::new(), "2026-10-16T10:00:02Z", None)
            .unwrap(),
        Started::Run(_)
    ));
}

#[test]
fn failed_runs_are_counted_in_a_row_from_the_newest() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&ws_with(d.path())).unwrap();
    let f = itero::workflow::parse(
        r#"{ "name": "sayac", "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
             "transitions": [ { "from": "basla", "to": "bitti", "on": "tamam" } ] }"#,
    )
    .unwrap();
    for (i, state) in ["failed", "bitti", "failed", "failed"].iter().enumerate() {
        let at = format!("2026-10-16T10:00:0{i}Z");
        let Started::Run(id) = j.start_run(&f, &Map::new(), &at, None).unwrap() else {
            panic!()
        };
        let mut rt = j.run(&id).unwrap().unwrap().runtime;
        rt.state = state.to_string();
        j.handle(Handled {
            run: Some((&id, &rt, true)),
            ..nothing(&at)
        })
        .unwrap();
    }
    assert_eq!(j.failed_in_a_row("sayac").unwrap(), 2);
    assert_eq!(j.failed_in_a_row("başka").unwrap(), 0);
}

#[test]
fn a_cursor_is_kept_per_source() {
    let d = tempfile::tempdir().unwrap();
    let j = Journal::open(&ws_with(d.path())).unwrap();
    assert_eq!(j.cursor("posta").unwrap(), None);
    j.set_cursor("posta", &json!({"uid": 42, "kutu": "Gelen"}))
        .unwrap();
    j.set_cursor("posta", &json!({"uid": 43, "kutu": "Gelen"}))
        .unwrap();
    assert_eq!(
        j.cursor("posta").unwrap(),
        Some(json!({"uid": 43, "kutu": "Gelen"}))
    );
    let other: Option<Value> = j.cursor("başka").unwrap();
    assert_eq!(other, None);
}
