use chrono::{DateTime, Utc};
use itero::engine::schedule::due_in;

fn at(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

#[test]
fn every_five_minutes_is_due_once_per_slot() {
    let c = "*/5 * * * *";
    assert!(
        due_in(
            c,
            at("2026-10-15T14:04:59Z"),
            at("2026-10-15T14:05:00Z"),
            &Utc
        )
        .unwrap()
    );
    assert!(
        !due_in(
            c,
            at("2026-10-15T14:05:00Z"),
            at("2026-10-15T14:09:59Z"),
            &Utc
        )
        .unwrap()
    );
    // a daemon asleep for an hour runs it once, not twelve times: one answer
    assert!(
        due_in(
            c,
            at("2026-10-15T13:00:00Z"),
            at("2026-10-15T14:00:30Z"),
            &Utc
        )
        .unwrap()
    );
}

#[test]
fn a_schedule_follows_the_given_time_zone() {
    // 09:00 every day, in a zone three hours ahead of UTC
    let tz = chrono::FixedOffset::east_opt(3 * 3600).unwrap();
    let c = "0 9 * * *";
    assert!(
        due_in(
            c,
            at("2026-10-15T05:59:00Z"),
            at("2026-10-15T06:00:00Z"),
            &tz
        )
        .unwrap()
    );
    assert!(
        !due_in(
            c,
            at("2026-10-15T08:59:00Z"),
            at("2026-10-15T09:00:00Z"),
            &tz
        )
        .unwrap()
    );
}

#[test]
fn a_bad_schedule_is_an_error_naming_it() {
    let e = due_in(
        "her beş dakika",
        at("2026-10-15T14:00:00Z"),
        at("2026-10-15T14:05:00Z"),
        &Utc,
    )
    .unwrap_err();
    assert!(e.contains("her beş dakika"), "{e}");
}

#[test]
fn a_workflow_with_a_bad_schedule_is_refused() {
    let flow = r#"{ "name": "akis", "triggers": [ { "schedule": "her gün" } ],
      "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ { "from": "basla", "to": "bitti" } ] }"#;
    let e = itero::workflow::parse(flow).unwrap_err().to_string();
    assert!(e.contains("her gün"), "{e}");
}

#[test]
fn a_trigger_has_a_schedule_or_an_event_not_both() {
    let with = |t: &str| {
        format!(
            r#"{{ "name": "akis", "triggers": [ {t} ],
      "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ {{ "from": "basla", "to": "bitti" }} ] }}"#
        )
    };
    itero::workflow::parse(&with(r#"{ "on": "foto.eklendi" }"#)).unwrap();
    for bad in [
        r#"{ "schedule": "* * * * *", "on": "foto.eklendi" }"#,
        r#"{ "args": { "a": 1 } }"#,
    ] {
        let e = itero::workflow::parse(&with(bad)).unwrap_err().to_string();
        assert!(e.contains("schedule or on"), "{e}");
    }
    let e = itero::workflow::parse(&with(r#"{ "on": "Foto Eklendi" }"#))
        .unwrap_err()
        .to_string();
    assert!(e.contains("Foto Eklendi"), "{e}");
}
