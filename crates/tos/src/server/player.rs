//! Enter on a video plays it in a window: in mpv, or in ffplay (which comes with ffmpeg) without mpv. It opens
//! on the display of the client that pressed Enter. The player is left to run on its own: `tos` doesn't track
//! it, and closing the finder doesn't stop it. Plan: docs/plans/2026-11-07-video-and-office-previews.md.

use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

use crate::protocol::{DISPLAY_VARS, Display};

/// The players, best first, with the arguments that come before the file.
const PLAYERS: [(&str, &[&str]); 2] =
    [("mpv", &["--force-window=immediate", "--"]), ("ffplay", &["-autoexit", "-loglevel", "error", "--"])];

#[cfg(target_os = "macos")]
const INSTALL_MPV: &str = "brew install mpv";
#[cfg(not(target_os = "macos"))]
const INSTALL_MPV: &str = "package mpv";

/// Plays `path` on `display`'s screen, and returns what to tell the user.
pub fn play(path: &Path, display: &Display) -> String {
    play_with(&PLAYERS, path, display)
}

fn play_with(players: &[(&str, &[&str])], path: &Path, display: &Display) -> String {
    if let Some(why) = no_screen(display) {
        return why.to_owned();
    }
    for (program, args) in players {
        match start(program, args, path, display) {
            Ok(()) if *program == "ffplay" => return "playing in ffplay (install mpv for a better player)".into(),
            Ok(()) => return format!("playing in {program}"),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return format!("can't start {program}: {e}"),
        }
    }
    format!("Enter plays videos with mpv ({INSTALL_MPV})")
}

/// Why there's nowhere to show a window, if there isn't. On macOS windows need no variables, but a login over
/// SSH would put them on the Mac's own screen.
fn no_screen(display: &Display) -> Option<&'static str> {
    let over_ssh = "no display to play on: you're attached over SSH";
    if cfg!(target_os = "macos") {
        return display.ssh.then_some(over_ssh);
    }
    match (display.has_screen(), display.ssh) {
        (true, _) => None,
        (false, true) => Some(over_ssh),
        (false, false) => Some("no display to play on"),
    }
}

/// Starts the player in its own session, with no input or output, and the display variables of `display` (and
/// none of the server's own).
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
        assert_eq!(no_screen(&none), Some("no display to play on"));
        assert_eq!(no_screen(&display(&[], true)), Some("no display to play on: you're attached over SSH"));
        // ssh -X forwards X11, so a display over SSH is fine.
        assert_eq!(no_screen(&display(&[("DISPLAY", "localhost:10.0")], true)), None);
        assert_eq!(no_screen(&display(&[("WAYLAND_DISPLAY", "wayland-0")], false)), None);
    }

    #[test]
    fn falls_back_through_the_players() {
        let screen = display(&[("DISPLAY", ":0"), ("WAYLAND_DISPLAY", "wayland-0")], false);
        let path = Path::new("/no/such/clip.mp4");
        let missing: [(&str, &[&str]); 2] = [("tos-no-such-mpv", &[]), ("tos-no-such-ffplay", &[])];
        assert!(play_with(&missing, path, &screen).starts_with("Enter plays videos with mpv ("));
        // `true` stands in for ffplay: it starts, ignores the file and exits.
        let second: [(&str, &[&str]); 2] = [("tos-no-such-mpv", &[]), ("true", &[])];
        assert_eq!(play_with(&second, path, &screen), "playing in true");
        let first: [(&str, &[&str]); 1] = [("true", &[])];
        assert_eq!(play_with(&first, path, &screen), "playing in true");
    }
}
