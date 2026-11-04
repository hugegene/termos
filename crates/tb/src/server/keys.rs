//! Keys: zellij's input modes with the user's zellij 0.44 bindings, plus the status bar and help that
//! describe them. See docs/plans/2026-11-04-zellij-keys-and-frames.md.

use alacritty_terminal::grid::Scroll;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as TextLine, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Widget, Wrap};

use super::render::put_str;
use super::{ClientId, Modal, Mode, NORMAL, Pane, Server};
use crate::protocol::{Direction, FocusTarget, PaneId, SplitDir};

fn ctrl(key: &KeyEvent, c: char) -> bool {
    key.code == KeyCode::Char(c)
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.contains(KeyModifiers::ALT)
}

fn alt(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::ALT) && !key.modifiers.contains(KeyModifiers::CONTROL)
}

fn plain(key: &KeyEvent) -> bool {
    !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}

/// Arrow keys and h j k l, without Ctrl or Alt.
fn arrow(key: &KeyEvent) -> Option<Direction> {
    if !plain(key) {
        return None;
    }
    match key.code {
        KeyCode::Left | KeyCode::Char('h') => Some(Direction::Left),
        KeyCode::Right | KeyCode::Char('l') => Some(Direction::Right),
        KeyCode::Up | KeyCode::Char('k') => Some(Direction::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(Direction::Down),
        _ => None,
    }
}

impl Server {
    pub(super) fn key(&mut self, client: ClientId, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        match std::mem::replace(&mut self.mode, NORMAL) {
            Mode::Modal(Modal::Locked) => {
                if !ctrl(&key, 'g') {
                    self.mode = Mode::Modal(Modal::Locked);
                    self.send_key(&key);
                }
            }
            Mode::Modal(m) => {
                if !self.global_key(m, &key) {
                    self.mode_key(client, m, &key);
                }
            }
            Mode::EnterSearch(text) => self.search_input_key(text, &key),
            Mode::RenameTab { old, old_auto } => self.rename_tab_key(old, old_auto, &key),
            Mode::RenamePane { pane, old } => self.rename_pane_key(pane, old, &key),
            Mode::Select(state) => self.copy_key(client, state, &key),
            Mode::ConfirmQuit => {
                if matches!(key.code, KeyCode::Char('y' | 'Y')) {
                    self.quit = true;
                }
            }
            Mode::Help => {}
        }
    }

    /// Switches mode. Leaving scroll and search mode ends the search.
    pub(super) fn set_modal(&mut self, next: Modal) {
        if !matches!(next, Modal::Scroll | Modal::Search) {
            self.search = None;
        }
        self.mode = Mode::Modal(next);
        self.dirty = true;
    }

    /// Keys that work in every mode but Locked: the mode switches, Ctrl+Q and the Alt shortcuts.
    /// Returns true if `key` was one of them.
    fn global_key(&mut self, m: Modal, key: &KeyEvent) -> bool {
        let toggle = |target: Modal| if m == target { Modal::Normal } else { target };
        let scrolling = matches!(m, Modal::Scroll | Modal::Search);
        let switch = if ctrl(key, 'g') {
            Some(Modal::Locked)
        } else if ctrl(key, 'p') {
            Some(toggle(Modal::Pane))
        } else if ctrl(key, 't') {
            Some(toggle(Modal::Tab))
        } else if ctrl(key, 'n') {
            Some(toggle(Modal::Resize))
        } else if ctrl(key, 'h') {
            Some(toggle(Modal::Move))
        } else if ctrl(key, 'o') {
            Some(toggle(Modal::Session))
        } else if ctrl(key, 's') {
            Some(if scrolling { Modal::Normal } else { Modal::Scroll })
        } else if ctrl(key, 'b') && !scrolling && m != Modal::Tmux {
            Some(Modal::Tmux)
        } else {
            None
        };
        if let Some(next) = switch {
            self.set_modal(next);
            return true;
        }
        if ctrl(key, 'q') {
            self.search = None;
            self.mode = Mode::ConfirmQuit;
            return true;
        }
        if !alt(key) {
            return false;
        }
        // Alt keys keep the mode, except that moving focus leaves scroll mode.
        let mut next = m;
        let mut focus = |s: &mut Server, dir: Direction| {
            match dir {
                Direction::Left | Direction::Right => s.focus_or_tab(dir),
                _ => s.focus(FocusTarget::Dir(dir)),
            }
            if scrolling {
                next = Modal::Normal;
            }
        };
        match key.code {
            KeyCode::Char('n') => self.new_pane(),
            KeyCode::Left | KeyCode::Char('h') => focus(self, Direction::Left),
            KeyCode::Right | KeyCode::Char('l') => focus(self, Direction::Right),
            KeyCode::Up | KeyCode::Char('k') => focus(self, Direction::Up),
            KeyCode::Down | KeyCode::Char('j') => focus(self, Direction::Down),
            KeyCode::Char('=' | '+') => self.resize_focused(None, true),
            KeyCode::Char('-') => self.resize_focused(None, false),
            KeyCode::Char('i') => self.move_tab(false),
            KeyCode::Char('o') => self.move_tab(true),
            // Alt+F (floating), Alt+[ ] (swap layouts) and Alt+P (groups) aren't in tb, so programs get them.
            _ => return false,
        }
        self.set_modal(next);
        true
    }

    fn mode_key(&mut self, client: ClientId, m: Modal, key: &KeyEvent) {
        if m == Modal::Normal {
            self.send_key(key);
            return;
        }
        self.mode = Mode::Modal(m);
        if plain(key) {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.set_modal(Modal::Normal);
                    return;
                }
                KeyCode::Char('?') => {
                    self.search = None;
                    self.mode = Mode::Help;
                    return;
                }
                _ => {}
            }
        }
        let next = match m {
            Modal::Pane => self.pane_key(key),
            Modal::Tab => self.tab_key(key),
            Modal::Resize => self.resize_key(key),
            Modal::Move => self.move_key(key),
            Modal::Scroll | Modal::Search => self.scroll_key(m, key),
            Modal::Session => self.session_key(client, key),
            Modal::Tmux => self.tmux_key(client, key),
            Modal::Normal | Modal::Locked => None,
        };
        if let Some(next) = next {
            self.set_modal(next);
        }
    }

    // Each mode's keys. They return the mode to switch to, or `None` to stay (or when they set the mode).

    fn pane_key(&mut self, key: &KeyEvent) -> Option<Modal> {
        if let Some(dir) = arrow(key) {
            self.focus(FocusTarget::Dir(dir));
            return None;
        }
        if !plain(key) {
            return None;
        }
        match key.code {
            KeyCode::Char('p') => self.focus(FocusTarget::Next),
            KeyCode::Char('n') => self.new_pane(),
            KeyCode::Char('d') => self.split_here(SplitDir::Down),
            KeyCode::Char('r') => self.split_here(SplitDir::Right),
            KeyCode::Char('x') => self.close_focused(),
            KeyCode::Char('f') => self.toggle_zoom(),
            KeyCode::Char('z') => self.toggle_frames(),
            KeyCode::Char('c') => {
                self.start_rename_pane();
                return None;
            }
            KeyCode::Char('s' | 'w' | 'e' | 'i') => self.flash("stacked, floating and pinned panes aren't in tb yet"),
            _ => return None,
        }
        Some(Modal::Normal)
    }

    fn tab_key(&mut self, key: &KeyEvent) -> Option<Modal> {
        if !plain(key) {
            return None;
        }
        match key.code {
            KeyCode::Left | KeyCode::Up | KeyCode::Char('h' | 'k') => {
                self.cycle_tab(false);
                return None;
            }
            KeyCode::Right | KeyCode::Down | KeyCode::Char('l' | 'j') => {
                self.cycle_tab(true);
                return None;
            }
            KeyCode::Tab => {
                self.toggle_last_tab();
                return None;
            }
            KeyCode::Char(d @ '1'..='9') => self.go_to_tab(usize::from(d as u8 - b'1')),
            KeyCode::Char('n') => self.new_tab_here(),
            KeyCode::Char('x') => self.close_tab(),
            KeyCode::Char('r') => {
                self.start_rename_tab();
                return None;
            }
            KeyCode::Char('s') => self.toggle_sync(),
            KeyCode::Char('b') => self.break_pane(),
            KeyCode::Char('[') => self.break_pane_to(false),
            KeyCode::Char(']') => self.break_pane_to(true),
            KeyCode::Char('f') => self.open_finder_tab(),
            _ => return None,
        }
        Some(Modal::Normal)
    }

    fn resize_key(&mut self, key: &KeyEvent) -> Option<Modal> {
        if !plain(key) {
            return None;
        }
        let side = match key.code {
            KeyCode::Left | KeyCode::Char('h' | 'H') => Some(Direction::Left),
            KeyCode::Right | KeyCode::Char('l' | 'L') => Some(Direction::Right),
            KeyCode::Up | KeyCode::Char('k' | 'K') => Some(Direction::Up),
            KeyCode::Down | KeyCode::Char('j' | 'J') => Some(Direction::Down),
            _ => None,
        };
        match key.code {
            KeyCode::Char('H' | 'J' | 'K' | 'L') => self.resize_focused(side, false),
            _ if side.is_some() => self.resize_focused(side, true),
            KeyCode::Char('+' | '=') => self.resize_focused(None, true),
            KeyCode::Char('-') => self.resize_focused(None, false),
            _ => {}
        }
        None
    }

    fn move_key(&mut self, key: &KeyEvent) -> Option<Modal> {
        if let Some(dir) = arrow(key) {
            self.move_pane(FocusTarget::Dir(dir));
        } else if plain(key) {
            match key.code {
                KeyCode::Char('n') | KeyCode::Tab => self.move_pane(FocusTarget::Next),
                KeyCode::Char('p') => self.move_pane(FocusTarget::Prev),
                _ => {}
            }
        }
        None
    }

    fn scroll_key(&mut self, m: Modal, key: &KeyEvent) -> Option<Modal> {
        if ctrl(key, 'c') {
            self.scroll_pane(Scroll::Bottom);
            return Some(Modal::Normal);
        }
        let half = self.half_page();
        let scroll = if ctrl(key, 'f') {
            Some(Scroll::PageDown)
        } else if ctrl(key, 'b') {
            Some(Scroll::PageUp)
        } else if !plain(key) {
            None
        } else {
            match key.code {
                KeyCode::PageDown | KeyCode::Right | KeyCode::Char('l') => Some(Scroll::PageDown),
                KeyCode::PageUp | KeyCode::Left | KeyCode::Char('h') => Some(Scroll::PageUp),
                KeyCode::Down | KeyCode::Char('j') => Some(Scroll::Delta(-1)),
                KeyCode::Up | KeyCode::Char('k') => Some(Scroll::Delta(1)),
                KeyCode::Char('d') => Some(Scroll::Delta(-half)),
                KeyCode::Char('u') => Some(Scroll::Delta(half)),
                _ => None,
            }
        };
        if let Some(s) = scroll {
            self.scroll_pane(s);
            return None;
        }
        if !plain(key) {
            return None;
        }
        match (m, key.code) {
            (Modal::Scroll, KeyCode::Char('e')) => {
                self.edit_scrollback();
                return Some(Modal::Normal);
            }
            (Modal::Scroll, KeyCode::Char('s')) => self.mode = Mode::EnterSearch(String::new()),
            (Modal::Scroll, KeyCode::Char('v')) => self.enter_copy_mode(),
            (Modal::Search, KeyCode::Char('n')) => self.search_step(true),
            (Modal::Search, KeyCode::Char('p')) => self.search_step(false),
            (Modal::Search, KeyCode::Char(c @ ('c' | 'o' | 'w'))) => self.toggle_search_option(c),
            _ => {}
        }
        None
    }

    fn session_key(&mut self, client: ClientId, key: &KeyEvent) -> Option<Modal> {
        if !plain(key) {
            return None;
        }
        match key.code {
            KeyCode::Char('d') => self.detach(client),
            KeyCode::Char('w') => self.flash("no session manager yet: `tb ls` lists sessions, `tb -s NAME` opens one"),
            KeyCode::Char('c' | 'p' | 'a' | 's' | 'l') => self.flash("zellij's plugins aren't in tb"),
            _ => return None,
        }
        Some(Modal::Normal)
    }

    fn tmux_key(&mut self, client: ClientId, key: &KeyEvent) -> Option<Modal> {
        if let Some(dir) = arrow(key) {
            self.focus(FocusTarget::Dir(dir));
            return Some(Modal::Normal);
        }
        if ctrl(key, 'b') {
            if let Some(p) = self.typing_target().and_then(|id| self.term(id)) {
                p.write(vec![0x02]);
            }
            return Some(Modal::Normal);
        }
        if !plain(key) {
            return None;
        }
        match key.code {
            KeyCode::Char('"') => self.split_here(SplitDir::Down),
            KeyCode::Char('%') => self.split_here(SplitDir::Right),
            KeyCode::Char(',') => {
                self.start_rename_tab();
                return None;
            }
            KeyCode::Char('[') => return Some(Modal::Scroll),
            KeyCode::Char('o') => {
                self.focus(FocusTarget::Next);
                return None;
            }
            KeyCode::Char('c') => self.new_tab_here(),
            KeyCode::Char('n') => self.cycle_tab(true),
            KeyCode::Char('p') => self.cycle_tab(false),
            KeyCode::Char('z') => self.toggle_zoom(),
            KeyCode::Char('d') => self.detach(client),
            KeyCode::Char('x') => self.close_focused(),
            KeyCode::Char('f') => self.open_finder_tab(),
            KeyCode::Char(' ') => self.flash("swap layouts aren't in tb yet"),
            _ => return None,
        }
        Some(Modal::Normal)
    }

    // ----- typing names and search terms -----

    fn start_rename_tab(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let old = std::mem::take(&mut tab.name);
        let old_auto = std::mem::replace(&mut tab.auto_name, false);
        self.mode = Mode::RenameTab { old, old_auto };
    }

    fn rename_tab_key(&mut self, old: String, old_auto: bool, key: &KeyEvent) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        if key.code == KeyCode::Enter || ctrl(key, 'c') {
            tab.name = tab.name.trim().to_owned();
            if tab.name.is_empty() {
                tab.name = "shell".into();
                tab.auto_name = true;
            }
            return;
        }
        match key.code {
            KeyCode::Esc => {
                tab.name = old;
                tab.auto_name = old_auto;
                self.mode = Mode::Modal(Modal::Tab);
                return;
            }
            KeyCode::Backspace => {
                tab.name.pop();
            }
            KeyCode::Char(c) if plain(key) => tab.name.push(c),
            _ => {}
        }
        self.mode = Mode::RenameTab { old, old_auto };
    }

    fn start_rename_pane(&mut self) {
        let Some(id) = self.focused_pane() else { return };
        let old = self.pane_names.insert(id, String::new());
        self.mode = Mode::RenamePane { pane: id, old };
    }

    fn rename_pane_key(&mut self, pane: PaneId, old: Option<String>, key: &KeyEvent) {
        let name = self.pane_names.entry(pane).or_default();
        if key.code == KeyCode::Enter || ctrl(key, 'c') {
            *name = name.trim().to_owned();
            if name.is_empty() {
                self.pane_names.remove(&pane);
            }
            return;
        }
        match key.code {
            KeyCode::Esc => {
                match old {
                    Some(o) => *name = o,
                    None => {
                        self.pane_names.remove(&pane);
                    }
                }
                self.mode = Mode::Modal(Modal::Pane);
                return;
            }
            KeyCode::Backspace => {
                name.pop();
            }
            KeyCode::Char(c) if plain(key) => name.push(c),
            _ => {}
        }
        self.mode = Mode::RenamePane { pane, old };
    }

    fn search_input_key(&mut self, mut text: String, key: &KeyEvent) {
        if key.code == KeyCode::Esc || ctrl(key, 'c') {
            self.set_modal(Modal::Scroll);
            return;
        }
        match key.code {
            KeyCode::Enter if text.is_empty() => {
                self.set_modal(Modal::Scroll);
                return;
            }
            KeyCode::Enter => {
                self.start_search(text);
                return;
            }
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Char(c) if plain(key) => text.push(c),
            _ => {}
        }
        self.mode = Mode::EnterSearch(text);
    }

    // ----- status bar and help -----

    /// The status bar: the modes in Normal mode, otherwise the current mode and its keys.
    pub(super) fn draw_status(&self, buf: &mut Buffer, y: u16) {
        let width = buf.area.width;
        let badge = |bg: Color| Style::default().fg(Color::Black).bg(bg).add_modifier(Modifier::BOLD);
        let dim = Style::default().fg(Color::DarkGray);
        let finder_hint = || match self.focused_pane().and_then(|id| self.panes.get(&id)) {
            Some(Pane::Finder(f)) => Some(f.hint()),
            _ => None,
        };
        let (mut x, hints) = match &self.mode {
            // A message briefly takes the mode list's place.
            Mode::Modal(Modal::Normal) if self.message.is_some() => (0, String::new()),
            Mode::Modal(Modal::Normal) => (self.draw_mode_list(buf, y), finder_hint().unwrap_or_default().to_owned()),
            Mode::Modal(m) => {
                let (name, bg) = match m {
                    Modal::Locked => ("LOCKED", Color::Red),
                    Modal::Pane => ("PANE", Color::Yellow),
                    Modal::Tab => ("TAB", Color::Yellow),
                    Modal::Resize => ("RESIZE", Color::Yellow),
                    Modal::Move => ("MOVE", Color::Yellow),
                    Modal::Scroll => ("SCROLL", Color::Yellow),
                    Modal::Search => ("SEARCH", Color::Yellow),
                    Modal::Session => ("SESSION", Color::Yellow),
                    Modal::Tmux => ("TMUX", Color::Yellow),
                    Modal::Normal => ("", Color::Reset),
                };
                (put_str(buf, 0, y, &format!(" {name} "), badge(bg), width), self.mode_hints(*m))
            }
            Mode::EnterSearch(text) => {
                let w = put_str(buf, 0, y, " SEARCH ", badge(Color::Yellow), width);
                let w = w + put_str(buf, w, y, &format!(" find: {text}▏"), Style::default(), width - w);
                (w, "Enter search · Esc back".into())
            }
            Mode::RenameTab { .. } => (
                put_str(buf, 0, y, " RENAME TAB ", badge(Color::Yellow), width),
                "type a name · Enter keep · Esc undo".into(),
            ),
            Mode::RenamePane { .. } => (
                put_str(buf, 0, y, " RENAME PANE ", badge(Color::Yellow), width),
                "type a name · Enter keep · Esc undo".into(),
            ),
            Mode::Select(c) => match &c.input {
                Some((backward, text)) => {
                    let w = put_str(buf, 0, y, " SELECT ", badge(Color::Magenta), width);
                    let prompt = format!(" {}{text}▏", if *backward { '?' } else { '/' });
                    (w + put_str(buf, w, y, &prompt, Style::default(), width - w), "Enter search · Esc cancel".into())
                }
                None => (
                    put_str(buf, 0, y, " SELECT ", badge(Color::Magenta), width),
                    "hjkl/w/b move · v V Ctrl+v select · y copy · / ? search · n N next · q back".into(),
                ),
            },
            Mode::ConfirmQuit => (
                put_str(buf, 0, y, " QUIT? ", badge(Color::Red), width),
                format!(
                    "y ends session '{}' and its programs; attach again to restore the layout · any other key cancels",
                    self.name
                ),
            ),
            Mode::Help => (put_str(buf, 0, y, " HELP ", badge(Color::Cyan), width), "any key closes".into()),
        };
        x += 1;
        if let Some((msg, _)) = &self.message {
            put_str(buf, x, y, msg, Style::default().fg(Color::Yellow), width.saturating_sub(x));
            return;
        }
        // Drop whole hints from the end until the rest fits.
        let room = usize::from(width.saturating_sub(x));
        let mut parts: Vec<&str> = hints.split(" · ").collect();
        while parts.len() > 1 && parts.join(" · ").chars().count() > room {
            parts.pop();
        }
        let shown = parts.join(" · ");
        if matches!(self.mode, Mode::Modal(Modal::Normal)) {
            // Right-aligned, like before.
            let w = (shown.chars().count() as u16).min(room as u16);
            put_str(buf, width - w, y, &shown, dim, w);
        } else {
            put_str(buf, x, y, &shown, dim, room as u16);
        }
    }

    /// "Ctrl + <g> LOCK <p> PANE …", shortened to fit. Returns the width used.
    fn draw_mode_list(&self, buf: &mut Buffer, y: u16) -> u16 {
        const MODES: [(char, &str); 8] = [
            ('g', "LOCK"),
            ('p', "PANE"),
            ('t', "TAB"),
            ('n', "RESIZE"),
            ('h', "MOVE"),
            ('s', "SCROLL"),
            ('o', "SESSION"),
            ('q', "QUIT"),
        ];
        let width = buf.area.width;
        let key = Style::default().add_modifier(Modifier::BOLD).fg(Color::Green);
        let label = Style::default();
        // Room left for a finder hint decides nothing; the mode list comes first.
        let full: u16 = 7 + MODES.iter().map(|(_, l)| 6 + l.len() as u16).sum::<u16>();
        let compact: u16 = 2 + MODES.iter().map(|(_, l)| 3 + l.len() as u16).sum::<u16>();
        let mut x = 0;
        if full <= width {
            x += put_str(buf, x, y, " Ctrl +", label, width - x);
            for (k, l) in MODES {
                x += put_str(buf, x, y, &format!("  <{k}> "), key, width - x);
                x += put_str(buf, x, y, l, label, width - x);
            }
        } else if compact <= width {
            x += put_str(buf, x, y, " ^", key, width - x);
            for (k, l) in MODES {
                x += put_str(buf, x, y, &format!(" {k}"), key, width - x);
                x += put_str(buf, x, y, &format!(" {}", l.to_lowercase()), label, width - x);
            }
        } else {
            x += put_str(buf, x, y, " Ctrl+", label, width - x);
            for (k, _) in MODES {
                x += put_str(buf, x, y, &format!("{k} "), key, width.saturating_sub(x));
            }
        }
        x
    }

    /// Key hints for a mode, most important first.
    fn mode_hints(&self, m: Modal) -> String {
        let on = |b: bool| if b { "on" } else { "off" };
        match m {
            Modal::Locked => "Ctrl+g unlock · all other keys go to the program".into(),
            Modal::Pane => "←↓↑→ focus · n new · d down · r right · x close · f fullscreen · z frames · c rename · p next · Esc back".into(),
            Modal::Tab => "←→ switch · n new · x close · r rename · 1-9 go to · Tab last · s sync · b break out · [ ] move pane to tab · f finder · Esc back".into(),
            Modal::Resize => "←↓↑→ grow · H J K L shrink · + - all sides · Esc back".into(),
            Modal::Move => "←↓↑→ swap with neighbour · n next · p previous · Esc back".into(),
            Modal::Scroll => "↓↑ line · d u half page · PgDn PgUp page · s search · e edit · v select · Ctrl+c bottom · Esc back".into(),
            Modal::Search => {
                let s = self.search.as_ref();
                format!(
                    "n down · p up · c case-sensitive: {} · w wrap: {} · o whole word: {} · ↓↑ d u scroll · Esc back",
                    on(s.is_some_and(|s| s.case_sensitive)),
                    on(s.is_some_and(|s| s.wrap)),
                    on(s.is_some_and(|s| s.whole_word)),
                )
            }
            Modal::Session => "d detach · Esc back".into(),
            Modal::Tmux => "c new tab · % split right · \" split down · ←↓↑→ focus · o next pane · n p tab · z fullscreen · x close · [ scroll · , rename · f finder · d detach · ? help".into(),
            Modal::Normal => String::new(),
        }
    }

    pub(super) fn draw_help(&self, buf: &mut Buffer) {
        const KEYS: &[(&str, &str)] = &[
            ("Ctrl+g", "lock: every key goes to the program, until Ctrl+g again"),
            ("Ctrl+p", "pane: ←↓↑→ focus · n new · d down · r right · x close · f fullscreen · z frames · c rename"),
            (
                "Ctrl+t",
                "tab: ←→ switch · n new · x close · r rename · 1-9 · Tab last · s sync · b break out · f finder",
            ),
            ("Ctrl+n", "resize: ←↓↑→ grow that side · H J K L shrink it · + - all sides"),
            ("Ctrl+h", "move: ←↓↑→ swap with the neighbour · n p next / previous"),
            ("Ctrl+s", "scroll: ↓↑ d u PgDn PgUp · s search (n p c w o) · e edit · v select with vi keys"),
            ("Ctrl+o", "session: d detach"),
            ("Ctrl+b", "tmux keys: c % \" , [ o n p z x d, f finder"),
            ("Ctrl+q", "quit: ends the session (asks first)"),
            ("Alt+n", "new pane · Alt+←↓↑→ focus · Alt+= Alt+- resize · Alt+i Alt+o move the tab"),
            ("Esc", "back to normal (Enter too)"),
        ];
        let area = buf.area;
        let width = 96.min(area.width);
        let height = (KEYS.len() as u16 * 2 + 5).min(area.height);
        let rect = Rect::new((area.width - width) / 2, (area.height - height) / 2, width, height);
        let mut lines: Vec<TextLine> = KEYS
            .iter()
            .map(|(k, d)| {
                TextLine::from(vec![
                    Span::styled(format!(" {k:<8}"), Style::default().fg(Color::Yellow)),
                    Span::raw(*d),
                ])
            })
            .collect();
        lines.push(TextLine::default());
        lines.push(TextLine::styled(
            " Mouse: click to focus, drag the lines between panes to resize, the wheel scrolls back, drag to \
             select and copy (hold Shift if the program uses the mouse).",
            Style::default().fg(Color::DarkGray),
        ));
        Clear.render(rect, buf);
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(" keys (zellij's) · any key closes "))
            .render(rect, buf);
    }
}
