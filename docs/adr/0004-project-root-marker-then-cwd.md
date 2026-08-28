# The project root is found by walking up for a marker, falling back to the current directory

Walking up from the current directory toward — but never past — the home directory, **the first directory carrying a marker (`.git`, `CLAUDE.md`) is the project root, and if the walk finds none the current directory itself is**, because requiring `.git` alone disqualified 5 of the 17 project directories on this machine and hid 308 Markdown files outright while widening the marker list to four still missed `session-seal`, a documentation store carrying no marker of any kind; the cost accepted is that a deep directory with no marker above it becomes the project, and that a workspace directory such as `~/projects` registers as one too.

At or above the home directory the project block stays empty.

This replaces the final paragraph of [ADR-0002](0002-discovery-global-always-project-follows-cwd.md) (*"The project boundary is the repository root ... show the global sources only"*). The rest of it — global always, project follows the current directory — still holds.

**The fallback is what makes this decision work.** A marker list is an enumeration, and an enumeration always misses one (widening it to four still did). But when the answer to "found nothing" is *use the current directory* rather than *show nothing*, a wrong list cannot fail silently: the worst case becomes "we did not climb far enough," and that is visible on screen. So the list starts minimal — `.git` and `CLAUDE.md` — and grows only after an actual case of climbing too little turns up.

The home directory serves as the ceiling for cost reasons. Without it, running from `C:\` would take everything below as the project and walk it recursively. Stopping at home makes that class of failure structurally impossible, and home is where the global sources live anyway, so it has no business being a project.

`session-seal` still shows no documentation under this decision. It is now identified as a root, but its files live in `Sessions/` and `Wiki/`, which the project source definition (`docs/`, root-level Markdown) does not cover. That is a source-definition problem rather than a boundary problem, and belongs to the config file in M10.
