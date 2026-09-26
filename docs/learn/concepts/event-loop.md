# event-loop

A terminal program with a screen is one loop: **draw everything, wait for something to happen, change the state, draw everything again**. Nothing is drawn in answer to an event directly — the event changes the state, and the next draw shows it.

## The loop

```rust
fn run(terminal: &mut DefaultTerminal, mut app: App) -> io::Result<()> {
    loop {
        app.tick(Instant::now());
        terminal.draw(|frame| app.render(frame))?;

        if let Some(left) = app.copied_left(Instant::now()) {
            if !event::poll(left)? {
                continue;
            }
        }

        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if key.code == KeyCode::Char('q') {
                    return Ok(());
                }
                app.handle(key.code);
            }
            Event::Mouse(mouse) => match mouse.kind { /* click, drag, release */ },
            _ => {}
        }
    }
}
```

`terminal.draw` hands the closure a fresh frame, and `render` paints the whole screen into it from `App` — every pane, every time. ratatui then compares the frame with the previous one and sends the terminal only the cells that changed, so drawing everything is cheap. This style is called *immediate mode*: the screen is a function of the state, not a set of widgets that each remember what they show.

`event::read()` blocks until a key, a mouse event or a resize arrives, so a screen nobody touches uses no CPU. `q` is the only way out: `return Ok(())` leaves the loop, and the caller hands the terminal back.

## State changes; drawing reads

`App` holds what is selected and focused. The loop turns each event into a method call — `handle(key)`, `click(column, row)`, `drag`, `release` — and those are the only places the state changes. `render` only reads it, apart from noting where it drew each pane and what the screen held, which the mouse needs: a click at column 35 means nothing until you know which pane was drawn there. Because the loop itself decides nothing, every one of those methods can be tested with a terminal made of memory ([[testing]]).

## The terminal has modes

A shell's terminal is in *cooked* mode: it collects a line, echoes what you type, and turns Ctrl+C into a signal. A screen wants every keystroke at once and nothing echoed, so it switches to **raw mode** — on Windows, by clearing three console flags: line input, echo, and processed input (which is why Ctrl+C becomes an ordinary key and `q` is needed). It also switches to the **alternate screen**, a second buffer the shell's scrollback never sees, and back to the first on the way out. `ratatui::init` does both and `ratatui::restore` undoes them.

**Mouse capture** is a third mode. On Windows crossterm sets it by changing the console input mode, saving the mode it found; turning it off puts that saved mode back. Turning raw mode off, by contrast, only adds three flags to whatever mode it finds — so if the mouse were still on, it would stay on. The mouse therefore comes off **first**, on the normal path in `tui::open` and in the panic hook alike ([[drop-and-unwinding]]).

## Windows sends every key twice

crossterm reports a key going down and a key coming up as two events, `KeyEventKind::Press` and `KeyEventKind::Release`. Without the `key.kind == KeyEventKind::Press` guard, every `j` would move two rows on Windows and one elsewhere.

## Mouse events

With capture on, the terminal reports the button going down (`Down`), the pointer moving with it held (`Drag`, once per cell crossed), and the button coming up (`Up`). A click is `Down` followed by `Up` in the same cell; a selection is the cells between a `Down` and the last `Drag`. The wheel arrives as `ScrollUp`/`ScrollDown` and is ignored on purpose until the preview can scroll (M8).

Capture has a price: the terminal's own selection now works only while Shift is held, and it selects whole rows of the screen, straight across all three panes. That is what made a selection inside the preview worth building.

## Waking up without an event

"copied to clipboard" has to disappear two seconds later even if nobody touches anything, and `read()` would wait forever. `event::poll(left)` waits **at most** `left`: `true` if an event came, `false` if the time ran out. While the notice shows, the loop polls for exactly the time it has left and, on `false`, goes round again — the `tick` at the top takes the notice down and the next draw shows the row without it.

## Talking to the terminal directly: OSC 52

A few things a terminal can do are asked for by writing an escape sequence between frames. `ESC ] 52 ; c ; <base64> BEL` asks it to put the text on the clipboard; nothing appears on screen and nothing comes back, so the program cannot tell whether it worked. It was checked by hand in the author's terminal, Korean included, before drag-to-copy was built — and "it works in herdr" was not taken as that check, because herdr on Windows uses the native clipboard API first and OSC 52 only as a fallback.

## Pitfalls hit

- **The wheel moved the selection before any mouse code existed.** The guess was that the terminal, not knowing the program wanted the mouse, translated the wheel into arrow keys. The author tested it by scrolling over one pane while the other had the focus: the focused pane moved, which is what arrow keys do wherever the pointer is. Once capture was on, the wheel arrived as its own event, and the author decided it should move nothing for now.

## Related

[[drop-and-unwinding]] · [[testing]] · [[enums-and-data]] · [[external-crates]] · [[closures]]
