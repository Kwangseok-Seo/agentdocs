use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::panic;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, List, ListState, Paragraph, Widget};
use ratatui::{DefaultTerminal, Frame};
use unicode_width::UnicodeWidthStr;

use crate::config::Problem;
use crate::editor;
use crate::entry::{Entry, EntryKind, Node};
use crate::listing::{failed, heading, printable, reason, unused, unused_said};
use crate::markdown;
use crate::source::{Scope, Source, Walked, same};

/// The keys the screen answers to, shown along its bottom row. The arrows
/// that do what `l` and `h` do, Enter, and clicking to select are left out:
/// with them the row runs into a notice on a screen 100 columns wide.
const KEYS: &str = " j/k ↓/↑ move   l/h open/close   tab pane   e edit   drag copy   q quit";

/// What the bottom row says after a drag is copied — herdr's words.
const COPIED: &str = "copied to clipboard";

/// What the bottom row says, whenever nothing else is said there, when the
/// screen could not set up watching: what changes on disk is not shown
/// until the next start.
const UNWATCHED: &str = "not watching for changes";

/// What it says when the system would not take another watch — on Linux,
/// its limit on inotify watches — so that a change in some directory may
/// not show.
const LIMITED: &str = "watch limit reached";

/// How long a notice stays at the right end of the bottom row — herdr's two
/// seconds.
const NOTICE_FOR: Duration = Duration::from_secs(2);

/// How many rows one notch of the wheel scrolls the preview — herdr's default.
const WHEEL_ROWS: isize = 3;

/// How often the preview scrolls a row toward a pointer dragged above or
/// below it — herdr's interval.
const AUTOSCROLL_EVERY: Duration = Duration::from_millis(30);

/// How long after a file is seen to change the Sources are read again. One
/// save arrives as several changes — eight, for a file written aside and
/// renamed over the old one — and is read once they are over.
const SETTLE: Duration = Duration::from_millis(100);

/// What the screen shows, and which part of it is selected. Keys and the mouse
/// change it; drawing only reads it — and notes where and what it drew, for
/// the mouse.
pub struct App {
    global: String,
    project: String,
    sources: Vec<(Source, io::Result<Walked>)>,
    /// Each config file that could not be used, and the scope whose Sources
    /// it would have added.
    problems: Vec<(Scope, Problem)>,
    /// The selected row of the Sources pane: a Source by its index, or past
    /// the last Source, a config file that could not be used, by its index
    /// among those.
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
    /// What the bottom row says at its right end, for a while.
    notice: Option<Notice>,
    /// When the Sources are next read again, once a file has been seen to
    /// change.
    reload_at: Option<Instant>,
    /// Whether changes on disk are heard.
    watching: Watching,
}

/// How much of what the screen shows is watched. Once less than all, it
/// stays so for as long as the screen is open: a watch the system refused
/// may have missed a change already.
#[derive(Clone, Copy, PartialEq)]
enum Watching {
    All,
    /// The system would not take another watch.
    Limited,
    /// No watcher could be made.
    Nothing,
}

/// A few words at the right end of the bottom row: that a drag was copied, or
/// how the editor ended when it did not end well.
struct Notice {
    text: String,
    colour: Color,
    until: Instant,
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
    pub fn new(sources: Vec<Source>, problems: Vec<(Scope, Problem)>, global: String, project: String) -> Self {
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
            problems,
            source: 0,
            entries: ListState::default().with_selected(Some(0)),
            open: HashSet::new(),
            scrolled: HashMap::new(),
            furthest: 0,
            focus: Pane::Sources,
            areas: [Rect::default(); 3],
            source_offset: 0,
            selection: None,
            notice: None,
            reload_at: None,
            watching: Watching::All,
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
        self.notice = Some(Notice { text: COPIED.to_string(), colour: Color::Green, until: now + NOTICE_FOR });
        Some(text)
    }

    /// Whatever has come due by `now`: a notice comes down once its time is
    /// up, the Sources are read again once a change has settled, and a drag
    /// held above or below the preview scrolls it a row toward the pointer,
    /// every `AUTOSCROLL_EVERY`.
    fn tick(&mut self, now: Instant) {
        if self.notice.as_ref().is_some_and(|notice| now >= notice.until) {
            self.notice = None;
        }
        if self.reload_at.is_some_and(|at| now >= at) {
            self.reload_at = None;
            self.reload();
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

    /// How long the loop may wait for a key, the mouse or a change before
    /// something comes due: a notice coming down, the Sources read again, or
    /// the next row of a scroll toward the pointer. `None` when nothing will.
    fn wake_in(&self, now: Instant) -> Option<Duration> {
        let notice = self.notice.as_ref().map(|notice| notice.until.saturating_duration_since(now));
        let reload = self.reload_at.map(|at| at.saturating_duration_since(now));
        let scroll = self.autoscroll().map(|_| {
            let at = self.selection.and_then(|selection| selection.scroll_at);
            at.map_or(Duration::ZERO, |at| at.saturating_duration_since(now))
        });
        [notice, reload, scroll].into_iter().flatten().min()
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

    /// The file `e` hands to the editor: the selected Entry's own, or its
    /// Bundle's Lead. A directory, something that could not be read, and a
    /// Bundle with no Lead have none.
    fn editable(&self) -> Option<&Path> {
        self.entry()?.doc()
    }

    /// The editor has finished, one way or another. Every Source is read
    /// again, since the file may be shown by more than one — a linked Bundle
    /// is — and when the editor did not end well, the bottom row says how.
    fn edited(&mut self, ended: io::Result<ExitStatus>, now: Instant) {
        self.reload();
        let text = match ended {
            Ok(status) if status.success() => return,
            Ok(status) => format!("editor: {status}"),
            Err(e) => format!("editor: {e}"),
        };
        self.notice = Some(Notice { text, colour: Color::Red, until: now + NOTICE_FOR });
    }

    /// A file may have changed: read the Sources again once the change has
    /// settled. Changes that follow do not put it off.
    fn changed(&mut self, now: Instant) {
        self.reload_at.get_or_insert(now + SETTLE);
    }

    /// No watcher could be made: the screen works as it did before M9, and
    /// says so for as long as it is open.
    fn unwatched(&mut self) {
        self.watching = Watching::Nothing;
    }

    /// The system would not take another watch: some changes may not show,
    /// and the screen says so for as long as it is open.
    fn limited(&mut self) {
        if self.watching == Watching::All {
            self.watching = Watching::Limited;
        }
    }

    /// Walk every Source again, and take what a Walk finds where it differs
    /// from what the screen has. Which rows are open and where each file was
    /// scrolled to are kept by path, so they hold for whatever the Walk finds
    /// now; so is the selected row, which otherwise stays in its place and
    /// moves up to the last row when fewer are left. Where nothing differs,
    /// nothing moves — text dragged over stays selected.
    fn reload(&mut self) {
        let selected = self.row().map(|row| row.node.path().to_path_buf());
        let mut differs = false;
        for (src, walked) in &mut self.sources {
            let now = src.entries();
            if !same(walked, &now) {
                *walked = now;
                differs = true;
            }
        }
        if !differs {
            return;
        }

        let found = selected.and_then(|path| self.rows().iter().position(|row| row.node.path() == path));
        let last = self.rows().len().saturating_sub(1);
        match found {
            Some(at) => self.entries.select(Some(at)),
            None if self.entries.selected().is_some_and(|at| at > last) => self.entries.select(Some(last)),
            None => {}
        }
        self.selection = None;
    }

    /// Everything to watch for a change the screen would show, from every
    /// Source, each path once: notify keeps one watch to a path, so giving up
    /// one of two would give up both.
    fn watched(&self) -> Vec<PathBuf> {
        let mut all: Vec<_> = self.sources.iter().flat_map(|(src, walked)| src.watched(walked)).collect();
        all.sort();
        all.dedup();
        all
    }

    /// One row down in the focused pane, staying put on the last row.
    fn down(&mut self) {
        match self.focus {
            Pane::Sources => {
                let picks = self.picks();
                if let Some(&next) = picks.iter().skip_while(|&&pick| pick != self.source).nth(1) {
                    self.pick_source(next);
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
                let picks = self.picks();
                if let Some(&before) = picks.iter().rev().skip_while(|&&pick| pick != self.source).nth(1) {
                    self.pick_source(before);
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
        let said = match (&self.notice, self.watching) {
            (Some(notice), _) => Some((notice.text.as_str(), notice.colour)),
            (None, Watching::Nothing) => Some((UNWATCHED, Color::Yellow)),
            (None, Watching::Limited) => Some((LIMITED, Color::Yellow)),
            (None, Watching::All) => None,
        };
        if let Some((text, colour)) = said {
            let line = Line::from(format!("{text} ")).right_aligned().style(Style::new().fg(colour));
            frame.render_widget(line, footer);
        }
    }

    /// The Sources pane row by row: the scope headings, and under them a line
    /// for each Source, worded exactly as the listing words it, together with
    /// that Source's index. Drawing and clicking both read this, so a click
    /// lands on the row that was drawn. A config file that could not be used
    /// has a row at the end of its scope, where its Sources would have been,
    /// numbered after the last Source.
    fn source_rows(&self) -> Vec<(String, Option<usize>)> {
        let mut rows = vec![(self.global.clone(), None)];
        let mut project_shown = false;

        for (i, (src, walked)) in self.sources.iter().enumerate() {
            if matches!(src.scope, Scope::Project) && !project_shown {
                rows.extend(self.unused_rows(Scope::Global));
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
            rows.extend(self.unused_rows(Scope::Global));
            rows.push((self.project.clone(), None));
        }
        rows.extend(self.unused_rows(Scope::Project));
        rows
    }

    /// The row of each config file of `scope` that could not be used.
    fn unused_rows(&self, scope: Scope) -> impl Iterator<Item = (String, Option<usize>)> + '_ {
        let after = self.sources.len();
        self.problems
            .iter()
            .enumerate()
            .filter(move |(_, (of, _))| *of == scope)
            .map(move |(k, (_, problem))| (unused(problem), Some(after + k)))
    }

    /// The selectable rows of the Sources pane, top to bottom: what j and k
    /// step through.
    fn picks(&self) -> Vec<usize> {
        self.source_rows().into_iter().filter_map(|(_, pick)| pick).collect()
    }

    /// The config file that could not be used, when its row is the one
    /// selected.
    fn problem(&self) -> Option<&Problem> {
        let k = self.source.checked_sub(self.sources.len())?;
        self.problems.get(k).map(|(_, problem)| problem)
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
    /// wide: its file drawn as Markdown, or a line saying why there is none —
    /// or, for a config file that could not be used, what stopped it.
    fn preview_lines(&self, width: u16) -> Vec<Line<'_>> {
        if let Some(problem) = self.problem() {
            return match problem {
                Problem::Read(e) => markdown::note(format!("(this could not be read: {})", reason(e.kind())), width),
                Problem::Parse(_) | Problem::Order(_) => {
                    unused_said(problem).into_iter().flat_map(|line| markdown::note(line, width)).collect()
                }
            };
        }
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
        // Only a file keeps a place. A note, however tall, cannot be
        // scrolled, so it is already as far as it goes.
        self.furthest = if path.is_some() { most } else { 0 };
        if let Some(path) = path {
            self.scrolled.insert(path, top);
        }

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

/// Hand the terminal to the editor for `path`, wait until it is done, and
/// take the terminal back. Only one program can own a terminal's modes at a
/// time, so what `open` sets up is taken down first — mouse reporting before
/// raw mode, as there — and set up again after, in the order `open` sets it
/// up. The editor drew over the screen, so all of it is drawn afresh.
///
/// How the editor ended is handed back as it is, for the screen to report:
/// failing to take the terminal back is the only error that ends the screen.
fn edit(terminal: &mut DefaultTerminal, path: &Path) -> io::Result<io::Result<ExitStatus>> {
    execute!(io::stdout(), DisableMouseCapture)?;
    ratatui::try_restore()?;

    let ended = editor::command(path).status();

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    terminal.clear()?;
    Ok(ended)
}

/// What the loop waits for: something from the terminal, or word that a file
/// may have changed.
enum Message {
    Input(io::Result<Event>),
    Changed,
    /// notify could not watch a directory that appeared below one it
    /// watches whole: the system would not take another watch.
    Limited,
}

/// Read the terminal with `read` on a thread of its own, one event at a time,
/// and hand each to the loop. The next is not read until the loop says so on
/// `go_on`: while the editor has the terminal, the loop does not, and every
/// key is the editor's. A thread reading all the while left the editor
/// "hello" whole in 5 of 20 tries, and in 7 took its `e`, which opened the
/// editor a second time (M9).
fn read_input(mut read: impl FnMut() -> io::Result<Event>, to_loop: Sender<Message>, go_on: Receiver<()>) {
    loop {
        if to_loop.send(Message::Input(read())).is_err() {
            return;
        }
        if go_on.recv().is_err() {
            return;
        }
    }
}

/// Watch each path `wanted` names, alone, and stop watching what it no longer
/// names. What cannot be watched — a link whose target is gone — is left out
/// of `watching`, and so tried again each time round, until it can be or is
/// no longer wanted. Whether the system refused a watch for being at its
/// limit is handed back, for the screen to say.
fn rewatch(watcher: &mut impl Watcher, watching: &mut Vec<PathBuf>, wanted: Vec<PathBuf>) -> bool {
    if *watching == wanted {
        return false;
    }
    for path in watching.iter().filter(|path| !wanted.contains(path)) {
        let _ = watcher.unwatch(path);
    }
    let mut now = Vec::new();
    let mut limited = false;
    for path in wanted {
        let made = watching.contains(&path)
            || match watcher.watch(&path, RecursiveMode::NonRecursive) {
                Ok(()) => true,
                Err(e) => {
                    limited |= matches!(e.kind, notify::ErrorKind::MaxFilesWatch);
                    false
                }
            };
        if made {
            now.push(path);
        }
    }
    *watching = now;
    limited
}

/// A watcher that tells the loop over `to_loop` when a file may have changed.
///
/// A link is never walked into (ADR-0007), so it is not watched into either.
/// Nothing is watched whole, which is where notify would follow one on Linux
/// and the BSDs, and it is told not to all the same. The links a Walk reads
/// through are watched by their own paths.
fn watcher(to_loop: Sender<Message>) -> notify::Result<RecommendedWatcher> {
    let config = notify::Config::default().with_follow_symlinks(false);
    RecommendedWatcher::new(tell(to_loop), config)
}

/// What notify calls with each thing it reports: word to the loop of anything
/// that could change what the screen shows — anything but a file being
/// opened, read or closed. On Linux notify reports every open, the Walks' own
/// among them, and taken for changes they would have the Sources read again
/// for ever. A watch that failed may have missed something, so it counts —
/// and one the system refused at its limit is said apart.
fn tell(to_loop: Sender<Message>) -> impl FnMut(notify::Result<notify::Event>) + Send + 'static {
    move |event| {
        let message = match &event {
            Ok(e) if e.kind.is_access() => return,
            Err(e) if matches!(e.kind, notify::ErrorKind::MaxFilesWatch) => Message::Limited,
            _ => Message::Changed,
        };
        let _ = to_loop.send(message);
    }
}

/// Draw, wait for the terminal or a change, act on it — and again, until `q`.
///
/// Two threads feed the loop: one reads the terminal, and one — notify's —
/// hears from the system that a file changed. Each hands what it has over the
/// same channel, so the loop waits in one place for whichever comes first.
fn run(terminal: &mut DefaultTerminal, mut app: App) -> io::Result<()> {
    let (to_loop, messages) = mpsc::channel();
    let (go_on, gone_on) = mpsc::channel();

    let input = to_loop.clone();
    thread::spawn(move || read_input(event::read, input, gone_on));

    // A watcher that cannot be made — on Linux, once the system's limit on
    // them is reached — leaves the screen as it was before M9, which says so,
    // rather than no screen at all.
    let mut watcher = match watcher(to_loop) {
        Ok(watcher) => Some(watcher),
        Err(_) => {
            app.unwatched();
            None
        }
    };
    let mut watching = Vec::new();

    loop {
        // What is watched follows what the Walks just read: a directory that
        // has appeared is watched before the loop waits, and not only once
        // something else has been heard.
        app.tick(Instant::now());
        if let Some(watcher) = &mut watcher {
            if rewatch(watcher, &mut watching, app.watched()) {
                app.limited();
            }
        }
        terminal.draw(|frame| app.render(frame))?;

        // While something is due — "copied to clipboard" coming down, the
        // Sources read again, the next row of a scroll toward the pointer —
        // wait no longer than until then: when the time runs out with nothing
        // arrived, go round again so that `tick` sees to it.
        let waited = match app.wake_in(Instant::now()) {
            Some(wait) => messages.recv_timeout(wait),
            None => messages.recv().map_err(RecvTimeoutError::from),
        };
        let message = match waited {
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout) => continue,
            // Each thread keeps its way in until the loop is over.
            Err(RecvTimeoutError::Disconnected) => return Err(io::Error::other("nothing left to wait for")),
        };
        let event = match message {
            Message::Changed => {
                app.changed(Instant::now());
                continue;
            }
            // A directory appeared that could not be watched: it is read
            // again as any change is, and the screen says what it missed.
            Message::Limited => {
                app.limited();
                app.changed(Instant::now());
                continue;
            }
            Message::Input(event) => event?,
        };

        match event {
            // Windows reports a key going up as well as going down, so every
            // keystroke arrives twice. Only the press counts.
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if key.code == KeyCode::Char('q') {
                    return Ok(());
                }
                app.handle(key.code);
                // The editor needs the terminal, which the App never touches.
                if key.code == KeyCode::Char('e') {
                    if let Some(path) = app.editable() {
                        let ended = edit(terminal, path)?;
                        app.edited(ended, Instant::now());
                    }
                }
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
        // Done with it, the editor included: the next may be read.
        let _ = go_on.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
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
        App::new(sources, Vec::new(), "GLOBAL".to_string(), "PROJECT here".to_string())
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
        // Written out, rather than compared with the `KEYS` that drew it.
        let mut app = three_rules("tui-keys");
        let rows = screen(&mut app, 100, 8);
        assert_eq!(rows[7].trim_end(), " j/k ↓/↑ move   l/h open/close   tab pane   e edit   drag copy   q quit");
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
    fn a_drag_held_below_a_note_taller_than_the_preview_wakes_nothing() {
        // A config file's words are a note: taller than this preview, and with
        // no place to scroll, so there is nothing to wake the loop for.
        let mut app = with_unused_config("tui-note-drag", "[[source]]\nname = \"s\"\npath = \"s\"\nwalk = \"tree\"\n");
        press(&mut app, &[KeyCode::Char('j')]);
        draw(&mut app, 120, 6);
        app.click(61, 1);
        app.drag(70, 5);
        draw(&mut app, 120, 6);
        assert_eq!(app.autoscroll(), None);
        assert_eq!(app.wake_in(Instant::now()), None);
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
        assert_eq!(app.wake_in(start), Some(NOTICE_FOR));
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
        assert!(row.ends_with("copied to clipboard "), "{row:?}");
        assert_eq!(app.wake_in(start), Some(NOTICE_FOR));

        app.tick(start + NOTICE_FOR - Duration::from_millis(1));
        assert!(bottom_row(&mut app).ends_with("copied to clipboard "));
        app.tick(start + NOTICE_FOR);
        assert!(!bottom_row(&mut app).contains(COPIED));
        assert_eq!(app.wake_in(start + NOTICE_FOR), None);
    }

    #[test]
    fn a_screen_that_cannot_watch_says_so_whenever_nothing_else_is_said() {
        let mut app = alpha("tui-unwatched");
        assert!(!bottom_row(&mut app).contains(UNWATCHED));
        app.unwatched();
        let row = bottom_row(&mut app);
        assert!(row.starts_with(KEYS) && row.ends_with("not watching for changes "), "{row:?}");

        // A copy is said in its place, for as long as a copy is said.
        let start = Instant::now();
        draw(&mut app, 100, 8);
        app.click(63, 1);
        app.drag(67, 1);
        app.release(start);
        assert!(bottom_row(&mut app).ends_with("copied to clipboard "));
        app.tick(start + NOTICE_FOR);
        assert!(bottom_row(&mut app).ends_with("not watching for changes "));
    }

    #[test]
    fn a_watch_the_system_refused_is_said_and_stays_said() {
        let mut app = alpha("tui-limited");
        app.limited();
        assert!(bottom_row(&mut app).ends_with("watch limit reached "));
        // No watcher at all says more, and is not taken back by a refusal.
        app.unwatched();
        app.limited();
        assert!(bottom_row(&mut app).ends_with("not watching for changes "));
    }

    /// A watcher that refuses every path, as notify does when the system
    /// will not take another watch — or, `at_limit` false, when a path is
    /// not there.
    struct Refusing {
        at_limit: bool,
    }

    impl Watcher for Refusing {
        fn new<F: notify::EventHandler>(_: F, _: notify::Config) -> notify::Result<Self> {
            Err(notify::Error::generic("made by hand in the tests"))
        }
        fn watch(&mut self, _: &Path, _: RecursiveMode) -> notify::Result<()> {
            let kind = if self.at_limit { notify::ErrorKind::MaxFilesWatch } else { notify::ErrorKind::PathNotFound };
            Err(notify::Error::new(kind))
        }
        fn unwatch(&mut self, _: &Path) -> notify::Result<()> {
            Ok(())
        }
        fn kind() -> notify::WatcherKind {
            notify::WatcherKind::NullWatcher
        }
    }

    #[test]
    fn a_watch_refused_at_the_systems_limit_is_handed_back_and_one_not_there_is_not() {
        // Found in review: on Linux at the limit of inotify watches, the
        // refusal was dropped with every other, and the screen said nothing.
        let wanted = vec![PathBuf::from("rules"), PathBuf::from("skills")];
        let mut watching = Vec::new();
        assert!(rewatch(&mut Refusing { at_limit: true }, &mut watching, wanted.clone()));
        assert!(watching.is_empty());
        assert!(!rewatch(&mut Refusing { at_limit: false }, &mut watching, wanted));
    }

    #[test]
    fn a_directory_notify_could_not_watch_is_told_as_the_limit() {
        let (to_loop, messages) = mpsc::channel();
        let mut told = tell(to_loop);
        told(Err(notify::Error::new(notify::ErrorKind::MaxFilesWatch)));
        assert!(matches!(messages.try_recv(), Ok(Message::Limited)));
        told(Err(notify::Error::generic("a watch was lost")));
        assert!(matches!(messages.try_recv(), Ok(Message::Changed)));
    }

    #[test]
    fn nothing_copied_says_nothing() {
        let mut app = alpha("tui-copied-not");
        drag_across(&mut app, (70, 4), (80, 4));
        drag_across(&mut app, (63, 1), (63, 1));
        assert!(!bottom_row(&mut app).contains(COPIED));
        assert_eq!(app.wake_in(Instant::now()), None);
    }

    #[test]
    fn osc52_carries_the_text_in_base64() {
        assert_eq!(osc52("hello"), "\x1b]52;c;aGVsbG8=\x07");
        assert_eq!(osc52("한글"), "\x1b]52;c;7ZWc6riA\x07");
    }

    // ----------------------------------------------------------- the editor
    //
    // Running it needs a terminal, so the loop does that; what the App decides
    // before and after is tested here.

    /// How a process that exited with `code` ended.
    fn exit(code: u32) -> ExitStatus {
        #[cfg(windows)]
        let status = std::os::windows::process::ExitStatusExt::from_raw(code);
        #[cfg(unix)]
        let status = std::os::unix::process::ExitStatusExt::from_raw((code as i32) << 8);
        status
    }

    #[test]
    fn e_hands_over_a_files_own_path_and_a_bundles_lead() {
        let dir = scratch("tui-edit-paths");
        write(&dir.join("alpha").join("SKILL.md"), "---\nname: alpha\n---\n");
        write(&dir.join("alpha").join("REFERENCE.md"), "# supporting\n");
        let mut app = app(vec![Source::new("skills", dir.clone(), Scope::Global, Walk::BundleDirs)]);

        assert_eq!(app.editable(), Some(dir.join("alpha").join("SKILL.md").as_path()));
        press(&mut app, &[KeyCode::Tab, KeyCode::Char('l'), KeyCode::Char('j')]);
        assert_eq!(app.editable(), Some(dir.join("alpha").join("REFERENCE.md").as_path()));
    }

    #[test]
    fn e_has_nothing_to_hand_over_on_a_directory_or_a_bundle_without_a_lead() {
        let docs = nested("tui-edit-dir");
        assert_eq!(docs.editable(), None);

        let dir = scratch("tui-edit-nolead");
        std::fs::create_dir_all(dir.join("beta")).unwrap();
        let skills = app(vec![Source::new("skills", dir, Scope::Global, Walk::BundleDirs)]);
        assert_eq!(skills.editable(), None);
    }

    #[test]
    fn what_the_editor_wrote_shows_once_it_is_done() {
        let dir = scratch("tui-edit-reload");
        write(&dir.join("alpha.md"), "# before\n");
        let mut app = app(vec![Source::new("rules", dir.clone(), Scope::Global, Walk::MarkdownFiles)]);
        assert_eq!(first_row(&mut app), "# before");

        write(&dir.join("alpha.md"), "# after\n");
        assert_eq!(first_row(&mut app), "# before");
        app.edited(Ok(exit(0)), Instant::now());
        assert_eq!(first_row(&mut app), "# after");
    }

    #[test]
    fn every_source_is_read_again_not_only_the_one_on_screen() {
        // The same file, as two Sources see it — as a linked Bundle is seen.
        let dir = scratch("tui-edit-both");
        write(&dir.join("alpha.md"), "# before\n");
        let mut app = app(vec![
            Source::new("rules", dir.clone(), Scope::Global, Walk::MarkdownFiles),
            Source::new("again", dir.clone(), Scope::Global, Walk::MarkdownFiles),
        ]);
        write(&dir.join("alpha.md"), "# after\n");
        app.edited(Ok(exit(0)), Instant::now());
        press(&mut app, &[KeyCode::Char('j')]);
        assert_eq!(app.source, 1);
        assert_eq!(first_row(&mut app), "# after");
    }

    #[test]
    fn the_editor_leaves_open_rows_and_scrolled_files_as_they_were() {
        let mut app = tall("tui-edit-kept");
        draw(&mut app, 100, 8);
        press(&mut app, &[KeyCode::Tab, KeyCode::PageDown]);
        assert_eq!(first_row(&mut app), "• 6");
        app.edited(Ok(exit(0)), Instant::now());
        assert_eq!(first_row(&mut app), "• 6");

        let mut app = nested("tui-edit-open");
        press(&mut app, &[KeyCode::Char('l')]);
        app.edited(Ok(exit(0)), Instant::now());
        assert_eq!(tree(&mut app), ["▾ guide/", "  ▸ deep/"]);
    }

    #[test]
    fn a_selected_row_that_is_gone_after_the_editor_moves_up_to_the_last_one() {
        let dir = scratch("tui-edit-gone");
        for rule in ["one", "two", "three"] {
            write(&dir.join(format!("{rule}.md")), &format!("# {rule}\n"));
        }
        let mut app = app(vec![Source::new("rules", dir.clone(), Scope::Global, Walk::MarkdownFiles)]);
        press(&mut app, &[KeyCode::Tab, KeyCode::Char('j'), KeyCode::Char('j')]);
        assert_eq!(app.entries.selected(), Some(2));

        for rule in ["one", "two", "three"] {
            std::fs::remove_file(dir.join(format!("{rule}.md"))).unwrap();
        }
        write(&dir.join("only.md"), "# only\n");
        app.edited(Ok(exit(0)), Instant::now());
        assert_eq!(app.entries.selected(), Some(0));
        assert_eq!(app.entry().unwrap().name, "only");
    }

    #[test]
    fn an_editor_that_ended_badly_says_how_for_two_seconds() {
        let mut app = alpha("tui-edit-failed");
        let start = Instant::now();
        app.edited(Ok(exit(1)), start);

        let said = format!("editor: {} ", exit(1));
        let cells = draw(&mut app, 100, 8);
        let row = bottom_row(&mut app);
        assert!(row.ends_with(&said), "{row:?}");
        assert_eq!(cells[(99 - 1, 7)].fg, Color::Red);
        assert_eq!(app.wake_in(start), Some(NOTICE_FOR));
        app.tick(start + NOTICE_FOR);
        assert!(!bottom_row(&mut app).contains("editor:"));

        app.edited(Err(io::Error::from(io::ErrorKind::NotFound)), start);
        let said = format!("editor: {} ", io::Error::from(io::ErrorKind::NotFound));
        assert!(bottom_row(&mut app).ends_with(&said));
    }

    #[test]
    fn an_editor_that_ended_well_says_nothing() {
        let mut app = alpha("tui-edit-fine");
        app.edited(Ok(exit(0)), Instant::now());
        assert!(!bottom_row(&mut app).contains("editor:"));
        assert_eq!(app.wake_in(Instant::now()), None);
    }

    // ------------------------------------------------------------- watching

    /// Longer than anything here should take to arrive.
    const ARRIVES: Duration = Duration::from_secs(2);

    #[test]
    fn the_input_thread_reads_the_next_event_only_once_told_to() {
        let (to_loop, messages) = mpsc::channel();
        let (go_on, gone_on) = mpsc::channel();
        let (read_once, reads) = mpsc::channel();
        let read = move || {
            read_once.send(()).unwrap();
            Ok(Event::FocusGained)
        };
        thread::spawn(move || read_input(read, to_loop, gone_on));

        assert!(matches!(messages.recv_timeout(ARRIVES), Ok(Message::Input(Ok(Event::FocusGained)))));
        // However long the loop takes over it — the editor may be open.
        thread::sleep(Duration::from_millis(200));
        assert_eq!(reads.try_iter().count(), 1);

        go_on.send(()).unwrap();
        assert!(matches!(messages.recv_timeout(ARRIVES), Ok(Message::Input(_))));
        assert_eq!(reads.try_iter().count(), 1);
    }

    #[test]
    fn a_change_is_read_once_it_has_settled_and_not_put_off_by_the_next() {
        let dir = scratch("tui-watch-settle");
        write(&dir.join("alpha.md"), "# before\n");
        let mut app = app(vec![Source::new("rules", dir.clone(), Scope::Global, Walk::MarkdownFiles)]);
        let start = Instant::now();

        write(&dir.join("alpha.md"), "# after\n");
        app.changed(start);
        assert_eq!(app.wake_in(start), Some(SETTLE));
        app.changed(start + SETTLE / 2);
        app.tick(start + SETTLE - Duration::from_millis(1));
        assert_eq!(first_row(&mut app), "# before");

        app.tick(start + SETTLE);
        assert_eq!(first_row(&mut app), "# after");
        assert_eq!(app.wake_in(start + SETTLE), None);
    }

    #[test]
    fn the_selected_row_stays_on_its_file_when_one_appears_above_it() {
        let dir = scratch("tui-watch-above");
        write(&dir.join("b.md"), "# b\n");
        write(&dir.join("c.md"), "# c\n");
        let mut app = app(vec![Source::new("rules", dir.clone(), Scope::Global, Walk::MarkdownFiles)]);
        press(&mut app, &[KeyCode::Tab, KeyCode::Char('j')]);
        assert_eq!(app.entry().unwrap().name, "c");

        write(&dir.join("a.md"), "# a\n");
        app.reload();
        assert_eq!(app.entry().unwrap().name, "c");
        assert_eq!(app.entries.selected(), Some(2));
    }

    #[test]
    fn a_change_the_screen_would_not_show_leaves_dragged_text_selected() {
        let dir = scratch("tui-watch-same");
        let items: String = (1..=12).map(|n| format!("- {n}\n")).collect();
        write(&dir.join("long.md"), &items);
        let mut app = app(vec![Source::new("rules", dir.clone(), Scope::Global, Walk::MarkdownFiles)]);
        drag_across(&mut app, (61, 1), (63, 1));

        write(&dir.join("notes.txt"), "not a document");
        app.reload();
        assert_eq!(reversed_rows(&mut app), [true, false, false, false, false]);

        write(&dir.join("long.md"), "- 1 again\n");
        app.reload();
        assert_eq!(reversed_rows(&mut app), [false; 5]);
    }

    #[test]
    fn a_file_opened_read_or_closed_has_not_changed() {
        use notify::event::{AccessKind, AccessMode, CreateKind, ModifyKind, RemoveKind};
        use notify::{Error, Event as Notice, EventKind};

        let (to_loop, messages) = mpsc::channel();
        let mut told = tell(to_loop);
        // Whether the loop hears of what notify reports.
        let mut heard = |report: notify::Result<Notice>| {
            told(report);
            matches!(messages.try_recv(), Ok(Message::Changed))
        };
        let said = |kind| Ok(Notice::new(kind));

        assert!(!heard(said(EventKind::Access(AccessKind::Open(AccessMode::Any)))));
        assert!(!heard(said(EventKind::Access(AccessKind::Read))));
        assert!(!heard(said(EventKind::Access(AccessKind::Close(AccessMode::Write)))));
        assert!(heard(said(EventKind::Create(CreateKind::Any))));
        assert!(heard(said(EventKind::Modify(ModifyKind::Any))));
        assert!(heard(said(EventKind::Remove(RemoveKind::Any))));
        assert!(heard(said(EventKind::Any)));
        assert!(heard(Err(Error::generic("a watch was lost"))));
    }

    /// Whether `messages` hears of a change: the first word of it, and then
    /// the rest of the same burst, so that the next change starts afresh.
    fn heard(messages: &Receiver<Message>, wait: Duration) -> bool {
        let first = matches!(messages.recv_timeout(wait), Ok(Message::Changed));
        while messages.recv_timeout(Duration::from_millis(200)).is_ok() {}
        first
    }

    /// Wait until nothing has come for half a second. macOS reports a file
    /// written just before the watch began once it has begun, so a test that
    /// waits for nothing to be heard first lets its own fixture be heard.
    fn settled(messages: &Receiver<Message>) {
        while messages.recv_timeout(Duration::from_millis(500)).is_ok() {}
    }

    #[test]
    fn every_change_a_walk_would_show_is_heard() {
        let dir = scratch("tui-watch-heard");
        let (rules, skills, docs) = (dir.join("rules"), dir.join("skills"), dir.join("docs"));
        // Apart from the rest, so that watching above it watches nothing else.
        let project = dir.join("project");
        std::fs::create_dir(&project).unwrap();
        let missing = project.join("missing");
        write(&rules.join("one.md"), "# one\n");
        write(&skills.join("alpha").join("SKILL.md"), "---\nname: alpha\n---\n");
        write(&skills.join("alpha").join("deep").join("ref.md"), "# ref\n");
        write(&docs.join("a").join("b.md"), "# b\n");
        write(&docs.join("plain").join("notes.txt"), "no document here yet\n");
        let elsewhere = dir.join("elsewhere").join("linked");
        write(&elsewhere.join("SKILL.md"), "---\nname: linked\n---\n");
        let linked = link_dir(&elsewhere, &skills.join("linked"));

        let mut app = app(vec![
            Source::new("rules", rules.clone(), Scope::Global, Walk::MarkdownFiles),
            Source::new("skills", skills.clone(), Scope::Global, Walk::BundleDirs),
            Source::new("docs", docs.clone(), Scope::Project, Walk::MarkdownTree),
            Source::new("missing", missing.clone(), Scope::Project, Walk::MarkdownFiles),
        ]);
        let (to_loop, messages) = mpsc::channel();
        let mut watcher = watcher(to_loop).unwrap();
        let mut watching = Vec::new();
        rewatch(&mut watcher, &mut watching, app.watched());

        let mut changes = vec![
            ("a rule", rules.join("one.md")),
            ("a Bundle's Lead", skills.join("alpha").join("SKILL.md")),
            ("a supporting file further down", skills.join("alpha").join("deep").join("ref.md")),
            ("a document further down a tree", docs.join("a").join("b.md")),
            ("a document where a tree had found none", docs.join("plain").join("new.md")),
            ("a directory that was not there", missing.join("new.md")),
        ];
        if linked {
            changes.push(("a Lead behind a link", elsewhere.join("SKILL.md")));
        }
        for (what, path) in changes {
            write(&path, "# changed\n");
            assert!(heard(&messages, ARRIVES), "{what} changed unheard");
        }

        // Only once the directory is there is it watched, and the one above it
        // no longer: that took the Sources being read again. So it is with a
        // directory that appeared in a tree.
        std::fs::create_dir(docs.join("later")).unwrap();
        assert!(heard(&messages, ARRIVES), "a directory appearing in a tree unheard");
        app.reload();
        rewatch(&mut watcher, &mut watching, app.watched());
        write(&missing.join("new.md"), "# changed again\n");
        assert!(heard(&messages, ARRIVES), "a file in the directory that appeared changed unheard");
        write(&docs.join("later").join("new.md"), "# new\n");
        assert!(heard(&messages, ARRIVES), "a file in a directory that appeared in a tree unheard");
        write(&project.join("notes.md"), "# not in any Source\n");
        assert!(!heard(&messages, Duration::from_millis(500)), "the directory above is still watched");
    }

    #[test]
    fn a_link_whose_target_comes_back_is_watched_from_then_on() {
        // Found in review: the watch that failed was taken as made, and never
        // tried again.
        let dir = scratch("tui-watch-back");
        let skills = dir.join("skills");
        let target = dir.join("elsewhere").join("linked");
        write(&skills.join("alpha").join("SKILL.md"), "---\nname: alpha\n---\n");
        write(&target.join("SKILL.md"), "---\nname: linked\n---\n");
        if !link_dir(&target, &skills.join("linked")) {
            return;
        }
        std::fs::remove_dir_all(&target).unwrap();
        let app = app(vec![Source::new("skills", skills.clone(), Scope::Global, Walk::BundleDirs)]);
        let (to_loop, messages) = mpsc::channel();
        let mut watcher = watcher(to_loop).unwrap();
        let mut watching = Vec::new();
        rewatch(&mut watcher, &mut watching, app.watched());
        assert_eq!(watching, [skills.clone(), skills.join("alpha")]);

        write(&target.join("SKILL.md"), "---\nname: linked, back\n---\n");
        rewatch(&mut watcher, &mut watching, app.watched());
        write(&target.join("SKILL.md"), "---\nname: linked, changed\n---\n");
        assert!(heard(&messages, ARRIVES), "a change behind a link that came back is unheard");
    }

    #[test]
    fn a_watched_file_read_again_is_not_heard() {
        // Every Walk opens and reads the files it shows. On Linux notify
        // reports each open; taken for a change, it would set off the next
        // Walk, for ever. Windows does not report them — this fails only
        // where it can.
        let dir = scratch("tui-watch-read");
        write(&dir.join("one.md"), "# one\n");
        let mut app = app(vec![Source::new("rules", dir.clone(), Scope::Global, Walk::MarkdownFiles)]);
        let (to_loop, messages) = mpsc::channel();
        let mut watcher = watcher(to_loop).unwrap();
        let mut watching = Vec::new();
        rewatch(&mut watcher, &mut watching, app.watched());
        settled(&messages);

        app.reload();
        assert!(!heard(&messages, Duration::from_millis(500)), "reading was heard as a change");
        write(&dir.join("one.md"), "# changed\n");
        assert!(heard(&messages, ARRIVES), "a change was not heard");
    }

    #[test]
    fn a_change_behind_a_link_below_a_tree_is_not_heard() {
        // A Walk does not go through a link (ADR-0007), and neither does the
        // watching: on Linux notify would, unless told not to. Windows does
        // not report there, told or not — this fails only where it can.
        let dir = scratch("tui-watch-link-below");
        let docs = dir.join("docs");
        write(&docs.join("a.md"), "# a\n");
        let elsewhere = dir.join("elsewhere");
        write(&elsewhere.join("deep").join("x.md"), "# x\n");
        if !link_dir(&elsewhere, &docs.join("outside")) {
            return;
        }
        let app = app(vec![Source::new("docs", docs.clone(), Scope::Project, Walk::MarkdownTree)]);
        let (to_loop, messages) = mpsc::channel();
        let mut watcher = watcher(to_loop).unwrap();
        let mut watching = Vec::new();
        rewatch(&mut watcher, &mut watching, app.watched());
        settled(&messages);

        write(&elsewhere.join("deep").join("x.md"), "# changed\n");
        assert!(!heard(&messages, Duration::from_millis(500)), "a change behind a link was heard");
        write(&docs.join("a.md"), "# changed\n");
        assert!(heard(&messages, ARRIVES), "the tree itself is not watched");
    }

    #[test]
    fn nothing_below_where_a_tree_does_not_read_is_heard() {
        // A project's root: git writes below `.git/` at every command, and a
        // build below an ignored `target/`. Neither is read, and watched
        // there, every commit and every build would have every Source read
        // again. The root itself is watched, and Windows may report `.git`
        // or `target` as changed when something inside them is — GitHub's
        // runner did for a file two levels down, this machine did not — as
        // it did while only the root's own files were read. So what is
        // asked is what notify names: nothing below either of them.
        let dir = scratch("tui-watch-passed-over");
        write(&dir.join("a.md"), "# a\n");
        write(&dir.join(".gitignore"), "target/\n");
        write(&dir.join(".git").join("objects").join("ab").join("cdef"), "first\n");
        write(&dir.join("target").join("debug").join("out.md"), "# first\n");
        let app = app(vec![Source::new("root md", dir.clone(), Scope::Project, Walk::MarkdownTree)]);
        let (to_test, events) = mpsc::channel();
        let config = notify::Config::default().with_follow_symlinks(false);
        let report = move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                let _ = to_test.send(event.paths);
            }
        };
        let mut watcher = RecommendedWatcher::new(report, config).unwrap();
        let mut watching = Vec::new();
        rewatch(&mut watcher, &mut watching, app.watched());
        while events.recv_timeout(Duration::from_millis(500)).is_ok() {}

        write(&dir.join(".git").join("objects").join("ab").join("cdef"), "second\n");
        write(&dir.join("target").join("debug").join("out.md"), "# second\n");
        write(&dir.join("a.md"), "# changed\n");
        // Each path from below the fixture's own directory, found by its name,
        // so that one spelled from another start — a link resolved, a prefix
        // added — still counts.
        let fixture = dir.file_name().unwrap();
        let mut named: Vec<PathBuf> = Vec::new();
        let mut wait = ARRIVES;
        while let Ok(paths) = events.recv_timeout(wait) {
            for path in paths {
                if let Some(at) = path.components().position(|part| part.as_os_str() == fixture) {
                    named.push(path.components().skip(at + 1).collect());
                }
            }
            wait = Duration::from_millis(500);
        }
        let below: Vec<_> = named.iter().filter(|path| path.components().count() > 1).collect();
        assert!(below.is_empty(), "heard below where nothing is read: {below:?}");
        assert!(named.contains(&PathBuf::from("a.md")), "the tree itself is not watched: {named:?}");
    }

    #[test]
    fn a_path_two_sources_watch_is_watched_once() {
        // Found in review: watched twice, the one given up took the other
        // with it — a project whose root is a Bundle Source.
        let dir = scratch("tui-watch-twice");
        write(&dir.join("alpha").join("SKILL.md"), "---\nname: alpha\n---\n");
        let app = app(vec![
            Source::new("skills", dir.clone(), Scope::Global, Walk::BundleDirs),
            Source::new("root md", dir.clone(), Scope::Project, Walk::MarkdownTree),
        ]);
        assert_eq!(app.watched(), [dir.clone(), dir.join("alpha")]);
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

    /// A global Source and a project one that are not there, and between
    /// them the home config file, saying `text`, which cannot be used.
    fn with_unused_config(name: &str, text: &str) -> App {
        let dir = scratch(name);
        write(&dir.join(config::FILE), text);
        let Err(problem) = config::add(&dir, &dir, Scope::Global, &mut Vec::new()) else { panic!("could be used") };
        App::new(
            vec![
                Source::new("rules", dir.join("rules"), Scope::Global, Walk::MarkdownFiles),
                Source::new("docs", dir.join("docs"), Scope::Project, Walk::MarkdownTree),
            ],
            vec![(Scope::Global, problem)],
            "GLOBAL".to_string(),
            "PROJECT here".to_string(),
        )
    }

    #[test]
    fn a_config_file_that_cannot_be_used_is_a_row_at_the_end_of_its_scope_that_j_and_k_stop_at() {
        let mut app = with_unused_config("tui-unused", "[[sources]]\n");
        let rows: Vec<(String, Option<usize>)> = app.source_rows();
        assert_eq!(
            rows,
            [
                ("GLOBAL".to_string(), None),
                ("  rules:(missing)".to_string(), Some(0)),
                ("  .agentdocs.toml:(invalid)".to_string(), Some(2)),
                ("PROJECT here".to_string(), None),
                ("  docs:(missing)".to_string(), Some(1)),
            ]
        );

        // Down the rows as they are drawn, not as they are numbered.
        let mut seen = vec![app.source];
        for _ in 0..3 {
            press(&mut app, &[KeyCode::Char('j')]);
            seen.push(app.source);
        }
        for _ in 0..3 {
            press(&mut app, &[KeyCode::Char('k')]);
            seen.push(app.source);
        }
        assert_eq!(seen, [0, 2, 1, 1, 2, 0, 0]);
    }

    #[test]
    fn the_preview_of_a_config_file_that_will_not_read_says_so_in_words_of_its_own() {
        let dir = scratch("tui-unused-unread");
        std::fs::write(dir.join(config::FILE), [0xff, 0xfe, 0x00]).unwrap();
        let Err(problem) = config::add(&dir, &dir, Scope::Global, &mut Vec::new()) else { panic!("read as text") };
        let mut app = App::new(
            vec![Source::new("rules", dir.join("rules"), Scope::Global, Walk::MarkdownFiles)],
            vec![(Scope::Global, problem)],
            "GLOBAL".to_string(),
            "PROJECT here".to_string(),
        );
        press(&mut app, &[KeyCode::Char('j')]);
        let rows = screen(&mut app, 120, 6).join("\n");
        assert!(rows.contains("(this could not be read: unreadable)"), "{rows}");
    }

    #[test]
    fn the_preview_of_a_config_file_that_cannot_be_used_is_what_stopped_it() {
        let mut app = with_unused_config("tui-unused-preview", "[[source]]\nname = \"s\"\npath = \"s\"\nwalk = \"tree\"\n");
        press(&mut app, &[KeyCode::Char('j')]);
        let rows = screen(&mut app, 120, 12);
        let preview: Vec<String> = rows
            .iter()
            .map(|r| r.chars().skip(61).take(58).collect::<String>().trim_end().to_string())
            .collect();
        assert_eq!(
            preview[1..8],
            [
                "TOML parse error at line 4, column 8",
                "  |",
                "4 | walk = \"tree\"",
                "  |        ^^^^^^",
                "unknown variant `tree`, expected one of `markdown-files`,",
                "`bundle-dirs`, `markdown-tree`",
                "",
            ]
        );
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
    fn a_format_character_in_a_sources_name_never_reaches_the_screen() {
        // A Source's name can come out of a config file somebody else wrote.
        let dir = scratch("tui-format-source");
        let mut app = app(vec![
            Source::new("left\u{202e}right", dir.clone(), Scope::Global, Walk::MarkdownFiles),
            Source::new("gone\u{200b}", dir.join("nope"), Scope::Global, Walk::MarkdownFiles),
        ]);
        let rows = screen(&mut app, 100, 8).join("\n");
        assert!(rows.contains("leftright:0") && rows.contains("gone:(missing)"), "{rows}");
        assert!(!rows.contains(FORMAT) && !rows.contains('\u{200b}'), "{rows:?}");
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
        let Some(held) = hold(&dir.join("alpha")) else { return };
        let row = format!("    alpha ({})", reason(held.kind));

        let mut app = app(vec![Source::new("skills", dir, Scope::Global, Walk::BundleDirs)]);
        press(&mut app, &[KeyCode::Tab, KeyCode::Char('l'), KeyCode::Char('j')]);
        assert_eq!(tree(&mut app), ["▾ alpha", row.as_str()]);
        press(&mut app, &[KeyCode::Char('h')]);
        assert_eq!(app.entries.selected(), Some(0));
        assert_eq!(tree(&mut app), ["▾ alpha", row.as_str()]);

        // Nor do l and Enter on that row reach the Bundle.
        press(&mut app, &[KeyCode::Char('j'), KeyCode::Char('l'), KeyCode::Enter]);
        assert_eq!(tree(&mut app), ["▾ alpha", row.as_str()]);
    }

    #[test]
    fn a_bundle_that_would_not_open_is_where_it_was_left_after_the_row_under_it() {
        // That row has the Bundle's path too, and shows a note, not a file.
        let dir = scratch("tui-tree-held-scroll");
        let items: String = (1..=12).map(|n| format!("- {n}\n")).collect();
        write(&dir.join("alpha").join("SKILL.md"), &format!("---\nname: alpha\n---\n{items}"));
        write(&dir.join("alpha").join("REFERENCE.md"), "# supporting\n");
        let Some(held) = hold(&dir.join("alpha")) else { return };
        let said = format!("(this could not be read: {})", reason(held.kind));

        let mut app = app(vec![Source::new("skills", dir, Scope::Global, Walk::BundleDirs)]);
        draw(&mut app, 100, 8);
        press(&mut app, &[KeyCode::Tab, KeyCode::PageDown]);
        assert_eq!(first_row(&mut app), "• 5");
        press(&mut app, &[KeyCode::Char('l'), KeyCode::Char('j'), KeyCode::PageDown]);
        // 38 columns: `permission denied`, Unix's reason, takes a second row.
        let rows: Vec<String> = preview(&mut app).into_iter().filter(|row| !row.is_empty()).collect();
        assert_eq!(rows.join(" "), said);
        // A drag over the note is counted from where the note is drawn.
        let note = drag_across(&mut app, (61, 1), (60 + rows[0].chars().count() as u16, 1));
        assert_eq!(note.as_deref(), Some(rows[0].as_str()));
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
        let Some(held) = hold(&dir.join("guide").join("secret")) else { return };
        let why = reason(held.kind);

        let mut app = nested_in(dir);
        press(&mut app, &[KeyCode::Char('l'), KeyCode::Char('j')]);
        assert_eq!(tree(&mut app), ["▾ guide/".to_string(), format!("    secret ({why})")]);
        // Wide enough for the note on one row, whichever reason it gives.
        let rows = screen(&mut app, 120, 12).join("\n");
        assert!(rows.contains("docs:0 (1 unreadable)"), "{rows}");
        assert!(rows.contains(&format!("(this could not be read: {why})")), "{rows}");
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
