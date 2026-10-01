# enums-and-data

A Rust `enum` is not a set of named integers. It is a type whose value is **exactly one of a listed set of shapes**, and each shape may carry its own data.

```rust
enum Scope { Global, Project }                  // no data

enum EntryKind {
    File,                                       // no data
    Bundle { lead: Option<PathBuf> },           // struct-style variant
}

enum Option<T> { Some(T), None }                // tuple-style variant, from the standard library
enum Result<T, E> { Ok(T), Err(E) }
```

`Option` and `Result` are ordinary enums — nothing about them is built into the language. Learning the shape once pays for all three ([[option-and-match]]).

## Exhaustiveness is the point

```rust
match scope {
    Scope::Global  => ...,
    Scope::Project => ...,
}                                   // no catch-all arm needed
```

The compiler knows the list is closed, so covering it is enough. Add a third variant later and **every** `match` that does not handle it becomes a compile error naming the variant — the compiler produces the list of places to edit. M7 did exactly that: `Node` gained `Unreadable`, and the build named the three `match`es to extend. It did not name the `if let` beside them ([[option-and-match]]).

Compare with the same thing done on strings:

```rust
match scope {
    "global"  => ...,
    "project" => ...,
    _         => ...,               // required: a string can be anything
}
```

Now `"gloabl"` compiles, falls into `_`, and is wrong at runtime. The enum makes the typo impossible to write.

Both forms appear in this codebase, and the contrast is deliberate: `match key.trim()` over frontmatter keys **does** need `_ => {}`, because the key really can be any string and unknown keys (`model:`, `origin:`) must be ignored.

## Data belongs to the variant that has it

```rust
match &entry.kind {
    EntryKind::File => ...,
    EntryKind::Bundle { lead: Some(path) } => ...,   // lead is reachable only here
    EntryKind::Bundle { lead: None } => ...,
}
```

`lead` cannot be read without going through the `Bundle` arm. The alternative — a flat struct with a tag — allows states that mean nothing:

```rust
struct Flat { is_bundle: bool, lead: Option<PathBuf> }

Flat { is_bundle: false, lead: Some(p) }   // a plain file that has a Lead?
Flat { is_bundle: true,  lead: None }      // deliberate, or forgotten?
```

Both compile. With the enum the first **cannot be written at all**, and the second is spelled `Bundle { lead: None }`, which says it on purpose. This is what "make illegal states unrepresentable" means in practice.

## And it is smaller, not larger

Measured on 64-bit Windows, on the shapes above as M2 wrote them (since M7 a Bundle also carries `inside`, the tree below its Lead — [[recursive-data]]):

```
Walk             1 byte    three variants, no data: a tag byte is enough
Option<PathBuf> 32 byte
EntryKind       32 byte    File | Bundle { lead } — the tag costs nothing
Flat            40 byte    the hand-rolled bool tag costs 8
Option<u8>       2 byte
```

`EntryKind` matching `Option<PathBuf>` exactly is a **niche optimization**: the pointer inside a `PathBuf` can never be null, so the compiler spends those unused bit patterns on `File` and on `None` instead of carrying a separate tag. `Option<u8>` needs two bytes because a `u8` has no spare pattern to borrow.

## The same idea for something that changes: a phase

M5's drag selection goes through three stages: the button is down but has not moved off its cell, it has moved and the cells between are highlighted, it has been let go and the text copied. Two booleans, `dragged` and `done`, would make four combinations, one of them — done without ever dragging — meaningless. An enum makes three:

```rust
enum Phase {
    Pressed,
    Dragging,
    Done,
}
```

Each event moves it along one edge: a drag turns `Pressed` into `Dragging`, letting go turns `Dragging` into `Done` — and a `Pressed` let go is a plain click, so it is dropped rather than finished. Drawing asks one question, `phase != Phase::Pressed`, to know whether to highlight.

## One type for two kinds of message

A channel carries one type, and M9's loop waits for two kinds of thing on one channel: an event from the terminal, which carries the event or the error reading it, and word that a file may have changed, which carries nothing. One enum is both ([[channels]]):

```rust
enum Message {
    Input(io::Result<Event>),
    Changed,
}
```

The loop's `match` on it has to say what each is done with, and a third kind added later would be refused until it did.

## Pitfalls hit — variant names must exclude each other

The first names proposed for the three [[structs]] traversal rules were `entries`, `bundleDirs`, `LeadDirs`. Two problems, both worth generalising:

- **`entries` did not distinguish anything.** All three rules produce Entries. A variant name has to say what makes *this* variant different from the others, so hold the names side by side and check that each one excludes the rest.
- **`bundleDirs` and `LeadDirs` named the same thing.** `CONTEXT.md` defines a Bundle as a directory with a Lead, so "directories with a Lead" was already spelled Bundle. When a name duplicates a glossary term, the glossary wins.

## Pitfalls hit — casing is a lint, not a preference

```
warning: variant `entries` should have an upper camel case name
  |                 ^^^^^^^ help: convert the identifier to upper camel case: `Entries`
  = note: `#[warn(non_camel_case_types)]` (part of `#[warn(nonstandard_style)]`) on by default

warning: variant `bundleDirs` should have an upper camel case name
  |                          ^^^^^^^^^^ help: ... `BundleDirs`
```

Types and variants are `UpperCamelCase`; fields, functions and variables are `snake_case`. `camelCase` does not count as upper camel case — the first letter has to be capital.

## Related

[[structs]] · [[option-and-match]] · [[impl-and-methods]] · [[str-scanning]] · [[event-loop]] · [[recursive-data]] · [[channels]]
