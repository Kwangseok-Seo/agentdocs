# agentdocs

A terminal viewer for the Markdown that AI coding agents run on — **skills, rules, agents, and docs**. It gathers files scattered across your machine into one screen, so you can find them and read them without remembering where they live.

> Two motifs. [Shpigford/chops](https://github.com/Shpigford/chops) for *what to show*, [herdrdev/herdr](https://github.com/herdrdev/herdr) for *how to ship it*. Where chops is a macOS-only GUI app, agentdocs is a **single binary that runs inside the terminal on every platform** — and it covers **rules and docs**, not just skills.

## Two goals

1. **The tool** — find and read scattered agent documentation from one screen.
2. **Learning in the open** — this tool is built in Rust *while learning Rust*, and the process is published to show that **learning and collaborating with an AI agent can happen at the same time**.

So this repository is not a finished codebase. It is a **record of something growing while its author learns**, kept in the commit history.

## Status

**M2 complete** — the binary discovers its sources, walks each one by its own rule, and reads a name and description out of each entry's frontmatter. No screen yet.

```
$ cd ~/projects/cli-maker && agentdocs        # abridged: the real output is 121 lines
GLOBAL
  skills:7
    dream                            Memory consolidation pass (auto-memory 시스템의 …
    get-api-docs                     Use this skill to get documentation for thir…
    grill-with-docs                  Grilling session that challenges your plan a…
    session-retro                    세션 회고 — 직전 발행 rule 의 재사용 성적을 먼저 보이고, 4축(잘한것·…
    ...
  rules:19
    adr-context-format               -
    adr-rationale-realization        -
    ...
  agents:1
    session-seal                     Claude Code 세션 스냅샷 draft → sealed 승격 전담 agen…
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

That output is already longer than a terminal window, which is the argument for the screen that arrives in M5.

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
| **M3** | Error handling + first tests | **`Result`, the `?` operator**, `thiserror`, `#[cfg(test)]` |
| **M4** | Search and filtering (still CLI) | **iterators & closures**, traits |
| **M5** | TUI skeleton — three panes, key navigation | external crates, **the event loop**, modules |
| **M6** | Markdown rendering | **lifetimes**, slices |
| **M7** | Bundle tree (expand / collapse) | recursive data structures, `Box` |
| **M8** | Code block highlighting + scrolling | `syntect`, state management |
| **M9** | `$EDITOR` delegation + file watching | `std::process`, **threads & channels** |
| **M10** | Config file (extra sources, ordering) | `serde`, TOML |
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
