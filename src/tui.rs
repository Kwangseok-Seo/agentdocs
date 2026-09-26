use std::io;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, KeyCode};
use ratatui::widgets::{Block, Paragraph};

/// Draw, wait for a key, act on it — and again, until `q`.
pub fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    loop {
        // Describe the whole screen as it should look now. ratatui compares it
        // with the previous frame and writes only the cells that changed.
        terminal.draw(|frame| {
            let hello = Paragraph::new("agentdocs - press q to quit").block(Block::bordered());
            frame.render_widget(hello, frame.area());
        })?;

        // Windows reports a key going up as well as going down, so every
        // keystroke arrives twice. Only the press counts.
        if let Some(key) = event::read()?.as_key_press_event() {
            if key.code == KeyCode::Char('q') {
                return Ok(());
            }
        }
    }
}
