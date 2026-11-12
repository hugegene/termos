//! `o` in the finder opens a file (or folder) in its default app: `xdg-open` on Linux, `open` on macOS. A video
//! with no default app plays in mpv, or else ffplay. It opens on the display of the client that pressed the key.
//! The app is left to run on its own: `tos` doesn't track it, and closing the finder doesn't stop it.
//! Plan: docs/plans/2026-11-11-grid-copy-and-open-in-app.md.

use std::ffi::OsStr;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

use finder_core::Kind;

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

/// Players for a video with no default app, best first, with the arguments that come before the file.
const PLAYERS: [(&str, &[&str]); 2] =
    [("mpv", &["--force-window=immediate", "--"]), ("ffplay", &["-autoexit", "-loglevel", "error", "--"])];

/// Opens `path` in its default app on `display`'s screen, and returns what to tell the user. A video with no
/// default app plays in mpv or ffplay instead. macOS's `open` always has an app for a video.
pub fn open_in_app(path: &Path, display: &Display) -> String {
    if let Some(why) = no_screen(display) {
        return why.to_owned();
    }
    if !cfg!(target_os = "macos") && finder_core::detect(path) == Kind::Video && !has_default_app(path) {
        return play_with(&PLAYERS, path, display);
    }
    open_with(OPENER, path, display)
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

fn open_with(program: &str, path: &Path, display: &Display) -> String {
    match start(program, &[], path, display) {
        Ok(()) => format!("opening {} in its default app", name_of(path)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => MISSING.to_owned(),
        Err(e) => format!("can't start {program}: {e}"),
    }
}

/// The first of `players` that's installed plays the video.
fn play_with(players: &[(&str, &[&str])], path: &Path, display: &Display) -> String {
    let name = name_of(path);
    for (program, args) in players {
        match start(program, args, path, display) {
            Ok(()) => return format!("playing {name} in {program} (no default app for videos)"),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return format!("can't start {program}: {e}"),
        }
    }
    format!("no app for {name}: set a default video player, or install mpv (package mpv)")
}

/// Whether `xdg-mime` knows a default app for the file's type: two queries of 10–20 ms each
/// (docs/findings/2026-11-12-video-default-app.md). False without xdg-utils.
fn has_default_app(path: &Path) -> bool {
    let query = |what: &str, arg: &OsStr| -> Option<String> {
        let out = Command::new("xdg-mime")
            .args(["query", what])
            .arg(arg)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        let text = String::from_utf8(out.stdout).ok()?.trim().to_owned();
        (out.status.success() && !text.is_empty()).then_some(text)
    };
    query("filetype", path.as_os_str()).and_then(|mime| query("default", OsStr::new(&mime))).is_some()
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

/// Starts `program ARGS FILE` in its own session, with no input or output, and the display variables of `display`
/// (and none of the server's own). The finder's paths are absolute, so a file can't pass for an option.
fn start(program: &str, args: &[&str], path: &Path, display: &Display) -> io::Result<()> {
    let mut cmd = Command::new(program);
    cmd.args(args).arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
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
        assert_eq!(open_in_app(path, &display(&[], true)), "no display to open it on: you're attached over SSH");
    }

    #[test]
    fn a_video_without_a_default_app_falls_back_through_the_players() {
        let screen = display(&[("DISPLAY", ":0")], false);
        let path = Path::new("/no/such/clip.mp4");
        let missing: [(&str, &[&str]); 2] = [("tos-no-such-mpv", &[]), ("tos-no-such-ffplay", &[])];
        assert_eq!(
            play_with(&missing, path, &screen),
            "no app for clip.mp4: set a default video player, or install mpv (package mpv)"
        );
        // `true` stands in for ffplay.
        let second: [(&str, &[&str]); 2] = [("tos-no-such-mpv", &[]), ("true", &[])];
        assert_eq!(play_with(&second, path, &screen), "playing clip.mp4 in true (no default app for videos)");
    }
}
