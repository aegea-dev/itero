//! Stopping a daemon a test started: SIGTERM, as `itero stop` does, so it
//! ends as it would for a user and writes its coverage (`make coverage`);
//! a SIGKILL ends it before it can. Killed if it has not ended in 10
//! seconds.
use std::process::Child;
use std::time::Duration;

pub fn stop(child: &mut Child) {
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let ended = wait_timeout::ChildExt::wait_timeout(child, Duration::from_secs(10));
    if !matches!(ended, Ok(Some(_))) {
        let _ = child.kill();
        let _ = child.wait();
    }
}
