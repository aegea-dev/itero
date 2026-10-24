//! Schedules: when a workflow's cron trigger is due.
use chrono::{DateTime, Local, TimeZone, Utc};
use croner::Cron;
use std::str::FromStr;

fn parse(cron: &str) -> Result<Cron, String> {
    Cron::from_str(cron).map_err(|e| format!("schedule \"{cron}\": {e}"))
}

/// Checks that `cron` reads as a schedule.
pub fn check(cron: &str) -> Result<(), String> {
    parse(cron).map(|_| ())
}

/// The first slot of `cron`, read in `tz`, after `last`.
pub fn next_in<Tz: TimeZone>(
    cron: &str,
    last: DateTime<Utc>,
    tz: &Tz,
) -> Result<DateTime<Utc>, String> {
    let next = parse(cron)?
        .find_next_occurrence(&last.with_timezone(tz), false)
        .map_err(|e| format!("schedule \"{cron}\": {e}"))?;
    Ok(next.with_timezone(&Utc))
}

/// `next_in` the machine's local time.
pub fn next(cron: &str, last: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
    next_in(cron, last, &Local)
}

/// Whether a slot of `cron`, read in `tz`, falls in `(last, now]`. However
/// many slots fall there, the answer is one: a daemon that slept runs a
/// schedule once, not once per slot missed.
pub fn due_in<Tz: TimeZone>(
    cron: &str,
    last: DateTime<Utc>,
    now: DateTime<Utc>,
    tz: &Tz,
) -> Result<bool, String> {
    Ok(next_in(cron, last, tz)? <= now)
}

/// `due_in` the machine's local time.
pub fn due(cron: &str, last: DateTime<Utc>, now: DateTime<Utc>) -> Result<bool, String> {
    due_in(cron, last, now, &Local)
}
