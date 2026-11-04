//! Where sockets, logs and helper files live.

use std::env;
use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

fn uid() -> u32 {
    rustix::process::getuid().as_raw()
}

/// Folder holding one socket per session. `TB_SOCKET_DIR` overrides it (used by tests).
pub fn socket_dir() -> PathBuf {
    if let Some(dir) = env::var_os("TB_SOCKET_DIR") {
        return dir.into();
    }
    match env::var_os("XDG_RUNTIME_DIR") {
        Some(rt) if !rt.is_empty() => PathBuf::from(rt).join("tb"),
        _ => PathBuf::from(format!("/tmp/tb-{}", uid())),
    }
}

pub fn socket_path(session: &str) -> PathBuf {
    socket_dir().join(format!("{session}.sock"))
}

fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Folder for logs. `TB_STATE_DIR` overrides it.
pub fn state_dir() -> PathBuf {
    if let Some(dir) = env::var_os("TB_STATE_DIR") {
        return dir.into();
    }
    match env::var_os("XDG_STATE_HOME") {
        Some(d) if !d.is_empty() => PathBuf::from(d).join("tb"),
        _ => home().join(".local/state/tb"),
    }
}

/// Folder for files we install for helper programs (the finder's Neovim config).
pub fn data_dir() -> PathBuf {
    match env::var_os("XDG_DATA_HOME") {
        Some(d) if !d.is_empty() => PathBuf::from(d).join("tb"),
        _ => home().join(".local/share/tb"),
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
    fn session_names_are_checked() {
        assert!(valid_session_name("main"));
        assert!(valid_session_name("work-2.b_c"));
        assert!(!valid_session_name(""));
        assert!(!valid_session_name("../x"));
        assert!(!valid_session_name(".hidden"));
        assert!(!valid_session_name("a b"));
    }
}
