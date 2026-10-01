# agentdocs

A terminal viewer for the Markdown that AI coding agents run on — **skills, rules, agents, and docs**. It gathers files scattered across your machine into one screen, so you can find them and read them without remembering where they live.

> Two motifs. [Shpigford/chops](https://github.com/Shpigford/chops) for *what to show*, [herdrdev/herdr](https://github.com/herdrdev/herdr) for *how to ship it*. Where chops is a macOS-only GUI app, agentdocs is a **single binary that runs inside the terminal on every platform** — and it covers **rules and docs**, not just skills.

## Two goals

1. **The tool** — find and read scattered agent documentation from one screen.
2. **Learning in the open** — this tool is built in Rust *while learning Rust*, and the process is published to show that **learning and collaborating with an AI agent can happen at the same time**.

So this repository is not a finished codebase. It is a **record of something growing while its author learns**, kept in the commit history.

## Status

**M11 complete** — typed at a terminal with no words, `agentdocs` opens a **screen of three panes**: the sources, the selected source's entries **as a tree**, and the selected entry's file **drawn as Markdown** — headings, bold and inline code, lists and quotes that keep their indent on every row, code blocks **coloured by their language**, tables, and frontmatter without its fences. A skill **opens onto its supporting files**, and a project's `docs/` onto its directories. `j`/`k` or the arrows move, `l`/`h` open and close a row, `Tab` goes round the panes, a click selects, **the wheel or Page Down scrolls the preview**, and **dragging across it copies the text** to the clipboard — held past its edge, the drag scrolls it and keeps selecting. **`e` opens the selected file in your `$EDITOR`**, and the screen comes back with what you wrote; and when another program — an agent in the next pane — changes a file the screen shows, **the screen reads it again by itself**. A **config file** adds sources of your own, and sets the order the sources are shown in. It is tested on **Linux, macOS and Windows** on every change, and a release carries a binary for each, with scripts that install it in one line — see [Install](#install).

```
$ cd ~/projects/cli-maker && agentdocs        # a 120 x 18 terminal; skills, session-retro opened, RULE-FORMAT selected
┌Sources───────────────────┐┌Entries───────────────────────┐┌Preview───────────────────────────────────────────1-15/155┐
│GLOBAL                    ││  dream                       ││# 전역 rule 파일 포맷                                     │
│  skills:6                ││  get-api-docs                ││                                                          │
│  rules:14                ││  grill-with-docs (link)      ││~/.claude/rules/*.md 에 발행하는 상시 가드레일의 형태.    │
│  agents:1                ││▾ session-retro               ││                                                          │
│  commands:1              ││  ▸ examples/                 ││핵심 제약: 이 파일들은 매 세션 컨텍스트에 실린다. 길이가  │
│  agents/skills:1         ││    RETRO-FORMAT              ││곧 상시 비용이고, 틀린 rule 은 상시 오염이다. 그래서 짧고,│
│PROJECT cli-maker         ││    RULE-FORMAT               ││강하고, 반증 가능해야 한다.                               │
│  root md:3               ││▸ synced                      ││                                                          │
│  docs:80                 ││  to-html                     ││## 골격                                                   │
│                          ││                              ││                                                          │
│                          ││                              ││# {제목 — 가드레일을 한 줄로 요약. 대시로 부제를 붙여도   │
│                          ││                              ││된다}                                                     │
│                          ││                              ││                                                          │
│                          ││                              ││## 가드레일                                               │
│                          ││                              ││                                                          │
└──────────────────────────┘└──────────────────────────────┘└──────────────────────────────────────────────────────────┘
 j/k ↓/↑ move   l/h open/close   tab pane   e edit   drag copy   q quit
```

Every row starts closed. `session-retro`'s own row shows its `SKILL.md`; the rows under it are what it carries besides. `grill-with-docs` is a link to a skill kept elsewhere, and says so, since what is behind a link is never walked into ([ADR-0007](docs/adr/0007-links-are-listed-not-followed.md)). The count beside a source stays the number of entries, however many rows are open — `docs:80` opens as two rows, `adr/` and `learn/`. In a terminal, headings are bold and cyan; code in a language the highlighter knows is coloured by kind — keywords, strings, comments, numbers, a diff's added and removed lines — in the terminal's own palette, and other code is yellow, as is Markdown shown as written, so that its headings are not taken for the file's own; a table that does not fit the preview becomes one block per row. Each file stays scrolled where it was left, and the title says which rows show.

`e` runs the editor `$VISUAL` names, or else `$EDITOR` — through your shell, so a value such as `code --wait` is read as it would be at a prompt — or else notepad on Windows and vi elsewhere ([ADR-0010](docs/adr/0010-the-editor-runs-through-the-shell.md)). The screen hears that a file changed from the system rather than by reading everything again on a timer, so nothing is read until a change is reported, and a change shows about a tenth of a second after it is written ([ADR-0011](docs/adr/0011-changes-are-heard-not-polled.md)).

With words, or with its output going to a pipe or a file, it prints the **listing** instead — which is also what an AI agent running it from its own shell gets ([ADR-0008](docs/adr/0008-screen-for-a-bare-command-at-a-terminal.md)). The listing is where M1–M4 happened: the binary discovers its sources, walks each one by its own rule, reads a name and description out of each entry's frontmatter, **says what it could not read instead of reporting zero**, and **searches**.

```
$ cd ~/projects/cli-maker && agentdocs | more   # abridged: the real output is 115 lines
GLOBAL
  skills:6
    dream                            Memory consolidation pass (auto-memory 시스…
    get-api-docs                     Use this skill to get documentation for thir…
    grill-with-docs                  Grilling session that challenges your plan a…
    ...
  rules:14
    adr-context-format               -
    adr-rationale-realization        -
    ...
  agents:1
    session-seal                     Claude Code 세션 스냅샷 draft → sealed 승격 …
  commands:1
    session-seal                     -
  agents/skills:1
    grill-with-docs                  Grilling session that challenges your plan a…
PROJECT C:\Users\adman\projects\cli-maker
  root md:3
    ...
  docs:80
    ...
```

The global block is identical wherever you run it; only the project block follows the current directory ([ADR-0002](docs/adr/0002-discovery-global-always-project-follows-cwd.md)). How each source is walked — one level of Markdown files, one level of directories, or a full recursive descent — is a column of the source table rather than a branch in the code ([ADR-0003](docs/adr/0003-source-table-claude-first.md)).

That output is longer than a terminal window, which was the argument for the screen above.

When something on disk is broken — and for a program that reads other people's directories, broken is the normal case rather than an edge one — the row says so instead of quietly rounding down:

```
  agents:(permission denied)     # the directory exists and holds a file; it will not open
  commands:0                     # genuinely empty
  agents/skills:(missing)        # no such path
  docs:3 (1 unreadable)          # three found, one subdirectory refused —
    C:\...\docs\guide\secret (permission denied)            # — and which one
```

Through M2 the first of those lines read `0`, indistinguishable from the second, because a failure that could not be expressed in the return type had already been discarded before anything could print it ([ADR-0006](docs/adr/0006-unreadable-is-counted-not-dropped.md)). Since M7 whatever could not be read is also a row of its own where it was found — in the listing by its full path, on the screen in the tree — and a file that will not open is one of them, rather than an entry with no description ([ADR-0009](docs/adr/0009-unreadable-is-a-row-where-it-was-found.md)). A directory link is now listed but never walked into — which is what stops a junction pointing at its own parent from turning a two-file directory into 128 rows ([ADR-0007](docs/adr/0007-links-are-listed-not-followed.md)).

Give it words and it keeps only the entries that contain every one of them — in the name, anywhere in the file, or anywhere in a skill's supporting files, ignoring case — counts what it kept out of the whole, and prints under each row the first line that holds a word, so you can see why it is there. A line from a supporting file is marked with that file's name, as in `RULE-FORMAT:88:`.

```
$ agentdocs adr 검증                           # abridged: 22 rows
GLOBAL
  skills:1/6
    session-retro                    세션 회고 — rule 판정을 원장에 쌓고, 사용자…
      74: | 비가역·의외·trade-off 결정 | **ADR** (repo) | `~/.agents/s…
  rules:5/14
    adr-context-format               -
      1: # ADR & CONTEXT Format Authority
    ...
PROJECT C:\Users\adman\projects\cli-maker
  root md:2/3
    CLAUDE                           -
      23: …성)은 `docs/adr/0001-runtime-interpreter-architecture.md`.
    ...
  docs:14/80
    ...
```

When the word sits further along a long line than fits, that line is shown from just before the word. The rules on this machine have no frontmatter and are written in Korean, which is why the search reads whole files: over names alone, `검증` finds 0 entries; over the text, 39 ([M4](docs/learn/milestones/M4.md)).

## Config files

Documentation does not always live where the defaults look. A file named `.agentdocs.toml` adds sources of your own and sets the order the sources are shown in: the one in your home directory speaks for the global block, and the one at a project's root for that project's, so a repository carries its own layout wherever under your home directory it is cloned ([ADR-0012](docs/adr/0012-config-files-sit-where-their-sources-do.md)). Neither has to exist.

```toml
# .agentdocs.toml at the root of session-seal, which keeps its documentation in two directories of its own.
order = ["Wiki"]             # shown first; every other source keeps its place

[[source]]
name = "Sessions"
path = "Sessions"            # from the project's root; ~/… is from your home directory
walk = "markdown-tree"       # every Markdown file below it, directories and all

[[source]]
name = "Wiki"
path = "Wiki"
walk = "markdown-files"      # one level of Markdown files
```

The third walk is `bundle-dirs`, one directory to each skill, as `~/.claude/skills` is walked. `order` has to come before the first `[[source]]`: in TOML every key after a table's header belongs to that table.

```
$ cd session-seal && agentdocs | more        # abridged: a copy of session-seal with that file, 365 lines in all
GLOBAL
  skills:6
  ...
PROJECT C:\...\session-seal
  Wiki:7
  root md:1
  docs:(missing)
  Sessions:323
```

A file that cannot be used — it will not read, it is not TOML, it holds a key that has no place in it, or `order` names a source that is not there — adds nothing and changes no order, and says so where its sources would have been, in the parser's own words. The same file with `markdown-tree` written `tree`:

```
PROJECT C:\...\session-seal                  # abridged: the root md row's entry left out
  root md:1
  docs:(missing)
  .agentdocs.toml:(invalid)
    TOML parse error at line 7, column 8
      |
    7 | walk = "tree"                # every Markdown file below it, directories and all
      |        ^^^^^^
    unknown variant `tree`, expected one of `markdown-files`, `bundle-dirs`, `markdown-tree`
```

On the screen the same row can be selected, and the preview shows those words. A key with no place is refused rather than passed over because `[[sources]]`, one letter too many, would otherwise add nothing and say nothing. Each file is read once, as the program starts.

## Where it is headed

```
$ cd ~/projects/cli-maker && agentdocs
┌ Sources ──────────────┬ Entries ─────────────┬ Preview ───────────────┐
│ GLOBAL        (always)│  ▸ dream             │                        │
│   skills           7  │  ▸ get-api-docs      │                        │
│   rules           19  │  ▸ grill-with-docs   │                        │
│   agents           1  │  ▾ session-retro     │                        │
│   commands         1  │      SKILL.md        │                        │
│ PROJECT  cli-maker    │      RETRO-FORMAT.md │                        │
│   docs            80  │      RULE-FORMAT.md  │                        │
│   root md          3  │      examples/       │                        │
│                       │  ▸ ship-with-review  │                        │
└───────────────────────┴──────────────────────┴────────────────────────┘
 / search   tab pane   e $EDITOR   q quit
```

The tree in that sketch is M7's, and the screen at the top of this page is how it came out — a skill's `SKILL.md` is shown on the skill's own row rather than as one of the rows under it. Handing a file to `$EDITOR` came in M9; searching from the screen is still ahead.

## Learning roadmap

One milestone = one Rust concept + one slice of functionality. Every milestone ends with **something you can run and see**.

| Milestone | Functionality | Rust concepts |
|---|---|---|
| **M0** ✅ | Project skeleton, `cargo run` | cargo, `Cargo.toml`, editions |
| **M1** ✅ | Source discovery — 5 global sources + the current repository | **ownership & borrowing**, `String` vs `&str`, `Vec`, `std::fs`, recursion |
| **M2** ✅ | Domain model + frontmatter parsing | `struct`, **`enum` + `match`**, `Option`, `impl` |
| **M3** ✅ | Error handling + first tests | **`Result`, the `?` operator**, `#[cfg(test)]`, entry vs target |
| **M4** ✅ | Search and filtering (still CLI) | **iterators & closures**, traits |
| **M5** ✅ | TUI skeleton — three panes, key and mouse navigation, drag to copy | external crates, **the event loop**, modules, `Drop` |
| **M6** ✅ | Markdown rendering — headings, lists, quotes, code blocks, tables, frontmatter, wrapped by the renderer | **lifetimes**, slices, `Cow` |
| **M7** ✅ | The tree — skills open onto their supporting files and `docs/` onto its directories; search reads supporting files; what cannot be read is a row | **recursive data structures**, `Box` and `Vec` |
| **M8** ✅ | Code block highlighting + scrolling — the wheel scrolls the preview, a drag can select past the rows in view | `syntect`, state management |
| **M9** ✅ | `$EDITOR` delegation + file watching — `e` opens the editor, and the screen follows what changes on disk | `std::process`, **threads & channels** |
| **M10** ✅ | Config files — `.agentdocs.toml` at home and at a project's root add sources and set their order; one that cannot be used says why where its sources would have been | `serde`, TOML, `thiserror` |
| **M11** ✅ | Distribution — tests on three systems, five binaries built from a tag, install scripts tried on them before each release | release profiles, cross-compilation |

> The theory, syntax, and pitfalls collected at each milestone live in [`docs/learn/`](docs/learn/) as a concept-by-concept knowledge base — the evidence for *what was actually learned*.

## Install

> No release has been published yet. The first, v0.1.0, comes once this repository is public; until then, `cargo install` below is the way in.

Each release has a binary for Linux (x86_64 and ARM, built static, so any distribution will do), macOS (Apple silicon and Intel) and Windows (x86_64), and two scripts that pick the right one, check it against the release's `SHA256SUMS`, and put it where your user can run it without an administrator ([ADR-0013](docs/adr/0013-releases-come-from-a-tag-and-the-installers-are-tried-first.md)).

Linux and macOS, to `~/.local/bin`:

```
curl -fsSL https://github.com/Kwangseok-Seo/agentdocs/releases/latest/download/install.sh | sh
```

Windows, in PowerShell, to `%LOCALAPPDATA%\Programs\agentdocs\bin`, which it adds to your `PATH`:

```
irm https://github.com/Kwangseok-Seo/agentdocs/releases/latest/download/install.ps1 | iex
```

`AGENTDOCS_VERSION=v0.1.0` installs that release rather than the latest, and `AGENTDOCS_INSTALL_DIR` puts it in another directory; on Windows, `AGENTDOCS_NO_MODIFY_PATH=1` leaves `PATH` as it is. The archives can also be downloaded from the [releases page](https://github.com/Kwangseok-Seo/agentdocs/releases) by hand. With Rust installed, `cargo install --git https://github.com/Kwangseok-Seo/agentdocs` builds it instead. `agentdocs --version` says which you have.

## Design decisions

Decisions that are expensive to reverse are recorded in [`docs/adr/`](docs/adr/README.md). Domain vocabulary lives in [`CONTEXT.md`](CONTEXT.md).

## Build

```
cargo run
```

Requires Rust 1.87 or newer. (The 2024 edition needs 1.85, but `std::env::home_dir` stayed marked deprecated until 1.87.)

## License

Dual-licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your option.

Each release archive also carries `THIRD-PARTY-LICENSES.txt`: the licences of the crates the binary is built from, of the Rust standard library, of musl in the Linux binaries, and of the syntax definitions it embeds to colour code ([ADR-0014](docs/adr/0014-every-archive-carries-the-licences-of-what-it-is-built-from.md)). The install scripts keep only the binary; the file is in the archive on the [releases page](https://github.com/Kwangseok-Seo/agentdocs/releases).
