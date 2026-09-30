# integer-overflow

An integer type has a smallest and a largest value — `usize` and `u16` start at 0 — and arithmetic that would leave that range **overflows**. Rust does not leave what happens next to chance, but what it does depends on how the program was built.

## Two builds, two answers

| build | `overflow-checks` | `0usize - 1` |
|---|---|---|
| `cargo build`, `cargo run`, `cargo test` | on | **panic**: `attempt to subtract with overflow` |
| `cargo build --release` | off | **wraps** round to the other end: `18446744073709551615` |

Both measured, with the same three lines:

```rust
let rows: isize = std::hint::black_box(-1);   // black_box: so the compiler cannot see it coming
let mut top: usize = 0;
top -= rows.unsigned_abs();
```

A `u16`, as the screen's coordinates are, wraps at 65,536: `1 - 2` panics in a debug build and prints `65535` in a release one.

Neither is undefined — wrapping is what the machine's arithmetic does, and Rust says so. But a release build that wraps gives a **wrong number with exit 0**, and the numbers in a screen are rows and columns. So the arithmetic that can go below zero here says which end it wants, in its name:

| method | at the edge | used for |
|---|---|---|
| `saturating_add_signed`, `saturating_sub` | stops at the edge | scrolling up past the first row; the room left in a row after a list's indent |
| `checked_sub` | `None` | — |
| `wrapping_sub` | wraps, on purpose | — |
| `.clamp(low, high)` before subtracting | cannot reach the edge | a point outside the preview, before its row inside is worked out |

## Where it lives here

```rust
fn scroll(&mut self, rows: isize) {                // rows < 0 scrolls up
    …
    *top = top.saturating_add_signed(rows);         // up from row 0 stays at 0
}

fn spot(at: Position, inner: Rect, top: usize) -> Spot {
    let at = clamp(at, inner);                      // first inside…
    Spot { row: top + usize::from(at.y - inner.y), column: at.x - inner.x }   // …then these cannot go below 0
}
```

A drag goes on reporting the pointer once it leaves the preview, above it too, so `at.y - inner.y` is a subtraction that meets a smaller left side in ordinary use.

`[profile.dev.package."*"] opt-level = 3`, which M8 added to make syntect fast under `cargo run`, changes only the dependencies ([[external-crates]]). This crate's own code is still built with overflow checks on, which is why a mistake of this kind shows up as a failing test before it can show up as a wrong screen.

## Pitfalls hit

- **"It stops at 0."** Asked how to scroll up, the pick was `*top -= rows.unsigned_abs()`, on the reasoning that a `usize` cannot go below zero, so it stops there. Put into the repository, two tests panicked with `attempt to subtract with overflow` at `src\tui.rs:444:13`. Built for release, it wrapped: the preview jumped to the file's last page (`• 8`), since drawing holds the row it starts at to the last one it can.
- **Debug and release told apart.** Asked next what `1u16 - 2` does in a release build, the pick was *panic*. It wraps, to `65535`; a table of the two builds followed, and changing `area.width.saturating_sub(2)` to `area.width - 2` in that slice's code made an existing test panic at `src\tui.rs:578:21`. In the next slice, asked what happens when `spot` loses its `clamp` and the pointer goes above the preview, the answer was right — a test panics, and a release build wraps. Measured on a test's drag held above a twelve-line file, where seven rows were expected, the release build copied 65,530 rows, 131,059 characters, nearly all of them empty.

## Related

[[state]] · [[testing]] · [[external-crates]] · [[mutability]]
