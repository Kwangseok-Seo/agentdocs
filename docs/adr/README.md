# Architecture Decision Records

An index of the settled design decisions in this repository. Check the relevant record before changing code or design. Reversals are never silent overwrites — they are superseding records.

| # | Title | Status |
|---|-------|--------|
| [0001](0001-read-only-viewer.md) | Read-only viewer — editing is delegated to $EDITOR | accepted |
| [0002](0002-discovery-global-always-project-follows-cwd.md) | Global sources are always shown; project sources follow the current directory | accepted |
| [0003](0003-source-table-claude-first.md) | Source definitions are data, and today that table holds Claude Code only | accepted |
| [0004](0004-project-root-marker-then-cwd.md) | The project root is found by walking up for a marker, falling back to the current directory | accepted (replaces the final paragraph of 0002) |
| [0005](0005-sources-report-their-own-directory.md) | A Source reports its own directory; Entries are never merged across Sources | accepted |
