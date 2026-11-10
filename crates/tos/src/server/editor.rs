//! Which editor the finder opens: `$TOS_EDITOR` if set, else Neovim with our notepad-style config, else
//! `$VISUAL`/`$EDITOR`/`vi`.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use super::pane::SpawnOptions;
use crate::paths;

/// The prototype's Neovim config (Alt+s save, Alt+q close, opens ready to type).
const NVIM_INIT: &str = include_str!("../../../../nvim/init.lua");

fn find_in_path(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?).map(|d| d.join(name)).find(|p| p.is_file())
}

fn find_nvim() -> Option<PathBuf> {
    find_in_path("nvim").or_else(|| {
        let p = PathBuf::from(env::var_os("HOME")?).join(".local/bin/nvim");
        p.is_file().then_some(p)
    })
}

/// Writes the Neovim config to our data folder if it's missing or out of date, and returns its path.
fn nvim_init() -> Option<PathBuf> {
    let path = paths::data_dir().join("nvim/init.lua");
    if fs::read_to_string(&path).ok().as_deref() != Some(NVIM_INIT) {
        fs::create_dir_all(path.parent()?).ok()?;
        fs::write(&path, NVIM_INIT).ok()?;
    }
    Some(path)
}

pub fn command(file: &Path) -> SpawnOptions {
    let cwd = file.parent().map(Path::to_path_buf);
    let file = file.to_string_lossy().into_owned();
    // TOS_EDITOR overrides everything (a command; the file name is added at the end).
    if let Ok(editor) = env::var("TOS_EDITOR")
        && !editor.trim().is_empty()
    {
        let mut command: Vec<String> = editor.split_whitespace().map(str::to_owned).collect();
        command.push(file);
        return SpawnOptions { cwd, command, env: HashMap::new() };
    }
    if let (Some(nvim), Some(init)) = (find_nvim(), nvim_init()) {
        let mut env = HashMap::new();
        // Same plugin folder the prototype's install.sh fills (render-markdown.nvim).
        env.insert("NVIM_APPNAME".into(), "tfinder-nvim".into());
        return SpawnOptions {
            cwd,
            command: vec![nvim.to_string_lossy().into_owned(), "-u".into(), init.to_string_lossy().into_owned(), file],
            env,
        };
    }
    let editor = env::var("VISUAL").or_else(|_| env::var("EDITOR")).unwrap_or_else(|_| "vi".into());
    let mut command: Vec<String> = editor.split_whitespace().map(str::to_owned).collect();
    command.push(file);
    SpawnOptions { cwd, command, env: HashMap::new() }
}
