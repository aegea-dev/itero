//! The daemon's alarm: one wall-clock time, kept by the kernel, that wakes
//! the daemon on the channel every other wake uses. It counts the time the
//! machine sleeps, so a time that passes during sleep goes off as soon as
//! the machine runs again, not later by the sleep. macOS: kqueue's
//! EVFILT_TIMER with NOTE_ABSOLUTE and NOTE_MACH_CONTINUOUS_TIME; Linux: a
//! timerfd on CLOCK_REALTIME.
use chrono::{DateTime, Utc};
use std::io;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(target_os = "macos")]
use macos as imp;

/// What woke the alarm's thread.
enum Woke {
    Rang,
    Stopped,
}

/// The alarm and the thread that waits on it. Dropped, the thread ends.
pub struct Alarm {
    clock: Arc<imp::Clock>,
    thread: Option<JoinHandle<()>>,
}

impl Alarm {
    /// Starts the thread that sends on `wake` each time the alarm goes off;
    /// `failed` hears why it stopped if the kernel fails it.
    pub fn start(
        wake: Sender<()>,
        failed: impl Fn(io::Error) + Send + 'static,
    ) -> io::Result<Alarm> {
        let clock = Arc::new(imp::Clock::new()?);
        let c = clock.clone();
        let thread = std::thread::spawn(move || {
            loop {
                match c.wait() {
                    Ok(Woke::Rang) => {
                        if wake.send(()).is_err() {
                            return;
                        }
                    }
                    Ok(Woke::Stopped) => return,
                    Err(e) => {
                        failed(e);
                        // a last wake, so the daemon looks again
                        let _ = wake.send(());
                        return;
                    }
                }
            }
        });
        Ok(Alarm {
            clock,
            thread: Some(thread),
        })
    }

    /// Sets the alarm to `at`, or to nothing; the time it had before no
    /// longer goes off. A time already past goes off at once.
    pub fn set(&self, at: Option<DateTime<Utc>>) -> io::Result<()> {
        self.clock.set(at)
    }
}

impl Drop for Alarm {
    fn drop(&mut self) {
        // a thread that cannot be told to stop is left, not waited for
        if self.clock.stop().is_ok()
            && let Some(t) = self.thread.take()
        {
            let _ = t.join();
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::Woke;
    use chrono::{DateTime, Utc};
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    const TIMER: usize = 1;
    const STOP: usize = 2;

    pub struct Clock {
        kq: OwnedFd,
    }

    impl Clock {
        pub fn new() -> io::Result<Clock> {
            // SAFETY: no arguments
            let fd = unsafe { libc::kqueue() };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: a new descriptor this function owns
            let c = Clock {
                kq: unsafe { OwnedFd::from_raw_fd(fd) },
            };
            c.change(STOP, libc::EVFILT_USER, libc::EV_ADD | libc::EV_CLEAR, 0, 0)?;
            Ok(c)
        }

        fn change(
            &self,
            ident: usize,
            filter: i16,
            flags: u16,
            fflags: u32,
            data: i64,
        ) -> io::Result<()> {
            let ev = libc::kevent {
                ident,
                filter,
                flags,
                fflags,
                data: data as libc::intptr_t,
                udata: std::ptr::null_mut(),
            };
            // SAFETY: one valid kevent in, none out, no timeout
            let n = unsafe {
                libc::kevent(
                    self.kq.as_raw_fd(),
                    &ev,
                    1,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null(),
                )
            };
            if n < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        pub fn set(&self, at: Option<DateTime<Utc>>) -> io::Result<()> {
            match at {
                // an absolute time on the gettimeofday epoch that keeps
                // counting while the machine sleeps
                Some(at) => {
                    let ns = at
                        .timestamp_nanos_opt()
                        .ok_or_else(|| io::Error::other(format!("{at} is out of range")))?;
                    self.change(
                        TIMER,
                        libc::EVFILT_TIMER,
                        libc::EV_ADD | libc::EV_ONESHOT,
                        libc::NOTE_ABSOLUTE | libc::NOTE_MACH_CONTINUOUS_TIME | libc::NOTE_NSECONDS,
                        ns,
                    )
                }
                None => match self.change(TIMER, libc::EVFILT_TIMER, libc::EV_DELETE, 0, 0) {
                    // none was set
                    Err(e) if e.raw_os_error() == Some(libc::ENOENT) => Ok(()),
                    other => other,
                },
            }
        }

        pub fn wait(&self) -> io::Result<Woke> {
            loop {
                let mut ev = std::mem::MaybeUninit::<libc::kevent>::uninit();
                // SAFETY: no changes in, room for one event, no timeout
                let n = unsafe {
                    libc::kevent(
                        self.kq.as_raw_fd(),
                        std::ptr::null(),
                        0,
                        ev.as_mut_ptr(),
                        1,
                        std::ptr::null(),
                    )
                };
                if n < 0 {
                    let e = io::Error::last_os_error();
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(e);
                }
                if n == 1 {
                    // SAFETY: the kernel wrote one event
                    let ev = unsafe { ev.assume_init() };
                    return Ok(if ev.ident == STOP {
                        Woke::Stopped
                    } else {
                        Woke::Rang
                    });
                }
            }
        }

        pub fn stop(&self) -> io::Result<()> {
            self.change(STOP, libc::EVFILT_USER, 0, libc::NOTE_TRIGGER, 0)
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::Woke;
    use chrono::{DateTime, Utc};
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    pub struct Clock {
        timer: OwnedFd,
        stop: OwnedFd,
    }

    fn owned(fd: libc::c_int) -> io::Result<OwnedFd> {
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a new descriptor the caller owns
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }

    impl Clock {
        pub fn new() -> io::Result<Clock> {
            Ok(Clock {
                // SAFETY: plain flags
                timer: owned(unsafe {
                    libc::timerfd_create(
                        libc::CLOCK_REALTIME,
                        libc::TFD_CLOEXEC | libc::TFD_NONBLOCK,
                    )
                })?,
                // SAFETY: plain flags
                stop: owned(unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) })?,
            })
        }

        pub fn set(&self, at: Option<DateTime<Utc>>) -> io::Result<()> {
            // all zero disarms it
            let mut spec: libc::itimerspec = unsafe { std::mem::zeroed() };
            let mut flags = 0;
            if let Some(at) = at {
                // the wall clock's time: on resume it has moved on by the
                // sleep, and a time it passed goes off at once
                spec.it_value.tv_sec = at.timestamp() as libc::time_t;
                spec.it_value.tv_nsec = at.timestamp_subsec_nanos() as libc::c_long;
                if spec.it_value.tv_sec <= 0 {
                    // a time before 1970 would disarm it: go off at once
                    spec.it_value.tv_sec = 0;
                    spec.it_value.tv_nsec = 1;
                }
                flags = libc::TFD_TIMER_ABSTIME;
            }
            // SAFETY: a valid descriptor and itimerspec, no old value asked
            let n = unsafe {
                libc::timerfd_settime(self.timer.as_raw_fd(), flags, &spec, std::ptr::null_mut())
            };
            if n < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        /// Reads the 8-byte count off `fd`; false when there was none.
        fn drain(fd: &OwnedFd) -> io::Result<bool> {
            let mut n = 0u64;
            // SAFETY: 8 bytes into a u64
            let r = unsafe { libc::read(fd.as_raw_fd(), (&raw mut n).cast(), 8) };
            if r < 0 {
                let e = io::Error::last_os_error();
                return match e.kind() {
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted => Ok(false),
                    _ => Err(e),
                };
            }
            Ok(r == 8)
        }

        pub fn wait(&self) -> io::Result<Woke> {
            loop {
                let mut fds = [
                    libc::pollfd {
                        fd: self.stop.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                    libc::pollfd {
                        fd: self.timer.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                ];
                // SAFETY: two valid pollfds, no timeout
                let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
                if n < 0 {
                    let e = io::Error::last_os_error();
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(e);
                }
                if fds[0].revents != 0 {
                    return Ok(Woke::Stopped);
                }
                // set again in between, it may have nothing to read
                if fds[1].revents != 0 && Self::drain(&self.timer)? {
                    return Ok(Woke::Rang);
                }
            }
        }

        pub fn stop(&self) -> io::Result<()> {
            let one = 1u64;
            // SAFETY: 8 bytes from a u64
            let r = unsafe { libc::write(self.stop.as_raw_fd(), (&raw const one).cast(), 8) };
            if r < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }
}
