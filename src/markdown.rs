use std::borrow::Cow;
use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::highlight;

/// The bullets of a list, then of a list inside it, then of any deeper one.
const BULLETS: [&str; 3] = ["•", "◦", "▪"];

/// What stands between two cells of a table drawn as a grid.
const BETWEEN: &str = " │ ";

/// `text` drawn as Markdown, in rows no wider than `width`. HTML is shown as
/// it is on disk; everything else is drawn. A blank line is kept where the
/// file has one above a block.
///
/// Every `Line` borrows from `text` — the parser hands back slices of it,
/// not copies — so none of them can outlive it.
pub fn render(text: &str, width: u16) -> Vec<Line<'_>> {
    // Nothing fits in no room at all, and cutting a file into rows of one
    // character each would make a row for every character in it.
    if width == 0 {
        return Vec::new();
    }
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
        code: None,
        gapped: false,
        table: None,
    };
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        renderer.event(event, range);
    }
    renderer.lines
}

/// A note of the screen's own, cut into rows no wider than `width` between
/// its words, as a file's lines are. It is not Markdown: nothing in it is
/// drawn as such.
pub fn note<'a>(text: impl Into<Cow<'a, str>>, width: u16) -> Vec<Line<'a>> {
    wrap(Vec::new(), Vec::new(), vec![Span::raw(text)], usize::from(width))
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
    /// The code block the text arriving belongs to, if it does.
    code: Option<Code<'a>>,
    /// Whether the last row is a blank line kept from the file.
    gapped: bool,
    /// The table being read, drawn once all of it is known.
    table: Option<Table<'a>>,
}

enum Container {
    Quote,
    /// A list item: its marker — `• `, `2. ` — in front of its first row,
    /// and as many spaces in front of every row after it.
    Item { marker: String, shown: bool },
    /// Nothing in front of the first row, two spaces in front of the rest:
    /// a line of a table's block or of the frontmatter, wrapped.
    Hang,
}

/// A code block as it is read.
struct Code<'a> {
    /// What colours its lines, or `None` when its language is not known:
    /// then the whole block is plain code.
    highlighter: Option<HighlightLines<'static>>,
    /// The line read so far. A line comes whole from the parser nearly always,
    /// and is then still borrowed from the file; one that comes in pieces is
    /// copied as the pieces are joined.
    line: Cow<'a, str>,
}

/// A table as it is read: how each column is aligned, and its rows — the
/// head first — each a list of cells, each cell the pieces of its text.
struct Table<'a> {
    alignments: Vec<Alignment>,
    rows: Vec<Vec<Vec<Span<'a>>>>,
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

            Event::Start(Tag::CodeBlock(kind)) => {
                self.begin(range.start);
                // The language is the first word after the fence.
                let highlighter = match kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().and_then(highlight::for_language),
                    CodeBlockKind::Indented => None,
                };
                self.code = Some(Code { highlighter, line: Cow::Borrowed("") });
            }
            Event::End(TagEnd::CodeBlock) => {
                // The last line has no break after it to end it.
                if self.code.as_ref().is_some_and(|code| !code.line.is_empty()) {
                    self.end_code_line();
                }
                self.code = None;
            }
            Event::Text(text) if self.code.is_some() => self.code_text(text),

            Event::Start(Tag::Table(alignments)) => {
                self.begin(range.start);
                self.table = Some(Table { alignments, rows: Vec::new() });
            }
            Event::Start(Tag::TableHead | Tag::TableRow) => {
                if let Some(table) = &mut self.table {
                    table.rows.push(Vec::new());
                }
            }
            // A cell's text is gathered as a paragraph's is, then moved in.
            Event::End(TagEnd::TableCell) => {
                let cell = std::mem::take(&mut self.current);
                if let Some(row) = self.table.as_mut().and_then(|table| table.rows.last_mut()) {
                    row.push(cell);
                }
            }
            Event::Start(Tag::TableCell) | Event::End(TagEnd::TableHead | TagEnd::TableRow) => {}
            Event::End(TagEnd::Table) => {
                if let Some(table) = self.table.take() {
                    if cut_short(&self.text[range.clone()]) {
                        self.as_on_disk(range);
                    } else {
                        self.draw_table(table);
                    }
                }
            }

            Event::Start(Tag::MetadataBlock(_)) => {
                self.frontmatter(range);
                self.skip = 1;
            }

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
    /// parser has already taken the carriage returns out — and in doing so
    /// hands each break over after its line, on its own, where nothing comes
    /// before it to join.
    fn code_text(&mut self, text: CowStr<'a>) {
        let text: Cow<'a, str> = text.into();
        let mut at = 0;
        while at < text.len() {
            let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
            let Some(code) = &mut self.code else { return };
            let piece = part(&text, at..end);
            if code.line.is_empty() {
                code.line = piece;
            } else if !piece.is_empty() {
                code.line.to_mut().push_str(&piece);
            }
            if end < text.len() {
                self.end_code_line();
            }
            at = end + 1;
        }
    }

    /// The line of code read so far, coloured, as a row of its own.
    fn end_code_line(&mut self) {
        let Some(code) = &mut self.code else { return };
        let line = std::mem::take(&mut code.line);
        // A tab is a control character, which the screen drops: Go's
        // indentation would vanish with it.
        let line = if line.contains('\t') { Cow::Owned(line.replace('\t', "    ")) } else { line };
        match code.highlighter.as_mut().and_then(|highlighter| highlight::line(highlighter, &line)) {
            Some(pieces) => {
                for (range, style) in pieces {
                    self.current.push(Span::styled(part(&line, range), style));
                }
            }
            None if !line.is_empty() => self.current.push(Span::styled(line, Style::new().fg(Color::Yellow))),
            None => {}
        }
        self.finish_line();
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
                Container::Hang => rest.push(Span::raw("  ")),
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
            self.blank_row();
        }
    }

    /// A blank row, still behind the bars of the quotes it is in.
    fn blank_row(&mut self) {
        let (_, rest) = self.prefix();
        self.lines.push(Line::from(rest));
        self.gapped = true;
    }

    /// `spans` cut into rows, the rows after the first stepped in by two.
    fn hanging(&mut self, spans: Vec<Span<'a>>) {
        self.containers.push(Container::Hang);
        self.emit(spans);
        self.containers.pop();
    }

    /// The block at `range`, line for line as the file has it.
    fn as_on_disk(&mut self, range: Range<usize>) {
        self.begin(range.start);
        for line in self.text[range].trim_end().lines() {
            self.emit(vec![Span::raw(line)]);
        }
    }

    /// A table as a grid when all of it fits in a row, or else one block per
    /// row, each cell behind its column's heading. A table with nothing
    /// below its head is a grid whatever its width: as blocks, it would show
    /// nothing at all.
    fn draw_table(&mut self, table: Table<'a>) {
        let Table { alignments, rows } = table;
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let widths: Vec<usize> = (0..columns)
            .map(|c| rows.iter().map(|row| row.get(c).map_or(0, |cell| cell_width(cell))).max().unwrap_or(0))
            .collect();
        let whole = widths.iter().sum::<usize>() + BETWEEN.width() * columns.saturating_sub(1);

        if whole <= self.width.saturating_sub(self.indent()) || rows.len() < 2 {
            self.grid(rows, &widths, &alignments);
        } else {
            self.blocks(rows);
        }
    }

    /// Every cell padded to its column's width, as its column is aligned,
    /// the head in bold and a line under it.
    fn grid(&mut self, rows: Vec<Vec<Vec<Span<'a>>>>, widths: &[usize], alignments: &[Alignment]) {
        let lines = Style::new().fg(Color::DarkGray);
        for (r, row) in rows.into_iter().enumerate() {
            let last = row.len().saturating_sub(1);
            let mut spans = Vec::new();
            for (c, cell) in row.into_iter().enumerate() {
                if c > 0 {
                    spans.push(Span::styled(BETWEEN, lines));
                }
                let room = widths[c] - cell_width(&cell);
                let (before, after) = match alignments.get(c) {
                    Some(Alignment::Right) => (room, 0),
                    Some(Alignment::Center) => (room / 2, room - room / 2),
                    _ => (0, room),
                };
                spans.push(Span::raw(" ".repeat(before)));
                let head = if r == 0 { Style::new().bold() } else { Style::new() };
                spans.extend(cell.into_iter().map(|span| span.patch_style(head)));
                // Nothing pads the last cell out: it would only be trailing blanks.
                if c < last {
                    spans.push(Span::raw(" ".repeat(after)));
                }
            }
            self.emit(spans);
            if r == 0 {
                let rule: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
                self.emit(vec![Span::styled(rule.join("─┼─"), lines)]);
            }
        }
    }

    /// Each row below the head as a block of its own, one line per cell:
    /// the column's heading, then the cell.
    fn blocks(&mut self, mut rows: Vec<Vec<Vec<Span<'a>>>>) {
        let head = rows.remove(0);
        let label = Style::new().fg(Color::Cyan);
        for (r, row) in rows.into_iter().enumerate() {
            if r > 0 {
                self.blank_row();
            }
            for (c, cell) in row.into_iter().enumerate() {
                // A heading is repeated in every block. Cloning a piece still
                // borrowed from the file copies where it points, not its text.
                let mut spans: Vec<Span<'a>> = match head.get(c) {
                    Some(heading) => heading.iter().map(|span| Span::styled(span.content.clone(), span.style.patch(label))).collect(),
                    None => Vec::new(),
                };
                // A column headed by nothing, such as a column of row labels,
                // shows its cells alone rather than behind a bare ": ".
                if cell_width(&spans) > 0 {
                    spans.push(Span::styled(": ", label));
                }
                spans.extend(cell);
                self.hanging(spans);
            }
        }
    }

    /// The frontmatter at `range`, without the lines that fence it, each key
    /// in the colour of a table's headings. Read from the file rather than
    /// from the parser's pieces, which it cuts differently for `\r\n`.
    fn frontmatter(&mut self, range: Range<usize>) {
        self.begin(range.start);
        let text = self.text;
        let mut lines: Vec<&'a str> = text[range].lines().skip(1).collect();
        if lines.last().is_some_and(|line| matches!(line.trim(), "---" | "...")) {
            lines.pop();
        }
        let label = Style::new().fg(Color::Cyan);
        for line in lines {
            let spans = match line.find(':') {
                // A key starts its line; an indented line continues a value.
                Some(colon) if !line.starts_with(char::is_whitespace) => {
                    vec![Span::styled(&line[..=colon], label), Span::raw(&line[colon + 1..])]
                }
                _ => vec![Span::raw(line)],
            };
            self.hanging(spans);
        }
    }
}

/// How many cells a table cell's text takes up.
fn cell_width(cell: &[Span<'_>]) -> usize {
    cell.iter().map(Span::width).sum()
}

/// Whether a row of the table written as `text` holds more cells than its
/// head. The parser drops the extra cells, and what they say with them —
/// most often after a `|` inside backticks, which still divides a row.
fn cut_short(text: &str) -> bool {
    // The line of dashes is counted too, harmlessly: unless it holds as many
    // cells as the head, the parser does not see a table at all.
    let mut rows = text.lines().map(cells);
    let Some(head) = rows.next() else { return false };
    rows.any(|count| count > head)
}

/// How many cells a line of a table holds, counted as the parser counts
/// them: divided at every `|` that no backslash escapes, a `|` at either
/// end dividing nothing.
fn cells(line: &str) -> usize {
    // A table in a quote has `>` in front of every line.
    let line = line.trim_start_matches(|c: char| c == '>' || c.is_whitespace()).trim_end();
    // A `|` at the end divides nothing whether escaped or not: escaped, it
    // is not counted anyway.
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    let mut count = 1;
    let mut escaped = false;
    for c in line.chars() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '|' {
            count += 1;
        }
    }
    count
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
/// fit moves to the next row whole, whatever styles it is written in. It is
/// cut where the row ends instead when no row is wide enough for it, or when
/// only spaces stand before it — a line of code's indentation, which the move
/// would leave alone on a row. Spaces with no room left after them are
/// dropped, ending the row when a word stands before them.
///
/// The rows are cut here rather than by ratatui's wrapping, for two reasons.
/// A row wrapped inside a list item or a quote has to start behind the item's
/// indentation or the quote's bar, which ratatui has no way to put there. And
/// ratatui lets a word that ends in a wide character reach one cell past the
/// edge, over the border.
fn wrap<'a>(first: Vec<Span<'a>>, rest: Vec<Span<'a>>, spans: Vec<Span<'a>>, width: usize) -> Vec<Line<'a>> {
    let mut rows = Rows::new(first, rest, width);
    for word in whole_words(spans) {
        let blank = word.iter().all(|piece| piece.content.trim().is_empty());
        // Spaces need room for something to follow them.
        if !rows.fits(word.iter().map(Span::width).sum::<usize>() + usize::from(blank)) {
            if rows.shown {
                rows.end_row();
            }
            if blank {
                continue;
            }
        }
        // The word fits now, or it cannot be moved: then its pieces go in one
        // at a time, each cut where the row ends.
        for piece in word {
            if rows.shown && !rows.fits(piece.width()) {
                rows.end_row();
            }
            rows.fill(piece);
        }
    }
    rows.finish()
}

/// Rows being filled from the left, for `wrap`.
struct Rows<'a> {
    width: usize,
    /// What goes in front of every row after the first, and how wide it is.
    rest: Vec<Span<'a>>,
    indent: usize,
    /// The rows finished so far, and the one being filled.
    done: Vec<Line<'a>>,
    row: Vec<Span<'a>>,
    used: usize,
    /// Whether the row holds anything yet besides what is in front of it,
    /// and whether it holds anything but spaces.
    empty: bool,
    shown: bool,
}

impl<'a> Rows<'a> {
    fn new(first: Vec<Span<'a>>, rest: Vec<Span<'a>>, width: usize) -> Self {
        let used = first.iter().map(Span::width).sum();
        let indent = rest.iter().map(Span::width).sum();
        Rows { width, rest, indent, done: Vec::new(), row: first, used, empty: true, shown: false }
    }

    /// Whether `width` more cells fit in the row.
    fn fits(&self, width: usize) -> bool {
        self.used + width <= self.width
    }

    fn end_row(&mut self) {
        let row = std::mem::replace(&mut self.row, self.rest.clone());
        self.done.push(Line::from(row));
        self.used = self.indent;
        self.empty = true;
        self.shown = false;
    }

    /// `piece` put in the row, which ends wherever the next grapheme would
    /// pass its edge. A grapheme — `⚠️`, a flag, a family of emoji — takes
    /// several characters and is never split between rows. An empty row takes
    /// one even when it does not fit, so that every row takes something.
    ///
    /// Each grapheme is measured once: measuring the rest of the piece anew
    /// for every row would make a long line take time by the square of its
    /// length.
    fn fill(&mut self, piece: Span<'a>) {
        let mut start = 0;
        for (i, grapheme) in piece.content.grapheme_indices(true) {
            let width = grapheme.width();
            if !self.empty && !self.fits(width) {
                if start < i {
                    self.row.push(Span::styled(part(&piece.content, start..i), piece.style));
                }
                self.end_row();
                start = i;
            }
            self.used += width;
            self.empty = false;
            self.shown |= !grapheme.trim().is_empty();
        }
        if start < piece.content.len() {
            self.row.push(Span::styled(part(&piece.content, start..piece.content.len()), piece.style));
        }
    }

    /// Every row. A row left with nothing in it is not one, unless it is
    /// the first: a space that did not fit ended the row before it.
    fn finish(mut self) -> Vec<Line<'a>> {
        if !self.empty || self.done.is_empty() {
            self.done.push(Line::from(self.row));
        }
        self.done
    }
}

/// `spans` cut into words and the runs of spaces between them. A word is
/// everything between two spaces, so it can take several pieces: `**bold**,`
/// is one word in two styles. So can a run of spaces: in `` `## ` heading ``
/// the space inside the backticks and the one after them are one run.
fn whole_words<'a>(spans: Vec<Span<'a>>) -> Vec<Vec<Span<'a>>> {
    let mut words_so_far: Vec<Vec<Span<'a>>> = Vec::new();
    let mut last_blank = None;
    for piece in spans.into_iter().flat_map(words) {
        let blank = piece.content.trim().is_empty();
        match words_so_far.last_mut() {
            Some(word) if last_blank == Some(blank) => word.push(piece),
            _ => words_so_far.push(vec![piece]),
        }
        last_blank = Some(blank);
    }
    words_so_far
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
    fn html_is_shown_as_on_disk() {
        let text = "<details>\n<summary>more</summary>\n</details>\n\nafter";
        assert_eq!(plain(text), ["<details>", "<summary>more</summary>", "</details>", "", "after"]);
    }

    // --------------------------------------------------------------- tables

    #[test]
    fn a_table_that_fits_is_a_grid() {
        let text = "| a | b |\n|---|---|\n| 1 | 22 |";
        assert_eq!(plain(text), ["a │ b", "──┼───", "1 │ 22"]);
        assert!(style_of(text, "a").add_modifier.contains(Modifier::BOLD));
        assert!(!style_of(text, "1").add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn a_table_exactly_as_wide_as_the_row_is_still_a_grid() {
        let text = "| a | b |\n|---|---|\n| 1 | 22 |";
        assert_eq!(narrow(text, 6), ["a │ b", "──┼───", "1 │ 22"]);
        assert_eq!(narrow(text, 5), ["a: 1", "b: 22"]);
    }

    #[test]
    fn a_table_in_a_list_item_has_the_indent_less_room() {
        let text = "- x\n\n  | a | b |\n  |---|---|\n  | 1 | 22 |";
        assert_eq!(narrow(text, 8), ["• x", "", "  a │ b", "  ──┼───", "  1 │ 22"]);
        assert_eq!(narrow(text, 7), ["• x", "", "  a: 1", "  b: 22"]);
    }

    #[test]
    fn a_column_is_aligned_as_the_file_says() {
        assert_eq!(plain("| n |\n|--:|\n| 1 |\n| 22 |"), [" n", "──", " 1", "22"]);
        // Centred, an odd space left over goes after.
        assert_eq!(plain("| n |\n|:-:|\n| 1 |\n| 2222 |"), [" n", "────", " 1", "2222"]);
    }

    #[test]
    fn a_heading_repeated_in_every_block_is_not_copied() {
        let text = String::from("| key | value |\n|---|---|\n| a | long long long text here |\n| b | x |");
        for row in render(&text, 20) {
            for span in &row.spans {
                assert!(matches!(span.content, Cow::Borrowed(_)), "{span:?}");
            }
        }
    }

    #[test]
    fn a_table_too_wide_is_one_block_per_row() {
        let text = "| key | value |\n|---|---|\n| a | long long long text here |\n| b | x |";
        assert_eq!(
            narrow(text, 20),
            ["key: a", "value: long long", "  long text here", "", "key: b", "value: x"]
        );
        let rows = render(text, 20);
        let heading = rows.iter().flat_map(|row| &row.spans).find(|span| span.content == "key").unwrap();
        assert_eq!(heading.style.fg, Some(Color::Cyan));
    }

    #[test]
    fn a_column_without_a_heading_shows_its_cells_alone() {
        // Seen in rule-format: a first column of row labels, headed by nothing.
        assert_eq!(narrow("| | b |\n|---|---|\n| x | y |", 4), ["x", "b: y"]);
    }

    #[test]
    fn a_cell_keeps_its_inline_styles() {
        assert_eq!(style_of("| a |\n|---|\n| `x` |", "x").fg, Some(Color::Yellow));
    }

    #[test]
    fn a_table_with_only_a_head_is_still_shown() {
        let text = "| alpha | beta |\n|---|---|";
        assert_eq!(plain(text), ["alpha │ beta", "──────┼─────"]);
        // Too wide for its row, it stays a grid: as blocks it would show nothing.
        let rows = narrow(text, 5);
        assert!(["alpha", "beta"].iter().all(|word| rows.iter().any(|row| row == word)), "{rows:?}");
    }

    #[test]
    fn a_table_whose_row_the_parser_would_cut_short_is_shown_as_written() {
        // From docs/learn/milestones/M2.md: the `|` inside backticks divides
        // the row, and the parser would drop "joined into one line".
        let text = "| Form | Result |\n|---|---|\n| `description: |` blocks | joined into one line |";
        assert_eq!(plain(text), text.lines().collect::<Vec<_>>());
    }

    #[test]
    fn cells_are_counted_as_the_parser_counts_them() {
        assert_eq!(cells("| a | b |"), 2);
        assert_eq!(cells("a | b"), 2);
        assert_eq!(cells("| `x|y` | b |"), 3);
        assert_eq!(cells("| a \\| b | c |"), 2);
        assert_eq!(cells("| a | b \\|"), 2);
        assert_eq!(cells("> | a | b |"), 2);
    }

    #[test]
    fn a_table_in_a_list_item_stays_in_the_item() {
        assert_eq!(plain("- item\n\n  | a |\n  |---|\n  | 1 |"), ["• item", "", "  a", "  ─", "  1"]);
    }

    // ---------------------------------------------------------- frontmatter

    #[test]
    fn frontmatter_loses_its_fences_and_colours_its_keys() {
        let text = "---\nname: a\ndescription: b c\n---\n\n# T\n";
        assert_eq!(plain(text), ["name: a", "description: b c", "", "# T"]);
        assert_eq!(style_of(text, "name:").fg, Some(Color::Cyan));
        assert_eq!(style_of(text, "a").fg, None);
    }

    #[test]
    fn a_long_frontmatter_value_wraps_in_by_two() {
        assert_eq!(narrow("---\ndescription: one two three\n---", 12), ["description:", "  one two", "  three"]);
    }

    #[test]
    fn an_indented_frontmatter_line_is_kept_as_written() {
        let text = "---\ntags:\n  - a\n  inner: b\n---";
        assert_eq!(plain(text), ["tags:", "  - a", "  inner: b"]);
        // Only a key that starts its line is one.
        assert_eq!(style_of(text, "inner:").fg, None);
    }

    #[test]
    fn frontmatter_from_a_crlf_file_reads_the_same() {
        let text = "---\r\nname: a\r\ndescription: >\r\n  one\r\n---\r\n";
        assert_eq!(plain(text), ["name: a", "description: >", "  one"]);
        // `plain` trims the ends of rows, where a carriage return would be.
        let rows = render(text, 80);
        assert!(rows.iter().flat_map(|row| &row.spans).all(|span| !span.content.contains('\r')), "{rows:?}");
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
    fn a_line_of_code_wraps_between_words() {
        assert_eq!(narrow("```\nfoo barbaz\n```", 8), ["foo", "barbaz"]);
    }

    #[test]
    fn indentation_is_never_left_alone_on_a_row() {
        // Seen in cli-printing-press: moved down whole, the word left its
        // indentation alone on a row of spaces.
        let text = "```\n\trootCmd.Flags().BoolVar(&asJSON, x)\n```";
        assert_eq!(narrow(text, 16), ["    rootCmd.Flag", "s().BoolVar(&asJ", "SON, x)"]);
        // Coloured, the word is in many pieces, and a piece that does not fit
        // moves down whole: the line is cut where its colours change.
        let text = "```go\n\trootCmd.Flags().BoolVar(&asJSON, x)\n```";
        assert_eq!(narrow(text, 16), ["    rootCmd.", "Flags().BoolVar(", "&asJSON, x)"]);
        // Indentation as wide as the row leaves no room after it.
        assert_eq!(narrow("```\n        \u{2514}\u{2500} x\n```", 8), ["\u{2514}\u{2500} x"]);
    }

    #[test]
    fn spaces_at_the_end_of_a_line_of_code_take_no_row() {
        assert_eq!(narrow("```\nab      \n```", 4), ["ab"]);
        // A line of nothing but spaces is still a line.
        assert_eq!(narrow("```\na\n      \nb\n```", 4), ["a", "", "b"]);
    }

    #[test]
    fn a_code_block_in_a_list_item_stays_in_the_item() {
        assert_eq!(plain("- item\n\n  ```\n  code\n  ```"), ["• item", "", "  code"]);
    }

    #[test]
    fn code_in_a_known_language_is_coloured_by_its_kind() {
        let text = "```rust\nfn main() { let s = \"hi\"; 42 } // note\n```";
        assert_eq!(plain(text), ["fn main() { let s = \"hi\"; 42 } // note"]);
        assert_eq!(style_of(text, "fn").fg, Some(Color::Magenta));
        assert_eq!(style_of(text, "main").fg, Some(Color::LightBlue));
        assert_eq!(style_of(text, "hi").fg, Some(Color::Green));
        assert_eq!(style_of(text, "42").fg, Some(Color::LightRed));
        assert_eq!(style_of(text, "note").fg, Some(Color::DarkGray));
        // What no kind is given to is in the colour of the text around it.
        assert_eq!(style_of(text, "s").fg, None);
        assert_eq!(style_of("```yaml\nname: x\n```", "name").fg, Some(Color::Cyan));
    }

    #[test]
    fn code_in_a_language_not_known_is_yellow() {
        assert_eq!(style_of("```mermaid\ngraph TD\n```", "graph").fg, Some(Color::Yellow));
        assert_eq!(style_of("    indented fn", "fn").fg, Some(Color::Yellow));
        // Only the first word after the fence names the language.
        assert_eq!(style_of("```rust ignore\nfn x\n```", "fn").fg, Some(Color::Magenta));
    }

    #[test]
    fn markdown_shown_as_written_is_yellow_as_a_language_not_known() {
        // Coloured, its headings would look like the file's own.
        for language in ["markdown", "md", "MultiMarkdown"] {
            let text = format!("# Real\n\n```{language}\n# Example\n```");
            assert_eq!(style_of(&text, "Example").fg, Some(Color::Yellow), "{language}");
            assert_eq!(style_of(&text, "Real").fg, Some(Color::Cyan), "{language}");
        }
    }

    #[test]
    fn a_diff_adds_in_green_and_takes_away_in_red() {
        let text = "```diff\n-old\n+new\n same\n```";
        assert_eq!(style_of(text, "old").fg, Some(Color::Red));
        assert_eq!(style_of(text, "new").fg, Some(Color::Green));
        assert_eq!(style_of(text, "same").fg, None);
    }

    #[test]
    fn what_a_line_leaves_open_colours_the_next() {
        // A string the first line opens is still a string on the second.
        let text = "```python\ns = \"\"\"one\ntwo\"\"\"\nx = 1\n```";
        assert_eq!(plain(text), ["s = \"\"\"one", "two\"\"\"", "x = 1"]);
        assert_eq!(style_of(text, "two").fg, Some(Color::Green));
        // And the string ends where it closes.
        assert_eq!(style_of(text, "x").fg, None);
    }

    #[test]
    fn a_line_of_code_that_comes_in_pieces_is_coloured_whole() {
        // A tab in a list item's indentation splits the line in two.
        let text = "- item\n\n  ```go\n  x := 1\n\ty := 2\n  ```";
        assert_eq!(plain(text), ["• item", "", "  x := 1", "    y := 2"]);
        assert_eq!(style_of(text, "2").fg, Some(Color::LightRed));
    }

    #[test]
    fn a_line_of_code_that_comes_whole_is_still_the_files() {
        // With CRLF, the parser hands each break over after its line, alone.
        for text in ["```rust\nfn main() {}\n```", "```rust\r\nfn main() {}\r\n```\r\n"] {
            let rows = render(text, 80);
            assert_eq!(plain(text), ["fn main() {}"]);
            assert!(rows[0].spans.iter().all(|span| matches!(span.content, Cow::Borrowed(_))), "{rows:?}");
        }
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
    fn a_word_of_two_styles_moves_to_the_next_row_whole() {
        // `bb` and `,cc` touch: one word to the eye, two pieces to the parser.
        assert_eq!(narrow("aaaa `bb`,cc", 7), ["aaaa", "bb,cc"]);
        // Seen in session-retro: a link's address left its `(` behind.
        assert_eq!(narrow("aaaa [t](uu)", 8), ["aaaa t", "(uu)"]);
    }

    #[test]
    fn a_word_of_two_styles_wider_than_a_row_never_passes_the_edge() {
        // The first style fills the row; the second starts the next one.
        assert_eq!(narrow("`aaaaa`bb", 5), ["aaaaa", "bb"]);
    }

    #[test]
    fn a_word_wider_than_a_row_is_cut_where_the_row_ends() {
        assert_eq!(narrow("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        // Moved off a row that holds a word, it starts the next row.
        assert_eq!(narrow("aa bbbbbbbbbb", 4), ["aa", "bbbb", "bbbb", "bb"]);
    }

    #[test]
    fn a_row_too_narrow_for_a_character_still_takes_one() {
        // Rather than being left empty, which would make a row for nothing.
        assert_eq!(narrow("한글", 1), ["한", "글"]);
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
    fn a_grapheme_is_never_split_across_rows() {
        // `⚠️` is two characters in two cells; counted one character at a
        // time it took one cell, and the row passed the edge.
        let warning = "\u{26A0}\u{FE0F}";
        assert_eq!(narrow(&format!("abc{warning}"), 4), ["abc", warning]);
        // A flag is two characters, a family five joined by zero-width joiners.
        let flag = "\u{1F1F0}\u{1F1F7}";
        assert_eq!(narrow(&format!("a{flag}"), 2), ["a", flag]);
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(narrow(&format!("a{family}"), 3), [format!("a{family}")]);
    }

    #[test]
    fn a_space_that_does_not_fit_never_takes_a_row_of_its_own() {
        // An indented line of frontmatter, its indentation wider than the row.
        assert_eq!(narrow("---\nk:\n      deep\n---", 4), ["k:", "deep"]);
        // A line of frontmatter with spaces after its value.
        assert_eq!(narrow("---\nname: abc   \n---", 9), ["name: abc"]);
        // Seen in cli-printing-press: two spaces in two styles are one run,
        // and none of it starts the next row.
        assert_eq!(narrow("the next `## ` heading", 11), ["the next ##", "heading"]);
    }

    #[test]
    fn a_long_line_without_spaces_is_cut_in_one_pass() {
        // Measuring what was left of the line again for every row took time
        // by the square of its length: in a release build 100 KB took 39 ms,
        // and 400 KB 635 ms. Now 400 KB takes 7 ms.
        let text = "x".repeat(400_000);
        let start = std::time::Instant::now();
        assert_eq!(render(&text, 80).len(), 5_000);
        assert!(start.elapsed() < std::time::Duration::from_secs(2), "{:?}", start.elapsed());
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
