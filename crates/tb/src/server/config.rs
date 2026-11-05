//! Key bindings from `config.kdl` (rebuild step 6), in zellij's keybind syntax, so a zellij `bind` block copies
//! across:
//!
//! ```kdl
//! keybinds {
//!     tab {
//!         bind "p" { Run "prbot" "ls"; SwitchToMode "Normal"; }
//!     }
//!     shared_except "locked" {
//!         bind "Alt y" { NewPane "right"; }
//!     }
//! }
//! ```
//!
//! A binding replaces `tb`'s own key in that mode. Actions `tb` doesn't have (plugins, floating panes, …) are
//! skipped with a warning naming the line, and so is a binding that uses one.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::Modal;
use crate::kdl::{self, Node, Value};
use crate::keyspec;
use crate::protocol::{CliAction, Direction, FocusTarget, ScrollHow, SplitDir};

/// What a bound key does.
#[derive(Clone, Debug)]
pub enum Bound {
    Mode(Modal),
    Action(CliAction),
    /// Detach the client that pressed the key.
    Detach,
    /// Ask whether to end the session (as Ctrl+Q does).
    Quit,
    /// Move the focused pane to a new tab of its own, or to the tab on the left or right.
    BreakPane,
    BreakPaneTo {
        right: bool,
    },
    /// Go back to the tab shown before.
    ToggleTab,
    /// The next or previous search match.
    Search {
        down: bool,
    },
    /// Flip a search option: 'c' case, 'w' wrap, 'o' whole word.
    SearchOption(char),
}

#[derive(Default)]
pub struct Bindings {
    binds: Vec<(Modal, KeyEvent, Vec<Bound>)>,
}

/// Keys as `tb` compares them: an upper-case letter counts as Shift plus the letter, and Shift is ignored
/// on other characters (a terminal may or may not report it for `?`).
fn normal_key(k: &KeyEvent) -> (KeyCode, KeyModifiers) {
    let mut mods =
        k.modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::SHIFT);
    let code = match k.code {
        KeyCode::Char(c) if c.is_ascii_uppercase() => {
            mods |= KeyModifiers::SHIFT;
            KeyCode::Char(c.to_ascii_lowercase())
        }
        KeyCode::Char(c) if c.is_ascii_lowercase() => KeyCode::Char(c),
        KeyCode::Char(c) => {
            mods.remove(KeyModifiers::SHIFT);
            KeyCode::Char(c)
        }
        other => other,
    };
    (code, mods)
}

impl Bindings {
    pub fn find(&self, mode: Modal, key: &KeyEvent) -> Option<Vec<Bound>> {
        let key = normal_key(key);
        self.binds.iter().rev().find(|(m, k, _)| *m == mode && normal_key(k) == key).map(|(_, _, a)| a.clone())
    }

    pub fn len(&self) -> usize {
        self.binds.len()
    }
}

/// The bindings, and warnings about what was skipped.
pub struct Loaded {
    pub bindings: Bindings,
    pub warnings: Vec<String>,
}

/// Where the config lives: `$TB_CONFIG`, else `$XDG_CONFIG_HOME/tb/config.kdl`, else `~/.config/tb/config.kdl`.
pub fn path() -> PathBuf {
    if let Some(p) = std::env::var_os("TB_CONFIG") {
        return p.into();
    }
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"),
    };
    base.join("tb/config.kdl")
}

/// Reads the config. A missing file means no bindings; a file that isn't valid KDL is an error.
pub fn load(path: &Path) -> Result<Loaded, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Loaded { bindings: Bindings::default(), warnings: Vec::new() });
        }
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let nodes = kdl::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(from_nodes(&nodes))
}

const ALL_MODES: [Modal; 10] = [
    Modal::Normal,
    Modal::Locked,
    Modal::Pane,
    Modal::Tab,
    Modal::Resize,
    Modal::Move,
    Modal::Scroll,
    Modal::Search,
    Modal::Session,
    Modal::Tmux,
];

fn mode_named(name: &str) -> Option<Modal> {
    Some(match name.to_ascii_lowercase().as_str() {
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
        _ => return None,
    })
}

fn names(node: &Node) -> Vec<String> {
    node.args.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()
}

pub fn from_nodes(nodes: &[Node]) -> Loaded {
    let mut out = Loaded { bindings: Bindings::default(), warnings: Vec::new() };
    for kb in nodes.iter().filter(|n| n.name == "keybinds") {
        if kb.prop("clear-defaults") == Some(&Value::Bool(true)) {
            out.warnings.push(format!(
                "line {}: clear-defaults is ignored: tb keeps its own keys, and bindings replace them one by one",
                kb.line
            ));
        }
        for block in &kb.children {
            let modes: Vec<Modal> = match block.name.as_str() {
                "shared" => ALL_MODES.to_vec(),
                "shared_except" => {
                    let except: Vec<Option<Modal>> = names(block).iter().map(|n| mode_named(n)).collect();
                    ALL_MODES.into_iter().filter(|m| !except.contains(&Some(*m))).collect()
                }
                "shared_among" => names(block).iter().filter_map(|n| mode_named(n)).collect(),
                name => match mode_named(name) {
                    Some(m) => vec![m],
                    None => {
                        out.warnings.push(format!("line {}: skipped mode '{name}': tb has no such mode", block.line));
                        continue;
                    }
                },
            };
            for bind in &block.children {
                if bind.name != "bind" {
                    out.warnings.push(format!("line {}: skipped '{}': only bind is supported", bind.line, bind.name));
                    continue;
                }
                let actions: Result<Vec<Bound>, String> = bind.children.iter().map(action).collect();
                let actions = match actions {
                    Ok(a) => a,
                    Err(e) => {
                        out.warnings.push(format!("line {}: skipped this binding: {e}", bind.line));
                        continue;
                    }
                };
                for key in names(bind) {
                    match keyspec::parse(&key) {
                        Ok(k) => {
                            for m in &modes {
                                out.bindings.binds.push((*m, k, actions.clone()));
                            }
                        }
                        Err(e) => out.warnings.push(format!("line {}: skipped key: {e}", bind.line)),
                    }
                }
            }
        }
    }
    out
}

fn arg_str(node: &Node, i: usize) -> Option<&str> {
    node.args.get(i).and_then(Value::as_str)
}

fn direction(s: &str) -> Result<Direction, String> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "left" => Direction::Left,
        "right" => Direction::Right,
        "up" => Direction::Up,
        "down" => Direction::Down,
        _ => return Err(format!("not a direction: '{s}'")),
    })
}

fn split_dir(s: &str) -> Result<SplitDir, String> {
    if s.eq_ignore_ascii_case("stacked") {
        return Err("stacked panes aren't in tb yet".into());
    }
    match direction(s)? {
        Direction::Right => Ok(SplitDir::Right),
        Direction::Down => Ok(SplitDir::Down),
        _ => Err(format!("panes open right or down, not '{s}'")),
    }
}

fn scroll(how: ScrollHow) -> Bound {
    Bound::Action(CliAction::Scroll { pane: None, how })
}

fn child_bool(node: &Node, name: &str) -> bool {
    node.child_arg(name) == Some(&Value::Bool(true))
}

/// One zellij keybind action, as `tb` does it.
fn action(node: &Node) -> Result<Bound, String> {
    use CliAction as A;
    let dir_arg = |i: usize| arg_str(node, i).map(direction).transpose();
    Ok(match node.name.as_str() {
        "SwitchToMode" => {
            let name = arg_str(node, 0).ok_or("SwitchToMode needs a mode")?;
            Bound::Mode(mode_named(name).ok_or_else(|| format!("tb has no mode '{name}'"))?)
        }
        "Run" => {
            let command = names(node);
            if command.is_empty() {
                return Err("Run needs a command".into());
            }
            for unsupported in ["floating", "in_place", "stacked"] {
                if child_bool(node, unsupported) {
                    return Err(format!("Run's {unsupported} isn't in tb yet"));
                }
            }
            Bound::Action(A::NewPane {
                dir: node.child_arg("direction").and_then(Value::as_str).map(split_dir).transpose()?,
                name: node.child_arg("name").and_then(Value::as_str).map(str::to_owned),
                cwd: node.child_arg("cwd").and_then(Value::as_str).map(PathBuf::from),
                command,
                start_suspended: child_bool(node, "start_suspended"),
            })
        }
        "NewTab" => {
            if node.children.iter().any(|c| c.name == "layout") {
                return Err("NewTab's layout isn't in tb yet".into());
            }
            Bound::Action(A::NewTab {
                name: node.child_arg("name").and_then(Value::as_str).map(str::to_owned),
                cwd: node.child_arg("cwd").and_then(Value::as_str).map(PathBuf::from),
                command: Vec::new(),
                start_suspended: false,
            })
        }
        "NewPane" => Bound::Action(A::NewPane {
            dir: arg_str(node, 0).map(split_dir).transpose()?,
            name: None,
            cwd: None,
            command: Vec::new(),
            start_suspended: false,
        }),
        "CloseFocus" => Bound::Action(A::ClosePane { pane: None }),
        "CloseTab" => Bound::Action(A::CloseTab { tab: None }),
        "MoveFocus" => Bound::Action(A::Focus(FocusTarget::Dir(dir_arg(0)?.ok_or("MoveFocus needs a direction")?))),
        "MoveFocusOrTab" => Bound::Action(A::FocusOrTab(dir_arg(0)?.ok_or("MoveFocusOrTab needs a direction")?)),
        "FocusNextPane" | "SwitchFocus" => Bound::Action(A::Focus(FocusTarget::Next)),
        "FocusPreviousPane" => Bound::Action(A::Focus(FocusTarget::Prev)),
        "GoToTab" => {
            let n = node.args.first().and_then(Value::as_int).ok_or("GoToTab needs a number")?;
            Bound::Action(A::GoToTab(usize::try_from(n).map_err(|_| "GoToTab needs a positive number")?))
        }
        "GoToNextTab" => Bound::Action(A::CycleTab { next: true }),
        "GoToPreviousTab" => Bound::Action(A::CycleTab { next: false }),
        "ToggleFocusFullscreen" => Bound::Action(A::ToggleFullscreen { pane: None }),
        "TogglePaneFrames" => Bound::Action(A::ToggleFrames),
        "ToggleActiveSyncTab" => Bound::Action(A::ToggleSync { tab: None }),
        "Resize" => {
            // "Increase", "Decrease left", "+", "-".
            let spec = arg_str(node, 0).ok_or("Resize needs Increase or Decrease")?;
            let mut words = spec.split_whitespace();
            let grow = match words.next().map(str::to_ascii_lowercase).as_deref() {
                Some("increase" | "+") => true,
                Some("decrease" | "-") => false,
                _ => return Err(format!("Resize needs Increase or Decrease, not '{spec}'")),
            };
            Bound::Action(A::Resize { pane: None, side: words.next().map(direction).transpose()?, grow })
        }
        "MovePane" => Bound::Action(A::MovePane { pane: None, dir: dir_arg(0)? }),
        "MovePaneBackwards" => Bound::Action(A::MovePaneBackwards { pane: None }),
        "MoveTab" => {
            let right = matches!(dir_arg(0)?, Some(Direction::Right));
            Bound::Action(A::MoveTab { tab: None, right })
        }
        "Clear" => Bound::Action(A::Clear { pane: None }),
        "EditScrollback" => Bound::Action(A::EditScrollback { pane: None }),
        "ScrollUp" => scroll(ScrollHow::Up),
        "ScrollDown" => scroll(ScrollHow::Down),
        "PageScrollUp" => scroll(ScrollHow::PageUp),
        "PageScrollDown" => scroll(ScrollHow::PageDown),
        "HalfPageScrollUp" => scroll(ScrollHow::HalfPageUp),
        "HalfPageScrollDown" => scroll(ScrollHow::HalfPageDown),
        "ScrollToTop" => scroll(ScrollHow::Top),
        "ScrollToBottom" => scroll(ScrollHow::Bottom),
        "UndoRenamePane" => Bound::Action(A::UndoRenamePane { pane: None }),
        "UndoRenameTab" => Bound::Action(A::UndoRenameTab { tab: None }),
        "Write" => {
            let bytes: Option<Vec<u8>> =
                node.args.iter().map(|v| v.as_int().and_then(|n| u8::try_from(n).ok())).collect();
            Bound::Action(A::Write { pane: None, bytes: bytes.ok_or("Write takes byte values (0-255)")? })
        }
        "WriteChars" => {
            let text = arg_str(node, 0).ok_or("WriteChars needs text")?;
            Bound::Action(A::Write { pane: None, bytes: text.as_bytes().to_vec() })
        }
        "Detach" => Bound::Detach,
        "Quit" => Bound::Quit,
        "BreakPane" => Bound::BreakPane,
        "BreakPaneLeft" => Bound::BreakPaneTo { right: false },
        "BreakPaneRight" => Bound::BreakPaneTo { right: true },
        "ToggleTab" => Bound::ToggleTab,
        "Search" => Bound::Search { down: !arg_str(node, 0).is_some_and(|d| d.eq_ignore_ascii_case("up")) },
        "SearchToggleOption" => Bound::SearchOption(match arg_str(node, 0).unwrap_or_default() {
            "CaseSensitivity" => 'c',
            "Wrap" => 'w',
            "WholeWord" => 'o',
            other => return Err(format!("unknown search option '{other}'")),
        }),
        other => return Err(format!("tb has no action '{other}' yet")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_str(text: &str) -> Loaded {
        from_nodes(&kdl::parse(text).unwrap())
    }

    #[test]
    fn bindings_follow_zellij_syntax() {
        let l = load_str(
            r#"
keybinds clear-defaults=true {
    tab {
        bind "p" { Run "prbot" "ls" { cwd "/tmp"; direction "Down"; }; SwitchToMode "Normal"; }
    }
    shared_except "locked" "tab" {
        bind "Alt y" "Alt Y" { NewPane "right"; }
    }
    pane {
        bind "w" { ToggleFloatingPanes; SwitchToMode "Normal"; }
    }
    renamepane { bind "Esc" { UndoRenamePane; } }
}
"#,
        );
        let key = |s: &str| keyspec::parse(s).unwrap();
        let p = l.bindings.find(Modal::Tab, &key("p")).expect("tab p");
        assert!(
            matches!(&p[0], Bound::Action(CliAction::NewPane { dir: Some(SplitDir::Down), command, cwd: Some(c), .. })
            if command == &["prbot", "ls"] && c == Path::new("/tmp"))
        );
        assert!(matches!(p[1], Bound::Mode(Modal::Normal)));
        assert!(l.bindings.find(Modal::Pane, &key("p")).is_none(), "only in tab mode");
        // shared_except: every mode but the ones named.
        assert!(l.bindings.find(Modal::Normal, &key("Alt y")).is_some());
        assert!(l.bindings.find(Modal::Locked, &key("Alt y")).is_none());
        assert!(l.bindings.find(Modal::Tab, &key("Alt y")).is_none());
        // An upper-case letter matches Shift plus the letter, however the terminal reports it.
        assert!(l.bindings.find(Modal::Pane, &KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::ALT)).is_some());
        // Unsupported actions and modes are skipped with the line.
        assert!(l.bindings.find(Modal::Pane, &key("w")).is_none());
        let w = l.warnings.join("\n");
        assert!(w.contains("clear-defaults is ignored"), "{w}");
        assert!(w.contains("line 10: skipped this binding: tb has no action 'ToggleFloatingPanes' yet"), "{w}");
        assert!(w.contains("skipped mode 'renamepane'"), "{w}");
    }
}
