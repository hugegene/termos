//! A small KDL parser: the parts of KDL that zellij's config files use (see
//! `docs/findings/2026-11-05-kdl-parser.md`).
//!
//! Nodes have a name, arguments, `key=value` properties and an optional `{ … }` block of child nodes. A
//! node ends at a newline, `;` or `}`. Comments are `// …`, `/* … */` (nested), and `/-`, which comments
//! out the next node, argument, property or block. Values are strings (`"…"` with escapes, or raw
//! `r"…"`/`r#"…"#`), numbers, `true`/`false`/`null` (also KDL 2's `#true`/`#false`/`#null`) and bare words.
//! Type annotations like `(u8)` are skipped.

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub name: String,
    pub args: Vec<Value>,
    pub props: Vec<(String, Value)>,
    pub children: Vec<Node>,
    /// The line the node starts on, counting from 1.
    pub line: usize,
}

impl Node {
    pub fn prop(&self, key: &str) -> Option<&Value> {
        self.props.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// The first argument of child `name` (`cwd "/tmp";` inside a block).
    pub fn child_arg(&self, name: &str) -> Option<&Value> {
        self.children.iter().find(|c| c.name == name).and_then(|c| c.args.first())
    }
}

struct Parser<'a> {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    _text: &'a str,
}

type Result<T> = std::result::Result<T, String>;

fn is_ident_char(c: char) -> bool {
    !c.is_whitespace() && !"\\/(){}<>;[]=,\"".contains(c)
}

impl<'a> Parser<'a> {
    fn err<T>(&self, msg: impl std::fmt::Display) -> Result<T> {
        Err(format!("line {}: {msg}", self.line))
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    /// Skips spaces and comments on the current line. With `newlines`, also skips line ends.
    fn skip(&mut self, newlines: bool) -> Result<()> {
        loop {
            match self.peek() {
                Some('\n') if newlines => {
                    self.bump();
                }
                Some(c) if c.is_whitespace() && c != '\n' => {
                    self.bump();
                }
                Some('\\') => {
                    // A line continuation: `\` then (spaces and a comment and) a newline.
                    self.bump();
                    self.skip(false)?;
                    if self.peek() == Some('\n') {
                        self.bump();
                    }
                }
                Some('/') if self.peek_at(1) == Some('/') => {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.bump();
                    }
                }
                Some('/') if self.peek_at(1) == Some('*') => {
                    self.bump();
                    self.bump();
                    let mut depth = 1;
                    while depth > 0 {
                        match (self.bump(), self.peek()) {
                            (Some('*'), Some('/')) => {
                                self.bump();
                                depth -= 1;
                            }
                            (Some('/'), Some('*')) => {
                                self.bump();
                                depth += 1;
                            }
                            (Some(_), _) => {}
                            (None, _) => return self.err("a /* comment isn't closed"),
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn string(&mut self) -> Result<String> {
        let start = self.line;
        self.bump(); // the opening quote
        let mut s = String::new();
        loop {
            match self.bump() {
                None => return Err(format!("line {start}: a string isn't closed")),
                Some('"') => return Ok(s),
                Some('\\') => match self.bump() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('b') => s.push('\u{8}'),
                    Some('f') => s.push('\u{c}'),
                    Some('s') => s.push(' '),
                    Some('"') => s.push('"'),
                    Some('\\') => s.push('\\'),
                    Some('/') => s.push('/'),
                    Some('u') => {
                        if self.bump() != Some('{') {
                            return self.err("\\u needs braces, like \\u{1b}");
                        }
                        let mut hex = String::new();
                        while let Some(c) = self.bump() {
                            if c == '}' {
                                break;
                            }
                            hex.push(c);
                        }
                        match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                            Some(c) => s.push(c),
                            None => return self.err(format!("bad \\u{{{hex}}}")),
                        }
                    }
                    Some(c) => return self.err(format!("unknown escape \\{c}")),
                    None => return Err(format!("line {start}: a string isn't closed")),
                },
                Some(c) => s.push(c),
            }
        }
    }

    fn raw_string(&mut self) -> Result<String> {
        self.bump(); // r
        let mut hashes = 0;
        while self.peek() == Some('#') {
            self.bump();
            hashes += 1;
        }
        if self.bump() != Some('"') {
            return self.err("a raw string needs a quote after r");
        }
        let mut s = String::new();
        loop {
            match self.bump() {
                None => return self.err("a raw string isn't closed"),
                Some('"') if (0..hashes).all(|i| self.peek_at(i) == Some('#')) => {
                    for _ in 0..hashes {
                        self.bump();
                    }
                    return Ok(s);
                }
                Some(c) => s.push(c),
            }
        }
    }

    fn word(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek().filter(|c| is_ident_char(*c)) {
            s.push(c);
            self.bump();
        }
        s
    }

    /// Skips a type annotation such as `(u8)`.
    fn annotation(&mut self) -> Result<()> {
        if self.peek() == Some('(') {
            while let Some(c) = self.bump() {
                if c == ')' {
                    return Ok(());
                }
            }
            return self.err("a ( isn't closed");
        }
        Ok(())
    }

    fn value(&mut self) -> Result<Value> {
        self.annotation()?;
        match self.peek() {
            Some('"') => Ok(Value::Str(self.string()?)),
            Some('r') if matches!(self.peek_at(1), Some('"' | '#')) => Ok(Value::Str(self.raw_string()?)),
            Some(_) => {
                let w = self.word();
                if w.is_empty() {
                    return self.err(format!("unexpected '{}'", self.peek().unwrap_or(' ')));
                }
                Ok(word_value(&w))
            }
            None => self.err("a value is missing"),
        }
    }

    /// Nodes until `}` (when `nested`) or the end.
    fn nodes(&mut self, nested: bool) -> Result<Vec<Node>> {
        let mut out = Vec::new();
        loop {
            self.skip(true)?;
            while self.peek() == Some(';') {
                self.bump();
                self.skip(true)?;
            }
            match self.peek() {
                None if nested => return self.err("a { isn't closed"),
                None => return Ok(out),
                Some('}') if nested => {
                    self.bump();
                    return Ok(out);
                }
                Some('}') => return self.err("a } has no matching {"),
                _ => {}
            }
            let discard = self.slashdash()?;
            let node = self.node()?;
            if !discard {
                out.push(node);
            }
        }
    }

    /// Reads `/-` if it's next.
    fn slashdash(&mut self) -> Result<bool> {
        if self.peek() == Some('/') && self.peek_at(1) == Some('-') {
            self.bump();
            self.bump();
            self.skip(true)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn node(&mut self) -> Result<Node> {
        let line = self.line;
        self.annotation()?;
        let name = match self.peek() {
            Some('"') => self.string()?,
            Some('r') if matches!(self.peek_at(1), Some('"' | '#')) => self.raw_string()?,
            _ => self.word(),
        };
        if name.is_empty() {
            return self.err(format!("expected a node name, found '{}'", self.peek().unwrap_or(' ')));
        }
        let mut node = Node { name, args: Vec::new(), props: Vec::new(), children: Vec::new(), line };
        loop {
            self.skip(false)?;
            match self.peek() {
                None | Some('\n' | ';' | '}') => return Ok(node),
                Some('{') => {
                    self.bump();
                    node.children = self.nodes(true)?;
                    return Ok(node);
                }
                _ => {}
            }
            let discard = self.slashdash()?;
            if self.peek() == Some('{') {
                self.bump();
                let children = self.nodes(true)?;
                if !discard {
                    node.children = children;
                    return Ok(node);
                }
                continue;
            }
            // A property is `key=value`, where the key is a word or a string.
            let start = (self.pos, self.line);
            let key = match self.peek() {
                Some('"') => Some(self.string()?),
                Some(c)
                    if (is_ident_char(c) && c != 'r') || (c == 'r' && !matches!(self.peek_at(1), Some('"' | '#'))) =>
                {
                    Some(self.word())
                }
                _ => None,
            };
            if let Some(key) = key.filter(|_| self.peek() == Some('=')) {
                self.bump();
                let v = self.value()?;
                if !discard {
                    node.props.push((key, v));
                }
                continue;
            }
            (self.pos, self.line) = start;
            let v = self.value()?;
            if !discard {
                node.args.push(v);
            }
        }
    }
}

fn word_value(w: &str) -> Value {
    match w {
        "true" | "#true" => return Value::Bool(true),
        "false" | "#false" => return Value::Bool(false),
        "null" | "#null" => return Value::Null,
        _ => {}
    }
    let digits = w.replace('_', "");
    let (sign, body) = match digits.strip_prefix('-') {
        Some(rest) => (-1, rest.to_owned()),
        None => (1, digits.strip_prefix('+').unwrap_or(&digits).to_owned()),
    };
    let radix = [("0x", 16), ("0o", 8), ("0b", 2)].into_iter().find(|(p, _)| body.starts_with(p));
    if let Some((p, r)) = radix {
        if let Ok(n) = i64::from_str_radix(&body[p.len()..], r) {
            return Value::Int(sign * n);
        }
    } else if body.starts_with(|c: char| c.is_ascii_digit()) {
        if let Ok(n) = body.parse::<i64>() {
            return Value::Int(sign * n);
        }
        if let Ok(f) = digits.parse::<f64>() {
            return Value::Float(f);
        }
    }
    Value::Str(w.to_owned())
}

/// Parses a KDL document. Errors name the line.
pub fn parse(text: &str) -> Result<Vec<Node>> {
    let mut p = Parser { chars: text.chars().collect(), pos: 0, line: 1, _text: text };
    p.nodes(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Value {
        Value::Str(v.into())
    }

    #[test]
    fn zellij_keybinds_parse() {
        let doc = r#"
// the user's config starts like this
keybinds clear-defaults=true {
    locked {
        bind "Ctrl g" { SwitchToMode "normal"; }
    }
    pane {
        bind "c" { SwitchToMode "renamepane"; PaneNameInput 0; }
        bind "Alt left" "Alt h" { MoveFocusOrTab "left"; }
    }
    shared_except "locked" {
        bind "Alt o" { LaunchOrFocusPlugin "zellij:about" { floating true; move_to_focused_tab true; }; }
    }
}
theme "catppuccin-latte"
"#;
        let nodes = parse(doc).unwrap();
        assert_eq!(nodes.len(), 2);
        let kb = &nodes[0];
        assert_eq!(kb.name, "keybinds");
        assert_eq!(kb.prop("clear-defaults"), Some(&Value::Bool(true)));
        assert_eq!(kb.line, 3);
        let locked = &kb.children[0];
        assert_eq!(locked.children[0].args, vec![s("Ctrl g")]);
        assert_eq!(locked.children[0].children[0].name, "SwitchToMode");
        let pane = &kb.children[1];
        assert_eq!(pane.children[0].children[1].args, vec![Value::Int(0)]);
        assert_eq!(pane.children[1].args, vec![s("Alt left"), s("Alt h")]);
        let plugin = &kb.children[2].children[0].children[0];
        assert_eq!(plugin.name, "LaunchOrFocusPlugin");
        assert_eq!(plugin.child_arg("floating"), Some(&Value::Bool(true)));
        assert_eq!(nodes[1].args, vec![s("catppuccin-latte")]);
    }

    #[test]
    fn values_comments_and_slashdash() {
        let doc = "a 1 -2 0x1f 1_000 2.5 #true null \"t\\u{1b}x\" r#\"raw \"q\"\"# bare /* c /* nested */ */ k=v\n\
                   /-skipped 1 { x }\n\
                   b /-1 2 /-{ gone } ; c\n";
        let nodes = parse(doc).unwrap();
        assert_eq!(nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(
            nodes[0].args,
            vec![
                Value::Int(1),
                Value::Int(-2),
                Value::Int(31),
                Value::Int(1000),
                Value::Float(2.5),
                Value::Bool(true),
                Value::Null,
                s("t\u{1b}x"),
                s("raw \"q\""),
                s("bare"),
            ]
        );
        assert_eq!(nodes[0].props, vec![("k".to_owned(), s("v"))]);
        assert_eq!(nodes[1].args, vec![Value::Int(2)]);
        assert!(nodes[1].children.is_empty());
    }

    #[test]
    fn errors_name_the_line() {
        assert_eq!(parse("a {\n  b \"open\n").unwrap_err(), "line 2: a string isn't closed");
        assert!(parse("a {\n b\n").unwrap_err().starts_with("line 3"));
        assert!(parse("}").is_err());
    }
}
