//! Where sockets, logs and helper files live.

use std::env;
use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

fn uid() -> u32 {
    rustix::process::getuid().as_raw()
}

/// Folder holding one socket per session. `TOS_SOCKET_DIR` overrides it (used by tests).
pub fn socket_dir() -> PathBuf {
    if let Some(dir) = env::var_os("TOS_SOCKET_DIR") {
        return dir.into();
    }
    match env::var_os("XDG_RUNTIME_DIR") {
        Some(rt) if !rt.is_empty() => PathBuf::from(rt).join("termos"),
        _ => PathBuf::from(format!("/tmp/termos-{}", uid())),
    }
}

pub fn socket_path(session: &str) -> PathBuf {
    socket_dir().join(format!("{session}.sock"))
}

fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// `$HOME`, then its real path if that's different, for showing a folder as `~`. A shell's folder comes from the
/// kernel, which gives the real path: on macOS a `$HOME` under `/var` is really under `/private/var`, and a home
/// can also sit behind a symlink on Linux. Read once.
pub fn home_dirs() -> &'static [PathBuf] {
    static HOMES: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    HOMES.get_or_init(|| {
        let Some(home) = env::var_os("HOME").map(PathBuf::from) else { return Vec::new() };
        let real = home.canonicalize().ok().filter(|real| *real != home);
        std::iter::once(home).chain(real).collect()
    })
}

/// Folder for logs. `TOS_STATE_DIR` overrides it.
pub fn state_dir() -> PathBuf {
    env::var_os("TOS_STATE_DIR").map(PathBuf::from).unwrap_or_else(default_state_dir)
}

fn default_state_dir() -> PathBuf {
    match env::var_os("XDG_STATE_HOME") {
        Some(d) if !d.is_empty() => PathBuf::from(d).join("termos"),
        _ => home().join(".local/state/termos"),
    }
}

/// Folder for `config.kdl`: `$XDG_CONFIG_HOME/termos`, else `~/.config/termos`.
pub fn config_dir() -> PathBuf {
    match env::var_os("XDG_CONFIG_HOME") {
        Some(d) if !d.is_empty() => PathBuf::from(d).join("termos"),
        _ => home().join(".config/termos"),
    }
}

/// `tos` was called `tb` before 0.2.0, and its folders were named `tb`. This moves the old state, data and
/// config folders to their `termos` names, once: only where the new folder doesn't exist yet, and only once no
/// session started by `tb` is still running, since that would keep saving into the old folder. Returns
/// what to tell the user.
pub fn migrate_from_tb() -> Vec<String> {
    let old_sockets = match env::var_os("XDG_RUNTIME_DIR") {
        Some(rt) if !rt.is_empty() => PathBuf::from(rt).join("tb"),
        _ => PathBuf::from(format!("/tmp/tb-{}", uid())),
    };
    let mut new = Vec::new();
    if env::var_os("TOS_STATE_DIR").is_none() {
        new.push(default_state_dir());
    }
    new.push(data_dir());
    if env::var_os("TOS_CONFIG").is_none() {
        new.push(config_dir());
    }
    migrate(&new, &old_sockets)
}

/// Moves each folder's `tb` sibling to it (see `migrate_from_tb`).
fn migrate(new_dirs: &[PathBuf], old_sockets: &Path) -> Vec<String> {
    let pending: Vec<(PathBuf, &PathBuf)> = new_dirs
        .iter()
        .map(|new| (new.with_file_name("tb"), new))
        .filter(|(old, new)| old.is_dir() && !new.exists())
        .collect();
    if pending.is_empty() {
        return Vec::new();
    }
    let running: Vec<String> = fs::read_dir(old_sockets)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| std::os::unix::net::UnixStream::connect(e.path()).is_ok())
        .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    if !running.is_empty() {
        return vec![format!(
            "tb is now called tos. Sessions started by tb are still running ({}), so their saved layouts stay \
             in {} for now. End them with `tb kill-all-sessions -y`, then run tos again to move them.",
            running.join(", "),
            pending[0].0.display()
        )];
    }
    pending
        .into_iter()
        .map(|(old, new)| match fs::rename(&old, new) {
            Ok(()) => format!("moved {} to {} (tb is now called tos)", old.display(), new.display()),
            Err(e) => format!("couldn't move {} to {}: {e}", old.display(), new.display()),
        })
        .collect()
}

/// Folder for files we install for helper programs (the finder's Neovim config).
pub fn data_dir() -> PathBuf {
    match env::var_os("XDG_DATA_HOME") {
        Some(d) if !d.is_empty() => PathBuf::from(d).join("termos"),
        _ => home().join(".local/share/termos"),
    }
}

pub fn log_path(session: &str) -> PathBuf {
    state_dir().join(format!("{session}.log"))
}

/// Creates `dir` if needed, makes it private (0700) and checks we own it.
pub fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let meta = fs::metadata(dir)?;
    if meta.uid() != uid() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is owned by another user", dir.display()),
        ));
    }
    if meta.permissions().mode() & 0o777 != 0o700 {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Where a session is saved for restoring (step 4).
pub fn saved_path(session: &str) -> PathBuf {
    state_dir().join("sessions").join(format!("{session}.json"))
}

/// Names of saved sessions, running or not.
pub fn saved_names() -> Vec<String> {
    let Ok(read) = fs::read_dir(state_dir().join("sessions")) else { return Vec::new() };
    let mut names: Vec<String> = read
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".json").map(str::to_owned))
        .filter(|n| valid_session_name(n))
        .collect();
    names.sort();
    names
}

/// Session names become file names, so keep them simple.
pub fn valid_session_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Names of sessions that have a socket. Some may be stale; callers check by connecting.
pub fn session_names() -> Vec<String> {
    let Ok(read) = fs::read_dir(socket_dir()) else { return Vec::new() };
    let mut names: Vec<String> = read
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".sock").map(str::to_owned))
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tb_folders_move_once_and_not_while_tb_runs() {
        let root = tempfile::tempdir().unwrap();
        let (state, config) = (root.path().join("state/termos"), root.path().join("config/termos"));
        fs::create_dir_all(root.path().join("state/tb/sessions")).unwrap();
        fs::write(root.path().join("state/tb/sessions/main.json"), "{}").unwrap();
        let sockets = root.path().join("run-tb");
        fs::create_dir_all(&sockets).unwrap();

        // A session started by tb still answers: nothing moves, and the note says which one.
        let listener = std::os::unix::net::UnixListener::bind(sockets.join("main2.sock")).unwrap();
        let notes = migrate(&[state.clone(), config.clone()], &sockets);
        assert!(notes.len() == 1 && notes[0].contains("(main2)") && !state.exists(), "{notes:?}");

        // Once it has ended (its socket file is left over), the state folder moves. There was no tb config.
        drop(listener);
        let notes = migrate(&[state.clone(), config.clone()], &sockets);
        assert!(notes.len() == 1 && notes[0].starts_with("moved "), "{notes:?}");
        assert!(state.join("sessions/main.json").exists() && !root.path().join("state/tb").exists());
        assert!(!config.exists());

        // Only once.
        assert!(migrate(&[state, config], &sockets).is_empty());
    }

    #[test]
    fn session_names_are_checked() {
        assert!(valid_session_name("main"));
        assert!(valid_session_name("work-2.b_c"));
        assert!(!valid_session_name(""));
        assert!(!valid_session_name("../x"));
        assert!(!valid_session_name(".hidden"));
        assert!(!valid_session_name("a b"));
    }
}
