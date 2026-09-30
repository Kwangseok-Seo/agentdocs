use std::ops::Range;
use std::sync::LazyLock;

use ratatui::style::{Color, Style};
use syntect::easy::HighlightLines;
use syntect::highlighting::{self, StyleModifier, Theme, ThemeItem, ThemeSettings};
use syntect::parsing::{Scope, SyntaxSet};

/// Every language syntect knows, read the first time a code block asks for
/// one and kept until the program ends.
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_nonewlines);

/// The colours, made once. Every highlighter borrows them for as long as it
/// lives.
static THEME: LazyLock<Theme> = LazyLock::new(theme);

/// Which colour each kind of token takes. The names are TextMate scopes,
/// which every syntax sorts its tokens into; the most specific name that
/// matches a token wins. The colours are the terminal's own, so they follow
/// its palette, light or dark. What no row matches is plain code, in the
/// colour of the text around it.
const COLOURS: [(&str, Color); 8] = [
    ("comment", Color::DarkGray),
    ("string", Color::Green),
    ("constant.numeric, constant.language, constant.character", Color::LightRed),
    ("keyword, storage", Color::Magenta),
    ("entity.name.function, support.function, variable.function", Color::LightBlue),
    (
        "entity.name.type, entity.name.struct, entity.name.enum, entity.name.trait, entity.name.class, support.type, support.class, entity.name.tag, markup.heading",
        Color::Cyan,
    ),
    // The lines of a diff.
    ("markup.inserted", Color::Green),
    ("markup.deleted", Color::Red),
];

/// `COLOURS` as syntect takes them. A syntect colour is red, green, blue
/// and alpha; here red holds a row of `COLOURS`, and plain code a row past
/// its end.
fn theme() -> Theme {
    let row = |row: usize| highlighting::Color { r: row as u8, g: 0, b: 0, a: 0 };
    let scopes = COLOURS
        .iter()
        .enumerate()
        .map(|(i, (scope, _))| ThemeItem {
            scope: scope.parse().expect("the scopes above are well formed"),
            style: StyleModifier { foreground: Some(row(i)), background: None, font_style: None },
        })
        .collect();
    let settings = ThemeSettings { foreground: Some(row(COLOURS.len())), ..ThemeSettings::default() };
    Theme { settings, scopes, ..Theme::default() }
}

/// A highlighter for code written in `language`, or `None` when syntect
/// knows no language by that name — or when the language is Markdown. A
/// block of Markdown is shown as written, and coloured, its headings would
/// look like those of the file around it; so it is left as code of no known
/// language is. Markdown is told by the scope its syntaxes share, which
/// covers every name syntect takes for them.
pub fn for_language(language: &str) -> Option<HighlightLines<'static>> {
    let syntax = SYNTAXES.find_syntax_by_token(language)?;
    if Scope::new("text.html.markdown").is_ok_and(|markdown| markdown.is_prefix_of(syntax.scope)) {
        return None;
    }
    Some(HighlightLines::new(syntax, &THEME))
}

/// Where the colours of `line` change: each piece's bytes and its style.
/// The highlighter carries what a line leaves open — a string, a comment —
/// over to the line after it, so the lines of a block go through it in order.
/// `None` when the syntax cannot read the line.
pub fn line(highlighter: &mut HighlightLines<'static>, line: &str) -> Option<Vec<(Range<usize>, Style)>> {
    let regions = highlighter.highlight_line(line, &SYNTAXES).ok()?;
    let mut at = 0;
    let pieces = regions
        .into_iter()
        .map(|(style, piece)| {
            let range = at..at + piece.len();
            at = range.end;
            let colour = COLOURS.get(usize::from(style.foreground.r)).map(|(_, colour)| *colour);
            (range, Style { fg: colour, ..Style::new() })
        })
        .collect();
    Some(pieces)
}
