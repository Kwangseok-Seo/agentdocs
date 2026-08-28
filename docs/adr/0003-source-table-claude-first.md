# Source definitions are data, and today that table holds Claude Code only

Which paths are shown, under which names, in which order — and *how each one is walked* — is written in a **single table** rather than in branching code, and today that table holds only the Claude Code family (`~/.claude`, `~/.agents`), because those are the only paths present on the development machine and therefore the only ones whose behavior can actually be observed, accepting that on release users of Cursor, Windsurf, or Codex will see nothing from the defaults alone.

The rejected alternative is **multi-tool support from the start**. chops carries paths for seven tools and copying them over would take minutes — but that would ship defaults we have never once executed. A typo in a path or a misunderstanding of a layout would be invisible to us, and would reach the user as "it says it supports this and shows me nothing." A support list is cheap to extend and, while wrong, keeps lying until someone fixes it.

Keeping it as a table is the substance of this decision. Adding a tool must be **adding a row**, so that a person who actually uses that tool can hand us a value they have verified. Scattered across branches, a contributor would first have to read the code to learn where to put it.

The same applies to traversal. Sources differ in how they must be walked — `rules` is one level of files filtered to `.md`, `skills` is one level of directories where each directory is itself an entry, `docs` is a full recursive descent. Encoding that as `if source == "rules"` chains is the branching this decision exists to prevent, so the traversal rule is a column of the table.
