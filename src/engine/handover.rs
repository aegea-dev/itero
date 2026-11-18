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
    let sent = send_with(to, text, lock)?;
    (&*to).write_all(&text[sent..])?;
    to.shutdown(std::net::Shutdown::Write)
}

/// Sends one of many steps on `to`, with its lock: its length (4 bytes,
/// big-endian), then `text`; what is sent is not ended, so a warm copy's
/// worker takes step after step on one socket.
pub fn send_framed(to: &UnixStream, text: &[u8], lock: BorrowedFd) -> io::Result<()> {
    let len = u32::try_from(text.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a step over 4 GB"))?;
    let mut all = len.to_be_bytes().to_vec();
    all.extend_from_slice(text);
    let sent = send_with(to, &all, lock)?;
    (&*to).write_all(&all[sent..])
}

/// What came on `from` to its end, and the file descriptor that came with
/// it, if one did. `from` may be a pipe (a worker started by hand): then
/// no descriptor comes. The descriptor is kept open across exec, so the
/// processes the worker starts hold it too.
pub fn receive(from: BorrowedFd) -> io::Result<(Vec<u8>, Option<OwnedFd>)> {
    let mut buf = vec![0u8; 65536];
    let (got, lock) = receive_with(from, &mut buf)?;
    match got {
        Some(n) => buf.truncate(n),
        None => buf.clear(),
    }
    // the rest, to its end; `from` stays open, it is only borrowed
    // SAFETY: the File is never dropped, so it never closes `from`
    let mut rest =
        std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(from.as_raw_fd()) });
    rest.read_to_end(&mut buf)?;
    Ok((buf, lock))
}

/// The next step sent with `send_framed`, and its lock; `None` once `from`
/// has ended (the daemon let the copy go).
pub fn receive_framed(from: BorrowedFd) -> io::Result<Option<(Vec<u8>, Option<OwnedFd>)>> {
    let mut len = [0u8; 4];
    let (got, lock) = receive_with(from, &mut len)?;
    let got = got.unwrap_or(0);
    if got == 0 {
        return Ok(None);
    }
    // SAFETY: the File is never dropped, so it never closes `from`
    let mut rest =
        std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(from.as_raw_fd()) });
    rest.read_exact(&mut len[got..])?;
    let mut text = vec![0u8; u32::from_be_bytes(len) as usize];
    rest.read_exact(&mut text)?;
    Ok(Some((text, lock)))
}

/// Sends the first of `bytes` with `lock` beside them (`SCM_RIGHTS`);
/// returns how many went.
fn send_with(to: &UnixStream, bytes: &[u8], lock: BorrowedFd) -> io::Result<usize> {
    if bytes.is_empty() {
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
        iov_base: bytes.as_ptr() as *mut libc::c_void,
        iov_len: bytes.len(),
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
    loop {
        // SAFETY: `msg` points at `iov`, `bytes` and `control`, alive here
        let n = unsafe { libc::sendmsg(to.as_raw_fd(), &msg, 0) };
        if n >= 0 {
            return Ok(n as usize);
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// Reads into `buf` once, with the descriptor that came beside it; `None`
/// for how many when `from` is a pipe (nothing read, nothing came with it).
fn receive_with(from: BorrowedFd, buf: &mut [u8]) -> io::Result<(Option<usize>, Option<OwnedFd>)> {
    let size = std::mem::size_of::<RawFd>();
    // SAFETY: CMSG_SPACE and CMSG_LEN only compute sizes
    let (space, empty) = unsafe {
        (
            libc::CMSG_SPACE(size as u32) as usize,
            libc::CMSG_LEN(0) as usize,
        )
    };
    let mut control = vec![0u64; space.div_ceil(8)];
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
            break n as usize;
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::EINTR) => {}
            // a pipe: nothing came with it, and nothing was read
            Some(libc::ENOTSOCK) => return Ok((None, None)),
            _ => return Err(e),
        }
    };
    let mut lock = None;
    // SAFETY: the kernel wrote `msg_controllen` bytes of whole control
    // messages into `control`; CMSG_FIRSTHDR and CMSG_NXTHDR walk them and
    // return null past the last. Each descriptor in an SCM_RIGHTS message is
    // new in this process and owned by no one else.
    unsafe {
        let mut c = libc::CMSG_FIRSTHDR(&msg);
        while !c.is_null() {
            if (*c).cmsg_level == libc::SOL_SOCKET && (*c).cmsg_type == libc::SCM_RIGHTS {
                let count = ((*c).cmsg_len as usize - empty) / size;
                for i in 0..count {
                    let fd = std::ptr::read_unaligned(libc::CMSG_DATA(c).cast::<RawFd>().add(i));
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
    Ok((Some(got), lock))
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
    fn steps_and_their_locks_come_one_by_one_on_one_socket() {
        let d = tempfile::tempdir().unwrap();
        let locks: Vec<_> = ["ılık.lock", "şeftali.lock"]
            .iter()
            .map(|n| std::fs::File::create(d.path().join(n)).unwrap())
            .collect();
        let (a, b) = UnixStream::pair().unwrap();
        // the second over what a socket holds at once: sent while it is read
        let texts = vec![
            "{\"adım\":\"çay\"}".to_string(),
            format!("{{\"adım\":\"{}\"}}", "üzüm ".repeat(50_000)),
        ];
        let (sent, fds): (Vec<String>, Vec<std::fs::File>) = (
            texts.clone(),
            locks.iter().map(|l| l.try_clone().unwrap()).collect(),
        );
        let sender = std::thread::spawn(move || {
            for (t, fd) in sent.iter().zip(&fds) {
                send_framed(&a, t.as_bytes(), fd.as_fd())?;
            }
            Ok::<_, io::Error>(())
        });
        for (text, lock) in texts.iter().zip(&locks) {
            let (got, fd) = receive_framed(b.as_fd()).unwrap().expect("a step");
            assert_eq!(&String::from_utf8(got).unwrap(), text);
            assert_eq!(ino(fd.expect("no lock came").as_fd()), ino(lock.as_fd()));
        }
        sender.join().unwrap().unwrap();
        // the daemon let the copy go
        assert!(receive_framed(b.as_fd()).unwrap().is_none());
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
