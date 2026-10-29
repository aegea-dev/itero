//! How commands print: each builds one JSON value, printed as is with
//! `--json`, else as the command's text, written from that same value so
//! the two never say different things.
use anyhow::Result;
use serde_json::Value;

/// Prints `v` as JSON when `json`, else as `text` writes it.
pub fn show(json: bool, v: &Value, text: impl FnOnce(&Value)) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(v)?);
    } else {
        text(v);
    }
    Ok(())
}

/// A field as text: a string as it is, anything else as JSON, none as "".
pub fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Rows in columns, each as wide as its widest cell, the last left ragged.
pub fn table(rows: &[Vec<String>]) {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..columns)
        .map(|c| {
            rows.iter()
                .map(|r| r.get(c).map_or(0, |s| s.chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect();
    for r in rows {
        let mut line = String::new();
        for (c, cell) in r.iter().enumerate() {
            if c + 1 == r.len() {
                line += cell;
            } else {
                line += &format!("{cell:<w$}   ", w = widths[c]);
            }
        }
        println!("{}", line.trim_end());
    }
}

/// How long before `now` an RFC 3339 time was: `2m ago`, `3h ago`.
pub fn ago(at: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    let Ok(t) = at.parse::<chrono::DateTime<chrono::Utc>>() else {
        return at.to_string();
    };
    let s = (now - t).num_seconds().max(0);
    match s {
        0..60 => format!("{s}s ago"),
        60..3600 => format!("{}m ago", s / 60),
        3600..86400 => format!("{}h ago", s / 3600),
        _ => format!("{}d ago", s / 86400),
    }
}

/// JSON given on the command line or in a file, for `set` commands.
pub fn given(data: Option<String>, file: Option<std::path::PathBuf>) -> Result<String> {
    match (data, file) {
        (Some(d), _) => Ok(d),
        (None, Some(f)) => std::fs::read_to_string(&f)
            .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", f.display())),
        (None, None) => anyhow::bail!("give it as JSON, or --file"),
    }
}

/// Whether `name` could be a workflow's, an action's or a connector's
/// name: no path, no `#`, `@` or `$`, not hidden.
pub fn plain_name(name: &str) -> bool {
    !(name.is_empty() || name.contains(['/', '\\', '#', '@', '$']) || name.starts_with('.'))
}

/// Asks `question` on the terminal, unless `yes` was given; whether to go
/// on. With no terminal and no `yes`, an error saying `no_terminal`.
pub fn confirm(question: &str, yes: bool, no_terminal: &str) -> Result<bool> {
    use std::io::IsTerminal;
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("{no_terminal}");
    }
    eprint!("{question} [y/N] ");
    let mut a = String::new();
    std::io::stdin().read_line(&mut a)?;
    Ok(a.trim().eq_ignore_ascii_case("y"))
}
