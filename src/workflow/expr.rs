//! CEL expressions (targets, conditions), `${...}` templates in strings, and
//! durations. Compiling only: evaluating is the manager's job.

/// Compiles a CEL expression, or says why it does not.
pub fn compile(src: &str) -> Result<(), String> {
    cel_interpreter::Program::compile(src)
        .map(|_| ())
        .map_err(|e| format!("bad expression `{src}`: {e}"))
}

/// The expressions inside a string's `${...}` templates, in order.
pub fn templates(s: &str) -> Result<Vec<String>, String> {
    let mut out = vec![];
    let mut rest = s;
    while let Some(i) = rest.find("${") {
        let after = &rest[i + 2..];
        let j = after
            .find('}')
            .ok_or_else(|| format!("unclosed `${{` in `{s}`"))?;
        out.push(after[..j].trim().to_string());
        rest = &after[j + 1..];
    }
    Ok(out)
}

/// A duration such as `30s`, `5m`, `2h`, `7d` or `1h 30m`.
pub fn duration(s: &str) -> Result<std::time::Duration, String> {
    humantime::parse_duration(s)
        .map_err(|_| format!("a duration is like 30s, 5m, 2h, 7d or 1h 30m, not `{s}`"))
}
