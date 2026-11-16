//! How the daemon gives a worker its step and its job's lock: over the
//! worker's stdin, a Unix socket, the step's JSON with the lock's file
//! descriptor beside it (`SCM_RIGHTS`).
//!
//! The lock is held without a gap: the daemon takes it, sends it, and lets
//! its own copy go; the worker, and its action's processes, hold it from
//! then on. Sent this way, the worker needs nothing run between fork and
//! exec, so starting it is a `posix_spawn`, not a `fork` of the whole
//! daemon (on macOS a fork holds every malloc lock while it copies).
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;

/// Sends `text` and `lock` on `to`, then ends what is sent: the worker
/// reads to the end. The descriptor goes with the first bytes, so `text`
/// is not empty.
pub fn send(to: &UnixStream, text: &[u8], lock: BorrowedFd) -> io::Result<()> {
    if text.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "nothing to send the lock with",
        ));
    }
    let size = std::mem::size_of::<RawFd>() as u32;
    // SAFETY: CMSG_SPACE and CMSG_LEN only compute sizes
    let (space, len) = unsafe { (libc::CMSG_SPACE(size) as usize, libc::CMSG_LEN(size)) };
    // u64s: a control message is aligned as its header
    let mut control = vec![0u64; space.div_ceil(8)];
    let mut iov = libc::iovec {
        iov_base: text.as_ptr() as *mut libc::c_void,
        iov_len: text.len(),
    };
    // SAFETY: an all-zero msghdr is a valid empty one
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = space as _;
    // SAFETY: `control` has room for one header and one descriptor, as
    // CMSG_SPACE said; CMSG_FIRSTHDR points at its start
    unsafe {
        let c = libc::CMSG_FIRSTHDR(&msg);
        (*c).cmsg_level = libc::SOL_SOCKET;
        (*c).cmsg_type = libc::SCM_RIGHTS;
        (*c).cmsg_len = len as _;
        std::ptr::write_unaligned(libc::CMSG_DATA(c).cast::<RawFd>(), lock.as_raw_fd());
    }
    let sent = loop {
        // SAFETY: `msg` points at `iov`, `text` and `control`, alive here
        let n = unsafe { libc::sendmsg(to.as_raw_fd(), &msg, 0) };
        if n >= 0 {
            break n as usize;
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    };
    (&*to).write_all(&text[sent..])?;
    to.shutdown(std::net::Shutdown::Write)
}

/// What came on `from` to its end, and the file descriptor that came with
/// it, if one did. `from` may be a pipe (a worker started by hand): then
/// no descriptor comes. The descriptor is kept open across exec, so the
/// processes the worker starts hold it too.
pub fn receive(from: BorrowedFd) -> io::Result<(Vec<u8>, Option<OwnedFd>)> {
    let size = std::mem::size_of::<RawFd>();
    // SAFETY: CMSG_SPACE and CMSG_LEN only compute sizes
    let (space, empty) = unsafe {
        (
            libc::CMSG_SPACE(size as u32) as usize,
            libc::CMSG_LEN(0) as usize,
        )
    };
    let mut control = vec![0u64; space.div_ceil(8)];
    let mut buf = vec![0u8; 65536];
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    };
    // SAFETY: an all-zero msghdr is a valid empty one
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = (control.len() * 8) as _;
    let got = loop {
        // SAFETY: `msg` points at `iov`, `buf` and `control`, alive here
        let n = unsafe { libc::recvmsg(from.as_raw_fd(), &mut msg, 0) };
        if n >= 0 {
            break Some(n as usize);
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::EINTR) => {}
            // a pipe: nothing came with it, and nothing was read
            Some(libc::ENOTSOCK) => break None,
            _ => return Err(e),
        }
    };
    let mut lock = None;
    if let Some(n) = got {
        buf.truncate(n);
        // SAFETY: the kernel wrote `msg_controllen` bytes of whole control
        // messages into `control`; CMSG_FIRSTHDR and CMSG_NXTHDR walk them
        // and return null past the last. Each descriptor in an SCM_RIGHTS
        // message is new in this process and owned by no one else.
        unsafe {
            let mut c = libc::CMSG_FIRSTHDR(&msg);
            while !c.is_null() {
                if (*c).cmsg_level == libc::SOL_SOCKET && (*c).cmsg_type == libc::SCM_RIGHTS {
                    let count = ((*c).cmsg_len as usize - empty) / size;
                    for i in 0..count {
                        let fd =
                            std::ptr::read_unaligned(libc::CMSG_DATA(c).cast::<RawFd>().add(i));
                        // one is sent; any other is closed as it drops
                        let fd = OwnedFd::from_raw_fd(fd);
                        lock.get_or_insert(fd);
                    }
                }
                c = libc::CMSG_NXTHDR(&msg, c);
            }
        }
        if msg.msg_flags & libc::MSG_CTRUNC != 0 {
            return Err(io::Error::other("the job's lock did not come whole"));
        }
    } else {
        buf.clear();
    }
    // the rest, to its end; `from` stays open, it is only borrowed
    // SAFETY: the File is never dropped, so it never closes `from`
    let mut rest =
        std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(from.as_raw_fd()) });
    rest.read_to_end(&mut buf)?;
    Ok((buf, lock))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsFd;

    /// The file's identity, to tell a descriptor of it from another's.
    fn ino(fd: BorrowedFd) -> (u64, u64) {
        // SAFETY: fstat fills `st` for a valid, borrowed descriptor
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::fstat(fd.as_raw_fd(), &mut st) }, 0);
        (st.st_dev as u64, st.st_ino as u64)
    }

    #[test]
    fn the_step_and_the_lock_arrive_together() {
        let d = tempfile::tempdir().unwrap();
        let lock = std::fs::File::create(d.path().join("iş.lock")).unwrap();
        let (a, b) = UnixStream::pair().unwrap();
        // more than a socket holds at once: sent while it is read
        let text = format!("{{\"adım\":\"{}\"}}", "çay ".repeat(50_000));
        let sent = text.clone();
        let fd = lock.try_clone().unwrap();
        let sender = std::thread::spawn(move || send(&a, sent.as_bytes(), fd.as_fd()));
        let (got, fd) = receive(b.as_fd()).unwrap();
        sender.join().unwrap().unwrap();
        assert_eq!(String::from_utf8(got).unwrap(), text);
        let fd = fd.expect("no lock came");
        assert_eq!(ino(fd.as_fd()), ino(lock.as_fd()), "another file came");
        // kept across the worker's exec of its action, so its processes
        // hold the lock too
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
        assert_eq!(flags & libc::FD_CLOEXEC, 0, "the lock would close on exec");
    }

    #[test]
    fn from_a_pipe_the_step_comes_alone() {
        let (mut r, mut w) = std::io::pipe().unwrap();
        w.write_all("{\"adım\":\"şeker\"}".as_bytes()).unwrap();
        drop(w);
        let (got, fd) = receive(r.as_fd()).unwrap();
        assert_eq!(String::from_utf8(got).unwrap(), "{\"adım\":\"şeker\"}");
        assert!(fd.is_none());
        let mut rest = vec![];
        assert_eq!(r.read_to_end(&mut rest).unwrap(), 0);
    }
}
