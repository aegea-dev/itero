//! What an action needs that is not there: a program or a package. Found
//! from the action itself, never from a list someone has to keep up to date:
//! its first line's interpreter, the commands of a shell script, and what a
//! failed step printed.
use std::ffi::OsStr;
use std::path::Path;

/// What a failed step's stderr says is missing, with its exit code: a
/// program a shell or `env` did not find, or a Python, Node or Bun package.
pub fn missing(exit: i32, stderr: &str) -> Option<String> {
    for line in stderr.lines().map(str::trim) {
        if exit == 127
            && let Some(p) = program(line)
        {
            return Some(format!("needs {}: not found on PATH", unescape(p)));
        }
        if let Some(m) = line.strip_prefix("ModuleNotFoundError: No module named ") {
            return Some(format!("needs the Python module {}", unquote(m)));
        }
        if let Some((_, rest)) = line.split_once("Cannot find module ") {
            return Some(format!("needs the module {}", unquote(first(rest))));
        }
        if let Some((_, rest)) = line.split_once("Cannot find package ") {
            return Some(format!(
                "needs the package {}",
                package(unquote(first(rest)))
            ));
        }
    }
    None
}

/// The program a shell's or `env`'s "not found" line names.
fn program(line: &str) -> Option<&str> {
    // zsh: "zsh:1: command not found: x"
    if let Some((_, p)) = line.split_once("command not found: ") {
        return Some(p.trim());
    }
    // bash: "bash: x: command not found", "./a: line 3: x: command not found"
    // dash: "sh: 1: x: not found"
    for end in [": command not found", ": not found"] {
        if let Some(head) = line.strip_suffix(end) {
            return head.rsplit(": ").next().map(str::trim);
        }
    }
    // env: "env: x: No such file or directory", GNU "/usr/bin/env: 'x': …"
    if let Some(head) = line.strip_suffix(": No such file or directory")
        && let Some((who, p)) = head.split_once(": ")
        && who.ends_with("env")
    {
        return Some(unquote(p));
    }
    None
}

/// A name as a program wrote it, its `\303\266` octal escapes (GNU env in
/// the C locale) turned back into the bytes they stand for.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let octal = b
            .get(i + 1..i + 4)
            .filter(|d| b[i] == b'\\' && d.iter().all(|c| (b'0'..=b'7').contains(c)));
        match octal.and_then(|d| u8::from_str_radix(std::str::from_utf8(d).ok()?, 8).ok()) {
            Some(byte) => {
                out.push(byte);
                i += 4;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `'x' imported from …`: the first quoted word.
fn first(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or(s)
}

/// A package's name without the version an import pinned: `x@1.2`, `@a/b@1`.
fn package(s: &str) -> &str {
    match s[1.min(s.len())..].find('@') {
        Some(i) => &s[..i + 1],
        None => s,
    }
}

fn unquote(s: &str) -> &str {
    s.trim()
        .trim_matches(|c| matches!(c, '\'' | '"' | '‘' | '’' | '`'))
}

/// The program an executable's first line runs it with: `#!/usr/bin/env -S
/// uv run --script` is `uv`, `#!/bin/sh` is `/bin/sh`; none without `#!`.
pub fn interpreter(first_line: &str) -> Option<String> {
    let rest = first_line.strip_prefix("#!")?;
    let mut words = rest.split_whitespace();
    let program = words.next()?;
    if Path::new(program).file_name() != Some(OsStr::new("env")) {
        return Some(program.to_string());
    }
    // env's own options and NAME=value settings come before the program
    words
        .find(|w| !w.starts_with('-') && !w.contains('='))
        .map(str::to_string)
}

/// What an action's file needs that `path` does not have: its first line's
/// interpreter ("needs"), and for a shell script, the commands it runs ("may
/// need": a script can build commands this reading misses, or guard them).
pub fn notes(file: &Path, first_line: &str, path: Option<&OsStr>) -> Vec<String> {
    let mut out = vec![];
    let Some(interpreter) = interpreter(first_line) else {
        return out;
    };
    if !found(&interpreter, path) {
        out.push(format!("needs {interpreter}: not found on PATH"));
        return out;
    }
    let shell = Path::new(&interpreter)
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|n| matches!(n, "sh" | "bash" | "dash" | "zsh" | "ksh"));
    if shell && let Ok(script) = std::fs::read_to_string(file) {
        for c in commands(&script) {
            if !found(&c, path) {
                out.push(format!("may need {c}: not found on PATH"));
            }
        }
    }
    out
}

/// Whether `program` can be run: a path that exists, or a name on `path`.
pub fn found(program: &str, path: Option<&OsStr>) -> bool {
    if program.contains('/') {
        return Path::new(program).is_file();
    }
    path.is_some_and(|p| std::env::split_paths(p).any(|d| d.join(program).is_file()))
}

/// The words a shell script runs as commands: the first word of each, after
/// `|`, `&&`, `||`, `;` and `$(`, apart from the shell's own words, the
/// script's functions, variables and paths. A hint: a command built at run
/// time is missed.
pub fn commands(script: &str) -> Vec<String> {
    let mut functions = vec![];
    let mut found: Vec<String> = vec![];
    for line in code(&without_heredocs(script)).lines() {
        let code = line.trim();
        if let Some(name) = code.split_once("()").map(|(n, _)| n.trim())
            && word(name)
        {
            functions.push(name.to_string());
        }
        for part in code.split(['|', ';', '&', '`', '(', ')', '{', '}']) {
            // settings and words like `if` and `exec` come before a command
            let mut words = part.split_whitespace();
            let command = words.find(|w| !w.contains('=') && !BEFORE.contains(w));
            if let Some(c) = command
                && word(c)
                && !BUILTIN.contains(&c)
                && !found.iter().any(|f| f == c)
            {
                found.push(c.to_string());
            }
        }
    }
    found.retain(|c| !functions.contains(c));
    found
}

/// A word that can name a program on the PATH.
fn word(w: &str) -> bool {
    w.chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
        && w.chars().all(|c| c.is_alphanumeric() || "_-.+".contains(c))
}

/// A script without its first line and without heredocs' bodies, which are
/// data.
fn without_heredocs(script: &str) -> String {
    let mut out = String::new();
    let mut heredoc: Option<String> = None;
    for line in script.lines().skip(1) {
        if let Some(end) = &heredoc {
            if line.trim() == end {
                heredoc = None;
            }
            continue;
        }
        if let Some((_, tag)) = line.split_once("<<") {
            let tag = tag.trim_start_matches('-').trim();
            let tag = unquote(tag.split_whitespace().next().unwrap_or(""));
            if !tag.is_empty() {
                heredoc = Some(tag.to_string());
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// A script's code: each quoted string emptied (what is inside is data,
/// across lines too), comments left out, escapes followed.
fn code(text: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    let mut comment = false;
    let mut escaped = false;
    let mut prev = '\n';
    for c in text.chars() {
        if comment {
            if c == '\n' {
                comment = false;
                out.push(c);
            }
            prev = c;
            continue;
        }
        if escaped {
            escaped = false;
            prev = c;
            continue;
        }
        match quote {
            Some(q) => {
                if c == '\\' && q == '"' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                    out.push(c);
                }
            }
            None => match c {
                '\\' => escaped = true,
                '#' if prev.is_whitespace() => comment = true,
                '\'' | '"' => {
                    quote = Some(c);
                    out.push(c);
                }
                _ => out.push(c),
            },
        }
        prev = c;
    }
    out
}

/// The shell's words that come before a command: `if docker …`,
/// `exec node …`.
const BEFORE: &[&str] = &[
    "if", "then", "else", "elif", "while", "until", "do", "!", "time", "exec", "command",
];

/// The shell's own words and the builtins every shell has: commands that
/// need no program.
const BUILTIN: &[&str] = &[
    "fi", "for", "done", "case", "esac", "in", "function", "return", "exit", "export", "local",
    "readonly", "set", "unset", "shift", "cd", "echo", "printf", "read", "test", "[", "[[", "]]",
    "true", "false", "eval", "trap", "wait", "type", ".", "source", "break", "continue", "umask",
    "alias", "getopts", "hash", "kill", "pwd", "times", "ulimit",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shell_or_env_not_finding_a_program_names_it() {
        for (stderr, says) in [
            ("sh: 1: yok-böyle: not found", "yok-böyle"),
            ("sh: yok-böyle: command not found", "yok-böyle"),
            ("./a: line 3: yok-böyle: command not found", "yok-böyle"),
            ("bash: docker: command not found", "docker"),
            ("zsh:1: command not found: docker", "docker"),
            ("env: bun: No such file or directory", "bun"),
            ("/usr/bin/env: ‘bun’: No such file or directory", "bun"),
            ("/usr/bin/env: 'uv': No such file or directory", "uv"),
            // GNU env in the C locale writes other bytes as octal escapes
            (
                r"env: 'yok-b\303\266yle-dil': No such file or directory",
                "yok-böyle-dil",
            ),
        ] {
            assert_eq!(
                missing(127, &format!("başka bir satır\n{stderr}\n")),
                Some(format!("needs {says}: not found on PATH")),
                "{stderr}"
            );
        }
        // only with exit 127: another failure that says "not found" is not this
        assert_eq!(missing(1, "dosya: not found"), None);
    }

    #[test]
    fn a_missing_package_is_named_by_its_language() {
        let python = "Traceback (most recent call last):\n  File \"<string>\", line 1\nModuleNotFoundError: No module named 'yok_böyle_modül'";
        assert_eq!(
            missing(1, python).as_deref(),
            Some("needs the Python module yok_böyle_modül")
        );
        let node =
            "node:internal\n  throw err;\nError: Cannot find module 'yok-paket'\nRequire stack:";
        assert_eq!(
            missing(1, node).as_deref(),
            Some("needs the module yok-paket")
        );
        let esm =
            "Error [ERR_MODULE_NOT_FOUND]: Cannot find package 'yok-paket' imported from /a/m.mjs";
        assert_eq!(
            missing(1, esm).as_deref(),
            Some("needs the package yok-paket")
        );
        let bun = "error: Cannot find package 'yok-paket@0.0.1' from '/a/b.js'";
        assert_eq!(
            missing(1, bun).as_deref(),
            Some("needs the package yok-paket")
        );
        let scoped = "error: Cannot find package '@örnek/paket@2.0.0' from '/a/b.js'";
        assert_eq!(
            missing(1, scoped).as_deref(),
            Some("needs the package @örnek/paket")
        );
        assert_eq!(missing(3, "çöktü"), None);
    }

    #[test]
    fn the_first_line_names_the_interpreter() {
        assert_eq!(interpreter("#!/bin/sh").as_deref(), Some("/bin/sh"));
        assert_eq!(interpreter("#!/usr/bin/env bun").as_deref(), Some("bun"));
        assert_eq!(
            interpreter("#!/usr/bin/env -S uv run --script").as_deref(),
            Some("uv")
        );
        assert_eq!(
            interpreter("#!/usr/bin/env -S DİL=tr python3 -u").as_deref(),
            Some("python3")
        );
        assert_eq!(interpreter("echo merhaba"), None);
    }

    #[test]
    fn a_shell_script_s_commands_are_read_apart_from_the_shell_s_own() {
        let script = r#"#!/bin/sh
# needs nothing said here
set -eu
selamla() { echo "merhaba"; }
body=$(mktemp) || exit 1
jq -n --arg q "$prompt | not a command" '{q: $q}' > "$body"
printf 'header = "x"\n' "$key" |
  curl -fsS -K - "https://örnek.test" | jq '{text: .a}'
if [ -n "$x" ]; then docker run --rm img; fi
cat <<EOF
heredoc-satırı çalışmaz
EOF
selamla
LANG=C sort dosya.txt && "$HOME/bin/kendi"
perl -ne '
  print "başlık: $1\n" if /<title>(.*?)</;
  my ($x) = @_;
' | wc -l
"#;
        assert_eq!(
            commands(script),
            [
                "mktemp", "jq", "curl", "docker", "cat", "sort", "perl", "wc"
            ]
        );
    }
}
