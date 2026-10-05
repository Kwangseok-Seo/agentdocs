# agentdocs learning wiki — index

A wiki of the **Rust and tooling knowledge** accumulated while building agentdocs. To look something up, **pick it from this index and read only that page** — the pages are atomic and linked to each other with `[[wikilink]]`, forming a graph rather than a tutorial to read front to back.

> Domain vocabulary → [`/CONTEXT.md`](../../CONTEXT.md) · architecture decisions → [`/docs/adr/`](../adr/) · **this directory is for language and tooling only.**

## Concepts (`concepts/`)

- [[ownership]] — one owner per value, move vs `Copy`, why the rule exists at all, and moving into another thread
- [[borrowing]] — `&T` and `&mut T`, the three rules, the silent bug each one prevents, a borrow that ends at its last use, a call that lends what is left of its dot, and a compiler that reads the signature, not the body
- [[owned-vs-borrowed-pairs]] — `String`/`&str`, `PathBuf`/`&Path`, why signatures borrow in and own out, and `Cow` — what `&` and `clone()` do to a box, and `to_mut`, which copies only when it has to
- [[option-and-match]] — `Option`, `match` exhaustiveness and the `if let` a new variant slips past — and the `match` that stopped the build for one, guard clauses, `None` against `Some` of nothing, `is_some_and` and `is_none_or` written out as a `match`, and what `unwrap` throws away
- [[paths]] — paths are not strings; `join`, `parent`, `extension`, `starts_with`, `display`
- [[fs-read-dir]] — one level only, `Result` twice over, what `count()` really counts, recursion
- [[macros-and-formatting]] — `println!` and why its first argument must be a literal
- [[mutability]] — immutable by default, what a leading `_` costs you, shadowing
- [[structs]] — named fields, where a tuple gives out, and how to pick a constructor's argument types
- [[enums-and-data]] — variants that carry data, illegal states made unwritable, why the tag is free, one type for two kinds of message, and a variant handed over as a function
- [[impl-and-methods]] — associated function vs method, `Self`, `&mut self`, and borrows that end early
- [[vec]] — growth and reallocation, the three ways to iterate, and `&[T]`
- [[str-scanning]] — `lines`, `split_once`, `peek`, why cutting by bytes is a lottery, and why a lowercase copy is not the same length
- [[result-and-errors]] — `Result`, the four ways an error can end, `?` and why propagation is contagious, and a `Result` inside a `Result` whose layers mean different things
- [[error-types]] — an error type of the program's own for two kinds of failure, a variant handed to `map_err`, what makes a type an error, `thiserror` writing `Display` and `From`, whose words are shown, and `From` with `Into`
- [[file-types-and-links]] — asking about the entry or about the target, the two opposite bugs one boolean produced, and watching through a link
- [[testing]] — `#[cfg(test)]`, fixtures without a crate, a path held open so nothing can read it, why a green suite proves nothing until you break the code, a key whose new meaning disarms old tests, the environment handed in, proving that nothing happens, the loop driven from outside, two tests given one directory, and a mutation only a long list catches
- [[iterators]] — one required method and 75 free ones, adapters vs consumers, laziness, stopping early, `filter_map`
- [[closures]] — functions that capture, the three ways they hold what they use, `Fn` / `FnMut` / `FnOnce`, a function handed over by its name, a closure taken or handed back as `impl FnMut`, and a value moved out of a closure called more than once
- [[traits]] — promises with default bodies, `derive` as a compiler-written `impl`, bounds and `impl Trait`, why a trait's methods need the trait in scope, a derived order set by the order fields are declared, `PartialEq` derived to compare two Walks, `impl Into<String>`, and derives from other crates
- [[modules]] — a file is a module once declared, private until `pub` one wall at a time, `crate::` and `super::` paths
- [[external-crates]] — crate vs package vs module, a version as a range, what `rust-version` holds back, what one dependency costs, features that leave a C compiler out, a profile for other people's crates, one crate whose backends differ by system, and crates that were being built already
- [[event-loop]] — draw everything, wait, change the state; raw mode, the alternate screen, mouse capture, the wheel, waking up without an event, waiting for two things, and handing the terminal to an editor
- [[drop-and-unwinding]] — what runs when a value goes, what a panic skips, why the screen needs a hook, and `let _` vs `let _name`
- [[lifetimes]] — how long a borrow is valid, the three elision rules checked against every function here, a borrow handed out through an argument, `<'a>` on a type, which way `'static` fits, why a thread's closure needs it, and a borrow of what a function made, inside what it returns
- [[slices]] — a window of start and length, why a window shows one run only, ranges, and bytes vs characters vs columns
- [[recursive-data]] — a type that holds itself, why that needs an arrow (E0072), `Box` for one and `Vec` for many, a level holding only the level below, and functions that call themselves
- [[state]] — what the screen keeps and what it works out again: kept by what does not move, corrected by drawing, a moment as an `Option<Instant>`, a highlighter's stack carried from line to line, and what a Walk finds again replacing only what differs
- [[hash-maps]] — `HashMap` and `HashSet`, keyed by path rather than row number, `entry().or_default()`, and what a key needs
- [[integer-overflow]] — a panic in a debug build and a wrong number in a release one, and arithmetic that says which end it wants
- [[statics]] — one value for the whole run, what the compiler can work out (E0015, E0010), `LazyLock` for the rest, and why the theme has to be one
- [[processes]] — another program: `Command` and its three ways to run, an exit code that is not an error, a program that is not a shell line, `OsString`, `cfg`, and the terminal handed over
- [[threads]] — a second line of execution: `spawn`, `move` and `'static`, where a thread is and when, and two readers of one terminal
- [[channels]] — `Sender` and `Receiver`, waiting with and without a limit, two kinds of message in one enum, a channel going back, and a closure another thread calls
- [[serde]] — text into values without either side knowing the other: the format, the shape and the derive between them, names taken from the enum, and a field missing or one too many
- [[toml]] — keys, tables and arrays of tables, a key that belongs to the header above it, one table where an array is wanted, and what an error carries
- [[release-profiles]] — what `--release` builds with: stripping, link-time optimisation, one codegen unit, optimising for size, and why a panic still unwinds — each measured
- [[cross-compilation]] — a target as a triple, the standard library and the linker each one needs, checking without a linker, musl for every Linux, and the C runtime linked into Windows
- [[platform-differences]] — what the same code met elsewhere: the order a directory is read in, a path no one may read, a link the system resolves, and the reason a test expected
- [[continuous-integration]] — workflows, jobs and a matrix, `SKIPPED` as a failure, a release that waits on its installers, actions pinned by commit, and what a run costs
- [[licences]] — a crate's licence as an SPDX expression, what a binary owes the crates in it, what a binary holds that is not a crate, and cargo-about's quiet template

## Milestones (`milestones/`)

The journey log: what was built, which concepts it required, and which pitfalls were actually hit.

- [M0](milestones/M0.md) — project skeleton
- [M1](milestones/M1.md) — source discovery
- [M2](milestones/M2.md) — domain model and frontmatter
- [M3](milestones/M3.md) — error handling and the first tests
- [M4](milestones/M4.md) — search
- [M5](milestones/M5.md) — the screen
- [M6](milestones/M6.md) — Markdown in the preview
- [M7](milestones/M7.md) — the tree
- [M8](milestones/M8.md) — scrolling, a selection past the rows in view, and code in colour
- [M9](milestones/M9.md) — the editor, and a screen that follows the disk
- [M10](milestones/M10.md) — config files
- [M11](milestones/M11.md) — distribution: three systems, five binaries, two installers
