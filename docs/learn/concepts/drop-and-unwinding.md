# drop-and-unwinding

When a value goes out of scope, Rust runs its `Drop` — at the end of the block in the ordinary way, **and also while a panic unwinds through the block**. What a panic skips is everything else: the statements still to come never run. Cleanup that must happen on both paths therefore belongs in a `Drop` or a panic hook, not in the last lines of a function.

## `Drop`: a trait with one method

`Drop` is a [[traits]] promise the language itself calls: `fn drop(&mut self)`, run once, when the value's owner lets go of it ([[ownership]]). A `Vec` frees its memory this way and a `File` closes its handle. Your own type can do anything there — this one, written for M5's probe, writes a line to a log:

```rust
pub struct Guard(pub &'static str);

impl Drop for Guard {
    fn drop(&mut self) {
        log(&format!("{} (panicking={})", self.0, std::thread::panicking()));
    }
}
```

`std::thread::panicking()` says whether this drop is happening because of a panic.

## What a panic does, in order

1. The **panic hook** runs — by default it prints `thread 'main' panicked at …`.
2. **Unwinding**: the stack is walked back frame by frame. In each frame the remaining statements are skipped and the local variables are dropped, in the **reverse** of the order they were declared.
3. When the unwinding leaves `main`, the process exits with code 101.

The probe logged every step of the screen's shutdown, on a normal exit and on a deliberate panic. Abridged:

```
q pressed                    Shift+P pressed
run returned -> Ok(())       hook: entered
DisableMouseCapture -> Ok    hook: DisableMouseCapture -> Ok
calling ratatui::restore     hook: ratatui's hook returned
restore returned             open: frame dropped (panicking=true)
frame dropped (panicking=false)
```

On the right, the four lines that come from the end of `tui::open` are missing — unwinding skipped them. Only the hook and the `Guard`'s drop ran.

## Why the screen needs a hook

`tui::open` ends by turning the mouse off and handing the terminal back. Those are ordinary statements, so a panic anywhere in the [[event-loop]] skips them and leaves the terminal in raw mode, with the mouse captured. ratatui installs a hook that restores raw mode and the alternate screen; agentdocs installs a second one for the mouse:

```rust
let ratatui_hook = panic::take_hook();
panic::set_hook(Box::new(move |info| {
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui_hook(info);
}));
```

`take_hook` hands over the hook currently installed, and the new one calls it last. Installed after ratatui's, this one therefore runs first — mouse off, then raw mode — which is the order Windows needs.

## `let _` drops at once; `let _name` does not

```rust
let _ = Guard("gone already");      // `_` binds nothing: dropped on this line
let _last = Guard("end of scope");  // bound: dropped when the scope ends
```

The leading underscore on `_last` only silences the unused-variable warning ([[mutability]]); the value is still owned and still lives to the end of the block. The probe needed exactly that, and declared `_last` **before** `terminal` so that, dropping in reverse, it logged after the terminal was gone.

## Pitfalls hit

- **After one deliberate panic the mouse stayed captured in the shell.** The probe above was built the next session to find out why: on the normal path and on the panic path alike, the hook ran, `DisableMouseCapture` returned `Ok`, and the console mode came back to the value it started at. Three more panics, two of them with the very binary that had shown it, did not reproduce it. The cause is unknown, so nothing on this page is offered as the explanation; if it shows again, the probe is how that run gets measured.

## Related

[[traits]] · [[ownership]] · [[event-loop]] · [[result-and-errors]] — a panic is for a bug; a failure the caller should handle is a `Result`
