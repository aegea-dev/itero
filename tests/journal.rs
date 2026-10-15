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
