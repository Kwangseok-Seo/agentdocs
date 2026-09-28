# external-crates

A **crate** is what the compiler compiles in one go — one `Compiling …` line in the build output. agentdocs is one crate, `main.rs` is its root and the other files are [[modules]] inside it, and ratatui is someone else's crate that ours now depends on. `Cargo.toml` says which crates and what range of versions; `Cargo.lock` records exactly which versions were chosen.

## Crate, package, module

| word | what it is | in agentdocs |
|---|---|---|
| module | a walled box of items inside a crate | `entry`, `listing`, `tui`, … |
| crate | one unit of compilation; a **binary** crate has `main.rs`, a **library** crate has `lib.rs` | the binary `agentdocs` |
| package | a `Cargo.toml` and the crates it builds | this repository |

Only a library crate can be depended on, and `use ratatui::…` reaches into the items its `lib.rs` makes public. Go uses the same words for other things — a Go package is closer to a Rust module, a Go module closer to a Rust package — which is why the words collide.

## A version is a range, and the resolver picks within it

`cargo add ratatui` wrote one line:

```toml
ratatui = "0.30.0"
```

That means `>=0.30.0, <0.31.0`: any release the author promises is compatible. Cargo picks **the newest version in that range that this package can build** — and `rust-version = "1.87"` in `Cargo.toml` is part of "can build". ratatui 0.30.2 requires Rust 1.88, so the resolver chose 0.30.0 and said so as it did. Go's resolver does the opposite by design (the *minimum* version anything asks for); Cargo's goes as high as the constraints allow.

`Cargo.lock` then pins the choice, with checksums, for every crate in the tree. A build next month gets the same code until someone runs `cargo update`.

## What one line costs

Measured on this machine before and after adding ratatui:

```
crates compiled        1  →  63   (62 new, on Windows)
packages in Cargo.lock    →  172  (78 buildable on some platform; 94 behind features nobody turned on)
clean debug build   0.7 s →  6.5–8.8 s
release binary     231 KB →  231 KB after `cargo add` alone
                          →  468 KB once the loop called into it
```

The third and fourth lines are the interesting pair. Adding the dependency cost build time at once and binary size not at all: the linker keeps only code that something calls, so the binary grew only when the [[event-loop]] started calling ratatui.

Later crates cost nothing new. M5 added `base64` and `unicode-width` as direct dependencies at the versions already in `Cargo.lock` — `unicode-width` because ratatui measures cells with it, `base64` because another of ratatui's optional backends had locked it — so the lock gained two lines naming them and no new packages. Using the exact function ratatui measures width with, rather than a second opinion, is the point of the first. M6's `unicode-segmentation` came the same way: one line in the lock, for the grapheme rule ratatui already follows.

A crate's **features** are the other lever. M6's Markdown parser, pulldown-cmark, turns on by default `getopts`, the argument parser of its own command-line tool, and `html`, a writer this viewer never calls; with `default-features = false` it cost three compiled crates — itself, `unicase` and `memchr` — and parsed every one of this machine's 558 files, 6.1 MB, in 99 ms. `Cargo.lock` gained only two packages, 172 to 174: `memchr` was locked already, named by `regex`, `nom` and three others that nothing here compiles, and pulldown-cmark is the first dependency here that builds it. The two counts answer different questions — what the lock knows of, and what the compiler builds.

## A crate's traits come with it

Most of what a crate adds to types you already have arrives as trait methods, and those exist only where the trait is imported: `use std::io::IsTerminal;` for `stdout().is_terminal()`, `use base64::Engine;` for `STANDARD.encode(…)`. See [[traits]].

## Pitfalls hit

- **"What is a crate? I don't know what it means."** The word had been used for a whole slice without being defined. The table above, the count of `Compiling` lines, and a look at ratatui's own `lib.rs` re-exporting its sub-crates answered it; asked next how many crates adding ratatui would pull in, the guess was about 30, and the build said 63.
- **Two wrong claims about resolution, both corrected by the tool.** The assistant said ratatui's newest release needed Rust 1.86, and that Cargo always takes the newest compatible version. The warning `cargo add` printed showed both wrong: 1.86 was 0.30.0's requirement, 0.30.2 needs 1.88, and `rust-version` is what held the choice back.

## Related

[[modules]] · [[traits]] · [[event-loop]]
