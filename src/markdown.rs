use std::ops::Range;

use pulldown_cmark::{CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// `text` drawn as Markdown: one `Line` per line, before the preview wraps
/// them. Headings and paragraphs are drawn; every other block is shown as it
/// is on disk, so nothing in the file goes missing while the rest waits for a
/// later slice. A blank line is kept where the file has one above a block.
///
/// Every `Line` borrows from `text` — the parser hands back slices of it,
/// not copies — so none of them can outlive it.
pub fn render(text: &str) -> Vec<Line<'_>> {
    // Tables have to be recognised even while they are shown as on disk:
    // unrecognised, one is a paragraph, and its rows run together into one.
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;

    let mut renderer = Renderer {
        text,
        lines: Vec::new(),
        current: Vec::new(),
        styles: Vec::new(),
        links: Vec::new(),
        skip: 0,
    };
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        renderer.event(event, range);
    }
    renderer.lines
}

struct Renderer<'a> {
    /// The whole file, for the blocks shown as they are on disk.
    text: &'a str,
    /// The lines finished so far.
    lines: Vec<Line<'a>>,
    /// The line being built, piece by piece.
    current: Vec<Span<'a>>,
    /// The styles in force, innermost last: bold inside a link inside a heading.
    styles: Vec<Style>,
    /// Where each open link points, shown after its text; `None` when the
    /// text already is the address.
    links: Vec<Option<CowStr<'a>>>,
    /// How deep inside a block shown as on disk. Its events are passed over.
    skip: usize,
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
                self.gap(range.start);
                self.styles.push(Style::new().bold().fg(Color::Cyan));
                self.push(format!("{} ", "#".repeat(level as usize)).into());
            }
            Event::Start(Tag::Paragraph) => self.gap(range.start),
            Event::End(TagEnd::Heading(_)) => {
                self.styles.pop();
                self.finish_line();
            }
            Event::End(TagEnd::Paragraph) => self.finish_line(),

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

    fn finish_line(&mut self) {
        let spans = std::mem::take(&mut self.current);
        self.lines.push(Line::from(spans));
    }

    /// Before a block: a blank line, if the file has one just above it. Not
    /// worked out from where the last block ended: the parser ends some
    /// blocks after their line break, some before it, and a list after the
    /// blank line that follows it.
    fn gap(&mut self, start: usize) {
        if !self.lines.is_empty() && blank_above(self.text, start) {
            self.lines.push(Line::default());
        }
    }

    /// The block at `range`, line for line as the file has it.
    fn as_on_disk(&mut self, range: Range<usize>) {
        self.gap(range.start);
        for line in self.text[range].trim_end().lines() {
            self.lines.push(Line::raw(line));
        }
    }
}

/// Whether the line above the one that `start` is on is blank. Not on the
/// first line, which has none above it.
fn blank_above(text: &str, start: usize) -> bool {
    let this_line = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let Some(above) = text[..this_line].strip_suffix('\n') else { return false };
    // `trim` takes a carriage return with the rest of the whitespace.
    above.rsplit('\n').next().is_some_and(|line| line.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    /// What each line says, styles aside.
    fn plain(text: &str) -> Vec<String> {
        render(text).iter().map(|line| line.to_string()).collect()
    }

    /// The style of the first piece of a rendered line that says `piece`.
    fn style_of(text: &str, piece: &str) -> Style {
        render(text)
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content == piece)
            .unwrap_or_else(|| panic!("no piece {piece:?} in {:?}", plain(text)))
            .style
    }

    // --------------------------------------------------------------- blocks

    #[test]
    fn nothing_renders_to_nothing() {
        assert!(render("").is_empty());
        assert!(render("\n\n  \n").is_empty());
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
        let text = "- a\n- b\n\n```\ncode\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n> quoted\n\n---\n";
        assert_eq!(
            plain(text),
            ["- a", "- b", "", "```", "code", "```", "", "| a | b |", "|---|---|", "| 1 | 2 |", "", "> quoted", "", "---"]
        );
    }

    #[test]
    fn frontmatter_is_shown_as_on_disk() {
        assert_eq!(plain("---\nname: a\n---\n\n# T\n"), ["---", "name: a", "---", "", "# T"]);
    }

    #[test]
    fn a_block_right_under_a_paragraph_gets_no_blank_line() {
        assert_eq!(plain("Before:\n- one\n- two"), ["Before:", "- one", "- two"]);
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
        assert!(!style_of("**b** after", " after").add_modifier.contains(Modifier::BOLD));
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
