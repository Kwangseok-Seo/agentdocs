use std::borrow::Cow;
use std::ops::Range;

use pulldown_cmark::{CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The bullets of a list, then of a list inside it, then of any deeper one.
const BULLETS: [&str; 3] = ["•", "◦", "▪"];

/// `text` drawn as Markdown, in rows no wider than `width`. Headings,
/// paragraphs, lists, quotes, code blocks and rules are drawn; every other
/// block is shown as it is on disk, so nothing in the file goes missing while
/// the rest waits for a later slice. A blank line is kept where the file has
/// one above a block.
///
/// Every `Line` borrows from `text` — the parser hands back slices of it,
/// not copies — so none of them can outlive it.
pub fn render(text: &str, width: u16) -> Vec<Line<'_>> {
    // Nothing fits in no room at all, and cutting a file into rows of one
    // character each would make a row for every character in it.
    if width == 0 {
        return Vec::new();
    }
    // Tables have to be recognised even while they are shown as on disk:
    // unrecognised, one is a paragraph, and its rows run together into one.
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;

    let mut renderer = Renderer {
        text,
        width: usize::from(width),
        lines: Vec::new(),
        current: Vec::new(),
        styles: Vec::new(),
        links: Vec::new(),
        skip: 0,
        lists: Vec::new(),
        containers: Vec::new(),
        code: false,
        gapped: false,
    };
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        renderer.event(event, range);
    }
    renderer.lines
}

struct Renderer<'a> {
    /// The whole file, for the blocks shown as they are on disk.
    text: &'a str,
    /// How wide a row may be.
    width: usize,
    /// The rows finished so far.
    lines: Vec<Line<'a>>,
    /// The line being built, piece by piece, before it is cut into rows.
    current: Vec<Span<'a>>,
    /// The styles in force, innermost last: bold inside a link inside a heading.
    styles: Vec<Style>,
    /// Where each open link points, shown after its text; `None` when the
    /// text already is the address.
    links: Vec<Option<CowStr<'a>>>,
    /// How deep inside a block shown as on disk. Its events are passed over.
    skip: usize,
    /// The lists open here, innermost last: the number the next item takes,
    /// or `None` for bullets.
    lists: Vec<Option<u64>>,
    /// The quotes and list items open here, outermost first. Each puts
    /// something in front of every row inside it.
    containers: Vec<Container>,
    /// Whether the text arriving belongs to a code block.
    code: bool,
    /// Whether the last row is a blank line kept from the file.
    gapped: bool,
}

enum Container {
    Quote,
    /// A list item: its marker — `• `, `2. ` — in front of its first row,
    /// and as many spaces in front of every row after it.
    Item { marker: String, shown: bool },
}

impl<'a> Renderer<'a> {
    fn event(&mut self, event: Event<'a>, range: Range<usize>) {
        if self.skip > 0 {
            match event {
                Event::Start(_) => self.skip += 1,
                Event::End(_) => self.skip -= 1,
                _ => {}
            }
            return;
        }

        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                self.begin(range.start);
                self.styles.push(Style::new().bold().fg(Color::Cyan));
                self.push(format!("{} ", "#".repeat(level as usize)).into());
            }
            Event::Start(Tag::Paragraph) => self.begin(range.start),
            Event::End(TagEnd::Heading(_)) => {
                self.styles.pop();
                self.finish_line();
            }
            Event::End(TagEnd::Paragraph) => self.finish_line(),

            Event::Start(Tag::List(first)) => {
                self.begin(range.start);
                self.lists.push(first);
            }
            Event::End(TagEnd::List(_)) => {
                self.lists.pop();
            }
            Event::Start(Tag::Item) => {
                self.begin(range.start);
                let marker = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        *number += 1;
                        format!("{}. ", *number - 1)
                    }
                    _ => {
                        let depth = self.lists.len().clamp(1, BULLETS.len());
                        format!("{} ", BULLETS[depth - 1])
                    }
                };
                self.containers.push(Container::Item { marker, shown: false });
            }
            Event::End(TagEnd::Item) => {
                // An item with nothing in it still shows its marker.
                let unshown = matches!(self.containers.last(), Some(Container::Item { shown: false, .. }));
                if !self.current.is_empty() || unshown {
                    self.finish_line();
                }
                self.containers.pop();
            }
            Event::TaskListMarker(done) => self.push(if done { "[x] " } else { "[ ] " }.into()),

            Event::Start(Tag::BlockQuote(_)) => {
                self.begin(range.start);
                self.containers.push(Container::Quote);
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.containers.pop();
            }

            Event::Start(Tag::CodeBlock(_)) => {
                self.begin(range.start);
                self.code = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                if !self.current.is_empty() {
                    self.finish_line();
                }
                self.code = false;
            }
            Event::Text(text) if self.code => self.code_text(text),

            Event::Rule => {
                self.begin(range.start);
                let room = self.width.saturating_sub(self.indent());
                let rule = Span::styled("─".repeat(room), Style::new().fg(Color::DarkGray));
                self.emit(vec![rule]);
            }

            Event::Start(Tag::Strong) => self.styles.push(Style::new().bold()),
            Event::Start(Tag::Emphasis) => self.styles.push(Style::new().italic()),
            Event::Start(Tag::Strikethrough) => self.styles.push(Style::new().crossed_out()),
            Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough) => {
                self.styles.pop();
            }

            Event::Start(Tag::Link { link_type, dest_url, .. } | Tag::Image { link_type, dest_url, .. }) => {
                let shown = !matches!(link_type, LinkType::Autolink | LinkType::Email);
                self.links.push(shown.then_some(dest_url));
                self.styles.push(Style::new().underlined());
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                self.styles.pop();
                if let Some(Some(url)) = self.links.pop() {
                    let dim = Style::new().fg(Color::DarkGray);
                    self.current.push(Span::styled(" (", dim));
                    self.current.push(Span::styled(url, dim));
                    self.current.push(Span::styled(")", dim));
                }
            }

            // `<cli>` in a sentence is a placeholder more often than it is
            // HTML, and hiding it would take the word out of the sentence.
            Event::Text(text) | Event::InlineHtml(text) => self.push(text),
            Event::Code(code) => {
                let style = self.style().fg(Color::Yellow);
                self.current.push(Span::styled(code, style));
            }
            Event::SoftBreak => self.push(" ".into()),
            Event::HardBreak => self.finish_line(),

            // Anything else begins a block this slice does not draw.
            other => {
                self.as_on_disk(range);
                if matches!(other, Event::Start(_)) {
                    self.skip = 1;
                }
            }
        }
    }

    /// Every style in force, the innermost winning where they disagree.
    fn style(&self) -> Style {
        self.styles.iter().fold(Style::new(), |all, s| all.patch(*s))
    }

    fn push(&mut self, text: CowStr<'a>) {
        let style = self.style();
        self.current.push(Span::styled(text, style));
    }

    /// Text inside a code block, as the file lays it out: every line break
    /// ends a row, and nothing is joined. A line may arrive in pieces, and a
    /// piece may begin with the break that ends the line before it. The
    /// parser has already taken the carriage returns out.
    fn code_text(&mut self, text: CowStr<'a>) {
        let style = Style::new().fg(Color::Yellow);
        let text: Cow<'a, str> = text.into();
        let mut at = 0;
        while at < text.len() {
            let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
            let line = part(&text, at..end);
            // A tab is a control character, which the screen drops: Go's
            // indentation would vanish with it.
            let line = if line.contains('\t') { Cow::Owned(line.replace('\t', "    ")) } else { line };
            if !line.is_empty() {
                self.current.push(Span::styled(line, style));
            }
            if end < text.len() {
                self.finish_line();
            }
            at = end + 1;
        }
    }

    /// The line built so far, cut into rows.
    fn finish_line(&mut self) {
        let spans = std::mem::take(&mut self.current);
        self.emit(spans);
    }

    /// `spans` cut into rows, each behind what the open quotes and list
    /// items put in front of it.
    fn emit(&mut self, spans: Vec<Span<'a>>) {
        let (first, rest) = self.prefix();
        self.lines.extend(wrap(first, rest, spans, self.width));
        for container in &mut self.containers {
            if let Container::Item { shown, .. } = container {
                *shown = true;
            }
        }
        self.gapped = false;
    }

    /// What goes in front of the next row, and in front of the rows it
    /// wraps onto.
    fn prefix(&self) -> (Vec<Span<'a>>, Vec<Span<'a>>) {
        let (mut first, mut rest) = (Vec::new(), Vec::new());
        for container in &self.containers {
            match container {
                Container::Quote => {
                    let bar = Span::styled("│ ", Style::new().fg(Color::DarkGray));
                    first.push(bar.clone());
                    rest.push(bar);
                }
                Container::Item { marker, shown } => {
                    let pad = " ".repeat(marker.width());
                    first.push(Span::raw(if *shown { pad.clone() } else { marker.clone() }));
                    rest.push(Span::raw(pad));
                }
            }
        }
        (first, rest)
    }

    /// How wide the prefix is: the same for every row, a marker and the
    /// spaces after it being equally wide.
    fn indent(&self) -> usize {
        self.prefix().1.iter().map(Span::width).sum()
    }

    /// A block begins at `start`. The line before it ends first — a list
    /// item's text, when a list is nested in the item.
    fn begin(&mut self, start: usize) {
        if !self.current.is_empty() {
            self.finish_line();
        }
        self.gap(start);
    }

    /// A blank line, if the file has one just above `start`, and one only.
    /// Not worked out from where the last block ended: the parser ends some
    /// blocks after their line break, some before it, and a list after the
    /// blank line that follows it.
    fn gap(&mut self, start: usize) {
        if !self.lines.is_empty() && !self.gapped && blank_above(self.text, start) {
            let (_, rest) = self.prefix();
            self.lines.push(Line::from(rest));
            self.gapped = true;
        }
    }

    /// The block at `range`, line for line as the file has it.
    fn as_on_disk(&mut self, range: Range<usize>) {
        self.begin(range.start);
        for line in self.text[range].trim_end().lines() {
            self.emit(vec![Span::raw(line)]);
        }
    }
}

/// Whether the line above the one that `start` is on is blank. Not on the
/// first line, which has none above it. Inside a quote a line holding only
/// `>` is its blank line.
fn blank_above(text: &str, start: usize) -> bool {
    let this_line = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let Some(above) = text[..this_line].strip_suffix('\n') else { return false };
    // Whitespace includes a carriage return.
    above.rsplit('\n').next().is_some_and(|line| line.trim_matches(|c: char| c == '>' || c.is_whitespace()).is_empty())
}

/// `spans` laid out in rows no wider than `width`, `first` in front of the
/// first row and `rest` in front of every row after it. A word that does not
/// fit moves to the next row whole; one wider than a whole row is cut where
/// the row ends. A space that does not fit ends its row and is dropped.
fn wrap<'a>(first: Vec<Span<'a>>, rest: Vec<Span<'a>>, spans: Vec<Span<'a>>, width: usize) -> Vec<Line<'a>> {
    let indent: usize = rest.iter().map(Span::width).sum();
    let mut rows = Vec::new();
    let mut used: usize = first.iter().map(Span::width).sum();
    let mut row = first;
    // Whether the row holds anything yet besides what is in front of it.
    let mut empty = true;

    for mut piece in spans.into_iter().flat_map(words) {
        if used + piece.width() > width {
            let blank = piece.content.trim().is_empty();
            if !empty || blank {
                rows.push(Line::from(std::mem::replace(&mut row, rest.clone())));
                used = indent;
                empty = true;
            }
            if blank {
                continue;
            }
            while !piece.content.is_empty() && used + piece.width() > width {
                let (head, tail) = cut(piece, width.saturating_sub(used));
                row.push(head);
                rows.push(Line::from(std::mem::replace(&mut row, rest.clone())));
                used = indent;
                piece = tail;
            }
            if piece.content.is_empty() {
                continue;
            }
        }
        used += piece.width();
        row.push(piece);
        empty = false;
    }
    rows.push(Line::from(row));
    rows
}

/// A span cut into its words and the runs of spaces between them, each
/// keeping the span's style.
fn words(span: Span<'_>) -> Vec<Span<'_>> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut last_was_space = None;
    for (i, c) in span.content.char_indices() {
        let space = c.is_whitespace();
        if last_was_space.is_some_and(|last| last != space) {
            pieces.push(Span::styled(part(&span.content, start..i), span.style));
            start = i;
        }
        last_was_space = Some(space);
    }
    if start < span.content.len() {
        pieces.push(Span::styled(part(&span.content, start..span.content.len()), span.style));
    }
    pieces
}

/// `piece` cut after as many characters as fit in `room` — at least one,
/// so that every row takes something.
fn cut(piece: Span<'_>, room: usize) -> (Span<'_>, Span<'_>) {
    let mut used = 0;
    let mut at = piece.content.len();
    for (i, c) in piece.content.char_indices() {
        // A control character counts as `width` counts it for a whole string.
        let w = c.width().unwrap_or(1);
        if i > 0 && used + w > room {
            at = i;
            break;
        }
        used += w;
    }
    let head = Span::styled(part(&piece.content, 0..at), piece.style);
    let tail = Span::styled(part(&piece.content, at..piece.content.len()), piece.style);
    (head, tail)
}

/// The bytes `range` of a piece of text — still borrowed from the file when
/// the text was, and a copy when it was not.
fn part<'a>(text: &Cow<'a, str>, range: Range<usize>) -> Cow<'a, str> {
    match text {
        Cow::Borrowed(s) => Cow::Borrowed(&s[range]),
        Cow::Owned(s) => Cow::Owned(s[range].to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    /// What each row says, styles aside, at a width nothing here reaches.
    /// Spaces at the end of a row are left out: they cannot be seen, and a
    /// drag does not copy them.
    fn plain(text: &str) -> Vec<String> {
        narrow(text, 80)
    }

    fn narrow(text: &str, width: u16) -> Vec<String> {
        render(text, width).iter().map(|line| line.to_string().trim_end().to_string()).collect()
    }

    /// The style of the first piece of a rendered row that says `piece`.
    fn style_of(text: &str, piece: &str) -> Style {
        render(text, 80)
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content == piece)
            .unwrap_or_else(|| panic!("no piece {piece:?} in {:?}", plain(text)))
            .style
    }

    // --------------------------------------------------------------- blocks

    #[test]
    fn nothing_renders_to_nothing() {
        assert!(render("", 80).is_empty());
        assert!(render("\n\n  \n", 80).is_empty());
        assert!(render("# a heading", 0).is_empty());
    }

    #[test]
    fn a_heading_keeps_its_marks_and_is_bold() {
        assert_eq!(plain("## Why"), ["## Why"]);
        assert!(style_of("## Why", "Why").add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn a_paragraph_is_one_line_for_the_preview_to_wrap() {
        assert_eq!(plain("one\ntwo\nthree"), ["one two three"]);
    }

    #[test]
    fn a_backslash_or_two_spaces_end_a_line_where_they_stand() {
        assert_eq!(plain("one\\\ntwo"), ["one", "two"]);
        assert_eq!(plain("one  \ntwo"), ["one", "two"]);
    }

    #[test]
    fn a_blank_line_is_kept_where_the_file_has_one() {
        assert_eq!(plain("# T\nbody\n\nnext"), ["# T", "body", "", "next"]);
    }

    #[test]
    fn several_blank_lines_are_one() {
        assert_eq!(plain("a\n\n\n\nb"), ["a", "", "b"]);
    }

    #[test]
    fn carriage_returns_change_nothing() {
        assert_eq!(plain("# T\r\n\r\npara\r\nnext\r\n"), ["# T", "", "para next"]);
    }

    #[test]
    fn blocks_not_drawn_yet_are_shown_as_on_disk() {
        let text = "| a | b |\n|---|---|\n| 1 | 2 |\n\n<details>\n<summary>more</summary>\n</details>\n";
        assert_eq!(
            plain(text),
            ["| a | b |", "|---|---|", "| 1 | 2 |", "", "<details>", "<summary>more</summary>", "</details>"]
        );
    }

    #[test]
    fn frontmatter_is_shown_as_on_disk() {
        assert_eq!(plain("---\nname: a\n---\n\n# T\n"), ["---", "name: a", "---", "", "# T"]);
    }

    // ---------------------------------------------------------------- lists

    #[test]
    fn a_list_gets_bullets() {
        assert_eq!(plain("- a\n- b"), ["• a", "• b"]);
    }

    #[test]
    fn a_list_right_under_a_paragraph_gets_no_blank_line() {
        assert_eq!(plain("Before:\n- one\n- two"), ["Before:", "• one", "• two"]);
    }

    #[test]
    fn nested_lists_step_in_and_change_their_bullets() {
        assert_eq!(plain("- a\n  - b\n    - c\n      - d"), ["• a", "  ◦ b", "    ▪ c", "      ▪ d"]);
    }

    #[test]
    fn an_ordered_list_counts_from_its_first_number() {
        assert_eq!(plain("3. x\n4. y"), ["3. x", "4. y"]);
        // The numbers the file wrote after the first do not matter.
        assert_eq!(plain("1. a\n1. b\n1. c"), ["1. a", "2. b", "3. c"]);
    }

    #[test]
    fn a_task_shows_whether_it_is_done() {
        assert_eq!(plain("- [ ] todo\n- [x] done"), ["• [ ] todo", "• [x] done"]);
    }

    #[test]
    fn a_loose_list_keeps_its_blank_lines() {
        assert_eq!(plain("- a\n\n- b"), ["• a", "", "• b"]);
    }

    #[test]
    fn an_item_with_nothing_in_it_still_shows_its_marker() {
        assert_eq!(plain("-\n- b"), ["•", "• b"]);
    }

    #[test]
    fn a_wrapped_item_lines_up_under_its_text() {
        assert_eq!(narrow("- one two three", 9), ["• one two", "  three"]);
        assert_eq!(narrow("10. aaa bbb", 8), ["10. aaa", "    bbb"]);
    }

    // --------------------------------------------------------------- quotes

    #[test]
    fn a_quote_has_a_bar_on_every_row() {
        assert_eq!(plain("> quoted"), ["│ quoted"]);
        assert_eq!(narrow("> one two three", 9), ["│ one two", "│ three"]);
    }

    #[test]
    fn a_blank_line_inside_a_quote_keeps_the_bar() {
        assert_eq!(plain("> a\n>\n> b"), ["│ a", "│", "│ b"]);
    }

    // ---------------------------------------------------------------- code

    #[test]
    fn a_code_block_keeps_its_lines_and_their_indentation() {
        let text = "```\nfn main() {\n    x\n\n}\n```";
        assert_eq!(plain(text), ["fn main() {", "    x", "", "}"]);
        assert_eq!(style_of(text, "x").fg, Some(Color::Yellow));
    }

    #[test]
    fn a_code_block_loses_its_carriage_returns() {
        let text = "```\r\na\r\nb\r\n```\r\n";
        assert_eq!(plain(text), ["a", "b"]);
        // `plain` trims the ends of rows, where a carriage return would be.
        let rows = render(text, 80);
        assert!(rows.iter().flat_map(|row| &row.spans).all(|span| !span.content.contains('\r')), "{rows:?}");
    }

    #[test]
    fn a_tab_in_code_becomes_four_spaces() {
        assert_eq!(plain("```go\n\tx\n```"), ["    x"]);
    }

    #[test]
    fn a_code_block_in_a_list_item_stays_in_the_item() {
        assert_eq!(plain("- item\n\n  ```\n  code\n  ```"), ["• item", "", "  code"]);
    }

    #[test]
    fn a_rule_runs_across_the_row() {
        assert_eq!(narrow("a\n\n---\n\nb", 5), ["a", "", "─────", "", "b"]);
        // Inside a quote, it stops where the row does.
        assert_eq!(narrow("> ---", 6), ["│ ────"]);
    }

    // --------------------------------------------------------------- inline

    #[test]
    fn a_placeholder_in_angle_brackets_stays_in_the_sentence() {
        // Seen in the corpus: `<cli>`, `<url>`, `<api>` outside code.
        assert_eq!(plain("run <cli> now"), ["run <cli> now"]);
    }

    #[test]
    fn inline_marks_become_styles() {
        let text = "**b** *i* ~~s~~ `c`";
        assert_eq!(plain(text), ["b i s c"]);
        assert!(style_of(text, "b").add_modifier.contains(Modifier::BOLD));
        assert!(style_of(text, "i").add_modifier.contains(Modifier::ITALIC));
        assert!(style_of(text, "s").add_modifier.contains(Modifier::CROSSED_OUT));
        assert_eq!(style_of(text, "c").fg, Some(Color::Yellow));
    }

    #[test]
    fn styles_inside_styles_add_up() {
        let style = style_of("**bold *both***", "both");
        assert!(style.add_modifier.contains(Modifier::BOLD | Modifier::ITALIC));
        let style = style_of("**bold `code`**", "code");
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(style.fg, Some(Color::Yellow));
        // And end where their marks do.
        assert!(!style_of("**b** after", "after").add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn a_link_says_where_it_points() {
        let text = "see [ADR-0001](docs/adr/0001.md)";
        assert_eq!(plain(text), ["see ADR-0001 (docs/adr/0001.md)"]);
        assert!(style_of(text, "ADR-0001").add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn an_address_written_as_a_link_is_not_said_twice() {
        assert_eq!(plain("<https://example.com>"), ["https://example.com"]);
    }

    // ------------------------------------------------------------- wrapping

    #[test]
    fn words_move_to_the_next_row_whole() {
        assert_eq!(narrow("one two three", 7), ["one two", "three"]);
    }

    #[test]
    fn a_word_wider_than_a_row_is_cut_where_the_row_ends() {
        assert_eq!(narrow("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    }

    #[test]
    fn no_row_is_wider_than_the_width() {
        // The case ratatui's own wrapping got wrong: a word ending in a wide
        // character, reaching one cell past the edge.
        for text in ["12345 789한 글", "123456789한글", "- 한국어 목록 항목이 길게 이어진다", "> 인용 안의 긴 한국어 문장이다"] {
            // Below four cells, a bullet and one wide character cannot fit
            // side by side at all.
            for width in 4..=12 {
                for line in render(text, width) {
                    assert!(line.width() <= usize::from(width), "{text:?} at {width}: {line:?}");
                }
            }
        }
        assert_eq!(narrow("12345 789한 글", 10), ["12345", "789한 글"]);
    }

    #[test]
    fn a_wide_character_is_never_split_across_rows() {
        assert_eq!(narrow("123456789한글", 10), ["123456789", "한글"]);
    }

    #[test]
    fn a_cut_piece_stays_borrowed_from_the_file() {
        let text = String::from("abcdefghij");
        for line in render(&text, 4) {
            for span in &line.spans {
                assert!(matches!(span.content, Cow::Borrowed(_)), "{span:?}");
            }
        }
    }

    // ----------------------------------------------------------- blank_above

    #[test]
    fn the_first_line_has_no_blank_line_above_it() {
        assert!(!blank_above("# T", 0));
    }

    #[test]
    fn a_blank_or_whitespace_line_above_counts() {
        assert!(blank_above("a\n\nb", 3));
        assert!(blank_above("a\n  \t\nb", 6));
        assert!(!blank_above("a\nb", 2));
    }

    #[test]
    fn a_carriage_return_above_is_still_a_blank_line() {
        assert!(blank_above("a\r\n\r\nb", 5));
        assert!(!blank_above("a\r\nb", 3));
    }

    #[test]
    fn a_start_part_way_along_its_line_looks_above_that_line() {
        // A block indented by two spaces begins at byte 5, not 3.
        assert!(blank_above("a\n\n  - b", 5));
        assert!(!blank_above("a\n  - b", 4));
    }
}
