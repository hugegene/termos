//! macOS: `getpeereid` and `LOCAL_PEERPID` for socket peers, and `proc_pidinfo`, `proc_name` and
//! `sysctl(KERN_PROCARGS2)` for processes, since there is no `/proc`.

use std::ffi::{OsString, c_int};
use std::io;
use std::mem;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::ptr;

use rustix::process::{Pid, Uid};

use super::Peer;

pub fn peer_cred(stream: &UnixStream) -> io::Result<Peer> {
    let fd = stream.as_raw_fd();
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: `fd` is an open socket, and both out-pointers point at live values of the right type.
    if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut pid: libc::pid_t = 0;
    let mut len = mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: as above; `len` is the size of `pid`.
    let r = unsafe { libc::getsockopt(fd, libc::SOL_LOCAL, libc::LOCAL_PEERPID, (&raw mut pid).cast(), &mut len) };
    if r != 0 {
        return Err(io::Error::last_os_error());
    }
    let pid = Pid::from_raw(pid).ok_or_else(|| io::Error::other("the socket's peer has no process id"))?;
    Ok(Peer { uid: Uid::from_raw(uid), pid })
}

pub fn process_cwd(pid: u32) -> Option<PathBuf> {
    let pid = c_int::try_from(pid).ok()?;
    // SAFETY: a plain C struct, for which all zeroes is a valid value.
    let mut info: libc::proc_vnodepathinfo = unsafe { mem::zeroed() };
    let size = mem::size_of_val(&info) as c_int;
    // SAFETY: `info` is writable for `size` bytes.
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDVNODEPATHINFO, 0, (&raw mut info).cast(), size) };
    if n != size {
        return None;
    }
    // A NUL-ended path of up to MAXPATHLEN bytes, which libc declares as 32 rows of 32.
    let path: Vec<u8> =
        info.pvi_cdir.vip_path.as_flattened().iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
    (!path.is_empty()).then(|| PathBuf::from(OsString::from_vec(path)))
}

pub fn process_name(pid: u32) -> Option<String> {
    let pid = c_int::try_from(pid).ok()?;
    // proc_name copies a name of up to 32 bytes, with no NUL when it's exactly 32; the zeroes after it end it.
    let mut buf = [0u8; 64];
    // SAFETY: `buf` is writable for its length.
    let n = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    let len = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    (n > 0 && len > 0).then(|| String::from_utf8_lossy(&buf[..len]).into_owned())
}

pub fn process_args(pid: u32) -> Option<Vec<String>> {
    let pid = c_int::try_from(pid).ok()?;
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    // Zeroed pages are only touched as the kernel writes to them, so the size costs little.
    let mut buf = vec![0u8; arg_max()];
    let mut len = buf.len();
    // SAFETY: `mib` holds `mib.len()` ints, and `buf` is writable for `len` bytes.
    let r = unsafe {
        libc::sysctl(mib.as_mut_ptr(), mib.len() as u32, buf.as_mut_ptr().cast(), &mut len, ptr::null_mut(), 0)
    };
    if r != 0 {
        return None;
    }
    super::split_procargs2(&buf[..len.min(buf.len())])
}

/// The most bytes a command line and its environment can take: what `KERN_PROCARGS2` may return.
fn arg_max() -> usize {
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    let mut max: c_int = 0;
    let mut len = mem::size_of::<c_int>();
    // SAFETY: `mib` holds 2 ints, and `max` is writable for `len` bytes.
    let r = unsafe {
        libc::sysctl(mib.as_mut_ptr(), mib.len() as u32, (&raw mut max).cast(), &mut len, ptr::null_mut(), 0)
    };
    match usize::try_from(max) {
        Ok(max) if r == 0 && max > 0 => max,
        // macOS's ARG_MAX.
        _ => 1 << 20,
    }
}

/// `/usr/bin/trash`, part of macOS 15 and later. The paths we give it are absolute, so none starts with `-`.
pub fn default_trash_command() -> Vec<String> {
    vec!["trash".into()]
}

/// `$SHELL -l` (else `/bin/zsh -l`): a login shell, as Terminal.app starts one. Left to itself,
/// `alacritty_terminal` starts it through `/usr/bin/login`, which runs as root. The pane's process would then
/// be `login`, so tos couldn't read the shell's folder (tab names, restored sessions) or tell the shell from a
/// command running in it.
pub fn default_shell() -> Option<Vec<String>> {
    let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/zsh".to_owned());
    Some(vec![shell, "-l".to_owned()])
}
