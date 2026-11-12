//! `o` in the finder opens a file (or folder) in its default app: `xdg-open` on Linux, `open` on macOS. It opens
//! on the display of the client that pressed the key. The app is left to run on its own: `tos` doesn't track it,
//! and closing the finder doesn't stop it. Plan: docs/plans/2026-11-11-grid-copy-and-open-in-app.md.

use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

use crate::protocol::{DISPLAY_VARS, Display};

/// The program that opens a file in its default app. On Linux it's named in full: `open` is a different program
/// on some systems (`openvt`).
#[cfg(target_os = "macos")]
const OPENER: &str = "open";
#[cfg(not(target_os = "macos"))]
const OPENER: &str = "xdg-open";

#[cfg(target_os = "macos")]
const MISSING: &str = "o needs macOS's open command, which isn't on PATH";
#[cfg(not(target_os = "macos"))]
const MISSING: &str = "o needs xdg-open (package xdg-utils)";

/// Opens `path` in its default app on `display`'s screen, and returns what to tell the user.
pub fn open_in_app(path: &Path, display: &Display) -> String {
    open_with(OPENER, path, display)
}

fn open_with(program: &str, path: &Path, display: &Display) -> String {
    if let Some(why) = no_screen(display) {
        return why.to_owned();
    }
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    match start(program, path, display) {
        Ok(()) => format!("opening {name} in its default app"),
        Err(e) if e.kind() == io::ErrorKind::NotFound => MISSING.to_owned(),
        Err(e) => format!("can't start {program}: {e}"),
    }
}

/// Why there's nowhere to show a window, if there isn't. On macOS windows need no variables, but a login over
/// SSH would put them on the Mac's own screen.
fn no_screen(display: &Display) -> Option<&'static str> {
    let over_ssh = "no display to open it on: you're attached over SSH";
    if cfg!(target_os = "macos") {
        return display.ssh.then_some(over_ssh);
    }
    match (display.has_screen(), display.ssh) {
        (true, _) => None,
        (false, true) => Some(over_ssh),
        (false, false) => Some("no display to open it on"),
    }
}

/// Starts `program FILE` in its own session, with no input or output, and the display variables of `display`
/// (and none of the server's own). The finder's paths are absolute, so a file can't pass for an option.
fn start(program: &str, path: &Path, display: &Display) -> io::Result<()> {
    let mut cmd = Command::new(program);
    cmd.arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    for key in DISPLAY_VARS {
        match display.get(key) {
            Some(value) => cmd.env(key, value),
            None => cmd.env_remove(key),
        };
    }
    // SAFETY: setsid is async-signal-safe, and nothing else runs between fork and exec.
    unsafe {
        cmd.pre_exec(|| rustix::process::setsid().map(drop).map_err(io::Error::from));
    }
    let mut child = cmd.spawn()?;
    // Collects its exit status when it ends, so it doesn't linger as a zombie.
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(vars: &[(&str, &str)], ssh: bool) -> Display {
        Display { vars: vars.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(), ssh }
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn needs_a_screen() {
        let none = display(&[("XDG_RUNTIME_DIR", "/run/user/1000")], false);
        assert_eq!(no_screen(&none), Some("no display to open it on"));
        assert_eq!(no_screen(&display(&[], true)), Some("no display to open it on: you're attached over SSH"));
        // ssh -X forwards X11, so a display over SSH is fine.
        assert_eq!(no_screen(&display(&[("DISPLAY", "localhost:10.0")], true)), None);
        assert_eq!(no_screen(&display(&[("WAYLAND_DISPLAY", "wayland-0")], false)), None);
    }

    #[test]
    fn opens_or_says_what_is_missing() {
        let screen = display(&[("DISPLAY", ":0"), ("WAYLAND_DISPLAY", "wayland-0")], false);
        let path = Path::new("/no/such/budget.xlsx");
        // `true` stands in for xdg-open: it starts, ignores the file and exits.
        assert_eq!(open_with("true", path, &screen), "opening budget.xlsx in its default app");
        assert_eq!(open_with("tos-no-such-opener", path, &screen), MISSING);
        assert_eq!(open_with("true", path, &display(&[], true)), "no display to open it on: you're attached over SSH");
    }
}
