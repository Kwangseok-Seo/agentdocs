use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::panic;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, List, ListState, Paragraph, Widget};
use ratatui::{DefaultTerminal, Frame};
use unicode_width::UnicodeWidthStr;

use crate::entry::{Entry, EntryKind, Node};
use crate::listing::{failed, heading, printable, reason};
use crate::markdown;
use crate::source::{Scope, Source, Walked};

/// The keys the screen answers to, shown along its bottom row. The arrows
/// that do what `l` and `h` do, and Enter, are left out: with them the row
/// runs into "copied to clipboard" on a screen 100 columns wide.
const KEYS: &str = " j/k ↓/↑ move   l/h open/close   tab pane   click select   drag copy   q quit";

/// What the bottom row says after a drag is copied, at its right end, and for
/// how long — herdr's words and herdr's two seconds.
const COPIED: &str = "copied to clipboard ";
const COPIED_FOR: Duration = Duration::from_secs(2);

/// How many rows one notch of the wheel scrolls the preview — herdr's default.
const WHEEL_ROWS: isize = 3;

/// How often the preview scrolls a row toward a pointer dragged above or
/// below it — herdr's interval.
const AUTOSCROLL_EVERY: Duration = Duration::from_millis(30);

/// What the screen shows, and which part of it is selected. Keys and the mouse
/// change it; drawing only reads it — and notes where and what it drew, for
/// the mouse.
pub struct App {
    global: String,
    project: String,
    sources: Vec<(Source, io::Result<Walked>)>,
    source: usize,
    entries: ListState,
    /// The directories and Bundles whose rows are showing, by path. Kept apart
    /// from what the Walk found, which the screen only reads, and by path, so
    /// that a row is still open when its Source is looked at again.
    open: HashSet<PathBuf>,
    /// How far down each file's preview has been scrolled, in rows, by its
    /// Entry's path: kept as `open` is, so that a file is where it was left
    /// when it is looked at again. Drawing holds it to the file's last row.
    /// A directory, or something that could not be read, shows a note and
    /// not a file, and keeps no place: what could not be read inside a Bundle
    /// that would not open has the Bundle's own path.
    scrolled: HashMap<PathBuf, usize>,
    /// How far down the preview as last drawn could be scrolled: to the row
    /// that puts its last row at the bottom.
    furthest: usize,
    focus: Pane,
    /// The Sources, Entries and Preview panes as last drawn.
    areas: [Rect; 3],
    /// How far the Sources pane had to scroll, when too short for every row.
    source_offset: usize,
    /// Text dragged over in the preview, from the moment the button goes down
    /// there until the next click or key.
    selection: Option<Selection>,
    /// Until when the bottom row says that a drag was copied.
    copied_until: Option<Instant>,
}

/// The pane that j and k move in. In the preview they scroll.
#[derive(Clone, Copy, PartialEq)]
enum Pane {
    Sources,
    Entries,
    Preview,
}

/// Text dragged over in the preview. It works as herdr's does: the button
/// going down only notes where a drag would start, nothing is selected until
/// the pointer reaches another cell, and letting go keeps the highlight until
/// the next click or key.
///
/// Both ends are cells of the text rather than of the screen, so that what is
/// selected stays on the words it covers while the preview scrolls under it.
#[derive(Clone, Copy)]
struct Selection {
    /// Where the button went down.
    anchor: Spot,
    /// Where the drag has got to: while dragging, the cell under the pointer
    /// in the text as last drawn — which is why drawing moves it when the
    /// text scrolls under a pointer that stays put.
    head: Spot,
    /// Where the pointer was last seen, on the screen. Held above or below
    /// the preview, it is what the preview scrolls toward.
    pointer: Position,
    phase: Phase,
    /// When the preview next scrolls a row toward the pointer, once it has
    /// begun to.
    scroll_at: Option<Instant>,
}

/// A cell of the preview's text: which of the rows it is drawn in, counted
/// from the file's first, and how many columns in from the preview's left
/// edge. Rows come first, so the derived order reads as a page does.
#[derive(Clone, Copy, PartialEq, PartialOrd)]
struct Spot {
    row: usize,
    column: u16,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    /// Down, and not yet off the cell it went down on.
    Pressed,
    /// Off it: the cells in between are highlighted.
    Dragging,
    /// Let go after dragging: copied, and still highlighted.
    Done,
}

impl Selection {
    /// The selected cells of each row of the text, top to bottom, as (row,
    /// first column, last column). The first row runs from where the drag
    /// started to the right edge, the rows between are whole, and the last
    /// row stops where the drag ended — the way a terminal selects. Columns
    /// are held inside `width`, which may have shrunk since, if the window
    /// was resized.
    fn rows(&self, width: u16) -> Vec<(usize, u16, u16)> {
        if width == 0 {
            return Vec::new();
        }
        let right = width - 1;
        let (start, end) = if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) };

        (start.row..=end.row)
            .map(|row| {
                let first = if row == start.row { start.column.min(right) } else { 0 };
                let last = if row == end.row { end.column.min(right) } else { right };
                (row, first, last)
            })
            .collect()
    }
}

/// The cell of the text under `at`, in a preview drawn inside `inner` from
/// row `top` down — the nearest one inside when `at` is outside. `inner`
/// must not be empty.
fn spot(at: Position, inner: Rect, top: usize) -> Spot {
    let at = clamp(at, inner);
    Spot { row: top + usize::from(at.y - inner.y), column: at.x - inner.x }
}

/// One row of the Entries pane: a row of the selected Source's tree, and how
/// many levels down it sits.
#[derive(Clone, Copy)]
struct Row<'a> {
    depth: usize,
    node: &'a Node,
}

impl Row<'_> {
    /// Whether there are rows below this one to show.
    fn opens(&self) -> bool {
        self.node.children().is_some_and(|rows| !rows.is_empty())
    }

    /// The row as the Entries pane draws it: indented by its depth, and marked
    /// `▸` when it has rows to show, `▾` once they are showing. A link standing
    /// in for a Bundle says so, since nothing below it was looked at, and so
    /// does anything that could not be read, with the reason. A name comes out
    /// of somebody else's file, so it passes through `printable`, as the
    /// listing's names do.
    fn line(&self, open: &HashSet<PathBuf>) -> String {
        let marker = match (self.opens(), open.contains(self.node.path())) {
            (false, _) => " ",
            (true, false) => "▸",
            (true, true) => "▾",
        };
        let label = match self.node {
            Node::Dir { path, .. } => format!("{}/", printable(&file_name(path))),
            Node::Entry(Entry { name, kind: EntryKind::Bundle { inside: None, .. }, .. }) => {
                format!("{} (link)", printable(name))
            }
            Node::Entry(entry) => printable(&entry.name),
            Node::Unreadable { path, reason: why } => format!("{} ({})", printable(&file_name(path)), reason(*why)),
        };
        format!("{}{marker} {label}", "  ".repeat(self.depth))
    }
}

/// The last part of `path`, which is how a directory or an unreadable row is
/// named on screen.
fn file_name(path: &Path) -> String {
    path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned()
}

/// The rows a tree shows: every node at this level, and under each open one
/// its own rows, a level deeper — in the order the Walk found them.
fn visible<'a>(nodes: &'a [Node], depth: usize, open: &HashSet<PathBuf>, out: &mut Vec<Row<'a>>) {
    for node in nodes {
        out.push(Row { depth, node });
        if let Some(children) = node.children() {
            if open.contains(node.path()) {
                visible(children, depth + 1, open, out);
            }
        }
    }
}

/// The cell inside `area` nearest to `at`. `area` must not be empty.
fn clamp(at: Position, area: Rect) -> Position {
    Position::new(
        at.x.clamp(area.x, area.right() - 1),
        at.y.clamp(area.y, area.bottom() - 1),
    )
}

impl App {
    /// Walk every Source once, before the screen opens: what it shows is what
    /// was on disk at that moment.
    pub fn new(sources: Vec<Source>, global: String, project: String) -> Self {
        let sources = sources
            .into_iter()
            .map(|src| {
                let walked = src.entries();
                (src, walked)
            })
            .collect();

        App {
            global,
            project,
            sources,
            source: 0,
            entries: ListState::default().with_selected(Some(0)),
            open: HashSet::new(),
            scrolled: HashMap::new(),
            furthest: 0,
            focus: Pane::Sources,
            areas: [Rect::default(); 3],
            source_offset: 0,
            selection: None,
            copied_until: None,
        }
    }

    /// Select what was clicked, and hand j and k to the pane it is in. A click
    /// on a border, a scope heading or below the last row selects nothing; one
    /// inside the preview also notes where a drag would start. Text selected
    /// earlier is let go either way.
    fn click(&mut self, column: u16, row: u16) {
        let at = Position::new(column, row);
        let [sources, entries, _] = self.areas;
        self.selection = None;

        if sources.contains(at) {
            self.focus = Pane::Sources;
            let Some(line) = row_in(sources, at) else { return };
            let rows = self.source_rows();
            if let Some((_, Some(index))) = rows.get(self.source_offset + line) {
                if *index != self.source {
                    self.pick_source(*index);
                }
            }
        } else if entries.contains(at) {
            self.focus = Pane::Entries;
            let Some(line) = row_in(entries, at) else { return };
            let index = self.entries.offset() + line;
            if index < self.rows().len() {
                self.entries.select(Some(index));
            }
        } else if self.preview_inner().contains(at) {
            self.focus = Pane::Preview;
            let cell = spot(at, self.preview_inner(), self.top());
            self.selection = Some(Selection {
                anchor: cell,
                head: cell,
                pointer: at,
                phase: Phase::Pressed,
                scroll_at: None,
            });
        }
    }

    /// The wheel turned by `rows` over the screen. Over the preview it scrolls;
    /// selected text stays on the words it covers, and a drag going on takes
    /// in what scrolls under the pointer. Elsewhere it does nothing.
    fn wheel(&mut self, column: u16, row: u16, rows: isize) {
        if self.areas[2].contains(Position::new(column, row)) {
            self.scroll(rows);
        }
    }

    /// The pointer moved with the button down. Only a drag that began inside
    /// the preview selects, and only what is inside it: past an edge, the
    /// nearest cell inside — and above or below, the preview scrolls toward
    /// the pointer, a row at a time, as `tick` is called.
    fn drag(&mut self, column: u16, row: u16) {
        let at = Position::new(column, row);
        let inner = self.preview_inner();
        let top = self.top();
        let Some(selection) = &mut self.selection else { return };
        if selection.phase == Phase::Done {
            return;
        }
        if at != selection.pointer {
            selection.phase = Phase::Dragging;
        }
        selection.pointer = at;
        if !inner.is_empty() {
            selection.head = spot(at, inner, top);
        }
    }

    /// The button came up at `now`. After a drag, the text it covered, to be
    /// copied — unless it covered nothing but blanks — and the bottom row says
    /// so for a while; the highlight stays. After a plain click, nothing.
    fn release(&mut self, now: Instant) -> Option<String> {
        let mut selection = self.selection?;
        if selection.phase != Phase::Dragging {
            self.selection = None;
            return None;
        }
        selection.phase = Phase::Done;
        self.selection = Some(selection);

        let text = Some(self.selected_text(selection)).filter(|text| !text.trim().is_empty())?;
        self.copied_until = Some(now + COPIED_FOR);
        Some(text)
    }

    /// Whatever has come due by `now`: "copied to clipboard" comes down once
    /// its time is up, and a drag held above or below the preview scrolls it
    /// a row toward the pointer, every `AUTOSCROLL_EVERY`.
    fn tick(&mut self, now: Instant) {
        if self.copied_until.is_some_and(|until| now >= until) {
            self.copied_until = None;
        }

        let Some(rows) = self.autoscroll() else { return };
        let Some(selection) = &mut self.selection else { return };
        if selection.scroll_at.is_some_and(|at| now < at) {
            return;
        }
        selection.scroll_at = Some(now + AUTOSCROLL_EVERY);
        self.scroll(rows);
    }

    /// Which way the preview scrolls without a key or the wheel: while text is
    /// dragged over with the pointer above the preview, up a row; below it,
    /// down one — unless it is already as far as it goes that way, when the
    /// loop has nothing to wake up for.
    fn autoscroll(&self) -> Option<isize> {
        let selection = self.selection?;
        if selection.phase != Phase::Dragging {
            return None;
        }
        let inner = self.preview_inner();
        if selection.pointer.y < inner.y && self.top() > 0 {
            Some(-1)
        } else if selection.pointer.y >= inner.bottom() && self.top() < self.furthest {
            Some(1)
        } else {
            None
        }
    }

    /// How long the loop may wait for a key or the mouse before something
    /// comes due: "copied to clipboard" coming down, or the next row of a
    /// scroll toward the pointer. `None` when nothing will.
    fn wake_in(&self, now: Instant) -> Option<Duration> {
        let copied = self.copied_until.map(|until| until.saturating_duration_since(now));
        let scroll = self.autoscroll().map(|_| {
            let at = self.selection.and_then(|selection| selection.scroll_at);
            at.map_or(Duration::ZERO, |at| at.saturating_duration_since(now))
        });
        [copied, scroll].into_iter().flatten().min()
    }

    /// The text under `selection`: one line per row, without the blanks that
    /// pad a row out to the edge. The rows are drawn again for this, at the
    /// width the preview was last drawn — as many of them as were selected,
    /// shown now or scrolled out of view.
    ///
    /// A wide character — Korean takes two cells — is written into its first
    /// cell, and ratatui blanks the cell it covers; that one is skipped. A wide
    /// character counts as selected when its first cell is, which is also the
    /// only cell whose highlight the terminal shows.
    fn selected_text(&self, selection: Selection) -> String {
        let width = self.preview_inner().width;
        let rows = self.preview_lines(width);
        let mut cells = Buffer::empty(Rect::new(0, 0, width, 1));
        let mut lines = Vec::new();

        for (row, first, last) in selection.rows(width) {
            let Some(text) = rows.get(row) else {
                // Below the file's last row, where the preview is blank.
                lines.push(String::new());
                continue;
            };
            cells.reset();
            Paragraph::new(text.clone()).render(cells.area, &mut cells);

            let mut line = String::new();
            let mut covered = 0;
            // From the left edge rather than from `first`: whether a cell is
            // covered depends on the cells before it.
            for x in 0..=last {
                if covered > 0 {
                    covered -= 1;
                    continue;
                }
                let symbol = cells[(x, 0)].symbol();
                covered = symbol.width().saturating_sub(1);
                if x >= first {
                    line.push_str(symbol);
                }
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }

    /// Change what is selected in answer to one key. Any key lets go of
    /// selected text. Quitting is left to the loop, so that everything here
    /// can be tested without a terminal.
    fn handle(&mut self, key: KeyCode) {
        self.selection = None;
        match key {
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Pane::Sources => Pane::Entries,
                    Pane::Entries => Pane::Preview,
                    Pane::Preview => Pane::Sources,
                };
            }
            KeyCode::Char('j') | KeyCode::Down => self.down(),
            KeyCode::Char('k') | KeyCode::Up => self.up(),
            // A page of the preview, whichever pane has j and k: these two
            // mean nothing anywhere else.
            KeyCode::PageDown => self.scroll(self.page()),
            KeyCode::PageUp => self.scroll(-self.page()),
            KeyCode::Char('l') | KeyCode::Right if self.focus == Pane::Entries => self.open_row(),
            KeyCode::Char('h') | KeyCode::Left if self.focus == Pane::Entries => self.close_row(),
            KeyCode::Enter if self.focus == Pane::Entries => self.toggle_row(),
            _ => {}
        }
    }

    /// Show the rows below the selected one, if it has any.
    fn open_row(&mut self) {
        let Some(row) = self.row() else { return };
        if row.opens() {
            let path = row.node.path().to_path_buf();
            self.open.insert(path);
        }
    }

    /// Hide the rows below the selected one. On a row with nothing showing
    /// below it, go up to the row it sits under instead. Whether a row is open
    /// is asked only of a row that opens: what could not be read inside a
    /// Bundle that would not open has the Bundle's own path.
    fn close_row(&mut self) {
        let rows = self.rows();
        let Some(at) = self.entries.selected() else { return };
        let Some(row) = rows.get(at) else { return };
        if row.opens() && self.open.contains(row.node.path()) {
            let path = row.node.path().to_path_buf();
            self.open.remove(&path);
        } else if let Some(parent) = rows[..at].iter().rposition(|r| r.depth + 1 == row.depth) {
            self.entries.select(Some(parent));
        }
    }

    /// Enter: open the selected row, or close it if it is open.
    fn toggle_row(&mut self) {
        let Some(row) = self.row() else { return };
        if !row.opens() {
            return;
        }
        let path = row.node.path().to_path_buf();
        if !self.open.remove(&path) {
            self.open.insert(path);
        }
    }

    /// One row down in the focused pane, staying put on the last row.
    fn down(&mut self) {
        match self.focus {
            Pane::Sources => {
                if self.source + 1 < self.sources.len() {
                    self.pick_source(self.source + 1);
                }
            }
            Pane::Entries => {
                let len = self.rows().len();
                let at = self.entries.selected().unwrap_or(0);
                if at + 1 < len {
                    self.entries.select(Some(at + 1));
                }
            }
            Pane::Preview => self.scroll(1),
        }
    }

    /// One row up in the focused pane, staying put on the first row.
    fn up(&mut self) {
        match self.focus {
            Pane::Sources => {
                if self.source > 0 {
                    self.pick_source(self.source - 1);
                }
            }
            Pane::Entries => {
                let at = self.entries.selected().unwrap_or(0);
                if at > 0 {
                    self.entries.select(Some(at - 1));
                }
            }
            Pane::Preview => self.scroll(-1),
        }
    }

    /// Scroll the selected Entry's preview down by `rows`, or up when `rows`
    /// is below zero. Up stops at the first row; how far down it may go
    /// depends on how the file is cut into rows, which only drawing knows, so
    /// drawing holds it there.
    fn scroll(&mut self, rows: isize) {
        let Some(entry) = self.entry() else { return };
        let path = entry.path.clone();
        let top = self.scrolled.entry(path).or_default();
        *top = top.saturating_add_signed(rows);
    }

    /// How many rows a page of the preview is: as many as it showed last time.
    fn page(&self) -> isize {
        self.preview_inner().height as isize
    }

    /// The row the selected row's preview begins at. Drawing writes back how
    /// far it really went, so between a drawing and the next key or scroll
    /// this is the row the preview was drawn from.
    fn top(&self) -> usize {
        self.entry().and_then(|entry| self.scrolled.get(&entry.path)).copied().unwrap_or(0)
    }

    /// Another Source's Entries start from their first row, scrolled to the top.
    fn pick_source(&mut self, index: usize) {
        self.source = index;
        self.entries = ListState::default().with_selected(Some(0));
    }

    /// How a pane's border is drawn: in colour when j and k move in it.
    fn border(&self, pane: Pane) -> Style {
        if self.focus == pane {
            Style::new().fg(Color::Yellow)
        } else {
            Style::new()
        }
    }

    /// The selected Source's Entries, when it could be walked at all.
    fn walked(&self) -> Option<&Walked> {
        match self.sources.get(self.source) {
            Some((_, Ok(walked))) => Some(walked),
            _ => None,
        }
    }

    /// The selected Source's tree as the Entries pane shows it, row by row.
    fn rows(&self) -> Vec<Row<'_>> {
        let mut out = Vec::new();
        if let Some(walked) = self.walked() {
            visible(&walked.nodes, 0, &self.open, &mut out);
        }
        out
    }

    /// The selected row, if there is one.
    fn row(&self) -> Option<Row<'_>> {
        self.rows().get(self.entries.selected()?).copied()
    }

    /// The selected Entry: the selected row, unless that is a directory or
    /// something that could not be read.
    fn entry(&self) -> Option<&Entry> {
        match self.row()?.node {
            Node::Entry(entry) => Some(entry),
            Node::Dir { .. } | Node::Unreadable { .. } => None,
        }
    }

    /// The preview inside its border, as last drawn: where text can be dragged over.
    fn preview_inner(&self) -> Rect {
        self.areas[2].inner(Margin::new(1, 1))
    }

    fn render(&mut self, frame: &mut Frame) {
        let [body, footer] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)])
            .areas(frame.area());
        let [left, middle, right] = Layout::horizontal([
            Constraint::Length(28),
            Constraint::Length(32),
            Constraint::Fill(1),
        ])
        .areas(body);

        self.areas = [left, middle, right];
        self.render_sources(frame, left);
        self.render_entries(frame, middle);
        self.render_preview(frame, right);
        frame.render_widget(KEYS, footer);
        if self.copied_until.is_some() {
            let copied = Line::from(COPIED).right_aligned().style(Style::new().fg(Color::Green));
            frame.render_widget(copied, footer);
        }
    }

    /// The Sources pane row by row: the scope headings, and under them a line
    /// for each Source, worded exactly as the listing words it, together with
    /// that Source's index. Drawing and clicking both read this, so a click
    /// lands on the row that was drawn.
    fn source_rows(&self) -> Vec<(String, Option<usize>)> {
        let mut rows = vec![(self.global.clone(), None)];
        let mut project_shown = false;

        for (i, (src, walked)) in self.sources.iter().enumerate() {
            if matches!(src.scope, Scope::Project) && !project_shown {
                rows.push((self.project.clone(), None));
                project_shown = true;
            }
            let line = match walked {
                Ok(walked) => heading(&src.name, walked, &[]),
                Err(e) => failed(&src.name, e),
            };
            rows.push((line, Some(i)));
        }
        if !project_shown {
            rows.push((self.project.clone(), None));
        }
        rows
    }

    fn render_sources(&mut self, frame: &mut Frame, area: Rect) {
        let rows = self.source_rows();
        let selected = rows.iter().position(|(_, source)| *source == Some(self.source));

        let list = List::new(rows.into_iter().map(|(line, _)| line))
            .block(Block::bordered().title("Sources").border_style(self.border(Pane::Sources)))
            .highlight_style(Style::new().reversed());
        let mut state = ListState::default().with_selected(selected);
        frame.render_stateful_widget(list, area, &mut state);
        self.source_offset = state.offset();
    }

    /// The selected Source's tree, a line for each row showing. Names come out
    /// of somebody else's file, so they pass through `printable` as the
    /// listing's do: a `List` hands a character that takes no room to the cell
    /// before it, where a `Paragraph` would have dropped it.
    fn render_entries(&mut self, frame: &mut Frame, area: Rect) {
        let lines: Vec<String> = self.rows().iter().map(|row| row.line(&self.open)).collect();

        let list = List::new(lines)
            .block(Block::bordered().title("Entries").border_style(self.border(Pane::Entries)))
            .highlight_style(Style::new().reversed());
        frame.render_stateful_widget(list, area, &mut self.entries);
    }

    /// What the preview shows for the selected row, cut into rows `width`
    /// wide: its file drawn as Markdown, or a line saying why there is none.
    fn preview_lines(&self, width: u16) -> Vec<Line<'_>> {
        match self.entry() {
            Some(entry) => match (&entry.text, entry.doc()) {
                (Some(text), _) => markdown::render(text, width),
                (None, None) => markdown::note("(this Bundle has no SKILL.md)", width),
                (None, Some(_)) => markdown::note("(the file could not be read)", width),
            },
            None => match self.row().map(|row| row.node) {
                Some(Node::Unreadable { reason: why, .. }) => {
                    markdown::note(format!("(this could not be read: {})", reason(*why)), width)
                }
                Some(_) => markdown::note("(a directory)", width),
                None => Vec::new(),
            },
        }
    }

    /// The selected row's preview, from where it was scrolled to, with any
    /// dragged-over text shown reversed. When the file has more rows than
    /// show, the title says which.
    ///
    /// Each line is one row: the renderer cuts them to fit, notes as well as
    /// files, and drawn across this machine's 585 files at four widths none
    /// came out wider. So the rows are drawn one to a line, and a selection's
    /// rows are the file's.
    fn render_preview(&mut self, frame: &mut Frame, area: Rect) {
        let inner = area.inner(Margin::new(1, 1));
        let height = usize::from(inner.height);
        let path = self.entry().map(|entry| entry.path.clone());
        let mut lines = self.preview_lines(inner.width);

        // Scrolled no further than puts the last row at the bottom. What is
        // held here is written back once `lines` is done with, since `lines`
        // borrows the file's text out of `self`.
        let rows = lines.len();
        let most = rows.saturating_sub(height);
        let top = path.as_ref().and_then(|path| self.scrolled.get(path)).copied().unwrap_or(0).min(most);
        lines.drain(..top);

        let mut block = Block::bordered().title("Preview").border_style(self.border(Pane::Preview));
        // With no room inside, no row shows to be counted.
        if height > 0 && rows > height {
            let shown = format!("{}-{}/{rows}", top + 1, (top + height).min(rows));
            block = block.title_top(Line::from(shown).right_aligned());
        }
        frame.render_widget(Paragraph::new(lines).block(block), area);
        if let Some(path) = path {
            self.scrolled.insert(path, top);
        }
        self.furthest = most;

        let Some(selection) = &mut self.selection else { return };
        if selection.phase == Phase::Pressed || inner.is_empty() {
            return;
        }
        // The text may have scrolled under a pointer that stayed put.
        if selection.phase == Phase::Dragging {
            selection.head = spot(selection.pointer, inner, top);
        }
        for (row, first, last) in selection.rows(inner.width) {
            if (top..top + height).contains(&row) {
                let y = inner.y + (row - top) as u16;
                let cells = Rect::new(inner.x + first, y, last - first + 1, 1);
                frame.buffer_mut().set_style(cells, Style::new().reversed());
            }
        }
    }
}

/// The row `at` falls on inside a bordered pane, counted from 0, or `None`
/// when it is on the border itself.
fn row_in(pane: Rect, at: Position) -> Option<usize> {
    let inside = pane.inner(Margin::new(1, 1));
    if inside.contains(at) {
        Some(usize::from(at.y - inside.y))
    } else {
        None
    }
}

/// Take the terminal over, run the loop, and hand the terminal back as it was.
///
/// ratatui restores raw mode and the alternate screen; mouse reporting is ours
/// to turn off. On Windows it has to come off before raw mode does: turning it
/// off puts back the console mode saved when it was turned on, and raw mode was
/// already on by then.
pub fn open(app: App) -> io::Result<()> {
    let mut terminal = ratatui::init();
    release_mouse_on_panic();

    let result = execute!(io::stdout(), EnableMouseCapture).and_then(|_| run(&mut terminal, app));

    // Nothing more can be done if this fails; restoring the rest still matters.
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

/// ratatui's panic hook restores raw mode and the alternate screen. This hook
/// is installed after it, so it runs first: the mouse comes off, then ratatui's
/// hook runs — the order Windows needs.
fn release_mouse_on_panic() {
    let ratatui_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableMouseCapture);
        ratatui_hook(info);
    }));
}

/// Put `text` on the clipboard with OSC 52: the terminal does the copying, so
/// nothing appears on screen. A terminal that does not allow it ignores the
/// request, and nothing here can tell.
fn copy(text: &str) -> io::Result<()> {
    let mut out = io::stdout();
    out.write_all(osc52(text).as_bytes())?;
    out.flush()
}

/// The sequence that asks a terminal to put `text` on its clipboard: ESC ] 52,
/// `c` for the clipboard, the text in base64, and BEL to end it.
fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", STANDARD.encode(text))
}

/// Draw, wait for a key or the mouse, act on it — and again, until `q`.
fn run(terminal: &mut DefaultTerminal, mut app: App) -> io::Result<()> {
    loop {
        app.tick(Instant::now());
        terminal.draw(|frame| app.render(frame))?;

        // While something is due — "copied to clipboard" coming down, the
        // next row of a scroll toward the pointer — wait no longer than until
        // then: when the time runs out with nothing pressed, go round again so
        // that `tick` sees to it.
        if let Some(wait) = app.wake_in(Instant::now()) {
            if !event::poll(wait)? {
                continue;
            }
        }

        match event::read()? {
            // Windows reports a key going up as well as going down, so every
            // keystroke arrives twice. Only the press counts.
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if key.code == KeyCode::Char('q') {
                    return Ok(());
                }
                app.handle(key.code);
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => app.click(mouse.column, mouse.row),
                MouseEventKind::Drag(MouseButton::Left) => app.drag(mouse.column, mouse.row),
                MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(text) = app.release(Instant::now()) {
                        copy(&text)?;
                    }
                }
                // Only over the preview: moving the selection with the wheel
                // surprised, in M5.
                MouseEventKind::ScrollDown => app.wheel(mouse.column, mouse.row, WHEEL_ROWS),
                MouseEventKind::ScrollUp => app.wheel(mouse.column, mouse.row, -WHEEL_ROWS),
                _ => {}
            },
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Walk;
    use crate::testutil::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;

    /// The cells a terminal of this size would be given.
    fn draw(app: &mut App, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// What a terminal of this size would show, one string per row.
    fn screen(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let buffer = draw(app, width, height);
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    fn app(sources: Vec<Source>) -> App {
        App::new(sources, "GLOBAL".to_string(), "PROJECT here".to_string())
    }

    /// Two Sources to move between: three rules, and one that is not there.
    fn three_rules(name: &str) -> App {
        let dir = scratch(name);
        for rule in ["one", "two", "three"] {
            write(&dir.join(format!("{rule}.md")), &format!("# {rule}\n"));
        }
        app(vec![
            Source::new("rules", dir.clone(), Scope::Global, Walk::MarkdownFiles),
            Source::new("gone", dir.join("nope"), Scope::Global, Walk::MarkdownFiles),
        ])
    }

    /// Press these keys, in order.
    fn press(app: &mut App, keys: &[KeyCode]) {
        for key in keys {
            app.handle(*key);
        }
    }

    // --------------------------------------------------------------- moving

    #[test]
    fn j_moves_down_the_sources_and_k_moves_back_up() {
        let mut app = three_rules("tui-jk");
        press(&mut app, &[KeyCode::Char('j')]);
        assert_eq!(app.source, 1);
        press(&mut app, &[KeyCode::Char('k')]);
        assert_eq!(app.source, 0);
    }

    #[test]
    fn the_arrow_keys_do_what_j_and_k_do() {
        let mut app = three_rules("tui-arrows");
        press(&mut app, &[KeyCode::Down]);
        assert_eq!(app.source, 1);
        press(&mut app, &[KeyCode::Up]);
        assert_eq!(app.source, 0);
    }

    #[test]
    fn the_sources_stop_at_either_end_rather_than_wrap() {
        let mut app = three_rules("tui-source-ends");
        press(&mut app, &[KeyCode::Char('k')]);
        assert_eq!(app.source, 0);
        press(&mut app, &[KeyCode::Char('j'); 5]);
        assert_eq!(app.source, 1);
    }

    #[test]
    fn tab_hands_j_and_k_to_the_entries_and_the_preview_follows() {
        let mut app = three_rules("tui-tab");
        let second = app.walked().unwrap().entries()[1].name.clone();

        press(&mut app, &[KeyCode::Tab, KeyCode::Char('j')]);
        assert_eq!(app.source, 0);
        assert_eq!(app.entries.selected(), Some(1));
        assert_eq!(app.entry().unwrap().name, second);
    }

    #[test]
    fn the_entries_stop_at_either_end_rather_than_wrap() {
        let mut app = three_rules("tui-entry-ends");
        press(&mut app, &[KeyCode::Tab]);
        press(&mut app, &[KeyCode::Char('j'); 5]);
        assert_eq!(app.entries.selected(), Some(2));
        press(&mut app, &[KeyCode::Char('k'); 5]);
        assert_eq!(app.entries.selected(), Some(0));
    }

    #[test]
    fn another_source_starts_from_its_first_entry() {
        let mut app = three_rules("tui-reset");
        press(&mut app, &[KeyCode::Tab, KeyCode::Char('j'), KeyCode::Char('j')]);
        // Round past the preview, back to the Sources.
        press(&mut app, &[KeyCode::Tab, KeyCode::Tab, KeyCode::Char('j'), KeyCode::Char('k')]);
        assert_eq!(app.source, 0);
        assert_eq!(app.entries.selected(), Some(0));
    }

    #[test]
    fn a_source_that_could_not_be_walked_has_nothing_to_move_through() {
        let mut app = three_rules("tui-gone");
        press(&mut app, &[KeyCode::Char('j'), KeyCode::Tab, KeyCode::Char('j')]);
        assert_eq!(app.source, 1);
        assert!(app.entry().is_none());
    }

    #[test]
    fn with_no_sources_at_all_no_key_moves_anything() {
        let mut app = app(Vec::new());
        press(&mut app, &[KeyCode::Char('j'), KeyCode::Tab, KeyCode::Char('j'), KeyCode::Char('k')]);
        assert_eq!(app.source, 0);
        assert!(app.entry().is_none());
    }

    #[test]
    fn the_selected_rows_and_the_focused_pane_are_drawn_so() {
        let mut app = three_rules("tui-drawn");
        press(&mut app, &[KeyCode::Tab, KeyCode::Char('j')]);
        let cells = draw(&mut app, 100, 8);

        // Sources: GLOBAL on row 1, `rules` on row 2. Entries begin at column 28,
        // and their second row is row 2.
        let reversed = |x: u16, y: u16| cells[(x, y)].modifier.contains(Modifier::REVERSED);
        assert!(!reversed(1, 1));
        assert!(reversed(1, 2));
        assert!(!reversed(29, 1));
        assert!(reversed(29, 2));

        assert_eq!(cells[(28, 0)].fg, Color::Yellow);
        assert_ne!(cells[(0, 0)].fg, Color::Yellow);
    }

    #[test]
    fn the_keys_are_listed_along_the_bottom_row() {
        let mut app = three_rules("tui-keys");
        let rows = screen(&mut app, 100, 8);
        assert!(rows[7].starts_with(KEYS), "{:?}", rows[7]);
    }

    // ------------------------------------------------------------- clicking
    //
    // On a 100 x 8 screen the panes are 7 rows tall, with the keys on row 7.
    // Sources span columns 0-27 and Entries 28-59; inside their borders, rows
    // 1-5. `three_rules` puts GLOBAL on row 1, `rules` on 2, `gone` on 3 and
    // the PROJECT heading on 4.

    /// Draw once, so that the App knows where its panes are, then click.
    fn click(app: &mut App, column: u16, row: u16) {
        draw(app, 100, 8);
        app.click(column, row);
    }

    #[test]
    fn clicking_a_source_selects_it_and_its_pane() {
        let mut app = three_rules("tui-click-source");
        press(&mut app, &[KeyCode::Tab]);
        click(&mut app, 5, 3);
        assert_eq!(app.source, 1);
        assert!(app.focus == Pane::Sources);
    }

    #[test]
    fn clicking_a_scope_heading_selects_nothing() {
        // GLOBAL on row 1 above `rules`, PROJECT on row 3 above `docs`: each
        // heading sits directly above a Source other than the selected one.
        let dir = scratch("tui-click-heading");
        let mut app = app(vec![
            Source::new("rules", dir.join("rules"), Scope::Global, Walk::MarkdownFiles),
            Source::new("docs", dir.join("docs"), Scope::Project, Walk::MarkdownTree),
        ]);
        click(&mut app, 5, 3);
        assert_eq!(app.source, 0);

        press(&mut app, &[KeyCode::Char('j')]);
        click(&mut app, 5, 1);
        assert_eq!(app.source, 1);
    }

    #[test]
    fn clicking_an_entry_selects_it_its_pane_and_its_preview() {
        let mut app = three_rules("tui-click-entry");
        let third = app.walked().unwrap().entries()[2].name.clone();

        click(&mut app, 35, 3);
        assert_eq!(app.entries.selected(), Some(2));
        assert!(app.focus == Pane::Entries);
        assert_eq!(app.entry().unwrap().name, third);
    }

    #[test]
    fn clicking_below_the_last_entry_selects_nothing() {
        let mut app = three_rules("tui-click-below");
        click(&mut app, 35, 5);
        assert_eq!(app.entries.selected(), Some(0));
    }

    #[test]
    fn clicking_a_border_or_the_preview_selects_nothing() {
        let mut app = three_rules("tui-click-border");
        click(&mut app, 35, 0);
        assert_eq!(app.entries.selected(), Some(0));
        click(&mut app, 5, 6);
        assert_eq!(app.source, 0);
        click(&mut app, 80, 3);
        assert_eq!((app.source, app.entries.selected()), (0, Some(0)));
    }

    #[test]
    fn clicking_the_selected_source_again_keeps_its_entry() {
        let mut app = three_rules("tui-click-again");
        press(&mut app, &[KeyCode::Tab, KeyCode::Char('j'), KeyCode::Char('j')]);
        click(&mut app, 5, 2);
        assert_eq!(app.entries.selected(), Some(2));
    }

    #[test]
    fn a_click_counts_from_where_the_entries_have_scrolled_to() {
        let dir = scratch("tui-click-scrolled");
        for n in 0..10 {
            write(&dir.join(format!("rule{n}.md")), "# rule\n");
        }
        let mut app = app(vec![Source::new("rules", dir, Scope::Global, Walk::MarkdownFiles)]);

        // Five rows show at a time, so selecting the ninth scrolls by four.
        press(&mut app, &[KeyCode::Tab]);
        press(&mut app, &[KeyCode::Char('j'); 8]);
        click(&mut app, 35, 1);
        assert_eq!(app.entries.offset(), 4);
        assert_eq!(app.entries.selected(), Some(4));
    }

    #[test]
    fn a_click_counts_from_where_the_sources_have_scrolled_to() {
        // Five rows tall: two rows inside the Sources pane, which has to scroll
        // to show `gone` — `rules` on row 1, `gone` on row 2.
        let mut app = three_rules("tui-click-short");
        press(&mut app, &[KeyCode::Char('j')]);
        draw(&mut app, 100, 5);
        app.click(5, 1);
        assert_eq!(app.source, 0);
    }

    // ------------------------------------------------------------ scrolling
    //
    // On a 100 x 8 screen the preview shows five rows, 1-5, in columns 61-98.
    // `tall` has two files: `long`, twelve rows — `• 1` to `• 12` — and
    // `short`, one.

    fn tall(name: &str) -> App {
        let dir = scratch(name);
        let items: String = (1..=12).map(|n| format!("- {n}\n")).collect();
        write(&dir.join("long.md"), &items);
        write(&dir.join("short.md"), "# short\n");
        app(vec![Source::new("rules", dir, Scope::Global, Walk::MarkdownFiles)])
    }

    /// The preview's five rows as a 100 x 8 screen shows them, each without
    /// the blanks after it.
    fn preview(app: &mut App) -> Vec<String> {
        let cells = draw(app, 100, 8);
        (1..6)
            .map(|y| (61..99).map(|x| cells[(x, y)].symbol()).collect::<String>().trim_end().to_string())
            .collect()
    }

    /// The first of them.
    fn first_row(app: &mut App) -> String {
        preview(app)[0].clone()
    }

    #[test]
    fn tab_goes_round_the_three_panes_and_the_preview_shows_when_it_has_j_and_k() {
        let mut app = tall("tui-tab-round");
        press(&mut app, &[KeyCode::Tab]);
        assert!(app.focus == Pane::Entries);
        press(&mut app, &[KeyCode::Tab]);
        assert!(app.focus == Pane::Preview);
        assert_eq!(draw(&mut app, 100, 8)[(60, 0)].fg, Color::Yellow);
        press(&mut app, &[KeyCode::Tab]);
        assert!(app.focus == Pane::Sources);
    }

    #[test]
    fn in_the_preview_j_scrolls_a_row_down_and_k_back_up() {
        let mut app = tall("tui-scroll-jk");
        press(&mut app, &[KeyCode::Tab, KeyCode::Tab, KeyCode::Char('j')]);
        assert_eq!(preview(&mut app), ["• 2", "• 3", "• 4", "• 5", "• 6"]);
        press(&mut app, &[KeyCode::Char('k')]);
        assert_eq!(first_row(&mut app), "• 1");
    }

    #[test]
    fn the_preview_stops_at_its_first_row() {
        let mut app = tall("tui-scroll-top");
        press(&mut app, &[KeyCode::Tab, KeyCode::Tab, KeyCode::Char('k')]);
        assert_eq!(first_row(&mut app), "• 1");
    }

    #[test]
    fn the_preview_stops_with_its_last_row_at_the_bottom_and_goes_back_up_from_there() {
        let mut app = tall("tui-scroll-end");
        press(&mut app, &[KeyCode::Tab, KeyCode::Tab]);
        press(&mut app, &[KeyCode::Char('j'); 20]);
        assert_eq!(preview(&mut app), ["• 8", "• 9", "• 10", "• 11", "• 12"]);
        // One row up from there, not from twenty rows down.
        press(&mut app, &[KeyCode::Char('k')]);
        assert_eq!(first_row(&mut app), "• 7");
    }

    #[test]
    fn a_taller_preview_takes_back_what_it_no_longer_needs_scrolled() {
        let mut app = tall("tui-scroll-taller");
        press(&mut app, &[KeyCode::Tab, KeyCode::Tab]);
        press(&mut app, &[KeyCode::Char('j'); 20]);
        draw(&mut app, 100, 8);
        // Twenty rows tall, the preview holds all twelve.
        draw(&mut app, 100, 20);
        assert_eq!(first_row(&mut app), "• 1");
    }

    #[test]
    fn the_wheel_scrolls_the_preview_three_rows_and_nothing_elsewhere() {
        let mut app = tall("tui-scroll-wheel");
        draw(&mut app, 100, 8);
        app.wheel(80, 3, WHEEL_ROWS);
        assert_eq!(first_row(&mut app), "• 4");
        app.wheel(35, 3, WHEEL_ROWS);
        assert_eq!(first_row(&mut app), "• 4");
        assert_eq!(app.entries.selected(), Some(0));
        app.wheel(80, 3, -WHEEL_ROWS);
        assert_eq!(first_row(&mut app), "• 1");
    }

    #[test]
    fn page_down_and_page_up_scroll_a_preview_of_rows_from_any_pane() {
        let mut app = tall("tui-scroll-page");
        draw(&mut app, 100, 8);
        for (n, pane) in [Pane::Sources, Pane::Entries, Pane::Preview].into_iter().enumerate() {
            assert!(app.focus == pane);
            press(&mut app, &[KeyCode::PageDown]);
            assert_eq!(first_row(&mut app), "• 6", "pane {n}");
            press(&mut app, &[KeyCode::PageUp]);
            assert_eq!(first_row(&mut app), "• 1", "pane {n}");
            press(&mut app, &[KeyCode::Tab]);
        }
    }

    #[test]
    fn a_file_is_where_it_was_left_when_it_is_looked_at_again() {
        let mut app = tall("tui-scroll-kept");
        draw(&mut app, 100, 8);
        press(&mut app, &[KeyCode::PageDown, KeyCode::Tab, KeyCode::Char('j')]);
        assert_eq!(first_row(&mut app), "# short");
        press(&mut app, &[KeyCode::Char('k')]);
        assert_eq!(first_row(&mut app), "• 6");
    }

    #[test]
    fn a_file_keeps_where_it_was_left_when_rows_open_above_it() {
        // `a/` above `long`: opening it puts `x` where `long` was.
        let dir = scratch("tui-scroll-above");
        let items: String = (1..=12).map(|n| format!("- {n}\n")).collect();
        write(&dir.join("long.md"), &items);
        write(&dir.join("a").join("x.md"), "# x\n");
        let mut app = app(vec![Source::new("docs", dir, Scope::Project, Walk::MarkdownTree)]);
        draw(&mut app, 100, 8);

        press(&mut app, &[KeyCode::Tab, KeyCode::Char('j'), KeyCode::PageDown]);
        assert_eq!(first_row(&mut app), "• 6");
        press(&mut app, &[KeyCode::Char('k'), KeyCode::Char('l'), KeyCode::Char('j')]);
        assert_eq!(first_row(&mut app), "# x");
        press(&mut app, &[KeyCode::Char('j')]);
        assert_eq!(first_row(&mut app), "• 6");
    }

    #[test]
    fn the_title_says_which_rows_show_only_when_not_all_of_them_do() {
        let mut app = tall("tui-scroll-title");
        // Three rows high, the preview is two borders and nothing between.
        assert!(!screen(&mut app, 100, 3)[0].contains('/'));
        assert!(screen(&mut app, 100, 8)[0].contains("1-5/12"));
        press(&mut app, &[KeyCode::PageDown]);
        assert!(screen(&mut app, 100, 8)[0].contains("6-10/12"));
        press(&mut app, &[KeyCode::PageDown]);
        assert!(screen(&mut app, 100, 8)[0].contains("8-12/12"));
        press(&mut app, &[KeyCode::Tab, KeyCode::Char('j')]);
        assert!(!screen(&mut app, 100, 8)[0].contains('/'));
    }

    #[test]
    fn a_click_in_the_preview_hands_it_j_and_k() {
        let mut app = tall("tui-scroll-click");
        click(&mut app, 80, 3);
        assert!(app.focus == Pane::Preview);
        press(&mut app, &[KeyCode::Char('j')]);
        assert_eq!(first_row(&mut app), "• 2");
    }

    // ------------------------------------------------ selecting and scrolling
    //
    // `tall` again: `• 1` to `• 12`, the bullet in column 61 and the number
    // from 63. Row 7 is the keys, below the preview; row 0 its top border.

    /// Whether the cell at the start of each of the preview's five rows is
    /// drawn reversed.
    fn reversed_rows(app: &mut App) -> Vec<bool> {
        let cells = draw(app, 100, 8);
        (1..6).map(|y| cells[(61, y)].modifier.contains(Modifier::REVERSED)).collect()
    }

    #[test]
    fn selected_text_moves_with_its_words_when_the_wheel_scrolls() {
        let mut app = tall("tui-select-wheel");
        assert_eq!(drag_across(&mut app, (61, 5), (63, 5)).as_deref(), Some("• 5"));
        app.wheel(80, 3, WHEEL_ROWS);
        assert_eq!(reversed_rows(&mut app), [false, true, false, false, false]);
        assert_eq!(preview(&mut app)[1], "• 5");
    }

    #[test]
    fn selected_text_scrolled_out_of_view_is_not_drawn() {
        let mut app = tall("tui-select-gone");
        drag_across(&mut app, (61, 1), (63, 1));
        app.wheel(80, 3, WHEEL_ROWS);
        assert_eq!(reversed_rows(&mut app), [false; 5]);
    }

    #[test]
    fn selected_text_running_on_below_the_preview_stops_at_its_border() {
        // `• 6` to `• 10` selected, then the wheel up three rows: the last
        // three of them are below the preview, where the border and the keys are.
        let mut app = tall("tui-select-border");
        draw(&mut app, 100, 8);
        press(&mut app, &[KeyCode::PageDown]);
        drag_across(&mut app, (61, 1), (64, 5));
        app.wheel(80, 3, -WHEEL_ROWS);
        let cells = draw(&mut app, 100, 8);
        let reversed = |y: u16| cells[(61, y)].modifier.contains(Modifier::REVERSED);
        assert_eq!((1..6).map(reversed).collect::<Vec<_>>(), [false, false, false, true, true]);
        assert!(!reversed(6));
        assert!(!reversed(7));
    }

    #[test]
    fn a_drag_held_below_the_preview_scrolls_it_a_row_each_time_one_is_due() {
        let mut app = tall("tui-select-below");
        let start = Instant::now();
        draw(&mut app, 100, 8);
        app.click(61, 1);
        app.drag(70, 7);
        draw(&mut app, 100, 8);

        app.tick(start);
        assert_eq!(first_row(&mut app), "• 2");
        app.tick(start + AUTOSCROLL_EVERY - Duration::from_millis(1));
        assert_eq!(first_row(&mut app), "• 2");
        app.tick(start + AUTOSCROLL_EVERY);
        assert_eq!(first_row(&mut app), "• 3");
    }

    #[test]
    fn what_a_drag_scrolls_past_is_copied_though_no_longer_in_view() {
        let mut app = tall("tui-select-copy-below");
        let start = Instant::now();
        draw(&mut app, 100, 8);
        app.click(61, 1);
        app.drag(70, 7);
        for n in 0..3 {
            app.tick(start + AUTOSCROLL_EVERY * n);
            draw(&mut app, 100, 8);
        }
        assert_eq!(first_row(&mut app), "• 4");
        let copied = app.release(start).unwrap();
        assert_eq!(copied, "• 1\n• 2\n• 3\n• 4\n• 5\n• 6\n• 7\n• 8");
    }

    #[test]
    fn a_drag_held_above_the_preview_scrolls_it_up() {
        let mut app = tall("tui-select-above");
        let start = Instant::now();
        draw(&mut app, 100, 8);
        press(&mut app, &[KeyCode::PageDown, KeyCode::PageDown]);
        draw(&mut app, 100, 8);
        // The end of `• 12`, on the last row, up to the top border.
        app.click(64, 5);
        app.drag(61, 0);
        for n in 0..2 {
            app.tick(start + AUTOSCROLL_EVERY * n);
            draw(&mut app, 100, 8);
        }
        assert_eq!(first_row(&mut app), "• 6");
        let copied = app.release(start).unwrap();
        assert_eq!(copied, "• 6\n• 7\n• 8\n• 9\n• 10\n• 11\n• 12");
    }

    #[test]
    fn a_drag_inside_the_preview_scrolls_nothing_and_wakes_nothing() {
        let mut app = tall("tui-select-inside");
        let start = Instant::now();
        draw(&mut app, 100, 8);
        app.click(61, 1);
        app.drag(70, 5);
        app.tick(start);
        assert_eq!(first_row(&mut app), "• 1");
        assert_eq!(app.wake_in(start), None);
    }

    #[test]
    fn a_drag_held_past_an_end_the_preview_has_reached_wakes_nothing() {
        let mut app = tall("tui-select-stuck");
        let start = Instant::now();
        draw(&mut app, 100, 8);

        // Above a preview at its first row.
        app.click(63, 3);
        app.drag(61, 0);
        draw(&mut app, 100, 8);
        assert_eq!(app.wake_in(start), None);
        // The drag still reaches the first row.
        assert_eq!(reversed_rows(&mut app), [true, true, true, false, false]);

        // Below one whose last row shows.
        press(&mut app, &[KeyCode::PageDown, KeyCode::PageDown]);
        draw(&mut app, 100, 8);
        app.click(61, 1);
        app.drag(70, 7);
        draw(&mut app, 100, 8);
        assert_eq!(app.wake_in(start), None);
        app.tick(start);
        assert_eq!(first_row(&mut app), "• 8");
    }

    #[test]
    fn the_loop_is_woken_when_the_next_row_is_due_and_not_after_letting_go() {
        let mut app = tall("tui-select-wake");
        let start = Instant::now();
        draw(&mut app, 100, 8);
        app.click(61, 1);
        app.drag(70, 7);
        assert_eq!(app.wake_in(start), Some(Duration::ZERO));
        app.tick(start);
        assert_eq!(app.wake_in(start), Some(AUTOSCROLL_EVERY));
        draw(&mut app, 100, 8);
        app.release(start);
        assert_eq!(app.wake_in(start), Some(COPIED_FOR));
    }

    #[test]
    fn the_wheel_during_a_drag_takes_in_what_scrolls_under_the_pointer() {
        let mut app = tall("tui-select-wheel-drag");
        draw(&mut app, 100, 8);
        app.click(61, 1);
        app.drag(70, 5);
        draw(&mut app, 100, 8);
        app.wheel(80, 3, WHEEL_ROWS);
        draw(&mut app, 100, 8);
        let copied = app.release(Instant::now()).unwrap();
        assert_eq!(copied, "• 1\n• 2\n• 3\n• 4\n• 5\n• 6\n• 7\n• 8");
    }

    // ------------------------------------------------------------- dragging
    //
    // On a 100 x 8 screen the preview spans columns 60-99; inside its border,
    // columns 61-98 and rows 1-5. `alpha` shows its one file there:
    //
    //   row 1  # Alpha rule    `A` on column 63, the `a` that ends Alpha on 67
    //   row 2  body line
    //   row 3  한글 줄          한 on 61-62, 글 on 63-64, 줄 on 66-67
    //
    // The backslash ends the line where it stands; without it the two lines
    // are one paragraph, and the preview joins them.

    fn alpha(name: &str) -> App {
        let dir = scratch(name);
        write(&dir.join("alpha.md"), "# Alpha rule\nbody line\\\n한글 줄\n");
        app(vec![Source::new("rules", dir, Scope::Global, Walk::MarkdownFiles)])
    }

    /// Press at `from`, drag to `to` and let go, drawing before each step as
    /// the loop does. What would be copied.
    fn drag_across(app: &mut App, from: (u16, u16), to: (u16, u16)) -> Option<String> {
        draw(app, 100, 8);
        app.click(from.0, from.1);
        draw(app, 100, 8);
        app.drag(to.0, to.1);
        draw(app, 100, 8);
        app.release(Instant::now())
    }

    #[test]
    fn dragging_along_a_line_copies_the_cells_it_covered() {
        let mut app = alpha("tui-drag-line");
        assert_eq!(drag_across(&mut app, (63, 1), (67, 1)).as_deref(), Some("Alpha"));
    }

    #[test]
    fn dragging_backwards_copies_the_same() {
        let mut app = alpha("tui-drag-back");
        assert_eq!(drag_across(&mut app, (67, 1), (63, 1)).as_deref(), Some("Alpha"));
    }

    #[test]
    fn a_drag_over_rows_takes_the_first_from_its_start_and_the_middle_whole() {
        let mut app = alpha("tui-drag-rows");
        let copied = drag_across(&mut app, (69, 1), (64, 3));
        assert_eq!(copied.as_deref(), Some("rule\nbody line\n한글"));
    }

    #[test]
    fn a_wide_character_is_copied_once_and_only_from_its_first_cell() {
        let mut app = alpha("tui-drag-wide");
        // Cell by cell, row 3 holds 한, a blank, 글, a blank, a space, 줄, a blank.
        assert_eq!(drag_across(&mut app, (61, 3), (67, 3)).as_deref(), Some("한글 줄"));
        // Starting on the cell that 한 covers leaves 한 out.
        assert_eq!(drag_across(&mut app, (62, 3), (64, 3)).as_deref(), Some("글"));
    }

    #[test]
    fn a_drag_stays_inside_the_preview_however_far_it_goes() {
        // Column 99 is the border and row 7 the keys: held to column 98, row 5.
        let mut app = alpha("tui-drag-far");
        let copied = drag_across(&mut app, (63, 1), (99, 7));
        assert_eq!(copied.as_deref(), Some("Alpha rule\nbody line\n한글 줄\n\n"));
    }

    #[test]
    fn a_plain_click_in_the_preview_copies_and_highlights_nothing() {
        let mut app = alpha("tui-drag-click");
        draw(&mut app, 100, 8);
        app.click(63, 1);
        let cells = draw(&mut app, 100, 8);
        assert!(!cells[(63, 1)].modifier.contains(Modifier::REVERSED));
        assert_eq!(app.release(Instant::now()), None);
    }

    #[test]
    fn a_drag_that_began_outside_the_preview_selects_nothing() {
        // A real drag reports every cell it crosses; two moves into the
        // preview, so that a selection begun by the first would show in the second.
        let mut app = alpha("tui-drag-outside");
        draw(&mut app, 100, 8);
        app.click(35, 1);
        app.drag(63, 1);
        app.drag(67, 1);
        let cells = draw(&mut app, 100, 8);
        assert!(!cells[(65, 1)].modifier.contains(Modifier::REVERSED));
        assert_eq!(app.release(Instant::now()), None);
    }

    #[test]
    fn nothing_is_copied_from_a_drag_over_blanks_alone() {
        let mut app = alpha("tui-drag-blank");
        assert_eq!(drag_across(&mut app, (70, 4), (80, 4)), None);
        // Across two blank rows as well: found in the author's terminal, where
        // the rows joined into a lone line break and "copied" showed.
        assert_eq!(drag_across(&mut app, (70, 4), (80, 5)), None);
    }

    #[test]
    fn the_dragged_cells_are_drawn_reversed_and_stay_so_after_letting_go() {
        let mut app = alpha("tui-drag-drawn");
        drag_across(&mut app, (63, 1), (67, 1));
        let cells = draw(&mut app, 100, 8);
        let reversed = |x: u16| cells[(x, 1)].modifier.contains(Modifier::REVERSED);
        assert!(!reversed(62));
        assert!((63..=67).all(reversed));
        assert!(!reversed(68));
    }

    #[test]
    fn a_key_or_another_click_lets_go_of_the_selection() {
        let mut app = alpha("tui-drag-clear");
        drag_across(&mut app, (63, 1), (67, 1));
        press(&mut app, &[KeyCode::Tab]);
        assert!(app.selection.is_none());

        drag_across(&mut app, (63, 1), (67, 1));
        click(&mut app, 35, 1);
        assert!(app.selection.is_none());
    }

    #[test]
    fn a_selection_is_cut_to_a_preview_that_shrank_under_it() {
        // At 80 columns the preview's inside ends at column 78, short of 90.
        let mut app = alpha("tui-drag-shrink");
        draw(&mut app, 100, 8);
        app.click(63, 1);
        app.drag(90, 1);
        draw(&mut app, 80, 8);
        assert_eq!(app.release(Instant::now()).as_deref(), Some("Alpha rule"));
    }

    #[test]
    fn a_selection_survives_the_preview_shrinking_to_nothing_under_it() {
        // 60 columns leave the preview none: the two panes left of it take them all.
        let mut app = alpha("tui-drag-vanish");
        draw(&mut app, 100, 8);
        app.click(63, 1);
        app.drag(67, 1);
        draw(&mut app, 60, 8);
        app.drag(70, 1);
        assert_eq!(app.release(Instant::now()), None);
    }

    #[test]
    fn a_wrapped_line_is_copied_as_the_rows_it_shows_on() {
        // 48 characters in a preview 38 wide: the screen, not the file, is read.
        let dir = scratch("tui-drag-wrapped");
        write(&dir.join("long.md"), "one two three four five six seven eight nine ten\n");
        let mut app = app(vec![Source::new("rules", dir, Scope::Global, Walk::MarkdownFiles)]);

        let copied = drag_across(&mut app, (61, 1), (98, 2)).unwrap();
        assert_eq!(copied, "one two three four five six seven\neight nine ten");
    }

    // The bottom row after a copy. `release` and `tick` are handed the time, so
    // a test can move it on without waiting.

    /// What the bottom row of a 100 x 8 screen shows.
    fn bottom_row(app: &mut App) -> String {
        screen(app, 100, 8)[7].clone()
    }

    #[test]
    fn a_copy_says_so_at_the_right_end_of_the_bottom_row_for_two_seconds() {
        let mut app = alpha("tui-copied");
        let start = Instant::now();
        draw(&mut app, 100, 8);
        app.click(63, 1);
        app.drag(67, 1);
        app.release(start);

        let row = bottom_row(&mut app);
        assert!(row.starts_with(KEYS), "{row:?}");
        assert!(row.ends_with(COPIED), "{row:?}");
        assert_eq!(app.wake_in(start), Some(COPIED_FOR));

        app.tick(start + COPIED_FOR - Duration::from_millis(1));
        assert!(bottom_row(&mut app).ends_with(COPIED));
        app.tick(start + COPIED_FOR);
        assert!(!bottom_row(&mut app).contains(COPIED.trim_end()));
        assert_eq!(app.wake_in(start + COPIED_FOR), None);
    }

    #[test]
    fn nothing_copied_says_nothing() {
        let mut app = alpha("tui-copied-not");
        drag_across(&mut app, (70, 4), (80, 4));
        drag_across(&mut app, (63, 1), (63, 1));
        assert!(!bottom_row(&mut app).contains(COPIED.trim_end()));
        assert_eq!(app.wake_in(Instant::now()), None);
    }

    #[test]
    fn osc52_carries_the_text_in_base64() {
        assert_eq!(osc52("hello"), "\x1b]52;c;aGVsbG8=\x07");
        assert_eq!(osc52("한글"), "\x1b]52;c;7ZWc6riA\x07");
    }

    // -------------------------------------------------------------- drawing

    #[test]
    fn the_three_panes_show_a_source_its_entries_and_the_first_file() {
        let dir = scratch("tui-panes");
        write(&dir.join("alpha.md"), "# Alpha rule\nbody line\n");

        let mut app = app(vec![Source::new("rules", dir, Scope::Global, Walk::MarkdownFiles)]);
        let rows = screen(&mut app, 100, 8).join("\n");

        assert!(rows.contains("rules:1"), "{rows}");
        assert!(rows.contains("alpha"), "{rows}");
        assert!(rows.contains("# Alpha rule"), "{rows}");
        assert!(rows.contains("body line"), "{rows}");
    }

    #[test]
    fn the_project_heading_comes_after_the_global_sources() {
        let dir = scratch("tui-order");
        let mut app = app(vec![
            Source::new("rules", dir.join("rules"), Scope::Global, Walk::MarkdownFiles),
            Source::new("docs", dir.join("docs"), Scope::Project, Walk::MarkdownTree),
        ]);
        let rows = screen(&mut app, 100, 8);

        // The Sources pane is the first 28 columns, and its border is one cell
        // on every side. Counted in characters: `│` is three bytes.
        let pane: Vec<String> = rows
            .iter()
            .map(|r| r.chars().skip(1).take(26).collect::<String>().trim_end().to_string())
            .collect();
        assert_eq!(pane[1..5], ["GLOBAL", "  rules:(missing)", "PROJECT here", "  docs:(missing)"]);
    }

    #[test]
    fn with_no_project_sources_the_project_heading_still_shows() {
        let dir = scratch("tui-noproject");
        let mut app = app(vec![Source::new("rules", dir, Scope::Global, Walk::MarkdownFiles)]);
        let rows = screen(&mut app, 100, 8).join("\n");
        assert!(rows.contains("PROJECT here"), "{rows}");
    }

    #[test]
    fn a_bundle_without_its_lead_says_so_in_the_preview() {
        let dir = scratch("tui-nolead");
        std::fs::create_dir_all(dir.join("beta")).unwrap();

        let mut app = app(vec![Source::new("skills", dir, Scope::Global, Walk::BundleDirs)]);
        let rows = screen(&mut app, 120, 8).join("\n");
        assert!(rows.contains("(this Bundle has no SKILL.md)"), "{rows}");
    }

    #[test]
    fn a_note_in_the_preview_is_cut_into_rows_that_fit() {
        // On a screen 80 wide, the preview has columns 61 to 78 inside.
        let dir = scratch("tui-nolead-narrow");
        std::fs::create_dir_all(dir.join("beta")).unwrap();
        let mut app = app(vec![Source::new("skills", dir, Scope::Global, Walk::BundleDirs)]);
        let cells = draw(&mut app, 80, 8);
        let row = |y: u16| (61..79).map(|x| cells[(x, y)].symbol()).collect::<String>().trim_end().to_string();
        assert_eq!([row(1), row(2)], ["(this Bundle has", "no SKILL.md)"]);
    }

    #[test]
    fn a_wide_character_at_the_end_of_a_row_leaves_the_border_alone() {
        // Seen in M5 with ratatui's wrapping: 33 cells and a space, then a
        // word five cells wide ending in 한, in a preview 38 wide — 한 went
        // into the last cell inside and blanked the border beside it.
        let dir = scratch("tui-wide-border");
        write(&dir.join("wide.md"), &format!("{} 789한 end\n", "x".repeat(33)));
        let mut app = app(vec![Source::new("rules", dir, Scope::Global, Walk::MarkdownFiles)]);

        let cells = draw(&mut app, 100, 8);
        // Rows 1 to 5 are inside the border.
        assert!((1..6).all(|y| cells[(99, y)].symbol() == "│"), "{:?}", screen(&mut app, 100, 8));
    }

    #[test]
    fn an_escape_sequence_in_a_name_never_reaches_the_screen() {
        let dir = scratch("tui-escape");
        write(&dir.join("x.md"), "---\nname: \u{1b}[31mred\n---\n");

        let mut app = app(vec![Source::new("rules", dir, Scope::Global, Walk::MarkdownFiles)]);
        let rows = screen(&mut app, 100, 8).join("\n");
        assert!(rows.contains("[31mred"), "{rows}");
        assert!(!rows.chars().any(|c| c != '\n' && c.is_control()), "{rows:?}");
    }

    /// Characters that are not control characters but still change how a
    /// terminal lays text out: a right-to-left override and a zero-width space.
    const FORMAT: [char; 2] = ['\u{202e}', '\u{200b}'];

    #[test]
    fn a_format_character_in_a_name_never_reaches_the_screen() {
        let dir = scratch("tui-format-name");
        write(&dir.join("x.md"), "---\nname: left\u{202e}right\u{200b}gap\n---\n");

        let mut app = app(vec![Source::new("rules", dir, Scope::Global, Walk::MarkdownFiles)]);
        let rows = screen(&mut app, 100, 8).join("\n");
        assert!(rows.contains("leftrightgap"), "{rows}");
        assert!(!rows.contains(FORMAT), "{rows:?}");
    }

    #[test]
    fn an_accent_in_a_name_stays_with_its_letter() {
        let dir = scratch("tui-accent-name");
        write(&dir.join("x.md"), "---\nname: cafe\u{301}\n---\n");

        let mut app = app(vec![Source::new("rules", dir, Scope::Global, Walk::MarkdownFiles)]);
        let rows = screen(&mut app, 100, 8).join("\n");
        assert!(rows.contains("cafe\u{301}"), "{rows:?}");
    }

    #[test]
    fn escapes_and_format_characters_in_a_file_never_reach_the_screen() {
        let dir = scratch("tui-format-file");
        write(&dir.join("x.md"), "\u{1b}[31mred left\u{202e}right\u{200b}gap\n");

        let mut app = app(vec![Source::new("rules", dir, Scope::Global, Walk::MarkdownFiles)]);
        let rows = screen(&mut app, 100, 8).join("\n");
        assert!(rows.contains("[31mred leftrightgap"), "{rows}");
        assert!(!rows.chars().any(|c| c != '\n' && c.is_control()), "{rows:?}");
        assert!(!rows.contains(FORMAT), "{rows:?}");
    }

    // ----------------------------------------------------------------- tree
    //
    // `nested` is a docs tree one row wide at every level, so that the order
    // `read_dir` gives — the platform's — cannot move a row:
    //
    //   guide/
    //     deep/
    //       end.md

    fn nested(name: &str) -> App {
        let dir = scratch(name);
        write(&dir.join("guide").join("deep").join("end.md"), "# end\n");
        nested_in(dir)
    }

    /// `dir` as a docs Source, with j and k handed to its Entries.
    fn nested_in(dir: PathBuf) -> App {
        let mut app = app(vec![Source::new("docs", dir, Scope::Project, Walk::MarkdownTree)]);
        press(&mut app, &[KeyCode::Tab]);
        app
    }

    /// The Entries pane's rows as drawn on a 100 x 12 screen — rows 1 to 9 and
    /// columns 29 to 58, inside its border — down to the last one that holds
    /// anything.
    fn tree(app: &mut App) -> Vec<String> {
        let rows: Vec<String> = screen(app, 100, 12)[1..10]
            .iter()
            .map(|r| r.chars().skip(29).take(30).collect::<String>().trim_end().to_string())
            .collect();
        let shown = rows.iter().rposition(|r| !r.is_empty()).map_or(0, |last| last + 1);
        rows[..shown].to_vec()
    }

    #[test]
    fn a_tree_starts_closed_and_l_opens_the_selected_row() {
        let mut app = nested("tui-tree-open");
        assert_eq!(tree(&mut app), ["▸ guide/"]);
        press(&mut app, &[KeyCode::Char('l')]);
        assert_eq!(tree(&mut app), ["▾ guide/", "  ▸ deep/"]);
        press(&mut app, &[KeyCode::Char('j'), KeyCode::Right]);
        assert_eq!(tree(&mut app), ["▾ guide/", "  ▾ deep/", "      end"]);
    }

    #[test]
    fn h_closes_an_open_row_and_from_any_other_goes_up_to_its_parent() {
        let mut app = nested("tui-tree-close");
        press(&mut app, &[KeyCode::Char('l'), KeyCode::Char('j'), KeyCode::Char('l'), KeyCode::Char('j')]);
        assert_eq!(app.entry().unwrap().name, "end");

        // `end` has nothing below it: up to `deep/`, which is open, so closed.
        press(&mut app, &[KeyCode::Char('h')]);
        assert_eq!(app.entries.selected(), Some(1));
        press(&mut app, &[KeyCode::Left]);
        assert_eq!(tree(&mut app), ["▾ guide/", "  ▸ deep/"]);

        // `deep/` is closed now: up to `guide/`, closed, and nothing is above it.
        press(&mut app, &[KeyCode::Char('h')]);
        assert_eq!(app.entries.selected(), Some(0));
        press(&mut app, &[KeyCode::Char('h'), KeyCode::Char('h')]);
        assert_eq!(tree(&mut app), ["▸ guide/"]);
        assert_eq!(app.entries.selected(), Some(0));
    }

    #[test]
    fn h_goes_up_to_the_parent_not_to_the_row_above() {
        // Two rows inside `guide/`, and one beside it. Rows are found by name,
        // since the order `read_dir` gives is the platform's.
        let dir = scratch("tui-tree-parent");
        write(&dir.join("guide").join("a.md"), "# a\n");
        write(&dir.join("guide").join("b.md"), "# b\n");
        write(&dir.join("top.md"), "# top\n");
        let mut app = nested_in(dir);
        let guide = tree(&mut app).iter().position(|r| r == "▸ guide/").unwrap();
        app.entries.select(Some(guide));
        press(&mut app, &[KeyCode::Char('l')]);

        // The second row inside `guide/`: the row above it is its sibling.
        app.entries.select(Some(guide + 2));
        press(&mut app, &[KeyCode::Char('h')]);
        assert_eq!(app.entries.selected(), Some(guide));

        // A row at the top has nothing to go up to.
        let top = tree(&mut app).iter().position(|r| r == "  top").unwrap();
        app.entries.select(Some(top));
        press(&mut app, &[KeyCode::Char('h')]);
        assert_eq!(app.entries.selected(), Some(top));
    }

    #[test]
    fn l_or_enter_on_a_row_with_nothing_below_leaves_h_to_go_up() {
        let mut app = nested("tui-tree-leaf");
        press(&mut app, &[KeyCode::Char('l'), KeyCode::Char('j'), KeyCode::Char('l'), KeyCode::Char('j')]);
        assert_eq!(app.entry().unwrap().name, "end");
        press(&mut app, &[KeyCode::Char('l'), KeyCode::Enter, KeyCode::Char('h')]);
        assert_eq!(app.entries.selected(), Some(1));
    }

    #[test]
    fn h_inside_a_bundle_that_would_not_open_goes_up_to_the_bundle() {
        // The row saying the directory would not open has the Bundle's path.
        let dir = scratch("tui-tree-held-bundle");
        write(&dir.join("alpha").join("SKILL.md"), "---\nname: alpha\n---\n");
        write(&dir.join("alpha").join("REFERENCE.md"), "# supporting\n");
        let Some(_held) = hold(&dir.join("alpha")) else { return };

        let mut app = app(vec![Source::new("skills", dir, Scope::Global, Walk::BundleDirs)]);
        press(&mut app, &[KeyCode::Tab, KeyCode::Char('l'), KeyCode::Char('j')]);
        assert_eq!(tree(&mut app), ["▾ alpha", "    alpha (unreadable)"]);
        press(&mut app, &[KeyCode::Char('h')]);
        assert_eq!(app.entries.selected(), Some(0));
        assert_eq!(tree(&mut app), ["▾ alpha", "    alpha (unreadable)"]);

        // Nor do l and Enter on that row reach the Bundle.
        press(&mut app, &[KeyCode::Char('j'), KeyCode::Char('l'), KeyCode::Enter]);
        assert_eq!(tree(&mut app), ["▾ alpha", "    alpha (unreadable)"]);
    }

    #[test]
    fn a_bundle_that_would_not_open_is_where_it_was_left_after_the_row_under_it() {
        // That row has the Bundle's path too, and shows a note, not a file.
        let dir = scratch("tui-tree-held-scroll");
        let items: String = (1..=12).map(|n| format!("- {n}\n")).collect();
        write(&dir.join("alpha").join("SKILL.md"), &format!("---\nname: alpha\n---\n{items}"));
        write(&dir.join("alpha").join("REFERENCE.md"), "# supporting\n");
        let Some(_held) = hold(&dir.join("alpha")) else { return };

        let mut app = app(vec![Source::new("skills", dir, Scope::Global, Walk::BundleDirs)]);
        draw(&mut app, 100, 8);
        press(&mut app, &[KeyCode::Tab, KeyCode::PageDown]);
        assert_eq!(first_row(&mut app), "• 5");
        press(&mut app, &[KeyCode::Char('l'), KeyCode::Char('j'), KeyCode::PageDown]);
        assert_eq!(first_row(&mut app), "(this could not be read: unreadable)");
        // A drag over the note is counted from where the note is drawn.
        let note = drag_across(&mut app, (61, 1), (96, 1));
        assert_eq!(note.as_deref(), Some("(this could not be read: unreadable)"));
        // The click handed j and k to the preview; round to the Entries.
        press(&mut app, &[KeyCode::Tab, KeyCode::Tab]);
        assert!(app.focus == Pane::Entries);
        press(&mut app, &[KeyCode::Char('k')]);
        assert_eq!(first_row(&mut app), "• 5");
    }

    #[test]
    fn enter_opens_a_row_and_closes_it_again() {
        let mut app = nested("tui-tree-enter");
        press(&mut app, &[KeyCode::Enter]);
        assert_eq!(tree(&mut app), ["▾ guide/", "  ▸ deep/"]);
        press(&mut app, &[KeyCode::Enter]);
        assert_eq!(tree(&mut app), ["▸ guide/"]);
    }

    #[test]
    fn the_tree_keys_do_nothing_in_the_preview_or_the_sources_pane() {
        let mut app = nested("tui-tree-sources");
        press(&mut app, &[KeyCode::Tab]);
        assert!(app.focus == Pane::Preview);
        press(&mut app, &[KeyCode::Char('l'), KeyCode::Right, KeyCode::Enter]);
        assert_eq!(tree(&mut app), ["▸ guide/"]);

        press(&mut app, &[KeyCode::Tab]);
        assert!(app.focus == Pane::Sources);
        press(&mut app, &[KeyCode::Char('l'), KeyCode::Right, KeyCode::Enter]);
        assert_eq!(tree(&mut app), ["▸ guide/"]);
    }

    #[test]
    fn a_directory_row_says_so_in_the_preview() {
        let mut app = nested("tui-tree-preview");
        let rows = screen(&mut app, 100, 8).join("\n");
        assert!(rows.contains("(a directory)"), "{rows}");
    }

    #[test]
    fn a_click_selects_a_row_inside_an_open_directory() {
        let mut app = nested("tui-tree-click");
        press(&mut app, &[KeyCode::Char('l'), KeyCode::Char('j'), KeyCode::Char('l'), KeyCode::Char('k')]);
        click(&mut app, 35, 3);
        assert_eq!(app.entry().unwrap().name, "end");
    }

    #[test]
    fn the_count_is_of_entries_however_many_rows_are_open() {
        let mut app = nested("tui-tree-count");
        press(&mut app, &[KeyCode::Char('l'), KeyCode::Char('j'), KeyCode::Char('l')]);
        assert_eq!(tree(&mut app).len(), 3);
        let rows = screen(&mut app, 100, 12).join("\n");
        assert!(rows.contains("docs:1"), "{rows}");
    }

    #[test]
    fn a_row_stays_open_while_another_source_is_looked_at() {
        let dir = scratch("tui-tree-remember");
        write(&dir.join("rules").join("one.md"), "# one\n");
        write(&dir.join("docs").join("guide").join("end.md"), "# end\n");
        let mut app = app(vec![
            Source::new("rules", dir.join("rules"), Scope::Global, Walk::MarkdownFiles),
            Source::new("docs", dir.join("docs"), Scope::Project, Walk::MarkdownTree),
        ]);
        press(&mut app, &[KeyCode::Char('j'), KeyCode::Tab, KeyCode::Char('l')]);
        // Round past the preview to the Sources, up to `rules`, and back.
        press(&mut app, &[KeyCode::Tab, KeyCode::Tab, KeyCode::Char('k')]);
        assert_eq!(app.source, 0);
        press(&mut app, &[KeyCode::Char('j'), KeyCode::Tab]);
        assert_eq!(tree(&mut app), ["▾ guide/", "    end"]);
    }

    #[test]
    fn a_bundle_opens_onto_its_supporting_files() {
        let dir = scratch("tui-tree-bundle");
        write(&dir.join("alpha").join("SKILL.md"), "---\nname: alpha\n---\n");
        write(&dir.join("alpha").join("REFERENCE.md"), "# supporting\n");
        let mut app = app(vec![Source::new("skills", dir, Scope::Global, Walk::BundleDirs)]);
        press(&mut app, &[KeyCode::Tab]);

        assert_eq!(tree(&mut app), ["▸ alpha"]);
        press(&mut app, &[KeyCode::Char('l'), KeyCode::Char('j')]);
        assert_eq!(tree(&mut app), ["▾ alpha", "    REFERENCE"]);
        assert_eq!(app.entry().unwrap().name, "REFERENCE");
    }

    #[test]
    fn a_bundle_holding_only_its_lead_has_nothing_to_open() {
        let dir = scratch("tui-tree-leadonly");
        write(&dir.join("dream").join("SKILL.md"), "---\nname: dream\n---\n");
        let mut app = app(vec![Source::new("skills", dir, Scope::Global, Walk::BundleDirs)]);
        press(&mut app, &[KeyCode::Tab, KeyCode::Char('l'), KeyCode::Enter]);
        assert_eq!(tree(&mut app), ["  dream"]);
    }

    #[test]
    fn what_could_not_be_read_is_a_row_that_says_why() {
        let dir = scratch("tui-tree-unread");
        write(&dir.join("guide").join("secret").join("end.md"), "# end\n");
        let Some(_held) = hold(&dir.join("guide").join("secret")) else { return };

        let mut app = nested_in(dir);
        press(&mut app, &[KeyCode::Char('l'), KeyCode::Char('j')]);
        assert_eq!(tree(&mut app), ["▾ guide/", "    secret (unreadable)"]);
        let rows = screen(&mut app, 100, 12).join("\n");
        assert!(rows.contains("docs:0 (1 unreadable)"), "{rows}");
        assert!(rows.contains("(this could not be read: unreadable)"), "{rows}");
    }

    #[test]
    fn a_bundle_reached_through_a_link_says_so_and_does_not_open() {
        let dir = scratch("tui-tree-link");
        let skills = dir.join("skills");
        std::fs::create_dir_all(&skills).unwrap();
        write(&dir.join("target").join("SKILL.md"), "---\nname: linked\n---\nread through the link\n");
        write(&dir.join("target").join("FORMAT.md"), "# supporting\n");
        if !link_dir(&dir.join("target"), &skills.join("linked")) {
            return;
        }

        let mut app = app(vec![Source::new("skills", skills, Scope::Global, Walk::BundleDirs)]);
        press(&mut app, &[KeyCode::Tab, KeyCode::Char('l'), KeyCode::Enter]);
        assert_eq!(tree(&mut app), ["  linked (link)"]);
        let rows = screen(&mut app, 100, 12).join("\n");
        assert!(rows.contains("read through the link"), "{rows}");
    }
}
