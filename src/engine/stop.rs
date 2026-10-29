//! Stopping one workspace's threads in a daemon that runs on: a flag its
//! loops look at, a wait its pauses use (so a stop ends a pause at once),
//! and the processes its connectors run, killed by pid.
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

#[derive(Default)]
pub struct Stop {
    /// Whether it is stopping, and the pids of its connectors' processes.
    state: Mutex<(bool, Vec<u32>)>,
    stopped: Condvar,
}

impl Stop {
    pub fn is_set(&self) -> bool {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).0
    }

    /// Stops: ends every pause, and asks each process it holds to end.
    pub fn set(&self) {
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        s.0 = true;
        for pid in &s.1 {
            term(*pid);
        }
        self.stopped.notify_all();
    }

    /// Holds a process to stop with the rest; false when stopping already
    /// (the process is asked to end at once).
    pub fn hold(&self, pid: u32) -> bool {
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if s.0 {
            term(pid);
            return false;
        }
        s.1.push(pid);
        true
    }

    /// The process has ended.
    pub fn release(&self, pid: u32) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .1
            .retain(|p| *p != pid);
    }

    /// The processes it holds.
    pub fn pids(&self) -> Vec<u32> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .1
            .clone()
    }

    /// Waits `d`, or less when it stops; whether it stopped.
    pub fn sleep(&self, d: Duration) -> bool {
        let s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let (s, _) = self
            .stopped
            .wait_timeout_while(s, d, |s| !s.0)
            .unwrap_or_else(PoisonError::into_inner);
        s.0
    }
}

fn term(pid: u32) {
    // SAFETY: kill only sends a signal, to a process this daemon started and
    // has not seen end
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
}
