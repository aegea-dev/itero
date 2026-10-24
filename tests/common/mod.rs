//! What the test binaries share.
use serde_json::Value;

/// The lines of a log, `.itero/itero.log`, each a JSON object. A running
/// daemon appends from its own process, so a reader can catch the last
/// line half written: only lines that end in a newline are read.
pub fn log_lines(text: &str) -> Vec<Value> {
    let complete = match text.rfind('\n') {
        Some(end) => &text[..end],
        None => "",
    };
    complete
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect()
}
