//! A problem `itero ws check` reports: what is wrong, and where, as an
//! address inside the workspace (`workflow/posta/steps/#ai`).
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Problem {
    pub at: String,
    pub message: String,
}

impl Problem {
    pub fn new(at: impl Into<String>, message: impl Into<String>) -> Problem {
        Problem {
            at: at.into(),
            message: message.into(),
        }
    }

    /// The same problem one level down: `prefix/at`.
    pub fn under(self, prefix: &str) -> Problem {
        let at = match self.at.as_str() {
            "" => prefix.to_string(),
            a => format!("{prefix}/{a}"),
        };
        Problem { at, ..self }
    }
}

/// `3 errors, 1 warning`.
pub fn count_line(errors: usize, warnings: usize) -> String {
    let n = |k: usize, one: &str| format!("{k} {one}{}", if k == 1 { "" } else { "s" });
    format!("{}, {}", n(errors, "error"), n(warnings, "warning"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_adds_a_level_and_the_count_says_one_or_more() {
        assert_eq!(Problem::new("", "x").under("on").at, "on");
        assert_eq!(Problem::new("2", "x").under("on").at, "on/2");
        assert_eq!(count_line(1, 0), "1 error, 0 warnings");
        assert_eq!(count_line(3, 1), "3 errors, 1 warning");
    }
}
