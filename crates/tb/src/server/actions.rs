//! `tb action …` (rebuild step 6): zellij's actions, plus `tb`'s own, run against the session.
//!
//! Queries answer with JSON that uses zellij's field names (see
//! `docs/findings/2026-11-05-zellij-action-formats.md`); the CLI turns it into zellij's text columns.
//! `pane: None` means the focused pane, and `tab: None` the active tab.

use std::path::PathBuf;
use std::time::Instant;

use alacritty_terminal::grid::Scroll;
use alacritty_terminal::vte::ansi::{ClearMode, Handler};
use anyhow::{Context, bail};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use serde_json::{Value, json};

use super::pane::SpawnOptions;
use super::persist::{PendingInput, shell_join};
use super::{ClientId, Modal, Pane, Server, editor, input};
use crate::paths;
use crate::protocol::{CliAction, FocusTarget, PaneId, ScrollHow, SplitDir};

/// A pane id as zellij prints it: `terminal_7`, or `finder_7` for a finder (which zellij has no kind for).
pub fn pane_ref(kind: &str, id: PaneId) -> String {
    format!("{kind}_{id}")
}

/// Parses a colour as zellij's `set-pane-color` takes it: `#00e000` or `rgb:00/e0/00`.
fn parse_color(text: &str) -> anyhow::Result<Color> {
    let hex = |s: &str| u8::from_str_radix(s, 16).ok();
    let rgb = if let Some(h) = text.strip_prefix('#').filter(|h| h.len() == 6) {
        (hex(&h[0..2]), hex(&h[2..4]), hex(&h[4..6]))
    } else if let Some(rest) = text.strip_prefix("rgb:") {
        let parts: Vec<&str> = rest.split('/').collect();
        match parts.as_slice() {
            // Each part may have 1–4 hex digits; keep the top 8 bits.
            [r, g, b] if [r, g, b].iter().all(|p| (1..=4).contains(&p.len())) => {
                let top = |p: &str| {
                    u16::from_str_radix(p, 16).ok().map(|v| (u32::from(v) * 255 / ((1 << (4 * p.len())) - 1)) as u8)
                };
                (top(r), top(g), top(b))
            }
            _ => (None, None, None),
        }
    } else {
        (None, None, None)
    };
    match rgb {
        (Some(r), Some(g), Some(b)) => Ok(Color::Rgb(r, g, b)),
        _ => bail!("not a colour: '{text}' (use #rrggbb or rgb:rr/gg/bb)"),
    }
}

fn modal(name: &str) -> anyhow::Result<Modal> {
    Ok(match name.to_ascii_lowercase().as_str() {
        "normal" => Modal::Normal,
        "locked" => Modal::Locked,
        "pane" => Modal::Pane,
        "tab" => Modal::Tab,
        "resize" => Modal::Resize,
        "move" => Modal::Move,
        "scroll" => Modal::Scroll,
        "search" => Modal::Search,
        "session" => Modal::Session,
        "tmux" => Modal::Tmux,
        other => {
            bail!("unknown mode '{other}' (normal, locked, pane, tab, resize, move, scroll, search, session, tmux)")
        }
    })
}

impl Server {
    pub(super) fn cli_action(&mut self, a: CliAction, from: ClientId) -> anyhow::Result<String> {
        let done = |s: &mut Self| {
            s.dirty = true;
            String::new()
        };
        Ok(match a {
            // ----- queries -----
            CliAction::ListTabs => {
                let tabs: Vec<Value> = (0..self.tabs.len()).map(|i| self.tab_json(i)).collect();
                serde_json::to_string(&tabs)?
            }
            CliAction::CurrentTabInfo => {
                if self.tabs.is_empty() {
                    bail!("no tab");
                }
                serde_json::to_string(&self.tab_json(self.active))?
            }
            CliAction::ListPanes => serde_json::to_string(&self.pane_list())?,
            CliAction::ListClients => serde_json::to_string(&self.client_list())?,
            CliAction::DumpScreen { pane, full, ansi } => {
                let id = self.pane_or_focused(pane)?;
                let term = match self.panes.get(&id).context("no such pane")? {
                    Pane::Term(p) => Some(p),
                    Pane::Finder(f) => f.editor.and_then(|e| self.term(e)),
                };
                match (term, &self.panes[&id]) {
                    (Some(p), _) if ansi => p.ansi_text(full),
                    (Some(p), Pane::Term(_)) => p.text(full),
                    (_, Pane::Finder(f)) => f.text(term),
                    (None, Pane::Term(_)) => unreachable!(),
                }
            }
            CliAction::DumpLayout => serde_json::to_string_pretty(&self.snapshot())?,

            // ----- tabs -----
            CliAction::NewTab { name, cwd, command, start_suspended } => {
                let cwd = cwd.or_else(|| self.focused_cwd());
                let (opts, typed) = Self::spawn_opts(cwd, command, start_suspended);
                let id = self.new_tab(name, opts)?;
                self.type_later(id, typed);
                self.tabs[self.active].id.to_string()
            }
            CliAction::CloseTab { tab } => {
                let i = self.tab_index(tab)?;
                self.close_tab_at(i);
                done(self)
            }
            CliAction::GoToTab(n) => {
                if n == 0 || n > self.tabs.len() {
                    bail!("no tab {n} (there are {})", self.tabs.len());
                }
                self.go_to_tab(n - 1);
                done(self)
            }
            CliAction::GoToTabId(id) => {
                let i = self.tab_index(Some(id))?;
                self.go_to_tab(i);
                done(self)
            }
            CliAction::GoToTabName { name, create } => {
                let found = self.tabs.iter().position(|t| t.name == name || self.tab_label(t) == name);
                match found {
                    Some(i) => self.go_to_tab(i),
                    None if create => {
                        self.new_tab(Some(name), self.cwd_opts())?;
                    }
                    None => bail!("no tab named '{name}'"),
                }
                done(self)
            }
            CliAction::CycleTab { next } => {
                self.cycle_tab(next);
                done(self)
            }
            CliAction::RenameTab { tab, name } => {
                let i = self.tab_index(tab)?;
                self.tabs[i].name = name;
                self.tabs[i].auto_name = false;
                done(self)
            }
            CliAction::UndoRenameTab { tab } => {
                let i = self.tab_index(tab)?;
                let focus = self.tabs[i].focus;
                // Back to the default: finders are "finder", other tabs follow their program.
                match self.tab_name_for(focus) {
                    Some(name) => {
                        self.tabs[i].name = name;
                        self.tabs[i].auto_name = false;
                    }
                    None => {
                        self.tabs[i].name = "shell".into();
                        self.tabs[i].auto_name = true;
                    }
                }
                done(self)
            }
            CliAction::MoveTab { tab, right } => {
                let i = self.tab_index(tab)?;
                let keep = self.tabs.get(self.active).map(|t| t.id);
                let moving = self.tabs[i].id;
                self.active = i;
                self.move_tab(right);
                // The tab that was active stays active.
                if let Some(k) = keep.filter(|k| *k != moving) {
                    self.active = self.tabs.iter().position(|t| t.id == k).unwrap_or(self.active);
                }
                done(self)
            }
            CliAction::ToggleSync { tab } => {
                let i = self.tab_index(tab)?;
                self.tabs[i].sync = !self.tabs[i].sync;
                done(self)
            }

            // ----- panes -----
            CliAction::NewPane { dir, name, cwd, command, start_suspended } => {
                let cwd = cwd.or_else(|| self.focused_cwd());
                let (opts, typed) = Self::spawn_opts(cwd, command, start_suspended);
                let id = self.open_pane(dir, opts)?;
                self.type_later(id, typed);
                if let Some(name) = name {
                    self.pane_names.insert(id, name);
                }
                pane_ref("terminal", id)
            }
            CliAction::Edit { file, line, dir, cwd } => {
                let mut opts = editor::command(&file);
                if let Some(line) = line {
                    // `+N file` opens at line N in Neovim, Vim, vi, nano, Emacs and most others.
                    let at = opts.command.len().saturating_sub(1);
                    opts.command.insert(at, format!("+{line}"));
                }
                if cwd.is_some() {
                    opts.cwd = cwd;
                }
                let id = self.open_pane(dir, opts)?;
                pane_ref("terminal", id)
            }
            CliAction::ClosePane { pane } => {
                let id = self.pane_or_focused(pane)?;
                self.close_pane(id);
                done(self)
            }
            CliAction::Focus(target) => {
                if let FocusTarget::Pane(id) = target
                    && !self.tabs.iter().any(|t| t.root.contains(id))
                {
                    bail!("no pane {id}");
                }
                self.focus(target);
                done(self)
            }
            CliAction::FocusOrTab(dir) => {
                self.focus_or_tab(dir);
                done(self)
            }
            CliAction::MovePane { pane, dir } => {
                self.focus_target(pane)?;
                self.move_pane(dir.map_or(FocusTarget::Next, FocusTarget::Dir));
                done(self)
            }
            CliAction::MovePaneBackwards { pane } => {
                self.focus_target(pane)?;
                self.move_pane(FocusTarget::Prev);
                done(self)
            }
            CliAction::RenamePane { pane, name } => {
                let id = self.pane_or_focused(pane)?;
                self.pane_names.insert(id, name);
                done(self)
            }
            CliAction::UndoRenamePane { pane } => {
                let id = self.pane_or_focused(pane)?;
                self.pane_names.remove(&id);
                done(self)
            }
            CliAction::ToggleFullscreen { pane } => {
                self.focus_target(pane)?;
                self.toggle_zoom();
                done(self)
            }
            CliAction::ToggleFrames => {
                self.toggle_frames();
                done(self)
            }
            CliAction::Resize { pane, side, grow } => {
                self.focus_target(pane)?;
                self.resize_focused(side, grow);
                done(self)
            }
            CliAction::MoveBorder { pane, dir, amount } => {
                self.focus_target(pane)?;
                self.move_border(dir, amount);
                done(self)
            }
            CliAction::Clear { pane } => {
                let id = self.term_target(pane)?;
                if let Some(p) = self.term(id) {
                    let mut term = p.term.lock();
                    term.clear_screen(ClearMode::Saved);
                    term.clear_screen(ClearMode::All);
                }
                done(self)
            }
            CliAction::EditScrollback { pane } => {
                self.focus_target(pane)?;
                self.edit_scrollback();
                done(self)
            }
            CliAction::SetPaneColor { pane, fg, bg, reset } => {
                let id = self.pane_or_focused(pane)?;
                if reset {
                    self.pane_colors.remove(&id);
                } else {
                    let fg = fg.as_deref().map(parse_color).transpose()?;
                    let bg = bg.as_deref().map(parse_color).transpose()?;
                    let entry = self.pane_colors.entry(id).or_default();
                    entry.0 = fg.or(entry.0);
                    entry.1 = bg.or(entry.1);
                }
                done(self)
            }
            CliAction::Scroll { pane, how } => {
                let id = self.term_target(pane)?;
                if let Some(p) = self.term(id) {
                    let half = i32::from(p.rows / 2).max(1);
                    let scroll = match how {
                        ScrollHow::Up => Scroll::Delta(1),
                        ScrollHow::Down => Scroll::Delta(-1),
                        ScrollHow::PageUp => Scroll::PageUp,
                        ScrollHow::PageDown => Scroll::PageDown,
                        ScrollHow::HalfPageUp => Scroll::Delta(half),
                        ScrollHow::HalfPageDown => Scroll::Delta(-half),
                        ScrollHow::Top => Scroll::Top,
                        ScrollHow::Bottom => Scroll::Bottom,
                    };
                    p.term.lock().scroll_display(scroll);
                }
                done(self)
            }

            // ----- input -----
            CliAction::Write { pane, bytes } => {
                let id = self.pane_or_focused(pane)?;
                self.write_to(id, bytes);
                done(self)
            }
            CliAction::Paste { pane, text } => {
                let id = self.term_target(pane)?;
                if let Some(p) = self.term(id) {
                    let mode = *p.term.lock().mode();
                    p.write(input::encode_paste(&text, mode));
                }
                done(self)
            }
            CliAction::SendKeys { pane, keys } => {
                match pane {
                    None => keys.iter().for_each(|k| self.send_key(k)),
                    Some(id) => {
                        if !self.panes.contains_key(&id) {
                            bail!("no pane {id}");
                        }
                        for k in &keys {
                            self.key_to(id, k);
                        }
                    }
                }
                done(self)
            }

            // ----- session -----
            CliAction::SwitchMode(name) => {
                self.set_modal(modal(&name)?);
                done(self)
            }
            CliAction::RenameSession(name) => {
                self.rename_session(&name)?;
                done(self)
            }
            CliAction::SaveSession => {
                self.last_saved = None;
                self.autosave();
                String::new()
            }
            CliAction::ReloadConfig => self.reload_config().map_err(anyhow::Error::msg)?,
            CliAction::SwitchSession(name) => {
                if !paths::valid_session_name(&name) {
                    bail!("session names may use letters, digits, '-', '_' and '.' (got '{name}')");
                }
                if name == self.name {
                    bail!("already in session '{name}'");
                }
                let id = self
                    .active_client
                    .filter(|c| self.clients.get(c).is_some_and(|c| c.attached))
                    .context("no client is attached to switch")?;
                self.send(id, crate::protocol::ServerMsg::Switch { session: name });
                self.remove_client(id);
                String::new()
            }
            CliAction::SetStatus { id, text, ttl } => {
                // One line of plain text: control characters would garble the bar.
                let text: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
                let until = ttl.map(|s| Instant::now() + std::time::Duration::from_secs(s));
                match self.status.iter_mut().find(|s| s.id == id) {
                    Some(seg) => {
                        seg.text = text;
                        seg.until = until;
                    }
                    None => self.status.push(super::StatusSegment { id, text, until }),
                }
                done(self)
            }
            CliAction::ClearStatus { id } => {
                match id {
                    Some(id) => self.status.retain(|s| s.id != id),
                    None => self.status.clear(),
                }
                done(self)
            }
            CliAction::Detach => {
                let ids: Vec<ClientId> =
                    self.clients.iter().filter(|(i, c)| c.attached && **i != from).map(|(i, _)| *i).collect();
                for id in ids {
                    self.detach(id);
                }
                String::new()
            }
        })
    }

    // ----- helpers -----

    /// Reads config.kdl. Returns its warnings, one per line; a file that can't be read or parsed is an error,
    /// and the bindings stay as they were.
    pub(super) fn reload_config(&mut self) -> Result<String, String> {
        let path = super::config::path();
        let loaded = super::config::load(&path).inspect_err(|e| log::warn!("{e}"))?;
        for w in &loaded.warnings {
            log::warn!("{}: {w}", path.display());
        }
        log::info!("{} key bindings from {}", loaded.bindings.len(), path.display());
        self.bindings = loaded.bindings;
        self.config_note = (!loaded.warnings.is_empty()).then(|| {
            format!("config.kdl: {} lines skipped; tb action reload-config lists them", loaded.warnings.len())
        });
        Ok(loaded.warnings.join("\n"))
    }

    /// The index of tab `id`, or of the active tab.
    fn tab_index(&self, tab: Option<u32>) -> anyhow::Result<usize> {
        match tab {
            None if self.tabs.is_empty() => bail!("no tab"),
            None => Ok(self.active),
            Some(id) => self.tabs.iter().position(|t| t.id == id).with_context(|| format!("no tab with id {id}")),
        }
    }

    /// Pane `pane` if it exists, else the focused pane.
    fn pane_or_focused(&self, pane: Option<PaneId>) -> anyhow::Result<PaneId> {
        match pane {
            Some(id) if self.panes.contains_key(&id) => Ok(id),
            Some(id) => bail!("no pane {id}"),
            None => self.focused_pane().context("no pane"),
        }
    }

    /// The terminal behind a pane: the pane itself, or a finder's editor.
    fn term_target(&self, pane: Option<PaneId>) -> anyhow::Result<PaneId> {
        let id = self.pane_or_focused(pane)?;
        match &self.panes[&id] {
            Pane::Term(_) => Ok(id),
            Pane::Finder(f) => f.editor.with_context(|| format!("pane {id} is a finder, not a terminal")),
        }
    }

    /// Focuses pane `pane` (if given), for actions that work on the focused pane and change what's shown.
    fn focus_target(&mut self, pane: Option<PaneId>) -> anyhow::Result<()> {
        if let Some(id) = pane {
            if !self.tabs.iter().any(|t| t.root.contains(id)) {
                bail!("no pane {id}");
            }
            self.focus_pane(id);
        }
        Ok(())
    }

    /// Spawn options for a pane or tab running `command`. With `start_suspended` the pane runs a shell
    /// and the command is typed at its prompt without Enter, returned as the text to type.
    fn spawn_opts(cwd: Option<PathBuf>, command: Vec<String>, start_suspended: bool) -> (SpawnOptions, Option<String>) {
        if start_suspended && !command.is_empty() {
            let typed = shell_join(&command);
            (SpawnOptions { cwd, ..SpawnOptions::default() }, Some(typed))
        } else {
            (SpawnOptions { cwd, command, ..SpawnOptions::default() }, None)
        }
    }

    fn type_later(&mut self, id: PaneId, text: Option<String>) {
        if let Some(text) = text {
            self.pending_input.insert(id, PendingInput { text, since: Instant::now(), output_seen: false });
        }
    }

    /// Opens a pane in the active tab: beside the focused pane in `dir`, or where there's most room.
    fn open_pane(&mut self, dir: Option<SplitDir>, opts: SpawnOptions) -> anyhow::Result<PaneId> {
        if self.tabs.is_empty() {
            return Ok(self.new_tab(None, opts)?);
        }
        if self.tabs[self.active].zoomed {
            self.tabs[self.active].zoomed = false;
            self.apply_layout();
        }
        let id = match dir {
            Some(dir) => self.split(dir, opts)?,
            None => {
                let (target, dir) = self.room_in_tab(self.active).context("not enough room for a new pane")?;
                self.split_at(target, dir, opts)?
            }
        };
        Ok(id)
    }

    /// Bytes for a pane: a terminal gets them as typed; a finder gets them as keys (or its editor gets them).
    pub(super) fn write_to(&mut self, id: PaneId, bytes: Vec<u8>) {
        match self.panes.get(&id) {
            Some(Pane::Term(p)) => p.write(bytes),
            Some(Pane::Finder(f)) => {
                if let Some(editor) = f.editor.and_then(|e| self.term(e)) {
                    editor.write(bytes);
                    return;
                }
                // Text for a finder is typed as keys, one character at a time.
                for c in String::from_utf8_lossy(&bytes).chars() {
                    let code = match c {
                        '\r' | '\n' => KeyCode::Enter,
                        '\x1b' => KeyCode::Esc,
                        '\t' => KeyCode::Tab,
                        '\x7f' => KeyCode::Backspace,
                        c => KeyCode::Char(c),
                    };
                    self.key_to(id, &KeyEvent::new(code, KeyModifiers::NONE));
                }
            }
            None => {}
        }
    }

    /// A key for one pane, whether or not it has focus.
    fn key_to(&mut self, id: PaneId, key: &KeyEvent) {
        let action = match self.panes.get_mut(&id) {
            Some(Pane::Term(_)) => {
                self.type_into(id, key);
                return;
            }
            Some(Pane::Finder(f)) => match f.editor {
                Some(editor) => {
                    self.type_into(editor, key);
                    return;
                }
                None => f.handle_key(key),
            },
            None => return,
        };
        self.finder_action(id, action);
    }

    /// Renames the session: its socket, saved layout and log follow the new name.
    fn rename_session(&mut self, name: &str) -> anyhow::Result<()> {
        if !paths::valid_session_name(name) {
            bail!("session names may use letters, digits, '-', '_' and '.' (got '{name}')");
        }
        if name == self.name {
            return Ok(());
        }
        let (old_sock, new_sock) = (paths::socket_path(&self.name), paths::socket_path(name));
        if new_sock.exists() || paths::saved_path(name).exists() {
            bail!("a session named '{name}' already exists");
        }
        // A bound socket keeps working when its file is renamed.
        std::fs::rename(&old_sock, &new_sock).context("renaming the session's socket")?;
        let _ = std::fs::rename(paths::saved_path(&self.name), paths::saved_path(name));
        let _ = std::fs::rename(paths::log_path(&self.name), paths::log_path(name));
        log::info!("session {} renamed to {name}", self.name);
        self.name = name.to_owned();
        self.last_saved = None;
        Ok(())
    }

    /// One tab as zellij's `list-tabs -j` shows it, plus `tb`'s own fields.
    fn tab_json(&self, i: usize) -> Value {
        let tab = &self.tabs[i];
        let area = self.content_area();
        json!({
            "position": i,
            "name": self.tab_label(tab),
            "active": i == self.active,
            "panes_to_hide": 0,
            "is_fullscreen_active": tab.zoomed,
            "is_sync_panes_active": tab.sync,
            "are_floating_panes_visible": false,
            "other_focused_clients": [],
            "active_swap_layout_name": null,
            "is_swap_layout_dirty": false,
            "viewport_rows": area.height,
            "viewport_columns": area.width,
            "display_area_rows": self.size.1,
            "display_area_columns": self.size.0,
            "selectable_tiled_panes_count": tab.root.panes().len(),
            "selectable_floating_panes_count": 0,
            "tab_id": tab.id,
            "has_bell_notification": false,
            "is_flashing_bell": false,
            // tb: true while the name follows the running program (no name was given).
            "auto_name": tab.auto_name,
        })
    }

    /// Every pane, as zellij's `list-panes -j` shows it, plus `tb`'s own fields (`kind`, `tab`, …).
    pub(super) fn pane_list(&self) -> Vec<Value> {
        let mut out = Vec::new();
        for (ti, tab) in self.tabs.iter().enumerate() {
            let layout = self.tab_layout(tab);
            for id in tab.root.panes() {
                let frame = layout.rect_of(id).unwrap_or_default();
                let body = self.body(frame);
                let focused = ti == self.active && tab.focus == id;
                let colors = self.pane_colors.get(&id).copied().unwrap_or_default();
                let hex = |c: Option<Color>| match c {
                    Some(Color::Rgb(r, g, b)) => Some(format!("#{r:02x}{g:02x}{b:02x}")),
                    _ => None,
                };
                let (kind, title, cwd, editor, terminal_command, pane_command, rect, cursor) = match self.panes.get(&id)
                {
                    Some(Pane::Term(p)) => {
                        let spawned = (!p.command.is_empty()).then(|| p.command.join(" "));
                        let running = p.running_command().map(|c| c.join(" ")).or_else(|| p.program());
                        let cursor = {
                            let term = p.term.lock();
                            let c = term.grid().cursor.point;
                            [c.column.0, c.line.0.max(0) as usize]
                        };
                        ("terminal", p.display_title(), p.cwd(), None, spawned, running, body, Some(cursor))
                    }
                    Some(Pane::Finder(f)) => {
                        ("finder", "finder".to_owned(), Some(f.dir.clone()), f.editor, None, None, frame, None)
                    }
                    None => continue,
                };
                out.push(json!({
                    // zellij's fields
                    "id": id,
                    "is_plugin": false,
                    "is_focused": focused,
                    "is_fullscreen": tab.zoomed && tab.focus == id,
                    "is_floating": false,
                    "is_suppressed": false,
                    "title": title,
                    "exited": false,
                    "exit_status": null,
                    "is_held": false,
                    "pane_x": frame.x,
                    "pane_content_x": body.x,
                    "pane_y": frame.y,
                    "pane_content_y": body.y,
                    "pane_rows": frame.height,
                    "pane_content_rows": body.height,
                    "pane_columns": frame.width,
                    "pane_content_columns": body.width,
                    "cursor_coordinates_in_pane": cursor,
                    "terminal_command": terminal_command,
                    "plugin_url": null,
                    "is_selectable": true,
                    "index_in_pane_group": {},
                    "default_fg": hex(colors.0),
                    "default_bg": hex(colors.1),
                    "tab_id": tab.id,
                    "tab_position": ti,
                    "tab_name": self.tab_label(tab),
                    "pane_command": pane_command,
                    "pane_cwd": cwd,
                    // tb's fields
                    "kind": kind,
                    "tab": ti + 1,
                    "name": self.pane_names.get(&id),
                    "focused": focused,
                    "visible": ti == self.active && (!tab.zoomed || tab.focus == id),
                    "x": rect.x, "y": rect.y, "cols": rect.width, "rows": rect.height,
                    "cwd": cwd,
                    "editor_pane": editor,
                }));
            }
        }
        out
    }

    /// Attached clients: zellij's `list-clients` columns, plus each client's size and image method.
    fn client_list(&self) -> Vec<Value> {
        let pane = self.focused_pane();
        let running = pane.and_then(|id| match self.panes.get(&id)? {
            Pane::Term(p) => p.running_command().map(|c| c.join(" ")).or_else(|| p.program()),
            Pane::Finder(_) => Some("finder".to_owned()),
        });
        let mut ids: Vec<&ClientId> = self.clients.iter().filter(|(_, c)| c.attached).map(|(id, _)| id).collect();
        ids.sort();
        ids.into_iter()
            .map(|id| {
                let c = &self.clients[id];
                json!({
                    "client_id": id,
                    "pane_id": pane,
                    "running_command": running,
                    "active": self.active_client == Some(*id),
                    "cols": c.cols,
                    "rows": c.rows,
                    "images": format!("{:?}", c.graphics.protocol).to_lowercase(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse_like_zellij() {
        assert_eq!(parse_color("#00e000").unwrap(), Color::Rgb(0, 0xe0, 0));
        assert_eq!(parse_color("rgb:00/1a/3a").unwrap(), Color::Rgb(0, 0x1a, 0x3a));
        assert_eq!(parse_color("rgb:ffff/0/8").unwrap(), Color::Rgb(255, 0, 136));
        assert!(parse_color("red").is_err());
        assert!(parse_color("#12345").is_err());
    }

    #[test]
    fn modes_have_zellij_names() {
        assert_eq!(modal("Locked").unwrap(), Modal::Locked);
        assert_eq!(modal("normal").unwrap(), Modal::Normal);
        assert!(modal("floating").is_err());
    }
}
