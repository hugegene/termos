//! Linux: `SO_PEERCRED` for socket peers and `/proc/<pid>` for processes.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use super::Peer;

pub fn peer_cred(stream: &UnixStream) -> io::Result<Peer> {
    let c = rustix::net::sockopt::socket_peercred(stream)?;
    Ok(Peer { uid: c.uid, pid: c.pid })
}

pub fn process_cwd(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

pub fn process_name(pid: u32) -> Option<String> {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    Some(comm.trim().to_owned())
}

pub fn process_args(pid: u32) -> Option<Vec<String>> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    super::split_args(raw.split(|b| *b == 0))
}

/// GNOME's `gio`; it says why when a file can't go to the trash, for example on a system mount.
pub fn default_trash_command() -> Vec<String> {
    vec!["gio".into(), "trash".into(), "--".into()]
}
