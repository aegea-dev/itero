//! Lines on stderr, each written whole.
//!
//! A daemon shares its stderr with its connectors (`itero webhook`, `itero
//! watch`, the workspace's own) and with the watchdog that started it.
//! `eprintln!` writes a line in pieces, one `write` for each part of its
//! format, so two processes writing at once mix their lines:
//! "listening on 127.0.0.1:50905itero webhook: listening on : /ikinci".
//! `say!` formats the line first and writes it with one `write`, which
//! the system keeps whole on a file, and on a pipe up to 4096 bytes.
//! Clippy refuses `eprintln!` (`clippy.toml`), so every line goes this way.
use std::fmt;
use std::io::{self, Write};

/// Writes a line to stderr in one `write`, as `eprintln!` would print it.
#[macro_export]
macro_rules! say {
    ($($arg:tt)*) => {
        $crate::stderr::line(format_args!($($arg)*))
    };
}

/// The line `args` and a newline on stderr, in one `write`. Panics when
/// stderr cannot be written, as `eprintln!` does.
pub fn line(args: fmt::Arguments) {
    if let Err(e) = line_to(io::stderr().lock(), args) {
        panic!("failed printing to stderr: {e}");
    }
}

/// The line `args` and a newline written to `w` at once.
pub fn line_to(mut w: impl Write, args: fmt::Arguments) -> io::Result<()> {
    let mut text = fmt::format(args);
    text.push('\n');
    w.write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keeps each `write` it is given apart.
    #[derive(Default)]
    struct Writes(Vec<Vec<u8>>);

    impl Write for &mut Writes {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.push(buf.to_vec());
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_line_with_values_is_one_write() {
        let mut w = Writes::default();
        let (at, path) = ("127.0.0.1:50905", "/ikinci");
        line_to(
            &mut w,
            format_args!("itero webhook: listening on {at}: {path}"),
        )
        .unwrap();
        line_to(&mut w, format_args!("kargo yüklendi: {} ışık", 3)).unwrap();
        let writes: Vec<String> =
            w.0.iter()
                .map(|b| String::from_utf8(b.clone()).unwrap())
                .collect();
        assert_eq!(
            writes,
            [
                "itero webhook: listening on 127.0.0.1:50905: /ikinci\n",
                "kargo yüklendi: 3 ışık\n",
            ]
        );
    }
}
