//! The control socket between the app and a box's keeper: a
//! `SOCK_SEQPACKET` socketpair, one JSON message per packet, file
//! descriptors (a spawned program's stdin, stdout and stderr) beside a
//! message as `SCM_RIGHTS`.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use serde::{Deserialize, Serialize};

/// The largest message (a one-shot command travels in one; `execve` refuses
/// an argument over 128 KiB anyway, as it does under `docker exec`).
pub const MAX_MESSAGE: usize = 256 * 1024;

/// What the app asks of a keeper.
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Request {
    /// The box to start (the first message).
    Start(Box<super::keeper::Spec>),
    /// A program entered into the box, with stdin, stdout and stderr given
    /// as three descriptors beside the message.
    Spawn {
        /// The caller's id for it.
        id: u64,
        /// argv; `argv[0]` is looked up in the box's `PATH`.
        argv: Vec<String>,
    },
    /// SIGKILL to a spawned program's process outside the box (today's
    /// kill of the `docker exec` client: what it started keeps running).
    Kill {
        /// The program's id.
        id: u64,
    },
    /// `kill -KILL -1` entered into the box: every process but its init.
    End {
        /// The caller's id for the answer.
        id: u64,
    },
}

/// What a keeper answers.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    /// The box is up; its init's pid on the host.
    Ready {
        /// The pid.
        pid: i32,
    },
    /// The box could not be started.
    Failed {
        /// Why, with bubblewrap's or pasta's words.
        message: String,
    },
    /// A spawned program ended: its exit code, 128 + the signal when the
    /// box's process was killed, -1 when its process outside the box was.
    Exited {
        /// The program's id.
        id: u64,
        /// The code.
        code: i32,
    },
    /// A spawn failed before the program ran.
    SpawnFailed {
        /// The program's id.
        id: u64,
        /// Why.
        message: String,
    },
    /// An `End` is done.
    Ended {
        /// The `End`'s id.
        id: u64,
    },
}

/// A socketpair: (the app's end, the keeper's end), both close-on-exec.
pub fn pair() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: socketpair writes two descriptors into the array on success.
    let done = unsafe {
        libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
            0,
            fds.as_mut_ptr(),
        )
    };
    if done != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: both descriptors are new and owned by nobody else.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// A pipe: (read end, write end), close-on-exec.
pub fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: pipe2 writes two descriptors into the array on success.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: both descriptors are new and owned by nobody else.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// Sends one packet with `fds` beside it. `MSG_DONTWAIT` when `nonblocking`.
pub fn send(socket: RawFd, data: &[u8], fds: &[RawFd], nonblocking: bool) -> io::Result<()> {
    assert!(
        data.len() <= MAX_MESSAGE,
        "a control message of {} bytes",
        data.len()
    );
    let mut iov = libc::iovec {
        iov_base: data.as_ptr() as *mut _,
        iov_len: data.len(),
    };
    // SAFETY: CMSG_SPACE is a pure size computation.
    let space = unsafe { libc::CMSG_SPACE(std::mem::size_of_val(fds) as u32) } as usize;
    let mut control = vec![0u8; if fds.is_empty() { 0 } else { space }];
    // SAFETY: msghdr is plain data; every pointer set below outlives the call.
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    if !fds.is_empty() {
        message.msg_control = control.as_mut_ptr() as *mut _;
        message.msg_controllen = space as _;
        // SAFETY: the control buffer is CMSG_SPACE bytes for this many fds,
        // so the first header and its data fit in it.
        unsafe {
            let header = libc::CMSG_FIRSTHDR(&message);
            (*header).cmsg_level = libc::SOL_SOCKET;
            (*header).cmsg_type = libc::SCM_RIGHTS;
            (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of_val(fds) as u32) as _;
            std::ptr::copy_nonoverlapping(
                fds.as_ptr(),
                libc::CMSG_DATA(header) as *mut RawFd,
                fds.len(),
            );
        }
    }
    let flags = libc::MSG_NOSIGNAL | if nonblocking { libc::MSG_DONTWAIT } else { 0 };
    // SAFETY: message points at live buffers for the call's duration.
    let sent = unsafe { libc::sendmsg(socket, &message, flags) };
    if sent < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Receives one packet and the descriptors beside it; `None` at the end.
pub fn receive(socket: RawFd, nonblocking: bool) -> io::Result<Option<(Vec<u8>, Vec<OwnedFd>)>> {
    let mut data = vec![0u8; MAX_MESSAGE];
    let mut iov = libc::iovec {
        iov_base: data.as_mut_ptr() as *mut _,
        iov_len: data.len(),
    };
    // SAFETY: a pure size computation.
    let space = unsafe { libc::CMSG_SPACE((8 * std::mem::size_of::<RawFd>()) as u32) } as usize;
    let mut control = vec![0u8; space];
    // SAFETY: msghdr is plain data; the pointers set below outlive the call.
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr() as *mut _;
    message.msg_controllen = space as _;
    let flags = libc::MSG_CMSG_CLOEXEC | if nonblocking { libc::MSG_DONTWAIT } else { 0 };
    // SAFETY: message points at live buffers for the call's duration.
    let got = unsafe { libc::recvmsg(socket, &mut message, flags) };
    if got < 0 {
        return Err(io::Error::last_os_error());
    }
    if got == 0 && message.msg_controllen == 0 {
        return Ok(None);
    }
    let mut fds = Vec::new();
    // SAFETY: walks the control headers recvmsg wrote, within its length;
    // each SCM_RIGHTS descriptor is new and now ours.
    unsafe {
        let mut header = libc::CMSG_FIRSTHDR(&message);
        while !header.is_null() {
            if (*header).cmsg_level == libc::SOL_SOCKET && (*header).cmsg_type == libc::SCM_RIGHTS {
                let count = ((*header).cmsg_len as usize - libc::CMSG_LEN(0) as usize)
                    / std::mem::size_of::<RawFd>();
                let at = libc::CMSG_DATA(header) as *const RawFd;
                for i in 0..count {
                    fds.push(OwnedFd::from_raw_fd(std::ptr::read_unaligned(at.add(i))));
                }
            }
            header = libc::CMSG_NXTHDR(&message, header);
        }
    }
    data.truncate(got as usize);
    Ok(Some((data, fds)))
}

/// Sets `O_NONBLOCK` on a descriptor.
pub fn set_nonblocking(fd: &impl AsRawFd) -> io::Result<()> {
    // SAFETY: fcntl on a descriptor we hold.
    unsafe {
        let flags = libc::fcntl(fd.as_raw_fd(), libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
