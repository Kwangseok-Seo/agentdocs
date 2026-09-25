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

M4 replaced the hand-written `Walked::new()` and `Frontmatter::none()` this way — both were already, field for field, what the derive produces. `#[derive(Debug)]`, added in M3 because `unwrap_err()` required it, is the same mechanism for a different trait.

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

## Pitfalls hit

- **An empty `impl` block compiles, silently.** Deleting `Frontmatter::none()` left `impl Frontmatter { }` holding nothing but a blank line. The build reported no warning; it was found by reading the diff.
- **A derive nothing uses.** `Frontmatter` gained `#[derive(Debug, Default)]` when only `Default` was needed; nothing prints a `Frontmatter`. It costs nothing at runtime, but in this codebase a derive marks that something relies on it — M3 added `Debug` only where `unwrap_err()` demanded it — so the extra one was removed.

## Related

[[iterators]] · [[closures]] · [[structs]] · [[enums-and-data]] · [[testing]]
