//! Turns key, paste, focus and mouse events into the bytes a program in a pane expects.
//!
//! Keys use xterm's legacy encoding: modifier parameters (`CSI 1;5A` for Ctrl+Up) and application cursor
//! keys. A program that turned on the kitty keyboard protocol gets its encoding instead, so it can tell
//! Shift+Enter, Ctrl+Enter and Ctrl+Shift+letter apart. Plan: docs/plans/2026-11-17-kitty-keys-to-programs.md.
//! Also bracketed paste, focus reports, and SGR or X10 mouse reports.

use alacritty_terminal::term::TermMode;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

/// xterm's modifier parameter: 1 + Shift(1) + Alt(2) + Ctrl(4).
fn modifier_param(m: KeyModifiers) -> u8 {
    1 + u8::from(m.contains(KeyModifiers::SHIFT))
        + 2 * u8::from(m.contains(KeyModifiers::ALT))
        + 4 * u8::from(m.contains(KeyModifiers::CONTROL))
}

fn cursor_key(final_byte: char, param: u8, app_cursor: bool) -> Vec<u8> {
    if param > 1 {
        format!("\x1b[1;{param}{final_byte}").into_bytes()
    } else if app_cursor {
        format!("\x1bO{final_byte}").into_bytes()
    } else {
        format!("\x1b[{final_byte}").into_bytes()
    }
}

fn tilde_key(number: u8, param: u8) -> Vec<u8> {
    if param > 1 { format!("\x1b[{number};{param}~").into_bytes() } else { format!("\x1b[{number}~").into_bytes() }
}

/// The control character for Ctrl+`c`, if there is one.
fn control_char(c: char) -> Option<u8> {
    let c = c.to_ascii_lowercase();
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        '@' | ' ' | '2' => 0,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '-' | '/' | '7' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    })
}

fn with_alt(alt: bool, mut bytes: Vec<u8>) -> Vec<u8> {
    if alt {
        bytes.insert(0, 0x1b);
    }
    bytes
}

/// The kitty protocol's modifier value: 1 + Shift(1) + Alt(2) + Ctrl(4) + Super(8) + Hyper(16) + Meta(32).
fn kitty_modifiers(m: KeyModifiers) -> u8 {
    [
        (KeyModifiers::SHIFT, 1),
        (KeyModifiers::ALT, 2),
        (KeyModifiers::CONTROL, 4),
        (KeyModifiers::SUPER, 8),
        (KeyModifiers::HYPER, 16),
        (KeyModifiers::META, 32),
    ]
    .iter()
    .filter(|(flag, _)| m.contains(*flag))
    .fold(1, |sum, (_, bit)| sum + bit)
}

/// A key for a program that turned on the kitty keyboard protocol (<https://sw.kovidgoyal.net/kitty/keyboard-protocol/>),
/// or `None` where the program's flags keep the legacy encoding:
/// - text typed without Ctrl or Alt, and Enter, Tab and Backspace without modifiers, unless the program asked for
///   every key as an escape code;
/// - Shift+Tab (`CSI Z`), arrows, Home/End, PgUp/PgDn, Insert/Delete and the F keys but F3, whose legacy forms
///   already carry modifiers.
///
/// Only presses reach here: `tos` asks its own terminal for disambiguation alone, so it never sees repeats or
/// releases to report.
fn kitty_key(key: &KeyEvent, mode: TermMode) -> Option<Vec<u8>> {
    let all_keys = mode.contains(TermMode::REPORT_ALL_KEYS_AS_ESC);
    let mut m = key.modifiers;
    let types_text = !m.intersects(
        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META,
    );
    let (code, text) = match key.code {
        KeyCode::Char(c) => {
            if types_text && !all_keys {
                return None;
            }
            // The key is its unshifted character: Ctrl+Shift+Z is `z` with Shift.
            let mut lower = c.to_lowercase();
            let base = match (lower.next(), lower.next()) {
                (Some(l), None) if l != c => {
                    m |= KeyModifiers::SHIFT;
                    l
                }
                _ => c,
            };
            (base, (types_text && mode.contains(TermMode::REPORT_ASSOCIATED_TEXT)).then_some(c))
        }
        KeyCode::Enter | KeyCode::Tab | KeyCode::Backspace if m.is_empty() && !all_keys => return None,
        KeyCode::BackTab if (m - KeyModifiers::SHIFT).is_empty() && !all_keys => return None,
        KeyCode::Enter => ('\r', None),
        KeyCode::Tab => ('\t', None),
        KeyCode::BackTab => {
            m |= KeyModifiers::SHIFT;
            ('\t', None)
        }
        KeyCode::Backspace => ('\x7f', None),
        KeyCode::Esc => ('\x1b', None),
        // The legacy `CSI 1;<mods>R` would read as a cursor position report.
        KeyCode::F(3) => return Some(tilde_key(13, kitty_modifiers(m))),
        _ => return None,
    };
    let mods = kitty_modifiers(m);
    let mut out = format!("\x1b[{}", u32::from(code));
    if mode.contains(TermMode::REPORT_ALTERNATE_KEYS) && m.contains(KeyModifiers::SHIFT) && code.is_ascii_lowercase() {
        out += &format!(":{}", u32::from(code.to_ascii_uppercase()));
    }
    if mods > 1 || text.is_some() {
        out.push(';');
        if mods > 1 {
            out += &mods.to_string();
        }
    }
    if let Some(t) = text {
        out += &format!(";{}", u32::from(t));
    }
    out.push('u');
    Some(out.into_bytes())
}

/// Encodes a key press for a program whose terminal is in `mode`. Releases produce nothing.
pub fn encode_key(key: &KeyEvent, mode: TermMode) -> Option<Vec<u8>> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    if mode.intersects(TermMode::KITTY_KEYBOARD_PROTOCOL)
        && let Some(bytes) = kitty_key(key, mode)
    {
        return Some(bytes);
    }
    let m = key.modifiers;
    let alt = m.contains(KeyModifiers::ALT);
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let shift = m.contains(KeyModifiers::SHIFT);
    let param = modifier_param(m);
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    Some(match key.code {
        KeyCode::Char(c) => {
            if ctrl && let Some(b) = control_char(c) {
                return Some(with_alt(alt, vec![b]));
            }
            let mut buf = [0u8; 4];
            with_alt(alt, c.encode_utf8(&mut buf).as_bytes().to_vec())
        }
        KeyCode::Enter => with_alt(alt, b"\r".to_vec()),
        KeyCode::Tab if shift => b"\x1b[Z".to_vec(),
        KeyCode::Tab => with_alt(alt, b"\t".to_vec()),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace if ctrl => with_alt(alt, vec![0x08]),
        KeyCode::Backspace => with_alt(alt, vec![0x7f]),
        KeyCode::Esc => with_alt(alt, vec![0x1b]),
        KeyCode::Up => cursor_key('A', param, app_cursor),
        KeyCode::Down => cursor_key('B', param, app_cursor),
        KeyCode::Right => cursor_key('C', param, app_cursor),
        KeyCode::Left => cursor_key('D', param, app_cursor),
        KeyCode::Home => cursor_key('H', param, app_cursor),
        KeyCode::End => cursor_key('F', param, app_cursor),
        KeyCode::Insert => tilde_key(2, param),
        KeyCode::Delete => tilde_key(3, param),
        KeyCode::PageUp => tilde_key(5, param),
        KeyCode::PageDown => tilde_key(6, param),
        KeyCode::F(n @ 1..=4) => {
            let c = (b'P' + n - 1) as char;
            if param > 1 { format!("\x1b[1;{param}{c}").into_bytes() } else { format!("\x1bO{c}").into_bytes() }
        }
        KeyCode::F(n @ 5..=12) => {
            let number = [15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)];
            tilde_key(number, param)
        }
        KeyCode::Null => vec![0],
        _ => return None,
    })
}

/// Encodes pasted text. Programs that asked for bracketed paste get it wrapped, with any ESC removed so
/// the paste can't end the bracket early.
pub fn encode_paste(text: &str, mode: TermMode) -> Vec<u8> {
    if mode.contains(TermMode::BRACKETED_PASTE) {
        let mut out = b"\x1b[200~".to_vec();
        out.extend(text.replace('\x1b', "").bytes());
        out.extend(b"\x1b[201~");
        out
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

/// Focus in/out report, if the program asked for them.
pub fn encode_focus(gained: bool, mode: TermMode) -> Option<Vec<u8>> {
    mode.contains(TermMode::FOCUS_IN_OUT).then(|| if gained { b"\x1b[I".to_vec() } else { b"\x1b[O".to_vec() })
}

/// Whether the program in the pane wants mouse events.
pub fn wants_mouse(mode: TermMode) -> bool {
    mode.intersects(TermMode::MOUSE_MODE)
}

fn button_code(b: MouseButton) -> u8 {
    match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

/// Encodes a mouse event at pane-relative `col`, `row` (0-based), or `None` if the program's mouse mode
/// doesn't include it.
pub fn encode_mouse(ev: &MouseEvent, col: u16, row: u16, mode: TermMode) -> Option<Vec<u8>> {
    if !wants_mouse(mode) {
        return None;
    }
    let any_motion = mode.contains(TermMode::MOUSE_MOTION);
    let (mut code, release) = match ev.kind {
        MouseEventKind::Down(b) => (button_code(b), false),
        MouseEventKind::Up(b) => (button_code(b), true),
        MouseEventKind::Drag(b) => {
            if !mode.contains(TermMode::MOUSE_DRAG) && !any_motion {
                return None;
            }
            (button_code(b) + 32, false)
        }
        MouseEventKind::Moved => {
            if !any_motion {
                return None;
            }
            (3 + 32, false)
        }
        MouseEventKind::ScrollUp => (64, false),
        MouseEventKind::ScrollDown => (65, false),
        MouseEventKind::ScrollLeft => (66, false),
        MouseEventKind::ScrollRight => (67, false),
    };
    if ev.modifiers.contains(KeyModifiers::SHIFT) {
        code += 4;
    }
    if ev.modifiers.contains(KeyModifiers::ALT) {
        code += 8;
    }
    if ev.modifiers.contains(KeyModifiers::CONTROL) {
        code += 16;
    }
    let (x, y) = (u32::from(col) + 1, u32::from(row) + 1);
    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if release { 'm' } else { 'M' };
        return Some(format!("\x1b[<{code};{x};{y}{end}").into_bytes());
    }
    // X10 encoding: releases report button 3; positions are limited to 223.
    if release {
        code = (code & !3) | 3;
    }
    let clamp = |v: u32| (v + 32).min(255) as u8;
    Some(vec![0x1b, b'[', b'M', code + 32, clamp(x), clamp(y)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, m: KeyModifiers) -> Vec<u8> {
        encode_key(&KeyEvent::new(code, m), TermMode::empty()).unwrap()
    }

    #[test]
    fn plain_and_control_keys() {
        assert_eq!(key(KeyCode::Char('a'), KeyModifiers::NONE), b"a");
        assert_eq!(key(KeyCode::Char('é'), KeyModifiers::NONE), "é".as_bytes());
        assert_eq!(key(KeyCode::Char('c'), KeyModifiers::CONTROL), [3]);
        assert_eq!(key(KeyCode::Char('C'), KeyModifiers::CONTROL | KeyModifiers::SHIFT), [3]);
        assert_eq!(key(KeyCode::Char(' '), KeyModifiers::CONTROL), [0]);
        assert_eq!(key(KeyCode::Char('x'), KeyModifiers::ALT), b"\x1bx");
        assert_eq!(key(KeyCode::Enter, KeyModifiers::NONE), b"\r");
        assert_eq!(key(KeyCode::Backspace, KeyModifiers::NONE), [0x7f]);
        assert_eq!(key(KeyCode::BackTab, KeyModifiers::SHIFT), b"\x1b[Z");
    }

    #[test]
    fn cursor_keys_follow_modes_and_modifiers() {
        assert_eq!(key(KeyCode::Up, KeyModifiers::NONE), b"\x1b[A");
        let app = encode_key(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), TermMode::APP_CURSOR).unwrap();
        assert_eq!(app, b"\x1bOA");
        assert_eq!(key(KeyCode::Left, KeyModifiers::CONTROL), b"\x1b[1;5D");
        assert_eq!(key(KeyCode::Right, KeyModifiers::SHIFT | KeyModifiers::ALT), b"\x1b[1;4C");
        assert_eq!(key(KeyCode::PageDown, KeyModifiers::NONE), b"\x1b[6~");
        assert_eq!(key(KeyCode::Delete, KeyModifiers::CONTROL), b"\x1b[3;5~");
        assert_eq!(key(KeyCode::F(1), KeyModifiers::NONE), b"\x1bOP");
        assert_eq!(key(KeyCode::F(5), KeyModifiers::NONE), b"\x1b[15~");
        assert_eq!(key(KeyCode::F(12), KeyModifiers::SHIFT), b"\x1b[24;2~");
    }

    fn kitty(code: KeyCode, m: KeyModifiers, mode: TermMode) -> Vec<u8> {
        encode_key(&KeyEvent::new(code, m), mode).unwrap()
    }

    #[test]
    fn kitty_flag_1_tells_modified_keys_apart() {
        let k = |code, m| kitty(code, m, TermMode::DISAMBIGUATE_ESC_CODES);
        let (none, ctrl, shift, alt) =
            (KeyModifiers::NONE, KeyModifiers::CONTROL, KeyModifiers::SHIFT, KeyModifiers::ALT);
        // The rows of docs/findings/2026-11-17-agent-cli-key-clashes.md.
        assert_eq!(k(KeyCode::Enter, shift), b"\x1b[13;2u");
        assert_eq!(k(KeyCode::Enter, ctrl), b"\x1b[13;5u");
        assert_eq!(k(KeyCode::Char('z'), ctrl | shift), b"\x1b[122;6u");
        assert_eq!(k(KeyCode::Char('Z'), ctrl | shift), b"\x1b[122;6u");
        assert_eq!(k(KeyCode::Char('b'), ctrl | shift), b"\x1b[98;6u");
        assert_eq!(k(KeyCode::Backspace, ctrl), b"\x1b[127;5u");
        assert_eq!(k(KeyCode::Char('m'), ctrl), b"\x1b[109;5u");
        assert_eq!(k(KeyCode::Char('='), ctrl), b"\x1b[61;5u");
        // Other keys with Ctrl or Alt, Esc, and F3.
        assert_eq!(k(KeyCode::Char('c'), ctrl), b"\x1b[99;5u");
        assert_eq!(k(KeyCode::Char('x'), alt), b"\x1b[120;3u");
        assert_eq!(k(KeyCode::Char(' '), ctrl), b"\x1b[32;5u");
        assert_eq!(k(KeyCode::Esc, none), b"\x1b[27u");
        assert_eq!(k(KeyCode::Enter, alt), b"\x1b[13;3u");
        assert_eq!(k(KeyCode::Tab, ctrl), b"\x1b[9;5u");
        assert_eq!(k(KeyCode::BackTab, ctrl | shift), b"\x1b[9;6u");
        assert_eq!(k(KeyCode::F(3), none), b"\x1b[13~");
        assert_eq!(k(KeyCode::F(3), ctrl), b"\x1b[13;5~");
        // Unchanged: text, Enter, Tab and Backspace without modifiers, Shift+Tab, arrows and other F keys.
        assert_eq!(k(KeyCode::Char('a'), none), b"a");
        assert_eq!(k(KeyCode::Char('A'), shift), b"A");
        assert_eq!(k(KeyCode::Char(' '), shift), b" ");
        assert_eq!(k(KeyCode::Enter, none), b"\r");
        assert_eq!(k(KeyCode::Tab, none), b"\t");
        assert_eq!(k(KeyCode::Backspace, none), [0x7f]);
        assert_eq!(k(KeyCode::BackTab, shift), b"\x1b[Z");
        assert_eq!(k(KeyCode::Left, ctrl), b"\x1b[1;5D");
        assert_eq!(k(KeyCode::F(5), none), b"\x1b[15~");
    }

    #[test]
    fn kitty_flags_8_4_and_16() {
        let (none, shift) = (KeyModifiers::NONE, KeyModifiers::SHIFT);
        let all = TermMode::DISAMBIGUATE_ESC_CODES | TermMode::REPORT_ALL_KEYS_AS_ESC;
        assert_eq!(kitty(KeyCode::Char('a'), none, all), b"\x1b[97u");
        assert_eq!(kitty(KeyCode::Char('A'), shift, all), b"\x1b[97;2u");
        assert_eq!(kitty(KeyCode::Enter, none, all), b"\x1b[13u");
        assert_eq!(kitty(KeyCode::BackTab, shift, all), b"\x1b[9;2u");
        let alternates = all | TermMode::REPORT_ALTERNATE_KEYS;
        assert_eq!(kitty(KeyCode::Char('A'), shift, alternates), b"\x1b[97:65;2u");
        let text = all | TermMode::REPORT_ASSOCIATED_TEXT;
        assert_eq!(kitty(KeyCode::Char('a'), none, text), b"\x1b[97;;97u");
        assert_eq!(kitty(KeyCode::Char('A'), shift, text), b"\x1b[97;2;65u");
        assert_eq!(kitty(KeyCode::Char('a'), KeyModifiers::CONTROL, text), b"\x1b[97;5u");
    }

    #[test]
    fn without_kitty_flags_modifiers_on_enter_are_dropped() {
        assert_eq!(key(KeyCode::Enter, KeyModifiers::SHIFT), b"\r");
        assert_eq!(key(KeyCode::Enter, KeyModifiers::CONTROL), b"\r");
        assert_eq!(key(KeyCode::Char('z'), KeyModifiers::CONTROL | KeyModifiers::SHIFT), [0x1a]);
    }

    #[test]
    fn releases_are_ignored() {
        let mut k = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        k.kind = KeyEventKind::Release;
        assert!(encode_key(&k, TermMode::empty()).is_none());
    }

    #[test]
    fn paste_brackets_and_newlines() {
        assert_eq!(encode_paste("a\nb", TermMode::empty()), b"a\rb");
        assert_eq!(encode_paste("x\x1by", TermMode::BRACKETED_PASTE), b"\x1b[200~xy\x1b[201~");
    }

    #[test]
    fn mouse_reports() {
        let ev = |kind| MouseEvent { kind, column: 0, row: 0, modifiers: KeyModifiers::NONE };
        let down = ev(MouseEventKind::Down(MouseButton::Left));
        assert!(encode_mouse(&down, 4, 2, TermMode::empty()).is_none());
        let sgr = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(encode_mouse(&down, 4, 2, sgr).unwrap(), b"\x1b[<0;5;3M");
        let up = ev(MouseEventKind::Up(MouseButton::Left));
        assert_eq!(encode_mouse(&up, 4, 2, sgr).unwrap(), b"\x1b[<0;5;3m");
        assert_eq!(encode_mouse(&up, 4, 2, TermMode::MOUSE_REPORT_CLICK).unwrap(), [0x1b, b'[', b'M', 35, 37, 35]);
        let drag = ev(MouseEventKind::Drag(MouseButton::Left));
        assert!(encode_mouse(&drag, 0, 0, sgr).is_none());
        assert_eq!(encode_mouse(&drag, 0, 0, sgr | TermMode::MOUSE_DRAG).unwrap(), b"\x1b[<32;1;1M");
        let wheel = ev(MouseEventKind::ScrollDown);
        assert_eq!(encode_mouse(&wheel, 0, 0, sgr).unwrap(), b"\x1b[<65;1;1M");
    }
}
