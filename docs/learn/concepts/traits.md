# traits

A trait is a promise about what a type can do: a named set of method signatures, some of which may come with a body already written. A type keeps the promise with an `impl Trait for Type` block.

## Against Go's interfaces

The idea is the one Go calls an interface, with two differences that matter:

| | Go interface | Rust trait |
|---|---|---|
| how a type satisfies it | implicitly — having the methods is enough | explicitly — `impl Iterator for Countdown` |
| what it contains | signatures only | signatures, **and default bodies** |

The second difference is how [[iterators]] work: `Iterator` requires `next` and supplies the other 75 methods itself, each written in terms of `next`. A type that implements one method gets the lot.

## `derive`: an `impl` the compiler writes

`Default` is a trait with one method, `fn default() -> Self` — "the natural starting value". The standard library implements it for its own types:

```
String::default()         = ""
usize::default()          = 0
Option::<String>::default = None
Vec::<String>::default()  = []
```

For a struct of such fields, implementing it by hand is mechanical: call each field type's `default()`.

```rust
impl Default for Walked {
    fn default() -> Self {
        Walked { entries: Vec::default(), unreadable: usize::default() }
    }
}
```

`#[derive(Default)]` asks the compiler to write exactly that block. Written side by side, the two print the same thing:

```
Walked::default()         = Walked { entries: [], unreadable: 0 }
WalkedByHand::default()   = WalkedByHand { entries: [], unreadable: 0 }
```

M4 replaced the hand-written `Walked::new()` and `Frontmatter::none()` this way — both were already, field for field, what the derive produces. (Since M7 `Walked` has one field, `nodes`, and its count of what could not be read is worked out from them; the derive writes the same block, a field shorter.) `#[derive(Debug)]`, added in M3 because `unwrap_err()` required it, is the same mechanism for a different trait.

Because a derive works field by field, one field without the trait stops it:

```
error[E0277]: the trait bound `EntryKind: Default` is not satisfied
   |
 8 | #[derive(Default)]
   |          ------- in this derive macro expansion
...
11 |     kind: EntryKind,
   |     ^^^^^^^^^^^^^^^ unsatisfied trait bound
```

An enum has no obvious first value, so `EntryKind` has no `Default`, and neither can `Entry`.

`#[derive(PartialEq)]` writes `==` the same way, field by field, and M9 needed it to tell whether a Walk found anything new: `Walked`, `Node`, `Entry` and `EntryKind` derive it, and two Walks are equal when every row is — names, paths, descriptions and the whole text of every file. `io::Error` does not implement `PartialEq`, so a failed Walk cannot be compared that way; `source::same` compares two failures by their `kind()`, which can be ([[result-and-errors]]). Comparing two Walks of everything this machine showed with this repository as the project — 100 files and 834 KB of text, before M9's own pages were written — took 26 µs.

## Bounds: "any type, as long as…"

The standard library's `all`:

```rust
fn all<F>(&mut self, f: F) -> bool
where
    F: FnMut(Self::Item) -> bool,
```

`F` may be any type at all, provided it implements `FnMut(Self::Item) -> bool`. That is the only way a function can accept a closure, since every closure has a type of its own that nobody can name ([[closures]]).

The short form of the same idea is `impl Trait` in argument position:

```rust
fn search_terms(args: impl Iterator<Item = String>) -> Vec<String>
```

"Any iterator of `String`s." `main` passes `env::args()` and the tests pass a plain list, and they are different types:

```
std::env::Args
alloc::vec::into_iter::IntoIter<alloc::string::String>
```

## Why M4 defines no trait of its own

A trait earns its place when **two or more types** must keep the same promise. Everything searched in M4 is an `Entry`; a `Searchable` trait with one implementor would be an abstraction with nothing on its other side. So this milestone uses traits only from the outside: deriving them, and handing closures to functions bounded by them.

## A trait's methods exist only where the trait is in scope

`io::stdout().is_terminal()` compiles only with `use std::io::IsTerminal;` at the top of the file: `is_terminal` is not a method of `Stdout` itself but of the trait `IsTerminal`, which `Stdout` implements. M5 met the same rule again in `STANDARD.encode(text)` — `encode` belongs to the trait `base64::Engine`. Remove that `use` and the call stops compiling:

```
error[E0599]: no method named `encode` found for struct `GeneralPurpose` in the current scope
    |
    = help: items from traits can only be used if the trait is in scope
```

So the same file has `use unicode_width::UnicodeWidthStr;` for `symbol.width()` on a `&str`, and `use std::io::Write;` for `out.write_all(…)` and `writeln!`. Each line looks unused — nothing in the file names the trait — and each is what makes a method exist. The rule keeps two crates that give the same type a method of the same name from colliding everywhere at once: only a file that imports one of them sees it ([[external-crates]]).

## Keeping a standard trait's promise: `Write`

To test what the listing does when a pipe closes early, M5 needed something to write into that fails on cue. `std::io::Write` requires two methods and supplies the rest:

```rust
impl Write for StopsAfter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.room == 0 {
            return Err(io::Error::from(self.kind));
        }
        let taken = buf.len().min(self.room);
        self.room -= taken;
        Ok(taken)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
```

`write_all`, and `write_fmt` — which `writeln!` calls — are default bodies written in terms of `write`, the way `Iterator`'s 75 methods are written in terms of `next`. A function taking `out: &mut impl Write` therefore accepts the real stdout, a `Vec<u8>`, and this test double alike.

## A derived order compares fields in the order they are declared

`#[derive(PartialOrd, Ord)]` compares the first field, and looks at the next only on a tie. ratatui's `Position` derives both and declares `x` before `y`, so it orders by column first:

```
Position { x: 5, y: 1 } < Position { x: 3, y: 2 }   = false
(1, 5) < (2, 3)          — the same two, as (y, x)  = true
```

A drag's two ends have to be put in reading order — row first — so M5 compares `(a.y, a.x)` tuples instead of the positions. Tuples compare element by element in the order written, which puts the choice in the caller's hands.

M8 keeps a drag's ends as cells of the text rather than of the screen, in a type of its own, and puts the choice in the declaration instead:

```rust
#[derive(Clone, Copy, PartialEq, PartialOrd)]
struct Spot {
    row: usize,     // declared first, compared first
    column: u16,
}
```

`anchor <= head` now reads as a page does, with no tuple to build at each comparison.

## Pitfalls hit

- **An empty `impl` block compiles, silently.** Deleting `Frontmatter::none()` left `impl Frontmatter { }` holding nothing but a blank line. The build reported no warning; it was found by reading the diff.
- **"Methods need no `use`" is half a rule.** Splitting `main.rs` into modules in M5, the assistant said a method can be called without importing anything, since it hangs off the value. That holds for a type's own methods, written in `impl Type`. Two slices later `is_terminal()` failed with E0599 until `IsTerminal` was imported, and the rule was corrected: a method from a trait needs the trait in scope.
- **A derive nothing uses.** `Frontmatter` gained `#[derive(Debug, Default)]` when only `Default` was needed; nothing prints a `Frontmatter`. It costs nothing at runtime, but in this codebase a derive marks that something relies on it — M3 added `Debug` only where `unwrap_err()` demanded it — so the extra one was removed.

## Related

[[iterators]] · [[closures]] · [[structs]] · [[enums-and-data]] · [[testing]] · [[external-crates]] · [[drop-and-unwinding]]
