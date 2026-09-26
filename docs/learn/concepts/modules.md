# modules

A module is a named box of items — functions, types, other modules — with a wall around it. Each file is one, but only once its parent declares it with `mod name;`. Everything inside is **private to that module** until marked `pub`, and other modules reach it by path: `crate::listing::heading`.

## A file is a module only once it is declared

```rust
// main.rs
mod entry;
mod frontmatter;
mod listing;
mod source;
mod tui;
#[cfg(test)]
mod testutil;
```

Each line tells the compiler to read `entry.rs` and treat it as the module `entry`. A file that no `mod` line names is not an error: it is simply never compiled — a probe in M5 put an `orphan.rs` beside `main.rs`, and the build said nothing about it. `#[cfg(test)]` on the last line compiles `testutil` only when building the tests, so the fixtures never reach the binary.

## Private until `pub`, one level at a time

Moving code out of one 1,403-line `main.rs` into five files produced errors that name each wall in turn — one code per wall, counted over the two builds it took:

```
E0603  a private function or type, used from another module    11 of the first 18
E0624  a private method of a public type                        16 of the next 48
E0616  a private field of a public struct                       28 of the next 48
```

`pub struct Entry` opens the type, not its fields; `pub fn` inside `impl Entry` has to be written per method. In Go, every file of a package sees everything in it, and only capitalised names leave the package. In Rust every module is walled, even from its neighbours in the same crate.

M5 opened only what another file actually used — all of `Entry`'s fields, because `source.rs` builds Entries, but not `Source`'s `path` and `walk`, which only its own `entries()` reads. Wrapping every field in an accessor was the alternative, and was not worth it while no field guards a rule an outside write could break.

## Paths

| written | means |
|---|---|
| `crate::listing::heading` | from the root of this crate down |
| `super::*` | everything in the parent module — how a `tests` module sees the code above it |
| `use crate::entry::Entry;` | bring the name in, so the rest of the file can say `Entry` |

`use` only shortens a path; it does not make anything visible that `pub` did not. It is also what puts a trait's methods in reach — see [[traits]].

## Modules may point at each other

Go refuses an import cycle between packages. Rust modules in one crate may refer to each other freely. M5 keeps the arrows one way anyway — `main → listing → source → entry → frontmatter`, with `tui` beside `listing` — because a reader can then start at any file and know that nothing below it depends on anything above.

## Pitfalls hit

- **Where does this go?** Shown a table of what each module could hold and asked how to divide `main.rs`, the answer was *"frontmatter.rs and entry.rs should be separate, beyond that I'm not sure."* An execution trace of one real run — discover, walk, print — came next and did not land: *"I still don't follow."* Both stalls were the placement layer, and after the second the split was done and explained file by file.
- **Count the root errors, not all of them.** The first build after the move reported 18 errors, and 7 of them (E0282, type annotations needed) were downstream of the 11 privacy errors: fixing the `pub`s made them vanish without being touched.

## Related

[[structs]] · [[impl-and-methods]] · [[traits]] · [[testing]] · [[external-crates]]
