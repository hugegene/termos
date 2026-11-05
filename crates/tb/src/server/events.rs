//! `tb subscribe` (rebuild step 6): events for extensions, one JSON object per line.
//!
//! The server keeps a small summary of the session (tabs, panes, mode, clients). While anyone is
//! subscribed, it compares the summary with the last one it sent and turns each difference into an event,
//! so every change is reported whichever code path made it. Panes a subscriber asked for (`-p`) also send
//! their text when it changes.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use super::{ClientId, Mode, Pane, Server};
use crate::protocol::{PaneId, ServerMsg};

/// What one subscriber asked for.
pub struct Subscription {
    /// Panes whose text to send when it changes.
    pub panes: Vec<PaneId>,
    pub ansi: bool,
    /// The last text sent for each pane, so unchanged screens aren't sent again.
    pub last: HashMap<PaneId, String>,
}

#[derive(Clone, PartialEq, Default)]
pub struct Summary {
    tabs: Vec<TabSummary>,
    panes: Vec<PaneSummary>,
    mode: String,
    clients: Vec<ClientId>,
}

#[derive(Clone, PartialEq)]
struct TabSummary {
    id: u32,
    name: String,
    position: usize,
    active: bool,
}

#[derive(Clone, PartialEq)]
struct PaneSummary {
    id: PaneId,
    tab_id: u32,
    kind: &'static str,
    title: String,
    name: Option<String>,
    focused: bool,
}

/// The events that turn `old` into `new`.
fn diff(old: &Summary, new: &Summary, exits: &HashMap<PaneId, i32>) -> Vec<Value> {
    let mut out = Vec::new();
    let old_tab = |id: u32| old.tabs.iter().find(|t| t.id == id);
    let new_tab = |id: u32| new.tabs.iter().find(|t| t.id == id);
    for t in &new.tabs {
        match old_tab(t.id) {
            None => out.push(json!({"event": "tab_opened", "tab_id": t.id, "name": t.name, "position": t.position})),
            Some(o) => {
                if o.name != t.name {
                    out.push(json!({"event": "tab_renamed", "tab_id": t.id, "name": t.name}));
                }
                if o.position != t.position {
                    out.push(json!({"event": "tab_moved", "tab_id": t.id, "position": t.position}));
                }
            }
        }
    }
    for t in old.tabs.iter().filter(|t| new_tab(t.id).is_none()) {
        out.push(json!({"event": "tab_closed", "tab_id": t.id}));
    }
    let old_pane = |id: PaneId| old.panes.iter().find(|p| p.id == id);
    for p in &new.panes {
        match old_pane(p.id) {
            None => out.push(json!({"event": "pane_opened", "pane_id": p.id, "tab_id": p.tab_id, "kind": p.kind,
                "title": p.title})),
            Some(o) => {
                if o.title != p.title {
                    out.push(json!({"event": "pane_retitled", "pane_id": p.id, "title": p.title}));
                }
                if o.name != p.name {
                    out.push(json!({"event": "pane_renamed", "pane_id": p.id, "name": p.name}));
                }
                if o.tab_id != p.tab_id {
                    out.push(json!({"event": "pane_moved", "pane_id": p.id, "tab_id": p.tab_id}));
                }
            }
        }
    }
    for p in old.panes.iter().filter(|p| !new.panes.iter().any(|n| n.id == p.id)) {
        out.push(json!({"event": "pane_closed", "pane_id": p.id, "tab_id": p.tab_id, "exit_status": exits.get(&p.id)}));
    }
    // Focus: the active tab, then the focused pane.
    let active = |s: &Summary| s.tabs.iter().find(|t| t.active).map(|t| t.id);
    if let Some(id) = active(new).filter(|id| active(old) != Some(*id)) {
        out.push(json!({"event": "tab_focused", "tab_id": id}));
    }
    let focused = |s: &Summary| s.panes.iter().find(|p| p.focused).map(|p| p.id);
    if let Some(id) = focused(new).filter(|id| focused(old) != Some(*id)) {
        out.push(json!({"event": "pane_focused", "pane_id": id}));
    }
    if old.mode != new.mode {
        out.push(json!({"event": "mode_changed", "mode": new.mode}));
    }
    for c in new.clients.iter().filter(|c| !old.clients.contains(c)) {
        out.push(json!({"event": "client_attached", "client_id": c}));
    }
    for c in old.clients.iter().filter(|c| !new.clients.contains(c)) {
        out.push(json!({"event": "client_detached", "client_id": c}));
    }
    out
}

impl Server {
    fn has_subscribers(&self) -> bool {
        self.clients.values().any(|c| c.subscription.is_some())
    }

    fn mode_name(&self) -> String {
        match &self.mode {
            Mode::Modal(m) => format!("{m:?}").to_lowercase(),
            Mode::EnterSearch(_) => "entersearch".into(),
            Mode::RenameTab { .. } => "renametab".into(),
            Mode::RenamePane { .. } => "renamepane".into(),
            Mode::Select(_) => "select".into(),
            Mode::ConfirmQuit => "confirmquit".into(),
            Mode::Help => "help".into(),
        }
    }

    pub(super) fn summary(&self) -> Summary {
        let mut tabs = Vec::new();
        let mut panes = Vec::new();
        for (i, tab) in self.tabs.iter().enumerate() {
            tabs.push(TabSummary { id: tab.id, name: self.tab_label(tab), position: i, active: i == self.active });
            for id in tab.root.panes() {
                let kind = match self.panes.get(&id) {
                    Some(Pane::Term(_)) => "terminal",
                    Some(Pane::Finder(_)) => "finder",
                    None => continue,
                };
                panes.push(PaneSummary {
                    id,
                    tab_id: tab.id,
                    kind,
                    title: self.pane_title(id),
                    name: self.pane_names.get(&id).cloned(),
                    focused: i == self.active && tab.focus == id,
                });
            }
        }
        let mut clients: Vec<ClientId> = self.clients.iter().filter(|(_, c)| c.attached).map(|(id, _)| *id).collect();
        clients.sort();
        Summary { tabs, panes, mode: self.mode_name(), clients }
    }

    /// A pane's screen as `subscribe` sends it; `scrollback` adds that many lines of history (`None`: all).
    fn pane_text(&self, id: PaneId, ansi: bool, scrollback: Option<Option<usize>>) -> Option<Value> {
        let term = match self.panes.get(&id)? {
            Pane::Term(p) => p,
            Pane::Finder(f) => match f.editor.and_then(|e| self.term(e)) {
                Some(p) => p,
                None => {
                    return Some(
                        json!({"event": "pane_text", "pane_id": id, "lines": f.text(None).lines().collect::<Vec<_>>()}),
                    );
                }
            },
        };
        let full = scrollback.is_some();
        let text = if ansi { term.ansi_text(full) } else { term.text(full) };
        let mut lines: Vec<&str> = text.lines().collect();
        if let Some(Some(n)) = scrollback {
            // The screen, plus the last `n` lines of history above it.
            let keep = usize::from(term.rows) + n;
            if lines.len() > keep {
                lines.drain(..lines.len() - keep);
            }
        }
        Some(json!({"event": "pane_text", "pane_id": id, "lines": lines}))
    }

    /// Starts sending events to client `id`. The first delivery is the subscribed panes' text.
    pub(super) fn subscribe(
        &mut self,
        id: ClientId,
        panes: Vec<PaneId>,
        scrollback: Option<Option<usize>>,
        ansi: bool,
    ) {
        if let Some(missing) = panes.iter().find(|p| !self.panes.contains_key(p)) {
            self.send(id, ServerMsg::Exit { reason: format!("no pane {missing}") });
            return;
        }
        // Events are differences from now on, not a replay of everything that exists.
        if !self.has_subscribers() {
            self.event_summary = Some(self.summary());
        }
        let mut last = HashMap::new();
        for p in &panes {
            if let Some(v) = self.pane_text(*p, ansi, scrollback) {
                last.insert(*p, v["lines"].to_string());
                self.send(id, ServerMsg::Event(v.to_string()));
            }
        }
        if let Some(c) = self.clients.get_mut(&id) {
            c.subscription = Some(Subscription { panes, ansi, last });
        }
    }

    /// Sends what changed to every subscriber. Called often; does nothing when nobody is subscribed.
    pub(super) fn emit_events(&mut self) {
        if !self.has_subscribers() {
            self.event_summary = None;
            self.changed_panes.clear();
            self.exit_codes.clear();
            return;
        }
        let now = self.summary();
        let events = match &self.event_summary {
            Some(old) if *old != now => diff(old, &now, &self.exit_codes),
            Some(_) => Vec::new(),
            None => Vec::new(),
        };
        self.event_summary = Some(now);
        self.exit_codes.clear();
        let changed: HashSet<PaneId> = std::mem::take(&mut self.changed_panes);
        let subscribers: Vec<ClientId> =
            self.clients.iter().filter(|(_, c)| c.subscription.is_some()).map(|(id, _)| *id).collect();
        for id in subscribers {
            for e in &events {
                self.send(id, ServerMsg::Event(e.to_string()));
            }
            let Some(sub) = self.clients.get(&id).and_then(|c| c.subscription.as_ref()) else { continue };
            let (wanted, ansi) = (sub.panes.clone(), sub.ansi);
            let mut updates = Vec::new();
            for p in wanted.iter().filter(|p| changed.contains(p)) {
                if let Some(v) = self.pane_text(*p, ansi, None) {
                    let key = v["lines"].to_string();
                    if sub.last.get(p) != Some(&key) {
                        updates.push((*p, key, v));
                    }
                }
            }
            for (p, key, v) in updates {
                self.send(id, ServerMsg::Event(v.to_string()));
                if let Some(s) = self.clients.get_mut(&id).and_then(|c| c.subscription.as_mut()) {
                    s.last.insert(p, key);
                }
            }
            // A subscriber to particular panes is done when they've all closed.
            if !wanted.is_empty() && wanted.iter().all(|p| !self.panes.contains_key(p)) {
                self.send(id, ServerMsg::Exit { reason: "the panes closed".into() });
            }
        }
    }

    /// Sends one event that isn't a change of state (for example a click on a status segment).
    pub(super) fn send_event(&self, event: Value) {
        for (id, c) in &self.clients {
            if c.subscription.is_some() {
                self.send(*id, ServerMsg::Event(event.to_string()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: u32, name: &str, position: usize, active: bool) -> TabSummary {
        TabSummary { id, name: name.into(), position, active }
    }

    fn pane(id: PaneId, tab_id: u32, title: &str, focused: bool) -> PaneSummary {
        PaneSummary { id, tab_id, kind: "terminal", title: title.into(), name: None, focused }
    }

    #[test]
    fn differences_become_events() {
        let old = Summary {
            tabs: vec![tab(1, "shell", 0, true), tab(2, "two", 1, false)],
            panes: vec![pane(1, 1, "bash", true), pane(2, 2, "vim", false)],
            mode: "normal".into(),
            clients: vec![1],
        };
        let new = Summary {
            tabs: vec![tab(1, "renamed", 0, false), tab(3, "three", 1, true)],
            panes: vec![pane(1, 1, "bash", false), pane(3, 3, "top", true)],
            mode: "locked".into(),
            clients: vec![],
        };
        let exits = HashMap::from([(2, 1)]);
        let events: Vec<String> =
            diff(&old, &new, &exits).iter().map(|e| e["event"].as_str().unwrap().to_owned()).collect();
        assert_eq!(
            events,
            [
                "tab_renamed",
                "tab_opened",
                "tab_closed",
                "pane_opened",
                "pane_closed",
                "tab_focused",
                "pane_focused",
                "mode_changed",
                "client_detached"
            ]
        );
        let closed = diff(&old, &new, &exits).into_iter().find(|e| e["event"] == "pane_closed").unwrap();
        assert_eq!(closed["exit_status"], 1);
        assert!(diff(&old, &old, &exits).is_empty());
    }
}
