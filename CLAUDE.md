# agentdocs — Project Instructions

A terminal viewer for AI agent documentation: skills, rules, agents, and docs written in Markdown. Two projects serve as motifs — [Shpigford/chops](https://github.com/Shpigford/chops) for *what to show*, [herdrdev/herdr](https://github.com/herdrdev/herdr) for *how to ship it*. Unlike chops, agentdocs is a **single binary that runs inside the terminal on every platform**, and it covers rules and docs in addition to skills.

## Language

**Everything in this repository is written in English** — Markdown files, code comments, identifiers, commit messages, CLI output. This is a public open-source project and its readers are not assumed to share the author's first language.

The working conversation between the author and the assistant is held in Korean; that is a property of the session, not of the repository. Nothing written in Korean lands in a committed file.

## Two goals (both first-class)

1. **The tool** — let a person find and read scattered agent documentation from one screen.
2. **Learning in the open** — build this tool in Rust while learning Rust, and publish the process to demonstrate that *learning and collaborating with an AI agent can happen at the same time*.

Goal 2 is the constraint that does not show up in the code. The workflow below enforces it. **The author is learning Rust from scratch.**

## Learning-first workflow (mandatory)

- **Theory before implementation.** Before building a feature, explain the relevant Rust concepts in conversation — why it works this way, why the idiom is what it is.
- **No dumps; quiz.** The author's part is chosen, not typed: each piece that carries the milestone's concept is asked as a quiz — multiple choice or fill-in-the-blank — and the chosen answer goes into the code. Every wrong option is a mistake actually compiled or tested beforehand, and a wrong pick is answered with what the compiler or the tests really said.
- **Diagnose the stuck layer.** When the author says "I don't follow," do not repeat the same explanation. First separate *purpose / concept / placement / syntax*, then treat that layer. If placement is stuck, **run an execution trace and show it**. If syntax is stuck, show the finished form and let them read it — syntax cannot be derived.
- **Small steps.** Advance one milestone at a time as listed in the README roadmap. One milestone = one Rust concept + one slice of functionality.
- **Keep the learning wiki.** Each milestone records its concepts in `docs/learn/concepts/<concept>.md` (atomic pages, `[[wikilink]]` >= 2, and a "Pitfalls hit" section **whenever that concept actually tripped someone up** — inventing one where nothing went wrong would be a lie, and the milestone log is where the full list lives anyway), and updates the `docs/learn/index.md` index and the `docs/learn/milestones/M*.md` journey log. This practice is itself the artifact of goal 2.
- **Report honestly.** Never say "it works" — show actual input → output.

## Architecture (only what is settled — do not record what is undecided)

- **Read-only viewer.** Editing is delegated to `$EDITOR`. See `docs/adr/0001-read-only-viewer.md`.
- **Discovery.** Global sources are always listed wherever the binary runs; the project block follows the repository that contains the current directory. See `docs/adr/0002-*.md`.
- **Source definitions are data**, not code branches — including *how* each source is walked. Today the table holds Claude Code paths only. See `docs/adr/0003-*.md`.
- **Project root** is found by walking up for a marker, falling back to the current directory. See `docs/adr/0004-*.md`.
- **TUI framework**: ratatui (+ crossterm backend).
- **Markdown**: parsed by pulldown-cmark; drawn, and cut into rows that fit the preview, by our own renderer. Code in a language syntect knows, Markdown aside, is coloured in the terminal's own palette.
- **Screen or listing.** A bare `agentdocs` at a terminal opens the screen; words, or stdout that is not a terminal, print the listing. See `docs/adr/0008-*.md`. The core is still verified through the listing.
- **The editor** is run by the platform's shell, and the file's path is kept out of the line the shell reads. See `docs/adr/0010-*.md`.
- **Watching.** A change on disk is heard from the system, through notify, and what to watch follows each Walk. See `docs/adr/0011-*.md`.
- **Config files.** `.agentdocs.toml` at home adds global Sources, and at a project's root that project's; each orders its scope's Sources by name, and one that cannot be used is a row where its Sources would have been. See `docs/adr/0012-*.md`.

## Layout (grow it as needed — never pre-create empty directories)

- `src/main.rs` — the command line, the source table, and the choice between screen and listing.
- `src/source.rs` · `src/entry.rs` · `src/frontmatter.rs` — where to look, what one Entry is and the tree a Walk builds of them, how fields are read. `src/config.rs` — the config files, read into Sources and their order.
- `src/listing.rs` — the lines of the listing, and `printable`, which every name and description passes on its way to a terminal. `src/tui.rs` — the screen, and the threads that feed its loop. `src/editor.rs` — which editor a file is handed to, and the command that runs it. `src/markdown.rs` — a file drawn as Markdown for the preview. `src/highlight.rs` — a line of code in the colours of its language. `src/testutil.rs` — fixtures shared by the tests.
- `docs/adr/` — architecture decision records.
- `docs/learn/` — the Rust learning wiki (`index.md` + `concepts/` + `milestones/`).
- `CONTEXT.md` — domain glossary (glossary only).

## Document authority

- **Decisions** -> `docs/adr/NNNN-*.md` (Y-statement form).
- **Terms** -> `CONTEXT.md` (glossary only — no rules, results, or parameters).
- **Status and roadmap** -> `README.md`.
- **Rust knowledge** -> `docs/learn/`.

## Verification rules

- This program reads other people's files, so **broken input is the normal case**. Surviving a missing path, an empty directory, malformed frontmatter, or an unreadable file is a feature, not an edge case.
- The corpus used for verification is this machine's `~/.claude`, `~/.agents`, and `~/projects/*/docs`. Verify with counts ("skills 7"), never with "it works".
- Catch `exit 0` with a wrong result before catching crashes. A silent wrong answer travels downstream; a crash does not.

## Git

- Default branch `main`. Commit and push only when the author asks.
- Dual-licensed MIT OR Apache-2.0 (the Rust ecosystem convention).
