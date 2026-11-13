//! Which editor the finder opens: `$TOS_EDITOR` if set, else Neovim with our notepad-style config, else
//! `$VISUAL`/`$EDITOR`/`vi`. It also says which it picked, so hints name the right keys. Plan:
//! docs/plans/2026-11-13-editor-save-keys.md.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use super::pane::SpawnOptions;
use crate::paths;

/// Our Neovim config (Ctrl+s save, Ctrl+e close, opens ready to type), which began as the zellij prototype's.
const NVIM_INIT: &str = include_str!("../../nvim/init.lua");

/// How to install Neovim, for the tip when it's missing.
#[cfg(target_os = "macos")]
const INSTALL_NEOVIM: &str = "brew install neovim";
#[cfg(not(target_os = "macos"))]
const INSTALL_NEOVIM: &str = "package neovim";

/// Which editor `command` picked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Editor {
    /// Neovim with our config: Ctrl+s saves, Ctrl+e closes.
    Ours,
    /// `$TOS_EDITOR`, by its program's name, with its own keys.
    Chosen(String),
    /// Neovim isn't installed: `$VISUAL`, `$EDITOR` or vi, by name, with its own keys.
    Fallback(String),
}

impl Editor {
    /// How to save and close, for the finder's caption and the editor's frame.
    pub fn keys(&self) -> String {
        match self {
            Editor::Ours => "Ctrl+s save · Ctrl+e close".into(),
            Editor::Chosen(name) | Editor::Fallback(name) if is_vi(name) => {
                format!("{name} · Esc :wq saves and closes")
            }
            Editor::Chosen(name) | Editor::Fallback(name) => format!("{name} · its own keys save and close"),
        }
    }
}

/// vi and the editors that take its `:wq`.
fn is_vi(name: &str) -> bool {
    matches!(name, "vi" | "vim" | "nvim" | "vim.basic" | "vim.tiny" | "nvi")
}

/// The tip shown once when Neovim is missing and `fallback` opened instead.
pub fn missing_neovim(fallback: &str) -> String {
    format!("editing in {fallback}: Neovim isn't installed ({INSTALL_NEOVIM}); with it, Ctrl+s saves and Ctrl+e closes")
}

/// A command's program name, without its folder: `/usr/bin/vim -p` is `vim`.
fn program_name(command: &[String]) -> String {
    let first = command.first().map(String::as_str).unwrap_or_default();
    Path::new(first).file_name().map_or_else(|| first.to_owned(), |n| n.to_string_lossy().into_owned())
}

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

pub fn command(file: &Path) -> (SpawnOptions, Editor) {
    let cwd = file.parent().map(Path::to_path_buf);
    let file = file.to_string_lossy().into_owned();
    // TOS_EDITOR overrides everything (a command; the file name is added at the end).
    if let Ok(editor) = env::var("TOS_EDITOR")
        && !editor.trim().is_empty()
    {
        let mut command: Vec<String> = editor.split_whitespace().map(str::to_owned).collect();
        let name = program_name(&command);
        command.push(file);
        return (SpawnOptions { cwd, command, env: HashMap::new() }, Editor::Chosen(name));
    }
    if let (Some(nvim), Some(init)) = (find_nvim(), nvim_init()) {
        let mut env = HashMap::new();
        // Same plugin folder the zellij prototype's install.sh fills (render-markdown.nvim, see ~/tfinder).
        env.insert("NVIM_APPNAME".into(), "tfinder-nvim".into());
        let command = vec![nvim.to_string_lossy().into_owned(), "-u".into(), init.to_string_lossy().into_owned(), file];
        return (SpawnOptions { cwd, command, env }, Editor::Ours);
    }
    let editor = env::var("VISUAL").or_else(|_| env::var("EDITOR")).unwrap_or_else(|_| "vi".into());
    let mut command: Vec<String> = editor.split_whitespace().map(str::to_owned).collect();
    let name = program_name(&command);
    command.push(file);
    (SpawnOptions { cwd, command, env: HashMap::new() }, Editor::Fallback(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_name_the_editors_keys() {
        assert_eq!(Editor::Ours.keys(), "Ctrl+s save · Ctrl+e close");
        assert_eq!(Editor::Fallback("vi".into()).keys(), "vi · Esc :wq saves and closes");
        assert_eq!(Editor::Chosen("micro".into()).keys(), "micro · its own keys save and close");
        assert!(missing_neovim("vi").starts_with("editing in vi: Neovim isn't installed ("));
    }

    #[test]
    fn program_names_drop_the_folder() {
        let words = |s: &str| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(program_name(&words("/usr/bin/vim -p")), "vim");
        assert_eq!(program_name(&words("micro")), "micro");
        assert_eq!(program_name(&[]), "");
    }
}
