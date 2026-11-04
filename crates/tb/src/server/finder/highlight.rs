//! Syntax highlighting with syntect, using bat's "ansi" theme so colours follow the user's terminal palette.

use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

pub struct Highlighter {
    syntaxes: SyntaxSet,
    theme: Theme,
}

static HIGHLIGHTER: OnceLock<Highlighter> = OnceLock::new();

impl Highlighter {
    /// The shared highlighter. Loading the syntax definitions takes a moment, so it happens once.
    pub fn get() -> &'static Highlighter {
        HIGHLIGHTER.get_or_init(|| Highlighter {
            syntaxes: two_face::syntax::extra_newlines(),
            theme: two_face::theme::extra().get(two_face::theme::EmbeddedThemeName::Ansi).clone(),
        })
    }

    fn syntax(&self, token: Option<&str>) -> &SyntaxReference {
        token
            .and_then(|t| {
                self.syntaxes
                    .find_syntax_by_token(t)
                    .or_else(|| self.syntaxes.find_syntax_by_extension(&t.to_lowercase()))
            })
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text())
    }

    /// Highlights up to `max_lines` lines of `text`; the rest come back unstyled.
    /// Each line is a list of (style, text) pieces without the line ending.
    pub fn highlight(&self, text: &str, token: Option<&str>, max_lines: usize) -> Vec<Vec<(Style, String)>> {
        let syntax = self.syntax(token);
        let plain = syntax.name == "Plain Text";
        let mut h = HighlightLines::new(syntax, &self.theme);
        let mut out = Vec::new();
        for (i, line) in LinesWithEndings::from(text).enumerate() {
            let bare = line.trim_end_matches(['\n', '\r']).replace('\t', "    ");
            if plain || i >= max_lines {
                out.push(vec![(Style::default(), bare)]);
                continue;
            }
            match h.highlight_line(line, &self.syntaxes) {
                Ok(pieces) => out.push(
                    pieces
                        .into_iter()
                        .map(|(s, t)| (convert(s), t.trim_end_matches(['\n', '\r']).replace('\t', "    ")))
                        .filter(|(_, t)| !t.is_empty())
                        .collect(),
                ),
                Err(_) => out.push(vec![(Style::default(), bare)]),
            }
        }
        out
    }
}

/// bat's ansi theme stores palette colours as RGBA with alpha 0 (red = palette index) or alpha 1
/// (the terminal's default colour).
fn color(c: syntect::highlighting::Color) -> Color {
    match c.a {
        0 => Color::Indexed(c.r),
        1 => Color::Reset,
        _ => Color::Rgb(c.r, c.g, c.b),
    }
}

fn convert(s: syntect::highlighting::Style) -> Style {
    let mut style = Style::default().fg(color(s.foreground));
    if s.font_style.contains(FontStyle::BOLD) {
        style = style.add_modifier(Modifier::BOLD);
    }
    if s.font_style.contains(FontStyle::ITALIC) {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if s.font_style.contains(FontStyle::UNDERLINE) {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    style
}
