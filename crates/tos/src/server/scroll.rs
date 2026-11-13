//! Scroll mode (zellij's Ctrl+S): move through a pane's history, search it, or open it in the editor.

use std::cell::RefCell;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

use alacritty_terminal::Term;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point, Side};
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};

use super::{Modal, Mode, Pane, Server, editor};
use crate::paths;
use crate::protocol::PaneId;

/// A search in scroll mode. The term is plain text; the options are zellij's.
pub struct Search {
    pub pane: PaneId,
    pub query: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub wrap: bool,
    regex: RefCell<RegexSearch>,
    pub current: Option<Match>,
}

/// Escapes regex syntax so the text is searched for as typed.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn build_regex(query: &str, case_sensitive: bool, whole_word: bool) -> Option<RegexSearch> {
    let mut pattern = escape(query);
    if whole_word {
        pattern = format!(r"\b{pattern}\b");
    }
    // An inline flag overrides alacritty's "case-insensitive unless there's a capital" default.
    let flag = if case_sensitive { "(?-i)" } else { "(?i)" };
    RegexSearch::new(&format!("{flag}{pattern}")).ok()
}

impl Search {
    /// The matches on the pane's screen, for highlighting.
    pub fn visible<T>(&self, term: &Term<T>) -> Vec<Match> {
        let mut regex = self.regex.borrow_mut();
        let offset = term.grid().display_offset() as i32;
        let start = Point::new(Line(-offset), Column(0));
        let end = Point::new(Line(term.screen_lines() as i32 - 1 - offset), Column(term.columns().saturating_sub(1)));
        RegexIter::new(start, end, Direction::Right, term, &mut regex).take(500).collect()
    }
}

impl Server {
    pub(super) fn scroll_pane(&mut self, scroll: Scroll) {
        if let Some(p) = self.typing_target().and_then(|id| self.term(id)) {
            p.term.lock().scroll_display(scroll);
            self.dirty = true;
        }
    }

    pub(super) fn half_page(&self) -> i32 {
        self.typing_target().and_then(|id| self.term(id)).map_or(10, |p| i32::from(p.rows / 2).max(1))
    }

    /// Starts searching the focused pane for `query` and jumps to the nearest match above the bottom.
    pub(super) fn start_search(&mut self, query: String) {
        let Some(pane) = self.typing_target() else {
            self.set_modal(Modal::Normal);
            return;
        };
        let (case_sensitive, whole_word, wrap) =
            self.search.as_ref().map_or((false, false, true), |s| (s.case_sensitive, s.whole_word, s.wrap));
        let Some(regex) = build_regex(&query, case_sensitive, whole_word) else {
            self.flash(format!("can't search for {query}"));
            self.set_modal(Modal::Scroll);
            return;
        };
        self.search =
            Some(Search { pane, query, case_sensitive, whole_word, wrap, regex: RefCell::new(regex), current: None });
        self.mode = Mode::Modal(Modal::Search);
        self.search_step(false);
    }

    /// Moves to the next match down (newer) or up (older), scrolling it into view.
    pub(super) fn search_step(&mut self, down: bool) {
        let Some(search) = self.search.as_mut() else { return };
        let Some(Pane::Term(p)) = self.panes.get(&search.pane) else { return };
        let found = {
            let mut term = p.term.lock();
            let origin = match &search.current {
                Some(m) if down => m.end().add(&*term, Boundary::None, 1),
                Some(m) => m.start().sub(&*term, Boundary::None, 1),
                None => {
                    let offset = term.grid().display_offset() as i32;
                    if down {
                        Point::new(Line(-offset), Column(0))
                    } else {
                        let bottom = term.screen_lines() as i32 - 1 - offset;
                        Point::new(Line(bottom), Column(term.columns().saturating_sub(1)))
                    }
                }
            };
            let (direction, side) = if down { (Direction::Right, Side::Left) } else { (Direction::Left, Side::Right) };
            let found = term.search_next(search.regex.get_mut(), origin, direction, side, None);
            // `search_next` wraps around the history; without wrap, a match behind us doesn't count.
            let wrapped = |m: &Match| if down { *m.start() < origin } else { *m.start() > origin };
            let found = found.filter(|m| search.wrap || search.current.is_none() || !wrapped(m));
            if let Some(m) = &found {
                term.scroll_to_point(*m.start());
            }
            found
        };
        let missing = found.is_none();
        if found.is_some() {
            search.current = found;
        }
        let query = search.query.clone();
        if missing {
            self.flash(format!("no more matches for {query}"));
        }
        self.dirty = true;
    }

    /// Toggles a search option: `c` case-sensitive, `o` whole word, `w` wrap.
    pub(super) fn toggle_search_option(&mut self, which: char) {
        let Some(s) = self.search.as_mut() else { return };
        match which {
            'c' => s.case_sensitive = !s.case_sensitive,
            'o' => s.whole_word = !s.whole_word,
            _ => s.wrap = !s.wrap,
        }
        if let Some(regex) = build_regex(&s.query, s.case_sensitive, s.whole_word) {
            s.regex = RefCell::new(regex);
        }
        self.dirty = true;
    }

    /// Opens the focused pane's scrollback in the editor, in the pane's place. When the editor exits, the
    /// pane comes back.
    pub(super) fn edit_scrollback(&mut self) {
        let Some(id) = self.focused_pane() else { return };
        let Some(p) = self.term(id) else {
            self.flash("only terminal panes have scrollback to edit");
            return;
        };
        let text = p.text(true);
        let cwd = p.cwd();
        let path = paths::state_dir().join(format!("scrollback-{}-{id}.txt", self.name));
        let written = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .and_then(|mut f| f.write_all(format!("{text}\n").as_bytes()));
        if let Err(e) = written {
            self.flash(format!("can't write the scrollback: {e}"));
            return;
        }
        let (mut opts, kind) = editor::command(&path);
        opts.cwd = opts.cwd.or(cwd);
        let editor = match self.spawn_term(opts) {
            Ok(e) => e,
            Err(e) => {
                let _ = fs::remove_file(&path);
                self.flash(format!("can't start the editor: {e}"));
                return;
            }
        };
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.root.contains(id)) {
            tab.root.replace(id, editor);
            if tab.focus == id {
                tab.focus = editor;
            }
        }
        self.mark_editor(editor, kind);
        self.pane_names.insert(editor, "scrollback".into());
        self.scrollback.insert(editor, (id, path));
        self.focus_changed(Some(id), Some(editor));
        self.apply_layout();
    }
}
