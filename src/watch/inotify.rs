//! Linux: inotify on the target and on its parent folder.
use super::{Backend, Change};
use std::path::Path;
use std::time::Duration;

pub struct Inotify;

impl Inotify {
    pub fn new(_: &Path) -> std::io::Result<Inotify> {
        Err(std::io::Error::other("not yet"))
    }
}

impl Backend for Inotify {
    fn wait(&mut self, _: Duration) -> std::io::Result<Vec<Change>> {
        Ok(vec![])
    }
}
