//! Copy mode: move through the scrollback with vi keys, select, search, and copy to the clipboard.

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Direction, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::search::{Match, RegexSearch};
use alacritty_terminal::vi_mode::ViMotion;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{ClientId, Mode, Server};
use crate::protocol::PaneId;

pub struct CopyState {
    pub pane: PaneId,
    /// A search being typed: (backwards?, text).
    pub input: Option<(bool, String)>,
    /// The last search: (pattern, backwards?).
    search: Option<(String, bool)>,
    pub last_match: Option<Match>,
    pending_g: bool,
}

enum Outcome {
    Stay,
    Exit,
    Copy,
}

impl Server {
    pub(super) fn enter_copy_mode(&mut self) {
        let Some(id) = self.typing_target() else { return };
        let Some(p) = self.term(id) else { return };
        let mut term = p.term.lock();
        if !term.mode().contains(TermMode::VI) {
            term.toggle_vi_mode();
        }
        drop(term);
        self.mode = Mode::Copy(CopyState { pane: id, input: None, search: None, last_match: None, pending_g: false });
    }

    fn exit_copy_mode(&mut self, pane: PaneId) {
        if let Some(p) = self.term(pane) {
            let mut term = p.term.lock();
            if term.mode().contains(TermMode::VI) {
                term.toggle_vi_mode();
            }
            term.selection = None;
            term.scroll_display(Scroll::Bottom);
        }
        self.mode = Mode::Normal;
    }

    pub(super) fn copy_key(&mut self, client: ClientId, mut state: CopyState, key: &KeyEvent) {
        let pane = state.pane;
        if self.term(pane).is_none() {
            self.mode = Mode::Normal;
            return;
        }
        if let Some((backward, mut text)) = state.input.take() {
            match key.code {
                KeyCode::Enter => {
                    if !text.is_empty() {
                        state.search = Some((text, backward));
                        state.last_match = None;
                        self.search(&mut state, false);
                    }
                }
                KeyCode::Esc => {}
                KeyCode::Backspace => {
                    text.pop();
                    state.input = Some((backward, text));
                }
                KeyCode::Char(c) => {
                    text.push(c);
                    state.input = Some((backward, text));
                }
                _ => state.input = Some((backward, text)),
            }
            self.mode = Mode::Copy(state);
            return;
        }
        let outcome = self.copy_motion(&mut state, key);
        match outcome {
            Outcome::Stay => self.mode = Mode::Copy(state),
            Outcome::Exit => self.exit_copy_mode(pane),
            Outcome::Copy => {
                let text = self.term(pane).and_then(|p| {
                    let term = p.term.lock();
                    term.selection_to_string().filter(|t| !t.is_empty()).or_else(|| {
                        // Nothing selected: copy the line under the cursor.
                        let line = term.vi_mode_cursor.point.line;
                        let end = Point::new(line, Column(term.columns() - 1));
                        Some(term.bounds_to_string(Point::new(line, Column(0)), end).trim_end().to_owned())
                    })
                });
                self.exit_copy_mode(pane);
                if let Some(text) = text {
                    self.copy_to_clipboard(Some(client), &text);
                }
            }
        }
    }

    fn copy_motion(&mut self, state: &mut CopyState, key: &KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let g_pending = std::mem::take(&mut state.pending_g);
        let Some(p) = self.term(state.pane) else { return Outcome::Exit };
        let mut term = p.term.lock();
        let lines = term.screen_lines() as i32;
        let motion = match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Outcome::Exit,
            KeyCode::Char('y') | KeyCode::Enter => return Outcome::Copy,
            KeyCode::Char('h') | KeyCode::Left => Some(ViMotion::Left),
            KeyCode::Char('j') | KeyCode::Down => Some(ViMotion::Down),
            KeyCode::Char('k') | KeyCode::Up => Some(ViMotion::Up),
            KeyCode::Char('l') | KeyCode::Right => Some(ViMotion::Right),
            KeyCode::Char('u') if ctrl => {
                term.scroll_display(Scroll::Delta(lines / 2));
                None
            }
            KeyCode::Char('d') if ctrl => {
                term.scroll_display(Scroll::Delta(-lines / 2));
                None
            }
            KeyCode::Char('b') if ctrl => {
                term.scroll_display(Scroll::PageUp);
                None
            }
            KeyCode::Char('f') if ctrl => {
                term.scroll_display(Scroll::PageDown);
                None
            }
            KeyCode::PageUp => {
                term.scroll_display(Scroll::PageUp);
                None
            }
            KeyCode::PageDown => {
                term.scroll_display(Scroll::PageDown);
                None
            }
            KeyCode::Char('v') if ctrl => {
                toggle_selection(&mut term, SelectionType::Block);
                None
            }
            KeyCode::Char('w') => Some(ViMotion::SemanticRight),
            KeyCode::Char('b') => Some(ViMotion::SemanticLeft),
            KeyCode::Char('e') => Some(ViMotion::SemanticRightEnd),
            KeyCode::Char('W') => Some(ViMotion::WordRight),
            KeyCode::Char('B') => Some(ViMotion::WordLeft),
            KeyCode::Char('E') => Some(ViMotion::WordRightEnd),
            KeyCode::Char('0') | KeyCode::Home => Some(ViMotion::First),
            KeyCode::Char('$') | KeyCode::End => Some(ViMotion::Last),
            KeyCode::Char('^') => Some(ViMotion::FirstOccupied),
            KeyCode::Char('H') => Some(ViMotion::High),
            KeyCode::Char('M') => Some(ViMotion::Middle),
            KeyCode::Char('L') => Some(ViMotion::Low),
            KeyCode::Char('%') => Some(ViMotion::Bracket),
            KeyCode::Char('{') => Some(ViMotion::ParagraphUp),
            KeyCode::Char('}') => Some(ViMotion::ParagraphDown),
            KeyCode::Char('g') if g_pending => {
                let top = Point::new(term.topmost_line(), Column(0));
                term.vi_goto_point(top);
                None
            }
            KeyCode::Char('g') => {
                state.pending_g = true;
                None
            }
            KeyCode::Char('G') => {
                let bottom = Point::new(term.bottommost_line(), Column(0));
                term.vi_goto_point(bottom);
                None
            }
            KeyCode::Char('v') => {
                toggle_selection(&mut term, SelectionType::Simple);
                None
            }
            KeyCode::Char('V') => {
                toggle_selection(&mut term, SelectionType::Lines);
                None
            }
            KeyCode::Char('/') => {
                state.input = Some((false, String::new()));
                None
            }
            KeyCode::Char('?') => {
                state.input = Some((true, String::new()));
                None
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                drop(term);
                self.search(state, key.code == KeyCode::Char('N'));
                return Outcome::Stay;
            }
            _ => None,
        };
        if let Some(m) = motion {
            term.vi_motion(m);
        }
        Outcome::Stay
    }

    /// Runs the current search from the cursor (or the last match). `reverse` flips its direction.
    fn search(&mut self, state: &mut CopyState, reverse: bool) {
        let Some((pattern, backward)) = state.search.clone() else { return };
        let backward = backward != reverse;
        let Ok(mut regex) = RegexSearch::new(&pattern) else {
            self.flash(format!("not a valid search: {pattern}"));
            return;
        };
        let Some(p) = self.term(state.pane) else { return };
        let mut term = p.term.lock();
        let origin = match &state.last_match {
            Some(m) if backward => m.start().sub(&*term, Boundary::None, 1),
            Some(m) => m.end().add(&*term, Boundary::None, 1),
            None => term.vi_mode_cursor.point,
        };
        let (direction, side) = if backward { (Direction::Left, Side::Right) } else { (Direction::Right, Side::Left) };
        let found = term.search_next(&mut regex, origin, direction, side, None);
        match found {
            Some(m) => {
                term.vi_goto_point(*m.start());
                state.last_match = Some(m);
            }
            None => {
                drop(term);
                self.flash(format!("not found: {pattern}"));
            }
        }
    }
}

fn toggle_selection<T>(term: &mut alacritty_terminal::Term<T>, ty: SelectionType) {
    if term.selection.as_ref().is_some_and(|s| s.ty == ty) {
        term.selection = None;
    } else {
        let point = term.vi_mode_cursor.point;
        term.selection = Some(Selection::new(ty, point, Side::Left));
    }
}
