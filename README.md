# agentdocs

A terminal viewer for the Markdown that AI coding agents run on — **skills, rules, agents, and docs**. It gathers files scattered across your machine into one screen, so you can find them and read them without remembering where they live.

> Two motifs. [Shpigford/chops](https://github.com/Shpigford/chops) for *what to show*, [herdrdev/herdr](https://github.com/herdrdev/herdr) for *how to ship it*. Where chops is a macOS-only GUI app, agentdocs is a **single binary that runs inside the terminal on every platform** — and it covers **rules and docs**, not just skills.

## Two goals

1. **The tool** — find and read scattered agent documentation from one screen.
2. **Learning in the open** — this tool is built in Rust *while learning Rust*, and the process is published to show that **learning and collaborating with an AI agent can happen at the same time**.

So this repository is not a finished codebase. It is a **record of something growing while its author learns**, kept in the commit history.

## Status

**M6 complete** — typed at a terminal with no words, `agentdocs` opens a **screen of three panes**: the sources, the selected source's entries, and the selected entry's file **drawn as Markdown** — headings, bold and inline code, lists and quotes that keep their indent on every row, code blocks, tables, and frontmatter without its fences. `j`/`k` or the arrows move, `Tab` switches pane, a click selects, and **dragging across the preview copies the text** to the clipboard.

```
$ cd ~/projects/cli-maker && agentdocs        # a 120 x 18 terminal; rules, then verify-your-evidence
┌Sources───────────────────┐┌Entries───────────────────────┐┌Preview───────────────────────────────────────────────────┐
│GLOBAL                    ││adr-context-format            ││# 증거물 자체를 검증한다 — 안 돌려 본 데모는 증거가 아니다│
│  skills:6                ││adr-rationale-realization     ││                                                          │
│  rules:14                ││check-your-guardrails         ││## 가드레일                                               │
│  agents:1                ││derive-dont-duplicate         ││                                                          │
│  commands:1              ││evidence-before-decision      ││데모·스켈레톤·측정 harness 도 건네기 전에 실행해 확인한다.│
│  agents/skills:1         ││git-conventions               ││증거를 만드는 것과 그 증거가 주장을 증명하는 것은 다른    │
│PROJECT cli-maker         ││karpathy-guidelines           ││일이다. 검증하지 않은 산출물은 결론의 근거로도, 사용자에게│
│  root md:3               ││obey-your-own-spec            ││건네는 재료로도 쓰지 않는다.                              │
│  docs:80                 ││performance                   ││                                                          │
│                          ││report-guideline              ││## 어디서 깨지나                                          │
│                          ││rule-format                   ││                                                          │
│                          ││session-snapshot-rule         ││양상: 애초에 안 돈다                                      │
│                          ││spec-before-first-build       ││점검: 건네기 직전 그 자리에서 빌드/실행                   │
│                          ││verify-your-evidence          ││실측: M4: net/http 만 import 한 빈 몸통 → 사용자 빌드가   │
│                          ││                              ││  imported and not used 로 막힘                           │
└──────────────────────────┘└──────────────────────────────┘└──────────────────────────────────────────────────────────┘
 j/k ↓/↑ move   tab pane   click select   drag copy   q quit
```

The table at the foot of that preview has three columns and does not fit a preview 58 wide, so each row becomes a block with every cell behind its column's heading; a table that fits is drawn as a grid. In a terminal, headings are bold and cyan, the table's headings cyan, and code yellow.

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
  docs:3 (1 unreadable)          # three found, one subdirectory refused
```

Through M2 the first of those lines read `0`, indistinguishable from the second, because a failure that could not be expressed in the return type had already been discarded before anything could print it ([ADR-0006](docs/adr/0006-unreadable-is-counted-not-dropped.md)). A directory link is now listed but never walked into — which is what stops a junction pointing at its own parent from turning a two-file directory into 128 rows ([ADR-0007](docs/adr/0007-links-are-listed-not-followed.md)).

Give it words and it keeps only the entries that contain every one of them — in the name or anywhere in the file, ignoring case — counts what it kept out of the whole, and prints under each row the first line that holds a word, so you can see why it is there:

```
$ agentdocs adr 검증                           # abridged: 21 rows
GLOBAL
  skills:0/6
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
| **M7** | Bundle tree (expand / collapse) | recursive data structures, `Box` |
| **M8** | Code block highlighting + scrolling — the wheel scrolls the preview, a drag can select past the rows in view | `syntect`, state management |
| **M9** | `$EDITOR` delegation + file watching | `std::process`, **threads & channels** |
| **M10** | Config file (extra sources, ordering) | `serde`, TOML, `thiserror` |
| **M11** | Distribution — multi-platform binaries, install scripts, CI | release profiles, cross-compilation |

> The theory, syntax, and pitfalls collected at each milestone live in [`docs/learn/`](docs/learn/) as a concept-by-concept knowledge base — the evidence for *what was actually learned*.

## Design decisions

Decisions that are expensive to reverse are recorded in [`docs/adr/`](docs/adr/README.md). Domain vocabulary lives in [`CONTEXT.md`](CONTEXT.md).

## Build

```
cargo run
```

Requires Rust 1.87 or newer. (The 2024 edition needs 1.85, but `std::env::home_dir` stayed marked deprecated until 1.87.)

## License

Dual-licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your option.
