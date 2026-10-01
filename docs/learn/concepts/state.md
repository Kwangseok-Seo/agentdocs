# state

What a program remembers between one event and the next. A screen is drawn again from its state after every key ([[event-loop]]), so everything it seems to remember — which row is selected, how far a file is scrolled, what is being dragged over — is a field somewhere, and each such field is a decision: what is kept, what it is kept by, and what is worked out again instead.

## Kept in the App, changed by events, read by drawing

M8 added three things to what the screen keeps:

```rust
pub struct App {
    scrolled: HashMap<PathBuf, usize>,   // how far each file's preview is scrolled
    focus: Pane,                         // now also Pane::Preview, where j and k scroll
    selection: Option<Selection>,        // what is being dragged over, and until when to scroll
    …
}
```

Keys, clicks and the wheel change them, through `handle`, `click`, `drag`, `wheel`, `release` and `tick`; drawing reads them. Kept apart like this, each method can be tested by calling it and drawing into memory ([[testing]]).

## Kept by what does not move

A row number says where something is **drawn**, and that changes when a directory above it opens or the file scrolls. So what should stay put is kept by something that does not change:

- how far a file is scrolled, **by its path** ([[hash-maps]]) — as M7 kept which rows are open;
- since M9, which row is selected when the Sources are read again, **by its path** too: with the screen reading them whenever a file changes, a file added above the selected one would otherwise have moved the selection onto its neighbour. Only a selected row that is gone falls back to its place;
- where a drag started and has got to, **by a cell of the text**: which row, counted from the file's first, and how far in from the left — a `Spot`, not a cell of the screen.

```rust
struct Spot { row: usize, column: u16 }   // row 4 of the file, not row 2 of the terminal
```

Kept as screen cells, a selection would stay where the pointer was while the words scrolled out from under it. Kept as text cells, it stays on its words: the wheel can turn in the middle of a drag, and what scrolls under the pointer is taken in.

## Worked out again where only drawing knows

How far down a file may scroll depends on how many rows it is cut into, which depends on the preview's width — and only drawing knows that. So keys only add and subtract, and drawing holds the number to the last row it can start from and writes it back:

```rust
let mut lines = self.preview_lines(inner.width);   // borrows the file's text out of self
let most = lines.len().saturating_sub(height);
let top = …self.scrolled.get(path)….min(most);
lines.drain(..top);
frame.render_widget(Paragraph::new(lines)…);        // the last use of lines
self.scrolled.insert(path, top);                    // written back only now
```

Written back one line too early, while `lines` still borrows from `self`, it does not build:

```
error[E0502]: cannot borrow `self.scrolled` as mutable because it is also borrowed as immutable
581 |         let mut lines = match self.entry() {
    |                               ---- immutable borrow occurs here
603 |             self.scrolled.insert(path.clone(), top);
    |             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ mutable borrow occurs here
605 |         lines.drain(..top);
    |         ----- immutable borrow later used here
```

A drag's far end is worked out again the same way. While dragging, drawing puts it on the text cell under the pointer as the text is now — so the wheel, and the preview scrolling on its own, both move it without a line of their own.

Some things were kept once and are not now. Until slice 2 the App kept a copy of the whole screen after each drawing, and selected text was read back out of it — which could only give back the rows in view. The copy is gone: the selected rows are drawn again, one at a time, into a buffer one row high, so rows scrolled out of view are copied too.

## A moment as state

"copied to clipboard" comes down two seconds after a copy; a drag held below the preview scrolls it a row every 30 ms; since M9, the Sources are read again 100 ms after a file is first seen to change. None of them is an event. Each is kept as the moment it is due, an `Option<Instant>` — the notice's inside a `Notice`, which also holds its words and colour — and the loop asks the App how long it may wait:

```rust
fn wake_in(&self, now: Instant) -> Option<Duration> {
    let notice = self.notice.as_ref().map(|notice| notice.until.saturating_duration_since(now));
    let reload = self.reload_at.map(|at| at.saturating_duration_since(now));
    let scroll = self.autoscroll().map(|_| …);
    [notice, reload, scroll].into_iter().flatten().min()   // the soonest, or None
}
```

Because the time is handed in, a test says *30 ms later* as `start + AUTOSCROLL_EVERY` and waits for nothing. A change noted while one is already due does not move it: `reload_at.get_or_insert(now + SETTLE)` sets the moment only when there is none, so a burst of reports is read once, 100 ms after the first.

## What a Walk finds again replaces only what differs

Reading the Sources again could clear the screen's state wholesale — the selected row, text dragged over. Since M9 each Source's new Walk is compared with the one the screen has, and taken only where it differs ([[traits]]); when none differs, nothing is touched. A file the screen does not show, written under it, leaves a selection where it was.

## State carried from one line to the next

A highlighter reads code a line at a time and keeps, between lines, a stack of what is open. Taken out at the end of each line of a Python string that runs over two:

```
s = """one    starts []
              ends   [source.python, meta.string.python, string.quoted.double.block.python]
two"""        starts [source.python, meta.string.python, string.quoted.double.block.python]
              ends   [source.python]
x = 1         starts [source.python]
```

`two` is green because the line starts inside a string. So a code block keeps one highlighter from its first line to its last, in the renderer's `Code`, and drops it at the block's end. A highlighter made again for each line leaves `two` uncoloured — tried, and a test fails with `left: None, right: Some(Green)`.

It also means a block cannot be coloured from the middle: to draw its 200th line, the 199 above have to be read. Every drawing reads every line of the file again — at worst 30 ms in a release build and 53 ms under `cargo run`, measured on the heaviest file here. That was judged cheap enough to keep nothing: no coloured rows are kept between drawings.

## Pitfalls hit

- **A `Spot` taken for a cell of the screen (not asked again).** Asked where a click on screen column 63, row 2 lands in a file scrolled down by 3, with the preview's text starting at column 61 and row 1, the pick was `Spot { row: 2, column: 63 }` — the screen's numbers as they were. It is row 4, column 2. Built in, 14 tests failed. A diagnostic question showed the purpose was the gap — what a `Spot` is *for* — and the review question after it drew *"I don't follow"*: the drawing meant to answer it had gone out half-written. The same screen was then shown with both kinds of number side by side, and the author chose a line-by-line explanation over more questions.

## Related

[[event-loop]] · [[hash-maps]] · [[borrowing]] · [[testing]] · [[enums-and-data]] · [[statics]] · [[traits]]
