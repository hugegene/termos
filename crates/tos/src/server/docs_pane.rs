//! The docs pane: the Markdown files that agents in a tab write, shown beside them.
//!
//! `tos hook` (from an agent started by `tos claude` and the like) sends `add-doc` for each `.md` file the agent
//! writes in its folder. The server appends the file to the tab's list, a file of JSON lines, and opens the tab's
//! docs pane on the right if it isn't open: an ordinary terminal pane running `tos docs --list FILE`, which
//! follows the list and draws the files as an accordion (`crate::docsview`).
//! Plan: docs/plans/2026-11-17-agent-plan-pane.md.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;

use super::layout::Node;
use super::pane::SpawnOptions;
use super::{Pane, Server, StatusSegment};
use crate::paths;
use crate::protocol::{PaneId, SplitDir};

/// The docs pane's share of the tab's width (an estimate, to tune).
const DOCS_SHARE: f32 = 0.4;
/// List files untouched for this long are removed when a new one is made.
const KEEP_LISTS: Duration = Duration::from_secs(30 * 24 * 3600);
/// The status-bar segment saying that a closed docs pane has new files.
pub const STATUS_ID: &str = "docs";

/// A tab's docs, kept while its docs pane is closed.
#[derive(Default, Debug)]
pub struct TabDocs {
    /// The tab's list file, once it has one.
    pub list: Option<PathBuf>,
    /// The user closed the docs pane: new files go into the list without opening it again.
    pub hidden: bool,
    /// Files added while hidden, for the status bar.
    pub new: usize,
    /// When the last of those came, so a click on the status bar opens the right tab's.
    pub new_at: Option<Instant>,
}

/// The list file named in a docs viewer's command line (`tos docs --list FILE`), if `cmd` is one.
pub fn viewer_list(cmd: &[String]) -> Option<PathBuf> {
    match cmd {
        [exe, docs, flag, list, ..]
            if docs == "docs" && flag == "--list" && Path::new(exe).file_name().is_some_and(|n| n == "tos") =>
        {
            Some(PathBuf::from(list))
        }
        _ => None,
    }
}

/// The command line of a docs viewer on `list`, run by this `tos`.
pub fn viewer_command(list: &Path) -> Vec<String> {
    let exe = std::env::current_exe().map_or_else(|_| "tos".to_owned(), |p| p.to_string_lossy().into_owned());
    vec![exe, "docs".into(), "--list".into(), list.to_string_lossy().into_owned()]
}

/// A new, empty list file in the state folder. Old ones are removed first.
fn new_list() -> io::Result<PathBuf> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = paths::state_dir().join("docs");
    paths::ensure_private_dir(&dir)?;
    prune(&dir, SystemTime::now());
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    let path = dir.join(format!("{stamp}-{}-{}.jsonl", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    OpenOptions::new().create(true).append(true).open(&path)?;
    Ok(path)
}

/// Removes list files in `dir` untouched since `KEEP_LISTS` before `now`.
fn prune(dir: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| now.duration_since(t).unwrap_or_default() > KEEP_LISTS);
        if old && path.extension().is_some_and(|x| x == "jsonl") {
            let _ = fs::remove_file(path);
        }
    }
}

/// Adds a write of `path` to the list: one JSON line. The viewer reads what's new.
pub fn append(list: &Path, path: &Path, cwd: Option<&Path>) -> io::Result<()> {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64();
    let line = serde_json::json!({ "path": path, "cwd": cwd, "t": t });
    let mut f = OpenOptions::new().create(true).append(true).open(list)?;
    // One write per line, so the viewer never reads half of one.
    f.write_all(format!("{line}\n").as_bytes())
}

impl Server {
    /// Tab `ti`'s docs pane and its list, if the pane is open.
    pub(super) fn docs_pane(&self, ti: usize) -> Option<(PaneId, PathBuf)> {
        self.tabs[ti].root.panes().into_iter().find_map(|id| match self.panes.get(&id) {
            Some(Pane::Term(p)) => viewer_list(&p.command).map(|list| (id, list)),
            _ => None,
        })
    }

    /// Whether pane `id` is a docs pane.
    pub(super) fn is_docs_pane(&self, id: PaneId) -> bool {
        matches!(self.panes.get(&id), Some(Pane::Term(p)) if viewer_list(&p.command).is_some())
    }

    /// Shows `path`, which the agent in pane `pane` wrote, in the docs pane of that pane's tab.
    pub(super) fn add_doc(&mut self, pane: PaneId, path: &Path, cwd: Option<&Path>) -> anyhow::Result<()> {
        let ti = self.tabs.iter().position(|t| t.root.contains(pane)).context("that pane is in no tab")?;
        let open = self.docs_pane(ti);
        let list = match open.as_ref().map(|(_, list)| list.clone()).or_else(|| self.tabs[ti].docs.list.clone()) {
            Some(list) => list,
            None => new_list().context("making the docs list")?,
        };
        append(&list, path, cwd).with_context(|| format!("writing {}", list.display()))?;
        self.tabs[ti].docs.list = Some(list.clone());
        if open.is_some() {
            return Ok(());
        }
        if self.tabs[ti].docs.hidden {
            let docs = &mut self.tabs[ti].docs;
            docs.new += 1;
            docs.new_at = Some(Instant::now());
            let text = format!("▤ docs: {} new", docs.new);
            self.set_docs_status(text);
        } else {
            self.open_docs(ti, &list, cwd.map(Path::to_path_buf))?;
        }
        Ok(())
    }

    /// Opens tab `ti`'s docs pane on `list`, along the tab's right edge. The focus stays where it is.
    fn open_docs(&mut self, ti: usize, list: &Path, cwd: Option<PathBuf>) -> io::Result<PaneId> {
        let id = self.spawn_term(SpawnOptions { cwd, command: viewer_command(list), ..SpawnOptions::default() })?;
        let tab = &mut self.tabs[ti];
        let old = std::mem::replace(&mut tab.root, Node::Leaf(id));
        tab.root = Node::Split {
            dir: SplitDir::Right,
            ratio: 1.0 - DOCS_SHARE,
            a: Box::new(old),
            b: Box::new(Node::Leaf(id)),
        };
        tab.docs.hidden = false;
        tab.docs.new = 0;
        tab.docs.new_at = None;
        if !self.tabs.iter().any(|t| t.docs.hidden && t.docs.new > 0) {
            self.status.retain(|s| s.id != STATUS_ID);
        }
        self.dirty = true;
        self.apply_layout();
        Ok(id)
    }

    /// Shows tab `ti`'s docs pane, or closes it if it's showing.
    pub(super) fn toggle_docs(&mut self, ti: usize) -> anyhow::Result<()> {
        if let Some((id, _)) = self.docs_pane(ti) {
            self.close_pane(id);
            return Ok(());
        }
        let list = match self.tabs[ti].docs.list.clone() {
            Some(list) => list,
            None => new_list().context("making the docs list")?,
        };
        self.tabs[ti].docs.list = Some(list.clone());
        self.open_docs(ti, &list, None)?;
        Ok(())
    }

    /// Pane `id` is closing. If it's a docs pane, its tab keeps the list and stops opening the pane by itself.
    pub(super) fn docs_closing(&mut self, id: PaneId) {
        let Some(Pane::Term(p)) = self.panes.get(&id) else { return };
        let Some(list) = viewer_list(&p.command) else { return };
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.root.contains(id)) {
            tab.docs = TabDocs { list: Some(list), hidden: true, new: 0, new_at: None };
        }
    }

    /// The status bar's docs segment was clicked: open the docs pane of the tab with the newest files.
    pub(super) fn docs_status_clicked(&mut self) {
        self.status.retain(|s| s.id != STATUS_ID);
        let newest = (0..self.tabs.len())
            .filter(|&i| self.tabs[i].docs.hidden && self.tabs[i].docs.new > 0)
            .max_by_key(|&i| self.tabs[i].docs.new_at);
        if let Some(ti) = newest {
            self.go_to_tab(ti);
            if let Err(e) = self.toggle_docs(ti) {
                self.flash(format!("{e:#}"));
            }
        }
    }

    fn set_docs_status(&mut self, text: String) {
        match self.status.iter_mut().find(|s| s.id == STATUS_ID) {
            Some(seg) => seg.text = text,
            None => self.status.push(StatusSegment { id: STATUS_ID.into(), text, until: None }),
        }
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn viewer_commands_are_recognised() {
        // Run by the test binary here, not by `tos`, so only the arguments are checked.
        let cmd = viewer_command(Path::new("/s/docs/1.jsonl"));
        assert_eq!(cmd[1..], strings(&["docs", "--list", "/s/docs/1.jsonl"]));
        assert_eq!(viewer_list(&strings(&["/x/tos", "docs", "--list", "/l"])).as_deref(), Some(Path::new("/l")));
        assert_eq!(viewer_list(&strings(&["/x/vim", "docs", "--list", "/l"])), None);
        assert_eq!(viewer_list(&strings(&["/x/tos", "docs"])), None);
        assert_eq!(viewer_list(&strings(&["htop"])), None);
    }

    #[test]
    fn lines_are_appended_and_old_lists_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let list = dir.path().join("a.jsonl");
        append(&list, Path::new("/w/x.md"), Some(Path::new("/w"))).unwrap();
        append(&list, Path::new("/w/y.md"), None).unwrap();
        let text = fs::read_to_string(&list).unwrap();
        let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["path"], "/w/x.md");
        assert_eq!(lines[0]["cwd"], "/w");
        assert!(lines[1]["cwd"].is_null());
        // Not old yet; then, 31 days on, it is.
        prune(dir.path(), SystemTime::now());
        assert!(list.exists());
        prune(dir.path(), SystemTime::now() + Duration::from_secs(31 * 24 * 3600));
        assert!(!list.exists());
    }
}
