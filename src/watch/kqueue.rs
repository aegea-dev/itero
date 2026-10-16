//! macOS: kqueue's vnode events on the target and on its parent folder.
use super::{Backend, Change};
use std::path::Path;
use std::time::Duration;

pub struct Kqueue;

impl Kqueue {
    pub fn new(_: &Path) -> std::io::Result<Kqueue> {
        Err(std::io::Error::other("not yet"))
    }
}

impl Backend for Kqueue {
    fn wait(&mut self, _: Duration) -> std::io::Result<Vec<Change>> {
        Ok(vec![])
    }
}
