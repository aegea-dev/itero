//! A model's text into its answer and its thinking, by the marks its
//! family uses. Qwen's `<think>...</think>` first; any other family's text
//! is all answer until its marks are written here.
use crate::engine::Piece;

const OPEN: &str = "<think>";
const CLOSE: &str = "</think>";

/// The architectures (`general.architecture`) whose thinking it splits.
pub fn splits(arch: &str) -> bool {
    matches!(arch, "qwen3" | "qwen3moe" | "qwen35" | "qwen35moe")
}

pub struct Splitter {
    on: bool,
    inside: bool,
    /// What may be the start of a mark, kept until the next piece says.
    held: String,
}

impl Splitter {
    pub fn for_architecture(arch: &str, thinking: bool) -> Splitter {
        Splitter {
            on: thinking && splits(arch),
            inside: false,
            held: String::new(),
        }
    }

    /// Starts inside the thinking: for a template whose prompt ends by
    /// opening it.
    pub fn inside(mut self) -> Splitter {
        self.inside = self.on;
        self
    }

    pub fn feed(&mut self, text: &str) -> Vec<Piece> {
        if !self.on {
            return vec![Piece::Content(text.into())];
        }
        self.held.push_str(text);
        let mut out = vec![];
        loop {
            let mark = if self.inside { CLOSE } else { OPEN };
            if let Some(i) = self.held.find(mark) {
                let before: String = self.held.drain(..i).collect();
                self.held.drain(..mark.len());
                self.push(&mut out, before);
                self.inside = !self.inside;
                if !self.inside {
                    // the blank line Qwen puts after its thinking
                    self.held = self.held.trim_start_matches('\n').to_string();
                }
                continue;
            }
            // the marks are ASCII, so these cuts are on character bounds
            let keep = (1..mark.len())
                .rev()
                .find(|n| self.held.ends_with(&mark[..*n]))
                .unwrap_or(0);
            let ready: String = self.held.drain(..self.held.len() - keep).collect();
            self.push(&mut out, ready);
            return out;
        }
    }

    /// What is still held, at the answer's end.
    pub fn end(&mut self) -> Vec<Piece> {
        let mut out = vec![];
        let rest = std::mem::take(&mut self.held);
        self.push(&mut out, rest);
        out
    }

    fn push(&self, out: &mut Vec<Piece>, t: String) {
        if !t.is_empty() {
            out.push(if self.inside {
                Piece::Reasoning(t)
            } else {
                Piece::Content(t)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Piece::{self, Content, Reasoning};

    fn all(s: &mut Splitter, parts: &[&str]) -> Vec<Piece> {
        let mut out: Vec<_> = parts.iter().flat_map(|p| s.feed(p)).collect();
        out.extend(s.end());
        out
    }

    fn joined(out: &[Piece]) -> (String, String) {
        let (mut r, mut c) = (String::new(), String::new());
        for p in out {
            match p {
                Reasoning(t) => r.push_str(t),
                Content(t) => c.push_str(t),
            }
        }
        (r, c)
    }

    #[test]
    fn qwen_thinking_is_cut_from_the_answer_even_across_pieces() {
        let mut s = Splitter::for_architecture("qwen35", true);
        let out = all(
            &mut s,
            &["<thi", "nk>önce düşün", "ür</th", "ink>\n\ncevap"],
        );
        assert_eq!(joined(&out), ("önce düşünür".into(), "cevap".into()));
    }

    #[test]
    fn a_template_that_opened_the_thinking_itself_starts_inside() {
        // Qwen3.5's template ends its prompt with <think>\n when thinking is on
        let mut s = Splitter::for_architecture("qwen35", true).inside();
        let out = all(&mut s, &["düşün", "</think>\n\ncevap"]);
        assert_eq!(joined(&out), ("düşün".into(), "cevap".into()));
    }

    #[test]
    fn without_thinking_or_an_unknown_family_everything_is_content() {
        for mut s in [
            Splitter::for_architecture("qwen35", false),
            Splitter::for_architecture("gemma4", true),
        ] {
            let out = all(&mut s, &["<think>x</think>y"]);
            assert_eq!(joined(&out), (String::new(), "<think>x</think>y".into()));
        }
        assert!(splits("qwen35moe") && !splits("llama"));
    }
}
