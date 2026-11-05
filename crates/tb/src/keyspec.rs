//! Key names as zellij writes them, for example `Ctrl a`, `Alt Shift b`, `F1`, `PageDown`, `Enter`.
//! Used by `tb action send-keys` and by key bindings in the config file.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Parses one key: optional modifiers (`Ctrl`, `Alt`, `Shift`, `Super`) and a key, separated by spaces.
pub fn parse(spec: &str) -> Result<KeyEvent, String> {
    let words: Vec<&str> = spec.split_whitespace().collect();
    let Some((key, mods)) = words.split_last() else { return Err("empty key".into()) };
    let mut modifiers = KeyModifiers::NONE;
    for m in mods {
        modifiers |= match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => KeyModifiers::CONTROL,
            "alt" | "meta" => KeyModifiers::ALT,
            "shift" => KeyModifiers::SHIFT,
            "super" | "cmd" => KeyModifiers::SUPER,
            _ => return Err(format!("unknown modifier '{m}' in '{spec}' (Ctrl, Alt, Shift, Super)")),
        };
    }
    let code = match key.to_ascii_lowercase().as_str() {
        "enter" | "return" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" | "ins" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "space" => KeyCode::Char(' '),
        k if k.len() > 1 && k.starts_with('f') && k[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) => {
            KeyCode::F(k[1..].parse().unwrap_or(1))
        }
        _ => {
            let mut chars = key.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => {
                    // "Shift a" is "A", as a terminal would send it.
                    if modifiers.contains(KeyModifiers::SHIFT) && c.is_ascii_lowercase() {
                        KeyCode::Char(c.to_ascii_uppercase())
                    } else {
                        KeyCode::Char(c)
                    }
                }
                _ => return Err(format!("unknown key '{key}' in '{spec}'")),
            }
        }
    };
    Ok(KeyEvent::new(code, modifiers))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zellij_key_names_parse() {
        assert_eq!(parse("Ctrl a").unwrap(), KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert_eq!(parse("F1").unwrap(), KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert_eq!(
            parse("Alt Shift b").unwrap(),
            KeyEvent::new(KeyCode::Char('B'), KeyModifiers::ALT | KeyModifiers::SHIFT)
        );
        assert_eq!(parse("PageDown").unwrap().code, KeyCode::PageDown);
        assert_eq!(parse("Enter").unwrap().code, KeyCode::Enter);
        assert_eq!(parse("x").unwrap().code, KeyCode::Char('x'));
        assert!(parse("Hyper x").is_err());
        assert!(parse("Ctrl").is_err(), "a modifier needs a key");
        assert!(parse("NoSuchKey").is_err());
        assert!(parse("").is_err());
    }
}
