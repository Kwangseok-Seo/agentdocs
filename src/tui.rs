use std::io;
use std::panic;

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, List, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::entry::Entry;
use crate::listing::{failed, heading};
use crate::source::{Scope, Source, Walked};

/// The keys the screen answers to, shown along its bottom row.
const KEYS: &str = " j/k ↓/↑ move   tab pane   click select   q quit";

/// What the screen shows, and which part of it is selected. Keys and clicks
/// change it; drawing only reads it — and notes where it drew, for the clicks.
pub struct App {
    global: String,
    project: String,
    sources: Vec<(Source, io::Result<Walked>)>,
    source: usize,
    entries: ListState,
    focus: Pane,
    /// The Sources, Entries and Preview panes as last drawn.
    areas: [Rect; 3],
    /// How far the Sources pane had to scroll, when too short for every row.
    source_offset: usize,
}

/// The pane that j and k move in. The preview has nothing to move through
/// until it scrolls, in M8.
#[derive(Clone, Copy, PartialEq)]
enum Pane {
    Sources,
    Entries,
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
            focus: Pane::Sources,
            areas: [Rect::default(); 3],
            source_offset: 0,
        }
    }

    /// Select what was clicked, and hand j and k to the pane it is in. A click
    /// on a border, a scope heading, below the last row or in the preview
    /// selects nothing.
    fn click(&mut self, column: u16, row: u16) {
        let at = Position::new(column, row);
        let [sources, entries, _] = self.areas;

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
            let len = self.walked().map_or(0, |walked| walked.entries.len());
            if index < len {
                self.entries.select(Some(index));
            }
        }
    }

    /// Change what is selected in answer to one key. Quitting is left to the
    /// loop, so that everything here can be tested without a terminal.
    fn handle(&mut self, key: KeyCode) {
        match key {
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Pane::Sources => Pane::Entries,
                    Pane::Entries => Pane::Sources,
                };
            }
            KeyCode::Char('j') | KeyCode::Down => self.down(),
            KeyCode::Char('k') | KeyCode::Up => self.up(),
            _ => {}
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
                let len = self.walked().map_or(0, |walked| walked.entries.len());
                let at = self.entries.selected().unwrap_or(0);
                if at + 1 < len {
                    self.entries.select(Some(at + 1));
                }
            }
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
        }
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

    /// The selected Entry, if there is one.
    fn entry(&self) -> Option<&Entry> {
        self.walked()?.entries.get(self.entries.selected()?)
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

    /// The selected Source's Entries by name. Names come out of somebody else's
    /// file, and ratatui drops control characters as it writes a cell — a test
    /// below holds it to that.
    fn render_entries(&mut self, frame: &mut Frame, area: Rect) {
        let names: Vec<String> = match self.walked() {
            Some(walked) => walked.entries.iter().map(|e| e.name.clone()).collect(),
            None => Vec::new(),
        };

        let list = List::new(names)
            .block(Block::bordered().title("Entries").border_style(self.border(Pane::Entries)))
            .highlight_style(Style::new().reversed());
        frame.render_stateful_widget(list, area, &mut self.entries);
    }

    /// The selected Entry's file as it is on disk. Rendering the Markdown is M6.
    fn render_preview(&self, frame: &mut Frame, area: Rect) {
        let text = match self.entry() {
            None => "",
            Some(entry) => match (&entry.text, entry.doc()) {
                (Some(text), _) => text.as_str(),
                (None, None) => "(this Bundle has no SKILL.md)",
                (None, Some(_)) => "(the file could not be read)",
            },
        };

        let preview = Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title("Preview"));
        frame.render_widget(preview, area);
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

/// Draw, wait for a key or a click, act on it — and again, until `q`.
fn run(terminal: &mut DefaultTerminal, mut app: App) -> io::Result<()> {
    loop {
        terminal.draw(|frame| app.render(frame))?;

        match event::read()? {
            // Windows reports a key going up as well as going down, so every
            // keystroke arrives twice. Only the press counts.
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if key.code == KeyCode::Char('q') {
                    return Ok(());
                }
                app.handle(key.code);
            }
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                app.click(mouse.column, mouse.row);
            }
            // The wheel is left alone on purpose. Moving the selection with it
            // surprised; it will scroll the preview once the preview scrolls, in M8.
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
    use ratatui::buffer::Buffer;
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
        let second = app.walked().unwrap().entries[1].name.clone();

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
        press(&mut app, &[KeyCode::Tab, KeyCode::Char('j'), KeyCode::Char('k')]);
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
        let third = app.walked().unwrap().entries[2].name.clone();

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
    fn an_escape_sequence_in_a_name_never_reaches_the_screen() {
        let dir = scratch("tui-escape");
        write(&dir.join("x.md"), "---\nname: \u{1b}[31mred\n---\n");

        let mut app = app(vec![Source::new("rules", dir, Scope::Global, Walk::MarkdownFiles)]);
        let rows = screen(&mut app, 100, 8).join("\n");
        assert!(rows.contains("[31mred"), "{rows}");
        assert!(!rows.chars().any(|c| c != '\n' && c.is_control()), "{rows:?}");
    }
}
