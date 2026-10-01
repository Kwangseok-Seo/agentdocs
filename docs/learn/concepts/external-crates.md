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

M8's syntect is where the lever mattered most. By default it builds its regular expressions with Oniguruma, a library written in C — `Compiling onig_sys` compiles C code, and needs a C compiler on whatever machine builds agentdocs, which a release for several platforms (M11) would have to provide for each. Its features offer a regex engine written in Rust instead:

```toml
syntect = { version = "5.3", default-features = false, features = ["default-syntaxes", "regex-fancy"] }
```

Both engines were built and timed on this machine's code blocks. Oniguruma was the faster, about twice: the heaviest file, 445 lines of code, took 13.5 ms against 28.4. The Rust engine was taken, for the build. It added 14 packages to `Cargo.lock`, 174 to 188, none of them a `-sys` crate. The binary grew from 1.0 MB to 3.46 MB, nearly all of it the language definitions — the themes syntect also ships, left out once the colours were our own, were 42 KB of it.

## Profiles: how each crate is built

A **profile** says how `cargo` builds: `dev` for `cargo build`, `cargo run` and `cargo test`, `release` for `--release`. `dev` does not optimise, and a regex engine that is not optimised is slow — the heaviest file took 358 ms to draw under `cargo run`, against 30 ms in a release build. A profile can be changed for other people's crates alone:

```toml
[profile.dev.package."*"]
opt-level = 3
```

`"*"` is every package but this one. With it, the slowest file takes 53 ms under `cargo run` at the preview's width in a 120-column terminal, and 42 ms at width 20; before M8 coloured anything, the slowest took 48 and 65. agentdocs itself is still built for debugging, [[integer-overflow]] checks included. The price is paid once: each dependency is built optimised the first time, and kept.

## One crate, several systems: notify

notify hears from the system that a file changed, and on each system it asks a different part of it — `ReadDirectoryChangesW` on Windows, inotify on Linux, FSEvents or kqueue on macOS and the BSDs. `Cargo.lock` lists every package any of them could need: adding notify put 17 there. A build compiles only those for its own system — seven new ones on Windows: notify and notify-types, `libc`, `log`, and `windows-sys` 0.60 with its two crates of targets.

The backends do not behave alike, and what one does is not shown by another:

| | Windows (run here) | Linux (read in notify 8.2's source) |
|---|---|---|
| a file changed behind a link, below a directory watched whole | not reported — 0 reports, whichever path it was written through; the link watched by its own path, 2 | reported — notify follows links when it sets up watches below a directory |
| a file opened or read | not reported: in the two threads' log, the reading that followed a change brought no report after it | reported, every open: notify asks inotify for them |

Both rows of the Linux column are notify's defaults, and neither suits the screen. Followed links would be watched into, which a Walk never does ([ADR-0007](../../adr/0007-links-are-listed-not-followed.md)); and the Walks open every file they read, so every reading would be reported, and taken for a change, would set off the next. The first is a setting, `Config::default().with_follow_symlinks(false)`; the second is what the closure handed to notify leaves out ([[channels]]). Neither was run on Linux, which was not at hand. The closure is tested by calling it with a report of an open made up for the purpose; the setting cannot be tested here at all — the Windows backend does not read it — and turning it back on passes every test on this machine ([ADR-0011](../../adr/0011-changes-are-heard-not-polled.md)).

## Crates already built: serde and thiserror (M10)

M10's config files ([[serde]], [[toml]], [[error-types]]) asked for three crates, and two of them were here already. syntect reads its packed language definitions with serde, and both ratatui and syntect report their errors through thiserror 2, so `serde` with its derive, `serde_derive` and `thiserror` were being compiled for them all along; named as dependencies of agentdocs, they compiled nothing new. The third, `toml`, turns on by default a writer of TOML, `toml_writer`, which a program that only reads does not call; with

```toml
toml = { version = "1.1.6", default-features = false, features = ["parse", "serde", "std"] }
```

it cost five compiled crates — `toml`, `toml_parser`, `toml_datetime`, `serde_spanned` and `winnow`. `Cargo.lock` grew by eight, 205 to 213: the writer, `indexmap` and a second `hashbrown` are locked, behind features nothing turns on, and not built. toml asks for Rust 1.85, under this package's 1.87. The release binary went from 3.81 MB to 4.08 MB.

## A crate's traits come with it

Most of what a crate adds to types you already have arrives as trait methods, and those exist only where the trait is imported: `use std::io::IsTerminal;` for `stdout().is_terminal()`, `use base64::Engine;` for `STANDARD.encode(…)`. See [[traits]].

## Pitfalls hit

- **"What is a crate? I don't know what it means."** The word had been used for a whole slice without being defined. The table above, the count of `Compiling` lines, and a look at ratatui's own `lib.rs` re-exporting its sub-crates answered it; asked next how many crates adding ratatui would pull in, the guess was about 30, and the build said 63.
- **Two wrong claims about resolution, both corrected by the tool.** The assistant said ratatui's newest release needed Rust 1.86, and that Cargo always takes the newest compatible version. The warning `cargo add` printed showed both wrong: 1.86 was 0.30.0's requirement, 0.30.2 needs 1.88, and `rust-version` is what held the choice back.

## Related

[[modules]] · [[traits]] · [[event-loop]] · [[statics]] · [[integer-overflow]] · [[channels]] · [[file-types-and-links]] · [[serde]] · [[toml]]
