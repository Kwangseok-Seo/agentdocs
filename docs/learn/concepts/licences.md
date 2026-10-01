# licences

A binary is made of other people's code, and their licences come with it. Each crate says its licence in `Cargo.toml`, as an **SPDX expression**:

| `license =` | means |
|---|---|
| `"MIT"` | that licence |
| `"MIT OR Apache-2.0"` | either, as whoever uses the crate chooses — the Rust convention, and agentdocs' own |
| `"(MIT OR Apache-2.0) AND Unicode-3.0"` | one of the first two, and the third as well: unicode-ident has Unicode's data in it |
| `"MIT/Apache-2.0"` | an old spelling of `OR` |

## What a binary owes

MIT, ISC and Apache-2.0 ask that the licence, with its copyright lines, go with every copy of the software, a binary included. Unlicense, 0BSD and CC0 ask nothing; Zlib asks it of copies of the source, Boost's BSL-1.0 exempts machine code, and Apache-2.0's LLVM exception exempts code compiled into another program. A crate under `OR` lets the user choose, and the choice can be the one that asks nothing — when there is one. This is reading the licences' own words, not advice from a lawyer.

Counted for the five release targets ([[cross-compilation]]) on 2026-10-01, with `cargo tree -e normal --target <triple>` for each:

```
crates in some binary                     89   (78 to 80 in each, 70 in all five)
  under MIT alone, or ISC                 22   ratatui, crossterm, pulldown-cmark, syntect, inotify, …
  under MIT OR Apache-2.0, any spelling   55   either one asks for the notice
  under one that asks nothing of a binary,
  or with one to choose                   12   notify (CC0), foldhash (Zlib), memchr (Unlicense OR MIT), …
crates that only run while compiling      21   proc-macros — serde_derive, syn, quote — and what only they use
```

So 77 of the 89 ask for their notice whichever licence is chosen, and the archives carry them ([ADR-0014](../../adr/0014-every-archive-carries-the-licences-of-what-it-is-built-from.md)). A proc-macro runs inside the compiler and writes code; none of its own is in the binary.

## What is in a binary that is not a crate

`cargo tree` lists crates, and a binary holds more:

- **The standard library**, linked into every Rust program, under MIT OR Apache-2.0.
- **musl**, the C library linked into the Linux binaries ([[cross-compilation]]), under MIT: Rust 1.94 ships musl 1.2.5, and its `COPYRIGHT` file is the notice.
- **Data a crate embeds.** syntect's default syntax definitions are 75 files from Sublime Text's Packages, compiled into the binary. The repository's licence asks nothing, but three of its files carry their own MIT licence — Rust, C# and YAML.
- **Microsoft's code in the Windows binary.** With `+crt-static` the linker takes the C runtime from three static libraries — `libcmt` and `libvcruntime` from Visual Studio, `libucrt` from the Windows SDK — and any Windows build also links the SDK's import libraries, `kernel32.lib` and the rest. Linking the C runtime as a DLL would not take Microsoft's code out: its start-up code, Microsoft's documentation says, is always linked in.

What Microsoft asks of that code is in the licence terms of Visual Studio 2026 and of the Windows SDK, read on 2026-10-01. The SDK's list of what may be distributed names its `.lib` files, "built as part of your program"; Visual Studio's names none of its static libraries, and its documentation marks only their debug builds "Not redistributable". Neither asks for a licence text to go with them. Both ask that whoever passes the code on add significant function of their own, require those they pass it to to protect it as much as Microsoft's terms do, and indemnify Microsoft; the SDK asks as well for the program's own copyright notice, which `LICENSE-MIT` carries. The second is the one a file can meet, and `THIRD-PARTY-LICENSES.txt` ends with a section that does, written after the one CPython ships with its Windows build, `PC/crtlicense.txt`.

## cargo-about

cargo-about reads the dependency graph for the targets it is given, chooses one licence for each crate from an `accepted` list, in that list's order, finds the crate's licence files, and fills a **handlebars** template with what it found — each licence text once, with the crates it covers. Where it reads no file of the crate's — there is none, or it passes over the one there is — it uses the licence's standard text, which for MIT is `Copyright (c) <year> <copyright holders>`, and exits 0. A **clarification** in `about.toml` names the file — in the crate, or in its repository at the commit it was packaged from, with a checksum — and a checksum that no longer matches is only a warning.

`.github/third-party-licenses.sh` runs it on every push and before a release is built ([[continuous-integration]]). It fails on a warning, and on a mark the template prints beside any text that has no file it was read from: `{{#unless source_path}}`. The mark asks cargo-about's own record of where each text came from, and fails the other way if that record is ever missing — every text is marked, and the script stops.

## Pitfalls hit

- **`{{text}}` escaped every quote.** Handlebars is written for HTML: `{{ }}` replaces `"` with `&quot;` and `<` with `&lt;`, and 312 quotes in the licences came out that way. `{{{ }}}` writes a value as it is. Counting `&quot;` in the output caught it.
- **Nine crates were given a placeholder for their copyright line, with exit 0.** Four ratatui crates ship no licence file, and five windows crates name theirs `license-mit`; each was given MIT's standard text. Searching the output for `<year>` caught it.
- **The first guard passed one.** It failed only on warnings. Run in a container with a copy of the tree that left out agentdocs' own `LICENSE-MIT`, it passed a template for agentdocs itself: a crate with no file is no warning. The mark was added, and each check was then taken out in turn — without the mark, a crate with no file passes.
- **`cargo install cargo-about` installed nothing**, exit 0, with one line of warning: its binary is behind a feature, `--features cli`.

## Related

[[external-crates]] · [[cross-compilation]] · [[continuous-integration]]
