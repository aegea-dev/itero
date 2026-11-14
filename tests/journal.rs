use itero::engine::journal::{
    CANCELLED, Cancelled, Child, Envelope, Handled, Journal, SCHEMA_VERSION, Start,
};
use serde_json::{Value, json};

const NOW: &str = "2026-10-21T10:00:00Z";

/// A journal as itero 0.4.0 to 0.8.0 made it: schema 7, with the tables
/// nothing reads any more (queued, paused, cursors, memory).
const VERSION_7: &str = "
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
  error TEXT,
  note TEXT,
  reply TEXT
);
CREATE TABLE runs (id TEXT PRIMARY KEY, workflow TEXT NOT NULL, pin TEXT NOT NULL, runtime TEXT NOT NULL, args TEXT NOT NULL, started TEXT NOT NULL, ended TEXT, input TEXT, name TEXT, message TEXT, reply TEXT);
CREATE TABLE workflows (pin TEXT PRIMARY KEY, definition TEXT NOT NULL);
CREATE TABLE timers (id INTEGER PRIMARY KEY AUTOINCREMENT, run TEXT NOT NULL, at TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE jobs (seq INTEGER PRIMARY KEY, runs INTEGER NOT NULL, lost TEXT);
CREATE TABLE schedules (key TEXT PRIMARY KEY, last TEXT NOT NULL);
CREATE TABLE memory (workflow TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY (workflow, key));
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE queued (seq INTEGER PRIMARY KEY AUTOINCREMENT, workflow TEXT NOT NULL, args TEXT NOT NULL, input TEXT NOT NULL, at TEXT NOT NULL);
CREATE TABLE paused (workflow TEXT PRIMARY KEY, pin TEXT NOT NULL, at TEXT NOT NULL, why TEXT NOT NULL);
CREATE TABLE cursors (source TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE workflow_ids (name TEXT PRIMARY KEY, id TEXT NOT NULL, created TEXT NOT NULL, itero TEXT NOT NULL);
CREATE TABLE replies (id INTEGER PRIMARY KEY AUTOINCREMENT, url TEXT NOT NULL, body TEXT NOT NULL, first TEXT NOT NULL, next TEXT NOT NULL, tries INTEGER NOT NULL DEFAULT 0);
INSERT INTO meta (key, value) VALUES ('schema', '7');
INSERT INTO events (id, time, type, source, subject, data) VALUES ('e-eski', '2026-10-15T14:00:00Z', 'eski.olay', 'outside', 'itero.app', '{\"söz\": \"çay\"}');
";

/// The tables a journal has, by name.
fn tables(ws: &itero::workspace::Workspace) -> Vec<String> {
    let db = rusqlite::Connection::open(ws.journal()).unwrap();
    let mut q = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap();
    q.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn ws(d: &std::path::Path) -> itero::workspace::Workspace {
    std::fs::write(d.join("itero.config.json"), "{}").unwrap();
    itero::workspace::find(d).unwrap()
}

#[test]
fn a_journal_from_itero_0_4_to_0_8_keeps_what_it_held() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    std::fs::create_dir_all(d.path().join(".itero")).unwrap();
    rusqlite::Connection::open(ws.journal())
        .unwrap()
        .execute_batch(VERSION_7)
        .unwrap();
    let j = Journal::open(&ws).unwrap();
    let all = j.events(None).unwrap();
    assert_eq!(all.len(), 1, "what it held is kept");
    assert_eq!(all[0].event.data["söz"], "çay");
    drop(j);
    let left = tables(&ws);
    for gone in ["queued", "paused", "cursors", "memory", "replies"] {
        assert!(!left.contains(&gone.to_string()), "{gone}: {left:?}");
    }
    // and the windows' tables, as a new journal has them
    for made in ["seen", "kept", "batches", "under_way"] {
        assert!(left.contains(&made.to_string()), "{made}: {left:?}");
    }
    // nor the columns callbacks used
    let db = rusqlite::Connection::open(ws.journal()).unwrap();
    for table in ["events", "runs"] {
        let mut s = db.prepare(&format!("PRAGMA table_info({table})")).unwrap();
        let columns: Vec<String> = s
            .query_map([], |r| r.get(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(
            !columns.contains(&"reply".to_string()),
            "{table}: {columns:?}"
        );
        if table == "runs" {
            // and a run's parent, empty for the runs it held
            for c in ["parent", "parent_path"] {
                assert!(columns.contains(&c.to_string()), "{c}: {columns:?}");
            }
        }
    }
    // opening again changes nothing
    assert_eq!(Journal::open(&ws).unwrap().events(None).unwrap().len(), 1);
    let version: String = rusqlite::Connection::open(ws.journal())
        .unwrap()
        .query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION.to_string());
}

#[test]
fn a_new_journal_has_only_the_tables_itero_reads() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    drop(Journal::open(&ws).unwrap());
    assert_eq!(
        tables(&ws),
        [
            "batches",
            "events",
            "jobs",
            "kept",
            "meta",
            "runs",
            "schedules",
            "seen",
            "timers",
            "under_way",
            "workflow_ids",
            "workflows"
        ]
    );
}

#[test]
fn a_journal_from_before_itero_0_4_is_refused_saying_what_to_do() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    std::fs::create_dir_all(d.path().join(".itero")).unwrap();
    let db = rusqlite::Connection::open(ws.journal()).unwrap();
    db.execute_batch(VERSION_7).unwrap();
    db.execute("UPDATE meta SET value = '6' WHERE key = 'schema'", [])
        .unwrap();
    drop(db);
    let e = format!("{:#}", Journal::open(&ws).err().unwrap());
    assert!(e.contains("0.4.0") && e.contains(".itero"), "{e}");
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

fn start(workflow: &str, flows: Value) -> Start {
    Start {
        workflow: workflow.into(),
        flows: serde_json::from_value(flows).unwrap(),
        input: json!({"sipariş": 43}),
        cause: None,
        gates: vec![],
    }
}

#[test]
fn a_version_of_a_runs_workflows_is_pinned_once() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&ws(d.path())).unwrap();
    let flows = json!({"sipariş": {"on": [{ "event": "order.placed" }], "steps": ["#gönder"]}});
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
    let m = j.next(0).unwrap().unwrap();
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

#[test]
fn a_run_started_by_a_step_keeps_its_parent_and_the_step() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&ws(d.path())).unwrap();
    let flows = json!({
        "sipariş": {"on": [{ "event": "order.placed" }], "steps": ["#öde", "günlük"]},
        "günlük": {"steps": ["#yaz"]}
    });
    let parent = j
        .start_run(&start("sipariş", flows.clone()), NOW, None)
        .unwrap();
    let rt = j.run(&parent).unwrap().unwrap().runtime;
    let started = j.next(0).unwrap().unwrap();
    let pinned = serde_json::from_value(flows).unwrap();
    let input = json!({"gün": "salı"});
    j.handle(Handled {
        let_through: vec![],
        seq: started.seq,
        at: NOW,
        error: None,
        run: Some((&parent, &rt, false)),
        timers: None,
        events: vec![],
        shown: vec![],
        next_round: None,
        children: vec![Child {
            workflow: "günlük",
            flows: &pinned,
            input: &input,
            seed: &serde_json::Map::new(),
            path: vec![0, 1],
        }],
        stopped: vec![],
        stopped_why: "",
    })
    .unwrap();
    let runs = j.runs().unwrap();
    assert_eq!(runs.len(), 2);
    let child = runs.iter().find(|r| r.workflow == "günlük").unwrap();
    assert_eq!(child.parent.as_deref(), Some(parent.as_str()));
    assert_eq!(child.parent_path, Some(vec![0, 1]));
    assert_eq!(child.input, input, "its $event: the step's args");
    let (name, flows) = j.pinned(&child.pin).unwrap().unwrap();
    assert_eq!(name, "günlük", "pinned as its own workflow");
    assert_eq!(flows, pinned, "with the versions its parent runs");
    let first = j.events(Some(&format!("itero.run.{}", child.id))).unwrap();
    assert_eq!(first[0].event.kind, "run.started");
    assert_eq!(first[0].event.data, input);
    let p = runs.iter().find(|r| r.id == parent).unwrap();
    assert_eq!((p.parent.as_deref(), p.parent_path.as_ref()), (None, None));
}

fn two_words(name: &str) -> bool {
    let w: Vec<&str> = name.split('-').collect();
    w.len() == 2
        && w.iter()
            .all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase()))
}

#[test]
fn the_workspace_has_one_identity_kept_in_its_journal() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    let a = Journal::open(&ws).unwrap().workspace().unwrap();
    assert_eq!(a.id.len(), 36, "a UUIDv7: {}", a.id);
    assert!(two_words(&a.name), "{}", a.name);
    assert_eq!(a.itero, env!("CARGO_PKG_VERSION"));
    assert!(a.created.ends_with('Z'), "{}", a.created);
    assert_eq!(Journal::open(&ws).unwrap().workspace().unwrap(), a);
}

#[test]
fn an_earlier_journal_gets_an_identity_too() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    std::fs::create_dir_all(d.path().join(".itero")).unwrap();
    rusqlite::Connection::open(ws.journal())
        .unwrap()
        .execute_batch(VERSION_7)
        .unwrap();
    let w = Journal::open(&ws).unwrap().workspace().unwrap();
    assert_eq!(w.id.len(), 36);
}

#[test]
fn a_workflow_keeps_its_id_while_its_name_stays() {
    let d = tempfile::tempdir().unwrap();
    let j = Journal::open(&ws(d.path())).unwrap();
    let a = j.workflow_id("sipariş", NOW).unwrap();
    let b = j.workflow_id("sipariş", "2026-10-22T10:00:00Z").unwrap();
    assert_eq!(a, b);
    assert_eq!((a.name.as_str(), a.created.as_str()), ("sipariş", NOW));
    assert_ne!(j.workflow_id("kargo", NOW).unwrap().id, a.id);
}

#[test]
fn a_run_has_a_two_word_name_and_is_found_by_it_or_its_id() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    let mut j = Journal::open(&ws).unwrap();
    let flows = json!({"sipariş": {"steps": ["#gönder"]}});
    let a = j
        .start_run(&start("sipariş", flows.clone()), NOW, None)
        .unwrap();
    let ra = j.run(&a).unwrap().unwrap();
    assert!(two_words(&ra.name), "{}", ra.name);
    assert_eq!(j.run_by(&a).unwrap(), vec![ra.clone()]);
    assert_eq!(j.run_by(&ra.name).unwrap(), vec![ra.clone()]);
    assert!(j.run_by("yok-böyle").unwrap().is_empty());
    let b = j
        .start_run(&start("sipariş", flows), "2026-10-21T11:00:00Z", None)
        .unwrap();
    rusqlite::Connection::open(ws.journal())
        .unwrap()
        .execute("UPDATE runs SET name = ?1", [&ra.name])
        .unwrap();
    let both: Vec<String> = j
        .run_by(&ra.name)
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(both, vec![b, a], "two with one name: the newest first");
}

#[test]
fn a_workspaces_id_is_read_while_the_journal_is_being_written() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("itero.config.json"), "{}").unwrap();
    let ws = itero::workspace::find(d.path()).unwrap();
    let id = Journal::open(&ws).unwrap().workspace().unwrap().id;
    let writing = rusqlite::Connection::open(ws.journal()).unwrap();
    writing.execute_batch("BEGIN IMMEDIATE").unwrap();
    let again = itero::workspace::find(d.path()).unwrap();
    let t = std::time::Instant::now();
    assert_eq!(again.id().unwrap(), id);
    assert!(
        t.elapsed() < std::time::Duration::from_secs(2),
        "{:?}",
        t.elapsed()
    );
    writing.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn an_event_not_yet_handled_is_cancelled_and_one_handled_is_not() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&ws(d.path())).unwrap();
    for kind in ["sipariş.geldi", "ödeme.geldi"] {
        j.publish(&Envelope::new(
            "outside",
            kind,
            "itero.app",
            NOW,
            None,
            json!({"n": 1}),
        ))
        .unwrap();
    }
    let q = j.queued().unwrap();
    assert_eq!(
        q.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(),
        ["sipariş.geldi", "ödeme.geldi"]
    );
    assert_eq!(q[0].data, json!({"n": 1}));
    assert!(matches!(j.cancel(&q[0].id, NOW).unwrap(), Cancelled::Done));
    assert_eq!(j.queued().unwrap().len(), 1);
    let e = j.events(Some("itero.app")).unwrap();
    assert_eq!(e[0].note.as_deref(), Some(CANCELLED));
    // handled already: refused, saying so
    assert!(matches!(
        j.cancel(&q[0].id, NOW).unwrap(),
        Cancelled::Handled { .. }
    ));
    assert!(matches!(j.cancel("yok", NOW).unwrap(), Cancelled::Missing));
    // taken by a daemon after it was cancelled: it starts nothing
    let r = j.next(0).unwrap().unwrap();
    assert_eq!(r.event.kind, "ödeme.geldi");
    assert!(matches!(
        j.cancel(&r.event.id, NOW).unwrap(),
        Cancelled::Done
    ));
    let mut s = start("kayıt", json!({"kayıt": {"steps": ["#yaz"]}}));
    s.cause = Some(r.event.id.clone());
    assert!(j.route(&r, &[s], &[], NOW).unwrap().started.is_empty());
    assert!(j.runs().unwrap().is_empty(), "cancelled first: no run");
    // and one routed is cancelled no more: it says what it started
    j.publish(&Envelope::new(
        "outside",
        "kayıt.geldi",
        "itero.app",
        NOW,
        None,
        json!({}),
    ))
    .unwrap();
    let r = j.next(0).unwrap().unwrap();
    let mut s = start("kayıt", json!({"kayıt": {"steps": ["#yaz"]}}));
    s.cause = Some(r.event.id.clone());
    let started = j.route(&r, &[s], &[], NOW).unwrap().started;
    match j.cancel(&r.event.id, NOW).unwrap() {
        Cancelled::Handled { started: s, .. } => assert_eq!(s, started),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_run_that_starts_waiting_misses_no_message_but_those_cancelled() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&ws(d.path())).unwrap();
    for kind in ["sipariş.geldi", "ödeme.geldi", "iade.geldi"] {
        j.publish(&Envelope::new(
            "outside",
            kind,
            "itero.app",
            NOW,
            None,
            json!({}),
        ))
        .unwrap();
    }
    // the first routed, the second cancelled, the third not yet taken
    let r = j.next(0).unwrap().unwrap();
    j.route(&r, &[], &[], NOW).unwrap();
    let q = j.queued().unwrap();
    assert!(matches!(j.cancel(&q[0].id, NOW).unwrap(), Cancelled::Done));
    let missed: Vec<String> = j
        .messages_since(NOW, None)
        .unwrap()
        .into_iter()
        .map(|m| m.event.kind)
        .collect();
    assert_eq!(missed, ["sipariş.geldi", "iade.geldi"]);
}

#[test]
fn a_message_copied_to_a_waiting_run_is_cancelled_no_more_naming_the_runs() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&ws(d.path())).unwrap();
    let m = Envelope::new(
        "outside",
        "sipariş.geldi",
        "itero.app",
        NOW,
        None,
        json!({}),
    );
    j.publish(&m).unwrap();
    // two runs started to wait before any daemon routed it
    for run in ["koşu-bir", "koşu-iki"] {
        j.publish(&itero::engine::journal::copy(&m, run, NOW))
            .unwrap();
    }
    match j.cancel(&m.id, NOW).unwrap() {
        Cancelled::Reached { runs } => assert_eq!(runs, ["koşu-bir", "koşu-iki"]),
        other => panic!("{other:?}"),
    }
    // not cancelled: a daemon still routes it
    assert_eq!(j.queued().unwrap().len(), 1);
}

#[test]
fn a_job_is_done_once_handled_or_forgotten_and_not_while_it_waits() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&ws(d.path())).unwrap();
    for _ in 0..2 {
        j.publish(&Envelope::new(
            "manager/r",
            "action.requested",
            "itero.worker",
            NOW,
            None,
            json!({}),
        ))
        .unwrap();
    }
    let jobs = j.jobs().unwrap();
    j.lose_job(jobs[0].seq, "denendi").unwrap();
    assert!(!j.job_done(jobs[0].seq).unwrap(), "waits");
    j.handle(Handled {
        let_through: vec![],
        seq: jobs[1].seq,
        at: NOW,
        error: None,
        run: None,
        timers: None,
        events: vec![],
        shown: vec![],
        next_round: None,
        children: vec![],
        stopped: vec![],
        stopped_why: itero::engine::journal::JOB_STOPPED,
    })
    .unwrap();
    assert!(j.job_done(jobs[1].seq).unwrap(), "handled");
    assert!(j.job_done(999).unwrap(), "forgotten");
}

/// Processes opening one journal that does not exist yet, as a daemon and
/// `itero ws emit` do when a workspace starts: each must wait its turn.
#[cfg(unix)]
#[test]
fn processes_opening_a_new_journal_at_once_all_succeed() {
    use std::process::{Command, Stdio};
    let mut failures = vec![];
    for _ in 0..40 {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("itero.config.json"), "{}").unwrap();
        let children: Vec<_> = (0..8)
            .map(|_| {
                Command::new(env!("CARGO_BIN_EXE_itero"))
                    .args(["ws", "emit", "sipariş.verildi", "{}"])
                    .current_dir(d.path())
                    .env("ITERO_NO_SECRET_STORE", "1")
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for c in children {
            let o = c.wait_with_output().unwrap();
            if !o.status.success() {
                failures.push(String::from_utf8_lossy(&o.stderr).into_owned());
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failed: {failures:#?}",
        failures.len()
    );
}

/// The runs under way are counted as they start and end, whatever ends
/// them, and a run forgotten after it ended counts nothing: the count a
/// pass asks for each message costs the same however many runs there are.
#[test]
fn the_runs_under_way_are_counted_as_they_start_and_end() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    let mut j = Journal::open(&ws).unwrap();
    let flows = json!({"a": {"steps": ["#b"]}, "iç": {"steps": ["#b"]}});
    let mut ids = vec![];
    for _ in 0..3 {
        ids.push(j.start_run(&start("a", flows.clone()), NOW, None).unwrap());
    }
    j.start_run(&start("iç", flows), NOW, None).unwrap();
    assert_eq!(j.active_count(&[]).unwrap(), 4);
    assert_eq!(j.active_count(&["iç".into()]).unwrap(), 3);
    let db = rusqlite::Connection::open(ws.journal()).unwrap();
    db.execute("UPDATE runs SET ended = ?2 WHERE id = ?1", [&ids[0], NOW])
        .unwrap();
    assert_eq!(j.active_count(&["iç".into()]).unwrap(), 2);
    // ended again: counted once
    db.execute("UPDATE runs SET ended = ?2 WHERE id = ?1", [&ids[0], NOW])
        .unwrap();
    assert_eq!(j.active_count(&["iç".into()]).unwrap(), 2);
    j.prune("2026-10-30T00:00:00Z").unwrap();
    assert_eq!(j.active_count(&["iç".into()]).unwrap(), 2);
    let counted: i64 = db
        .query_row("SELECT SUM(runs) FROM under_way", [], |r| r.get(0))
        .unwrap();
    assert_eq!(counted, 3, "kept as runs start and end, not counted");
}
