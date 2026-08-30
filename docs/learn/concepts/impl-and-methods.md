# impl-and-methods

An `impl` block attaches functions to a type. There are two kinds, and the difference is whether the function takes `self`.

```rust
impl Frontmatter {
    fn none() -> Self {                      // associated function: no self
        Frontmatter { name: None, description: None }
    }
}

impl Entry {
    fn doc(&self) -> Option<&Path> { ... }   // method: borrows the value
    fn load_frontmatter(&mut self) { ... }   // method: may change it
}

let fm = Frontmatter::none();                // :: — there is no value yet
entry.load_frontmatter();                    // .  — asking an existing value
```

- **Rust has no constructor syntax.** `new` is a convention, nothing more, and a type may have several (`Frontmatter::none`, `Source::new`).
- **`Self`** is an alias for the type being implemented. Writing `-> Self` rather than `-> Frontmatter` means renaming the type does not drag the signature along.
- Free functions are still fine. `md_files(dir)` is attached to nothing because it belongs to no single value; `entries()` is a method because it answers a question **about one Source**.

## The three ways to take `self`

| Receiver | Means | In this code |
|---|---|---|
| `&self` | read it | `doc()` — which file describes this Entry |
| `&mut self` | change it in place | `load_frontmatter()` — overwrite name, fill description |
| `self` | consume it | not used yet |

## Matching on a field without moving it

```rust
fn doc(&self) -> Option<&Path> {
    match &self.kind {                       // note the &
        EntryKind::File => Some(&self.path),
        ...
    }
}
```

Dropping that `&` asks to move `self.kind` out of a value the method has only **borrowed**, and the compiler refuses with `cannot move out of borrowed content`. Matching on `&self.kind` looks at the variant in place ([[borrowing]]).

## Why a shared borrow and a mutable one can sit in one function

```rust
fn load_frontmatter(&mut self) {
    let Some(doc) = self.doc() else { return };            // shared borrow of self
    let Ok(text) = fs::read_to_string(doc) else { return };

    let fm = parse_frontmatter(&text);
    if let Some(name) = fm.name {
        self.name = name;                                  // mutable use of self
    }
    self.description = fm.description;
}
```

By the rules in [[borrowing]] this looks illegal. It compiles because a borrow lasts until its **last use**, not to the end of the block: `doc` is never touched after `read_to_string`, so `self` is free again from that line on. (This is NLL, non-lexical lifetimes. Older Rust really did reject it.)

## Pitfalls hit — a field nothing reads is a field nothing checks

`bundle_dirs` was first written with the Lead hard-coded:

```rust
kind: EntryKind::Bundle { lead: Option::None }    // every Bundle claims to have no Lead
```

All eight Bundles on this machine do have a `SKILL.md`, so the value was wrong for every one of them, and **nothing failed** — at that point no code read `lead`. The compiler said only `warning: field lead is never read`, which is easy to wave off as "not wired up yet".

Two things follow. A dead-code warning on a field is worth reading as *"nothing has ever checked this value"*. And the first honest test of a value is the first code that consumes it — here `doc()`, which is exactly where the mistake surfaced.

(`Option::None` also spells out what `None` already says: `Option`, `Some` and `None` are in the prelude.)

## Related

[[structs]] · [[enums-and-data]] · [[borrowing]] · [[option-and-match]]
