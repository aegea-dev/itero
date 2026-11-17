//! Open files: how many the daemon may have, and running out of them.
//! Each running job holds a few (its worker's pipes), so the system's
//! default (256 on macOS) would cap how many steps run at once.

/// Whether `e` says this process, or the system, has no open files left.
pub fn out_of(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(libc::EMFILE | libc::ENFILE))
}

/// How many files this process may have open now: its soft limit.
pub fn limit() -> std::io::Result<u64> {
    Ok(get()?.rlim_cur as u64)
}

/// Raises this process's soft limit on open files to its hard limit (on
/// macOS no further than `kern.maxfilesperproc`, past which the call is
/// refused). What it starts inherits the new limit. Returns the limit it
/// has now.
pub fn raise_limit() -> std::io::Result<u64> {
    let mut l = get()?;
    let most = most(l.rlim_max)?;
    if l.rlim_cur < most {
        l.rlim_cur = most;
        // SAFETY: setrlimit reads the struct it is given
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &l) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(l.rlim_cur as u64)
}

fn get() -> std::io::Result<libc::rlimit> {
    let mut l = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit writes the struct it is given
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut l) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(l)
}

/// The highest soft limit allowed under the hard limit `hard`.
#[cfg(target_os = "macos")]
fn most(hard: libc::rlim_t) -> std::io::Result<libc::rlim_t> {
    let mut per_process: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>();
    // SAFETY: the name is a C string; the value is an int, `size` its size
    let read = unsafe {
        libc::sysctlbyname(
            c"kern.maxfilesperproc".as_ptr(),
            (&raw mut per_process).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if read != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(hard.min(per_process.max(0) as libc::rlim_t))
}

#[cfg(not(target_os = "macos"))]
fn most(hard: libc::rlim_t) -> std::io::Result<libc::rlim_t> {
    Ok(hard)
}

/// Files a worker holds in the daemon at most, while it starts: its job's
/// lock, both ends of its stdin's socket and of its two pipes. Once it
/// runs, two.
pub const PER_WORKER: u64 = 7;

/// How many workers may run at once in this process, all workspaces
/// together, with `limit` open files: a quarter of them (64 at least) is
/// kept for the rest (the journal, the log, connectors, the API's
/// connections, a step's lookups). One at least.
pub fn workers_allowed(limit: u64) -> u64 {
    let rest = (limit / 4).max(64);
    (limit.saturating_sub(rest) / PER_WORKER).max(1)
}

static WORKERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A running worker's share of the process's open files; given back when
/// it drops.
#[derive(Debug)]
pub struct Share(());

impl Drop for Share {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// A share for one more worker, else why there is none: as many run as
/// the open files allow.
pub fn worker_share() -> Result<Share, String> {
    use std::sync::atomic::Ordering;
    let limit = limit().map_err(|e| format!("cannot read the limit on open files: {e}"))?;
    let most = workers_allowed(limit);
    WORKERS
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
            (n < most).then_some(n + 1)
        })
        .map(|_| Share(()))
        .map_err(|n| format!("{n} steps run, as many as {limit} open files allow"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workers_get_what_the_rest_leaves() {
        // macOS's default: 64 kept, 192 left
        assert_eq!(workers_allowed(256), 27);
        assert_eq!(workers_allowed(1024), 109);
        assert_eq!(workers_allowed(122_880), 13_165);
        // too few to keep the rest: one still runs
        assert_eq!(workers_allowed(48), 1);
        assert_eq!(workers_allowed(0), 1);
    }

    /// Shares run out at the limit and come back as they drop.
    #[test]
    fn shares_run_out_and_come_back() {
        let most = workers_allowed(limit().unwrap()) as usize;
        let mut held = vec![];
        while let Ok(s) = worker_share() {
            held.push(s);
            assert!(held.len() <= most, "more than {most}");
        }
        assert_eq!(held.len(), most);
        let why = worker_share().unwrap_err();
        assert!(why.contains("open files allow"), "{why}");
        held.pop();
        let again = worker_share().unwrap();
        drop((again, held));
        assert_eq!(WORKERS.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn running_out_is_told_apart() {
        assert!(out_of(&std::io::Error::from_raw_os_error(libc::EMFILE)));
        assert!(out_of(&std::io::Error::from_raw_os_error(libc::ENFILE)));
        assert!(!out_of(&std::io::Error::from_raw_os_error(libc::ENOENT)));
        assert!(!out_of(&std::io::Error::other("başka")));
    }

    /// From a low soft limit to the highest this process may have; put
    /// back after.
    #[test]
    fn the_limit_is_raised_to_the_most_allowed() {
        let was = get().unwrap();
        let low = libc::rlimit {
            rlim_cur: 64.min(was.rlim_max),
            rlim_max: was.rlim_max,
        };
        // SAFETY: as in raise_limit
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &low) }, 0);
        assert_eq!(limit().unwrap(), low.rlim_cur as u64);
        let raised = raise_limit();
        let now = limit().unwrap();
        // SAFETY: as in raise_limit
        unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &was) };
        let raised = raised.unwrap();
        assert_eq!(raised, now);
        assert_eq!(raised, most(was.rlim_max).unwrap() as u64);
        assert!(raised > 64 || was.rlim_max <= 64, "{raised}");
    }
}
