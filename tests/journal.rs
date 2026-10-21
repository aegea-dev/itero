use itero::engine::journal::{Envelope, Journal, SCHEMA_VERSION, Start};
use serde_json::{Value, json};

const NOW: &str = "2026-10-21T10:00:00Z";

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

#[test]
fn the_runs_of_the_old_form_end_and_their_events_stay() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    std::fs::create_dir_all(d.path().join(".itero")).unwrap();
    let db = rusqlite::Connection::open(ws.journal()).unwrap();
    db.execute_batch(VERSION_1).unwrap();
    db.execute_batch(
        "UPDATE meta SET value = '4' WHERE key = 'schema';
         ALTER TABLE events ADD COLUMN note TEXT;
         CREATE TABLE queued (seq INTEGER PRIMARY KEY AUTOINCREMENT, workflow TEXT NOT NULL, args TEXT NOT NULL, input TEXT NOT NULL, at TEXT NOT NULL);
         CREATE TABLE paused (workflow TEXT PRIMARY KEY, pin TEXT NOT NULL, at TEXT NOT NULL, why TEXT NOT NULL);
         CREATE TABLE cursors (source TEXT PRIMARY KEY, value TEXT NOT NULL);
         ALTER TABLE runs ADD COLUMN input TEXT;
         INSERT INTO runs (id, workflow, pin, runtime, args, started) VALUES ('r-eski', 'posta', 'posta@1', '{\"state\": \"bekle\"}', '{}', '2026-10-15T14:00:00Z');
         INSERT INTO timers (run, at, data) VALUES ('r-eski', '2026-10-15T15:00:00Z', '{}');
         INSERT INTO events (id, time, type, source, subject, data) VALUES ('e-koşu', '2026-10-15T14:00:00Z', 'run.started', 'daemon', 'itero.run.r-eski', '{}');",
    )
    .unwrap();
    drop(db);
    let j = Journal::open(&ws).unwrap();
    assert!(j.runs().unwrap().is_empty(), "the old runs end");
    assert!(j.due_timers("2027-01-01T00:00:00Z").unwrap().is_empty());
    let all = j.events(None).unwrap();
    assert_eq!(all.len(), 2, "their events stay, as history");
    let old = all.iter().find(|e| e.event.id == "e-koşu").unwrap();
    assert!(old.handled.is_some());
    assert_eq!(old.note.as_deref(), Some("a run of the old form"));
    // a message not taken yet is still waiting for the daemon
    assert_eq!(j.next().unwrap().unwrap().event.id, "e-eski");
}

fn start(workflow: &str, flows: Value) -> Start {
    Start {
        workflow: workflow.into(),
        flows: serde_json::from_value(flows).unwrap(),
        input: json!({"sipariş": 43}),
        cause: None,
    }
}

#[test]
fn a_version_of_a_runs_workflows_is_pinned_once() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&ws(d.path())).unwrap();
    let flows = json!({"sipariş": {"on": ["order.placed"], "steps": ["#gönder"]}});
    let a = j
        .start_run(&start("sipariş", flows.clone()), NOW, None)
        .unwrap();
    let b = j.start_run(&start("sipariş", flows), NOW, None).unwrap();
    let (ra, rb) = (j.run(&a).unwrap().unwrap(), j.run(&b).unwrap().unwrap());
    assert_eq!(ra.pin, rb.pin, "the same files, the same version");
    assert_eq!(ra.pin.len(), 36, "a UUIDv7: {}", ra.pin);
    assert_eq!(ra.input, json!({"sipariş": 43}));
    let c = j
        .start_run(
            &start("sipariş", json!({"sipariş": {"steps": ["#kargola"]}})),
            NOW,
            None,
        )
        .unwrap();
    let rc = j.run(&c).unwrap().unwrap();
    assert_ne!(rc.pin, ra.pin);
    let (name, pinned) = j.pinned(&rc.pin).unwrap().unwrap();
    assert_eq!(name, "sipariş");
    assert_eq!(pinned["sipariş"]["steps"][0], "#kargola");
}

#[test]
fn a_message_starts_runs_and_reaches_waiting_runs_in_one_go() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&ws(d.path())).unwrap();
    j.publish(&Envelope::new(
        "outside",
        "payment.received",
        "itero.app",
        NOW,
        None,
        json!({"order": 43}),
    ))
    .unwrap();
    let m = j.next().unwrap().unwrap();
    let waiting = j
        .start_run(
            &start("sipariş", json!({"sipariş": {"steps": ["#a"]}})),
            NOW,
            None,
        )
        .unwrap();
    let mut s = start("kayıt", json!({"kayıt": {"steps": ["#yaz"]}}));
    s.cause = Some(m.event.id.clone());
    j.route(&m, &[s], std::slice::from_ref(&waiting), NOW)
        .unwrap();
    let copies = j.events(Some(&format!("itero.run.{waiting}"))).unwrap();
    let copy = copies.iter().find(|e| e.event.kind == "message").unwrap();
    assert_eq!(copy.event.data["id"], m.event.id);
    assert_eq!(copy.event.data["type"], "payment.received");
    assert_eq!(copy.event.data["data"]["order"], 43);
    let started = j.runs().unwrap();
    let new = started.iter().find(|r| r.workflow == "kayıt").unwrap();
    assert_eq!(
        j.started_by(&new.id).unwrap().as_deref(),
        Some(m.event.id.as_str())
    );
    assert!(j.events(Some("itero.app")).unwrap()[0].handled.is_some());
}

#[test]
fn a_day_after_they_end_runs_and_their_messages_are_forgotten() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    let mut j = Journal::open(&ws).unwrap();
    let flows = json!({"a": {"steps": ["#b"]}});
    let old = j
        .start_run(&start("a", flows.clone()), "2026-10-13T10:00:00Z", None)
        .unwrap();
    let recent = j
        .start_run(&start("a", flows.clone()), "2026-10-15T10:00:00Z", None)
        .unwrap();
    let under_way = j
        .start_run(&start("a", flows), "2026-10-14T09:00:00Z", None)
        .unwrap();
    let db = rusqlite::Connection::open(ws.journal()).unwrap();
    db.execute(
        "UPDATE runs SET ended = '2026-10-13T11:00:00Z' WHERE id = ?1",
        [&old],
    )
    .unwrap();
    db.execute(
        "UPDATE runs SET ended = '2026-10-15T11:00:00Z' WHERE id = ?1",
        [&recent],
    )
    .unwrap();
    for (id, time) in [
        ("m-eski", "2026-10-13T08:00:00Z"),
        ("m-yeni", "2026-10-14T10:00:00Z"),
    ] {
        db.execute(
            "INSERT INTO events (id, time, type, source, subject, data, handled) VALUES (?1, ?2, 'a.b', 'outside', 'itero.app', '{}', ?2)",
            [id, time],
        )
        .unwrap();
    }
    j.prune("2026-10-14T12:00:00Z").unwrap();
    let ids: Vec<String> = j.runs().unwrap().into_iter().map(|r| r.id).collect();
    assert!(!ids.contains(&old), "ended two days ago: gone");
    assert!(ids.contains(&recent) && ids.contains(&under_way));
    assert!(
        j.events(Some(&format!("itero.run.{old}")))
            .unwrap()
            .is_empty()
    );
    let messages: Vec<String> = j
        .events(Some("itero.app"))
        .unwrap()
        .into_iter()
        .map(|e| e.event.id)
        .collect();
    // the old one goes; the newer one may still be wanted by the run under way
    assert_eq!(messages, vec!["m-yeni"]);
}
