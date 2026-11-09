//! Stopping one workspace's threads in a daemon that runs on: a flag its
//! loops look at, a wait its pauses use (so a stop ends a pause at once),
//! and the processes its connectors run, killed by pid. A stop may have
//! stops under it (one connector copy's): stopping it stops them too.
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

#[derive(Default)]
pub struct Stop {
    /// Whether it is stopping, and the pids of its connectors' processes.
    state: Mutex<(bool, Vec<u32>)>,
    stopped: Condvar,
    under: Mutex<Vec<Arc<Stop>>>,
}

impl Stop {
    pub fn is_set(&self) -> bool {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).0
    }

    /// Stops: ends every pause, and asks each process it holds to end;
    /// the stops under it too.
    pub fn set(&self) {
        {
            let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            s.0 = true;
            for pid in &s.1 {
                term(*pid);
            }
            self.stopped.notify_all();
        }
        let under = self
            .under
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        for u in under {
            u.set();
        }
    }

    /// A stop under this one: set with it, or on its own. Set at once when
    /// this one is.
    pub fn under(&self) -> Arc<Stop> {
        let u = Arc::new(Stop::default());
        self.under
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(u.clone());
        if self.is_set() {
            u.set();
        }
        u
    }

    /// Forgets a stop under this one, once what it stopped has ended.
    pub fn forget(&self, u: &Arc<Stop>) {
        self.under
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|s| !Arc::ptr_eq(s, u));
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

    /// The processes it holds, and those the stops under it hold.
    pub fn pids(&self) -> Vec<u32> {
        let mut pids = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .1
            .clone();
        for u in self
            .under
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            pids.extend(u.pids());
        }
        pids
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

/// How long a daemon that is going gives its connectors to end once asked,
/// before it kills those still there.
pub const GRACE: Duration = Duration::from_secs(3);

/// Ends what `stops` hold, as a daemon does that is going: each is set
/// (its connectors' processes asked to end, SIGTERM), then up to `grace`
/// for them to end, then SIGKILL for those still there. A connector that
/// never reads its stdin would not see the daemon go otherwise, and the
/// next daemon would start a second copy of it.
pub fn end(stops: &[Arc<Stop>], grace: Duration) {
    for s in stops {
        s.set();
    }
    let until = std::time::Instant::now() + grace;
    loop {
        // a pid stays held until its process is reaped: never another's
        let left: Vec<u32> = stops.iter().flat_map(|s| s.pids()).collect();
        if left.is_empty() {
            return;
        }
        if std::time::Instant::now() >= until {
            for pid in left {
                // SAFETY: a process this daemon started and has not reaped
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGKILL);
                }
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn term(pid: u32) {
    // SAFETY: kill only sends a signal, to a process this daemon started and
    // has not seen end
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
}
