use chrono::{DateTime, Utc};
use itero::engine::schedule::{due_in, next_in};

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
fn the_next_slot_comes_after_the_last() {
    let c = "*/5 * * * *";
    assert_eq!(
        next_in(c, at("2026-10-15T14:04:59Z"), &Utc).unwrap(),
        at("2026-10-15T14:05:00Z")
    );
    // a slot that is the last one is not the next
    assert_eq!(
        next_in(c, at("2026-10-15T14:05:00Z"), &Utc).unwrap(),
        at("2026-10-15T14:10:00Z")
    );
}

#[test]
fn the_next_slot_follows_the_time_zone_and_its_clock_changes() {
    // 09:00 in Berlin: UTC+2 in summer, UTC+1 from 25 October 2026
    let berlin = chrono_tz::Europe::Berlin;
    let c = "0 9 * * *";
    assert_eq!(
        next_in(c, at("2026-10-24T08:00:00Z"), &berlin).unwrap(),
        at("2026-10-25T08:00:00Z")
    );
    assert_eq!(
        next_in(c, at("2026-10-25T08:00:00Z"), &berlin).unwrap(),
        at("2026-10-26T08:00:00Z")
    );
    // whatever slot a clock change gives, the next slot is the moment the
    // schedule becomes due: not a second sooner, and due then
    for (c, last) in [
        ("30 2 * * *", "2026-03-28T12:00:00Z"),
        ("30 2 * * *", "2026-10-24T12:00:00Z"),
        ("0 9 * * *", "2026-03-28T12:00:00Z"),
    ] {
        let last = at(last);
        let next = next_in(c, last, &berlin).unwrap();
        assert!(due_in(c, last, next, &berlin).unwrap(), "{c} from {last}");
        let before = next - chrono::Duration::seconds(1);
        assert!(
            !due_in(c, last, before, &berlin).unwrap(),
            "{c} from {last}"
        );
    }
}
