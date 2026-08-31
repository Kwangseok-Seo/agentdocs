# agentdocs learning wiki — index

A wiki of the **Rust and tooling knowledge** accumulated while building agentdocs. To look something up, **pick it from this index and read only that page** — the pages are atomic and linked to each other with `[[wikilink]]`, forming a graph rather than a tutorial to read front to back.

> Domain vocabulary → [`/CONTEXT.md`](../../CONTEXT.md) · architecture decisions → [`/docs/adr/`](../adr/) · **this directory is for language and tooling only.**

## Concepts (`concepts/`)

- [[ownership]] — one owner per value, move vs `Copy`, why the rule exists at all
- [[borrowing]] — `&T` and `&mut T`, the three rules, and the silent bug each one prevents
- [[owned-vs-borrowed-pairs]] — `String`/`&str`, `PathBuf`/`&Path`, and why signatures borrow in and own out
- [[option-and-match]] — `Option`, `match` exhaustiveness, `if let`, guard clauses, and what `unwrap` throws away
- [[paths]] — paths are not strings; `join`, `parent`, `extension`, `starts_with`, `display`
- [[fs-read-dir]] — one level only, `Result` twice over, what `count()` really counts, recursion
- [[macros-and-formatting]] — `println!` and why its first argument must be a literal
- [[mutability]] — immutable by default, what a leading `_` costs you, shadowing
- [[structs]] — named fields, where a tuple gives out, and how to pick a constructor's argument types
- [[enums-and-data]] — variants that carry data, illegal states made unwritable, and why the tag is free
- [[impl-and-methods]] — associated function vs method, `Self`, `&mut self`, and borrows that end early
- [[vec]] — growth and reallocation, the three ways to iterate, and `&[T]`
- [[str-scanning]] — `lines`, `split_once`, `peek`, and why cutting by bytes is a lottery
- [[result-and-errors]] — `Result`, the four ways an error can end, `?` and why propagation is contagious
- [[file-types-and-links]] — asking about the entry or about the target, and the two opposite bugs one boolean produced
- [[testing]] — `#[cfg(test)]`, fixtures without a crate, and why a green suite proves nothing until you break the code

## Milestones (`milestones/`)

The journey log: what was built, which concepts it required, and which pitfalls were actually hit.

- [M0](milestones/M0.md) — project skeleton
- [M1](milestones/M1.md) — source discovery
- [M2](milestones/M2.md) — domain model and frontmatter
- [M3](milestones/M3.md) — error handling and the first tests
