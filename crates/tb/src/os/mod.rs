//! What differs between Linux and macOS, in one place: who is on the other end of a socket, what a process is
//! running and in which folder, and how to move a file to the trash.
//!
//! `linux.rs` reads `/proc` and uses `SO_PEERCRED`; `macos.rs` asks the kernel through `libc`
//! (`docs/plans/2026-11-09-macos-support.md`). Callers use only the functions below.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use rustix::process::{Pid, Uid};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as sys;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as sys;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("tb runs on Linux and macOS only");

/// The process on the other end of a Unix socket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
    pub uid: Uid,
    pub pid: Pid,
}

/// Who is on the other end of `stream`: their user, to refuse other users, and their process, to end an old
/// server.
pub fn peer_cred(stream: &UnixStream) -> io::Result<Peer> {
    sys::peer_cred(stream)
}

/// A process's working folder (it follows the shell's `cd`). `None` if the process is gone.
pub fn process_cwd(pid: u32) -> Option<PathBuf> {
    sys::process_cwd(pid)
}

/// A process's program name, for example "nvim" or "bash". Linux cuts it to 15 bytes, macOS to 32.
pub fn process_name(pid: u32) -> Option<String> {
    sys::process_name(pid)
}

/// A process's command line: the program as it was started, then its arguments. Empty arguments are left
/// out. `None` if the process is gone or has no arguments.
pub fn process_args(pid: u32) -> Option<Vec<String>> {
    sys::process_args(pid)
}

/// The command that moves a file to the trash when `$TB_TRASH` isn't set. The file's path goes at the end.
pub fn default_trash_command() -> Vec<String> {
    sys::default_trash_command()
}

/// The shell, with its arguments, for a pane started without a command. `None` leaves the choice to
/// `alacritty_terminal`: `$SHELL`, else the user's shell from the password database.
pub fn default_shell() -> Option<Vec<String>> {
    sys::default_shell()
}

/// Splits NUL-separated arguments, as the kernel stores them, leaving out empty ones.
fn split_args<'a>(raw: impl IntoIterator<Item = &'a [u8]>) -> Option<Vec<String>> {
    let args: Vec<String> =
        raw.into_iter().filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
    (!args.is_empty()).then_some(args)
}

/// Reads macOS's `KERN_PROCARGS2` layout: the argument count as a native `i32`, the program's path, NULs up to
/// a word boundary, that many NUL-ended arguments, then the environment. It's here rather than in `macos.rs`
/// so its test runs on Linux too.
#[cfg(any(target_os = "macos", test))]
fn split_procargs2(buf: &[u8]) -> Option<Vec<String>> {
    let (count, rest) = buf.split_first_chunk::<4>()?;
    let argc = usize::try_from(i32::from_ne_bytes(*count)).ok()?;
    let rest = &rest[rest.iter().position(|b| *b == 0)?..];
    let rest = &rest[rest.iter().position(|b| *b != 0)?..];
    split_args(rest.split(|b| *b == 0).take(argc))
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;

    use super::*;

    /// The test process asks about itself, so these run the same way on Linux and macOS.
    fn me() -> u32 {
        std::process::id()
    }

    #[test]
    fn cwd_is_the_current_folder() {
        let cwd = process_cwd(me()).expect("our own folder");
        assert_eq!(cwd.canonicalize().unwrap(), std::env::current_dir().unwrap().canonicalize().unwrap());
    }

    #[test]
    fn name_is_the_test_binary() {
        let exe = std::env::current_exe().unwrap();
        let exe = exe.file_name().unwrap().to_string_lossy();
        let name = process_name(me()).expect("our own name");
        // The binary is "tb-<hash>", which Linux cuts to 15 bytes.
        assert!(name.len() >= 2 && exe.starts_with(&name), "name {name:?}, binary {exe:?}");
    }

    #[test]
    fn args_are_ours() {
        let expected: Vec<String> = std::env::args().filter(|a| !a.is_empty()).collect();
        assert_eq!(process_args(me()), Some(expected));
    }

    #[test]
    fn a_gone_process_has_nothing() {
        // Larger than any real pid on Linux (at most 2^22) and on macOS (99998).
        let gone = 0x7fff_fff0;
        assert_eq!(process_cwd(gone), None);
        assert_eq!(process_name(gone), None);
        assert_eq!(process_args(gone), None);
    }

    /// Both ends of a socket made as the server makes it: bind, connect, accept.
    #[test]
    fn peer_is_us_on_both_ends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s");
        let listener = UnixListener::bind(&path).unwrap();
        let client = UnixStream::connect(&path).unwrap();
        let (server, _) = listener.accept().unwrap();
        let us = Peer { uid: rustix::process::getuid(), pid: rustix::process::getpid() };
        assert_eq!(peer_cred(&server).unwrap(), us);
        assert_eq!(peer_cred(&client).unwrap(), us);
    }

    #[test]
    fn args_split_at_nul_without_empty_ones() {
        let raw = b"vim\0\0notes.md\0";
        assert_eq!(split_args(raw.split(|b| *b == 0)), Some(vec!["vim".to_owned(), "notes.md".to_owned()]));
        assert_eq!(split_args(b"\0\0".split(|b| *b == 0)), None);
    }

    #[test]
    fn procargs2_gives_the_arguments_without_the_environment() {
        let mut buf = 2i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/opt/homebrew/bin/nvim\0\0\0\0nvim\0notes.md\0HOME=/Users/u\0TERM=xterm\0");
        assert_eq!(split_procargs2(&buf), Some(vec!["nvim".to_owned(), "notes.md".to_owned()]));

        // An empty argument still counts, so the environment stays out.
        let mut buf = 3i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/bin/sh\0sh\0\0x\0HOME=/Users/u\0");
        assert_eq!(split_procargs2(&buf), Some(vec!["sh".to_owned(), "x".to_owned()]));

        assert_eq!(split_procargs2(&[1, 0]), None);
        assert_eq!(split_procargs2(&0i32.to_ne_bytes()), None);
    }
}
