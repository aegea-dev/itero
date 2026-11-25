//! A problem `itero ws check` reports: what is wrong, and where: as an
//! address inside the workspace (`workflows/posta/steps/#ai`), and as the
//! file it is in with its place there (`workflows/posta.json`,
//! `steps.#ai`), as people read it.
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Problem {
    pub at: String,
    pub message: String,
    /// The file it is in, from the workspace (`itero.config.json`); none
    /// for the workspace's own.
    pub file: Option<String>,
    /// Its place in that file (`limits.keep`, `steps.#ai`); empty for the
    /// whole file.
    pub place: String,
}

impl Problem {
    pub fn new(at: impl Into<String>, message: impl Into<String>) -> Problem {
        Problem {
            at: at.into(),
            message: message.into(),
            file: None,
            place: String::new(),
        }
    }

    /// The same problem in `file`, at `place` there.
    pub fn placed(self, file: impl Into<String>, place: impl Into<String>) -> Problem {
        Problem {
            file: Some(file.into()),
            place: place.into(),
            ..self
        }
    }

    /// The same problem in `file`, its address so far (inside that file,
    /// `steps/#ai`) its place there, with dots (`steps.#ai`). Before
    /// `under`, which puts the address under the file's own.
    pub fn in_file(self, file: impl Into<String>) -> Problem {
        let place = self.at.replace('/', ".");
        self.placed(file, place)
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

/// `1 error`, `3 errors`.
pub fn count(k: usize, one: &str) -> String {
    format!("{k} {one}{}", if k == 1 { "" } else { "s" })
}

/// `3 errors, 1 warning`.
pub fn count_line(errors: usize, warnings: usize) -> String {
    format!("{}, {}", count(errors, "error"), count(warnings, "warning"))
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
