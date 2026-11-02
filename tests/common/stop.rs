//! Stopping a daemon a test started: SIGTERM, as `itero stop` does, so it
//! ends as it would for a user and writes its coverage (`make coverage`);
//! a SIGKILL ends it before it can. Killed if it has not ended in 10
//! seconds.
use std::process::Child;
use std::time::Duration;

/// A daemon a test started, stopped however the test ends: a failed
/// assertion unwinds through this drop, where a bare `Child` would leave
/// the process running. Stop one sooner with `drop`.
pub struct Daemon(pub Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        stop(&mut self.0);
    }
}

pub fn stop(child: &mut Child) {
    // waited on already: its pid may be another process's by now
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let ended = wait_timeout::ChildExt::wait_timeout(child, Duration::from_secs(10));
    if !matches!(ended, Ok(Some(_))) {
        let _ = child.kill();
        let _ = child.wait();
    }
}
