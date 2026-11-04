//! Saving a session to disk and bringing it back after a reboot, crash, `tb kill` or Ctrl+Q (step 4).
//!
//! What comes back: tabs, the split layout, names, each terminal's folder, and the finders' state. Programs
//! can't come back; a command that was running is typed at the new shell's prompt, without Enter.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::finder::{Finder, FinderState};
use super::layout::Node;
use super::pane::SpawnOptions;
use super::{Pane, Server, Tab};
use crate::paths;
use crate::protocol::{PaneId, SplitDir};

const VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Saved {
    pub version: u32,
    pub frames: bool,
    pub active: usize,
    pub tabs: Vec<SavedTab>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct SavedTab {
    pub name: String,
    pub auto_name: bool,
    pub zoomed: bool,
    pub sync: bool,
    /// The focused pane, counted in reading order.
    pub focus: usize,
    pub layout: SavedNode,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum SavedNode {
    Pane(SavedPane),
    Split { dir: SplitDir, ratio: f32, a: Box<SavedNode>, b: Box<SavedNode> },
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub enum PaneKind {
    Terminal,
    Finder,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct SavedPane {
    pub kind: PaneKind,
    /// A name given with "rename pane".
    pub name: Option<String>,
    /// The terminal's working folder.
    pub cwd: Option<PathBuf>,
    /// The command running in the terminal, if it wasn't the shell.
    pub command: Option<Vec<String>>,
    pub finder: Option<FinderState>,
}

pub fn load(session: &str) -> Option<Saved> {
    let path = paths::saved_path(session);
    let text = fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<Saved>(&text) {
        Ok(saved) if saved.version == VERSION => Some(saved),
        Ok(saved) => {
            log::warn!("{}: unknown version {}; not restoring", path.display(), saved.version);
            None
        }
        Err(e) => {
            log::warn!("{}: {e}; not restoring", path.display());
            None
        }
    }
}

/// Writes the file under a temporary name and renames it, so a crash never leaves half a file.
pub fn write(session: &str, json: &str) -> io::Result<()> {
    let path = paths::saved_path(session);
    if let Some(dir) = path.parent() {
        paths::ensure_private_dir(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut f = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
    f.write_all(json.as_bytes())?;
    f.sync_all()?;
    fs::rename(&tmp, &path)
}

pub fn delete(session: &str) {
    let _ = fs::remove_file(paths::saved_path(session));
}

/// Joins a command line for typing at a shell prompt, quoting what the shell would split or expand.
pub fn shell_join(args: &[String]) -> String {
    let safe = |a: &str| !a.is_empty() && a.chars().all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c));
    args.iter()
        .map(|a| if safe(a) { a.clone() } else { format!("'{}'", a.replace('\'', r"'\''")) })
        .collect::<Vec<_>>()
        .join(" ")
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// A folder to start in: the saved one if it still exists, else the home folder.
fn usable_dir(dir: Option<&Path>) -> Option<PathBuf> {
    dir.filter(|d| d.is_dir()).map(Path::to_path_buf).or_else(home)
}

/// Text waiting to be typed into a restored shell once it has drawn its prompt.
pub struct PendingInput {
    pub text: String,
    pub since: Instant,
    pub output_seen: bool,
}

impl Server {
    pub(super) fn snapshot(&self) -> Saved {
        let tabs = self
            .tabs
            .iter()
            .map(|t| SavedTab {
                name: t.name.clone(),
                auto_name: t.auto_name,
                zoomed: t.zoomed,
                sync: t.sync,
                focus: t.root.panes().iter().position(|p| *p == t.focus).unwrap_or(0),
                layout: self.save_node(&t.root),
            })
            .collect();
        Saved { version: VERSION, frames: self.frames, active: self.active, tabs }
    }

    fn save_node(&self, node: &Node) -> SavedNode {
        match node {
            Node::Leaf(id) => SavedNode::Pane(self.save_pane(*id)),
            Node::Split { dir, ratio, a, b } => SavedNode::Split {
                dir: *dir,
                ratio: *ratio,
                a: Box::new(self.save_node(a)),
                b: Box::new(self.save_node(b)),
            },
        }
    }

    fn save_pane(&self, id: PaneId) -> SavedPane {
        // A scrollback editor stands in for another pane; save that one.
        let id = self.scrollback.get(&id).map_or(id, |(orig, _)| *orig);
        let name = self.pane_names.get(&id).filter(|n| !n.is_empty()).cloned();
        match self.panes.get(&id) {
            Some(Pane::Finder(f)) => SavedPane {
                kind: PaneKind::Finder,
                name,
                cwd: Some(f.dir.clone()),
                command: None,
                finder: Some(f.state()),
            },
            Some(Pane::Term(p)) => {
                SavedPane { kind: PaneKind::Terminal, name, cwd: p.cwd(), command: p.running_command(), finder: None }
            }
            None => SavedPane { kind: PaneKind::Terminal, name, cwd: None, command: None, finder: None },
        }
    }

    /// Saves the session if it changed since the last save.
    pub(super) fn autosave(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        let Ok(json) = serde_json::to_string_pretty(&self.snapshot()) else { return };
        if self.last_saved.as_deref() == Some(json.as_str()) {
            return;
        }
        match write(&self.name, &json) {
            Ok(()) => self.last_saved = Some(json),
            Err(e) => log::warn!("saving the session: {e}"),
        }
    }

    /// Rebuilds a saved session. Called before any client attaches.
    /// With `run_commands`, the commands that were running are run, not just typed.
    pub(super) fn restore(&mut self, saved: Saved, run_commands: bool) {
        self.frames = saved.frames;
        for t in saved.tabs {
            let mut leaves = Vec::new();
            let Some(root) = self.restore_node(&t.layout, &mut leaves) else { continue };
            let focus = leaves.get(t.focus).copied().unwrap_or(leaves[0]);
            self.next_tab_id += 1;
            self.tabs.push(Tab {
                id: self.next_tab_id,
                name: t.name,
                auto_name: t.auto_name,
                root,
                focus,
                zoomed: t.zoomed,
                sync: t.sync,
            });
        }
        if run_commands {
            for input in self.pending_input.values_mut() {
                input.text.push('\r');
            }
        }
        self.active = saved.active.min(self.tabs.len().saturating_sub(1));
        self.had_tabs = !self.tabs.is_empty();
        self.apply_layout();
        log::info!("restored {} tabs", self.tabs.len());
    }

    fn restore_node(&mut self, node: &SavedNode, leaves: &mut Vec<PaneId>) -> Option<Node> {
        match node {
            SavedNode::Pane(p) => {
                let id = self.restore_pane(p)?;
                leaves.push(id);
                Some(Node::Leaf(id))
            }
            SavedNode::Split { dir, ratio, a, b } => {
                let a = self.restore_node(a, leaves);
                let b = self.restore_node(b, leaves);
                match (a, b) {
                    (Some(a), Some(b)) => {
                        Some(Node::Split { dir: *dir, ratio: *ratio, a: Box::new(a), b: Box::new(b) })
                    }
                    (Some(x), None) | (None, Some(x)) => Some(x),
                    (None, None) => None,
                }
            }
        }
    }

    fn restore_pane(&mut self, p: &SavedPane) -> Option<PaneId> {
        let dir = usable_dir(p.cwd.as_deref());
        let id = match p.kind {
            PaneKind::Terminal => {
                let id = match self.spawn_term(SpawnOptions { cwd: dir, ..SpawnOptions::default() }) {
                    Ok(id) => id,
                    Err(e) => {
                        log::warn!("restoring a terminal: {e}");
                        return None;
                    }
                };
                if let Some(cmd) = p.command.as_ref().filter(|c| !c.is_empty()) {
                    let text = shell_join(cmd);
                    self.pending_input.insert(id, PendingInput { text, since: Instant::now(), output_seen: false });
                }
                id
            }
            PaneKind::Finder => {
                let dir = dir.unwrap_or_else(|| "/".into());
                let id = self.alloc_pane();
                let mut finder = Finder::new(id, dir, self.content_area(), self.frames, self.tx.clone());
                let editing = p.finder.as_ref().and_then(|state| {
                    finder.restore_state(state);
                    state.editing.clone().filter(|f| f.is_file())
                });
                self.panes.insert(id, Pane::Finder(Box::new(finder)));
                if let Some(file) = editing {
                    self.open_editor(id, file);
                }
                id
            }
        };
        if let Some(name) = &p.name {
            self.pane_names.insert(id, name.clone());
        }
        Some(id)
    }

    /// Types restored commands into shells that have drawn their prompt. Waiting for the prompt keeps the
    /// text from being echoed twice (once by the terminal driver, once by the shell's line editor).
    pub(super) fn flush_pending_input(&mut self) {
        let ready: Vec<PaneId> = self
            .pending_input
            .iter()
            .filter(|(_, p)| p.output_seen && p.since.elapsed().as_millis() >= 300)
            .map(|(id, _)| *id)
            .collect();
        for id in ready {
            if let Some(p) = self.pending_input.remove(&id)
                && let Some(term) = self.term(id)
            {
                term.write(p.text.into_bytes());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_quoted_for_the_shell() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(shell_join(&args(&["sleep", "100"])), "sleep 100");
        assert_eq!(shell_join(&args(&["grep", "-r", "two words", "./src"])), "grep -r 'two words' ./src");
        assert_eq!(shell_join(&args(&["echo", "it's", "$HOME", ""])), r"echo 'it'\''s' '$HOME' ''");
    }

    #[test]
    fn the_format_round_trips() {
        let saved = Saved {
            version: VERSION,
            frames: true,
            active: 0,
            tabs: vec![SavedTab {
                name: "work".into(),
                auto_name: false,
                zoomed: false,
                sync: false,
                focus: 1,
                layout: SavedNode::Split {
                    dir: SplitDir::Right,
                    ratio: 0.4,
                    a: Box::new(SavedNode::Pane(SavedPane {
                        kind: PaneKind::Terminal,
                        name: Some("build".into()),
                        cwd: Some("/tmp".into()),
                        command: Some(vec!["make".into()]),
                        finder: None,
                    })),
                    b: Box::new(SavedNode::Pane(SavedPane {
                        kind: PaneKind::Finder,
                        name: None,
                        cwd: Some("/".into()),
                        command: None,
                        finder: Some(FinderState { dir: "/".into(), ..FinderState::default() }),
                    })),
                },
            }],
        };
        let json = serde_json::to_string(&saved).unwrap();
        assert_eq!(serde_json::from_str::<Saved>(&json).unwrap(), saved);
    }
}
