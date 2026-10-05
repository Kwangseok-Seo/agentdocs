# Global sources are always shown; project sources follow the current directory

> **The final paragraph of this record (how the project boundary is determined) is replaced by [ADR-0004](0004-project-root-marker-then-cwd.md).** Requiring `.git` is void — measured against this machine, 5 of 17 project directories failed it. The rest of this decision (global always, project follows the current directory) still holds.
>
> **The list of project sources in the first paragraph (`docs/`, `.claude/skills`, root-level Markdown) is replaced by [ADR-0015](0015-a-project-is-every-markdown-file-below-its-root.md)**: every Markdown file below the root, less what its `.gitignore` files leave out, and the root's `.claude/`.

Global sources (`~/.claude/{skills,rules,agents,commands}`, `~/.agents/skills`) appear wherever the binary runs while project sources (`docs/`, `.claude/skills`, root-level Markdown) attach for **exactly one** repository — the one containing the current directory — because global documentation lives at fixed paths and is always relevant whereas "which project's docs" has no fixed answer, and the current directory is context the user has already declared rather than something to ask for; the cost accepted is that reading another project requires a `cd`, and that running outside one leaves the project block empty.

The first rejected alternative is **registering paths in a config file**. It is powerful — every project on one screen regardless of where you run — but the first run would be an empty screen, making configuration a prerequisite for using the tool at all. That places the barrier at the program's first impression. A path for *adding* sources through config can be opened later, and at that point it sits on top of a default that already works, which is a different proposition.

The second rejected alternative is **scanning the whole workspace** (everything under `~/projects`). On this machine that pulls in 17 project directories and over a thousand Markdown files at once, and it forces us to invent the boundary: how deep to descend, what counts as a project. That boundary differs per user, and when it is wrong it is wrong silently — a document that is missing and a document that never existed look identical on screen.

The project boundary is the **repository root**: walk up from the current directory looking for `.git`, and if none is found, show the global sources only. Failure is not silent — the project block says why it is empty.
