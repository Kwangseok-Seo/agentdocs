# Architecture Decision Records

An index of the settled design decisions in this repository. Check the relevant record before changing code or design. Reversals are never silent overwrites — they are superseding records.

| # | Title | Status |
|---|-------|--------|
| [0001](0001-read-only-viewer.md) | Read-only viewer — editing is delegated to $EDITOR | accepted |
| [0002](0002-discovery-global-always-project-follows-cwd.md) | Global sources are always shown; project sources follow the current directory | accepted |
| [0003](0003-source-table-claude-first.md) | Source definitions are data, and today that table holds Claude Code only | accepted |
| [0004](0004-project-root-marker-then-cwd.md) | The project root is found by walking up for a marker, falling back to the current directory | accepted (replaces the final paragraph of 0002) |
| [0005](0005-sources-report-their-own-directory.md) | A Source reports its own directory; Entries are never merged across Sources | accepted |
| [0006](0006-unreadable-is-counted-not-dropped.md) | What a Walk could not read is counted, not dropped | accepted (third paragraph replaced by 0009) |
| [0007](0007-links-are-listed-not-followed.md) | A link is listed but never walked into | accepted |
| [0008](0008-screen-for-a-bare-command-at-a-terminal.md) | Only a bare `agentdocs` at a terminal opens the screen; words and pipes get the listing | accepted |
| [0009](0009-unreadable-is-a-row-where-it-was-found.md) | What a Walk could not read is a row where it was found | accepted (replaces the third paragraph of 0006) |
| [0010](0010-the-editor-runs-through-the-shell.md) | The editor is run by the platform's shell, and the path never goes through the shell's reading of a line | accepted |
| [0011](0011-changes-are-heard-not-polled.md) | Changes on disk are heard from the system, and what to watch follows each Walk | accepted |
