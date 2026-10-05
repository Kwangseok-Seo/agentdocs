# agentdocs

The domain language of a viewer that gathers AI agent documentation scattered across a machine into one screen.

## Language

**Source**:
One place that holds documentation, occupying a single line in the left pane with a name and a count.
_Avoid_: folder, path, category

**Scope**:
Whether a Source belongs to the machine at large or to the repository you are currently in — `Global` or `Project`.
_Avoid_: level, range, context

**Walk**:
The rule a Source carries for what counts as one Entry inside it, and how deep to look for one.
_Avoid_: traversal, scan, strategy

**Config file**:
A file named `.agentdocs.toml`, at home or at a project's root, that adds Sources to one Scope and sets the order of that Scope's Sources.
_Avoid_: settings, preferences, dotfile

**Entry**:
One listable, readable unit. Either a single file or one Bundle.
_Avoid_: item, document, file

**Bundle**:
A directory made of one Lead file and the supporting files it carries.
_Avoid_: package, skill folder

**Lead**:
The file that represents a Bundle, supplying its name and preview (for example `SKILL.md`).
_Avoid_: main file, index, root document

**Unreadable**:
Something a Walk found but could not look at — an entry the system would not describe, a directory that would not open, or a document that would not open. It is shown where it was found, with the reason.
_Avoid_: error, failure, broken

**Hit**:
The first line of an Entry's text that holds any of the words being searched for — for a Bundle, a line of its Lead, or else of the first of its supporting files that has one.
_Avoid_: snippet, match line, excerpt

## Relationships

- A **Source** has exactly one **Scope**
- A **Source** has exactly one **Walk**
- A **Config file** speaks for exactly one **Scope** — `Global` at home, `Project` at a project's root — and adds zero or more **Sources** to it
- A **Source** holds zero or more **Entries**
- An **Entry** is either a single file or one **Bundle**
- A **Bundle** has exactly one **Lead** and zero or more supporting files
- A **Walk** reports the **Entries** it found and the things it found **Unreadable**, each where it found it
- An **Entry** kept by a search has at most one **Hit**; one kept for its name alone has none

## Example dialogue

> **Dev:** The 19 markdown files under `~/.claude/rules` — is that 19 **Entries**?
> **Author:** Yes. Those are all standalone files, so none of them is a **Bundle**.
>
> **Dev:** What about `session-retro/`, where `SKILL.md` sits next to `RETRO-FORMAT.md` and `examples/`?
> **Author:** That is one **Entry** — a **Bundle**. The list shows `session-retro` on a single line, and expanding it reveals the supporting files. Its name and preview come from the **Lead**, `SKILL.md`.
>
> **Dev:** `~/.claude/rules` and `~/.claude/skills` are both one directory deep. Why do they need different rules?
> **Author:** Because a subdirectory means opposite things in the two. Under `rules` it is not an **Entry** at all; under `skills` it *is* one — a **Bundle**. That difference is the **Walk**, and it belongs to the **Source**, not to the code that reads it.
>
> **Dev:** `session-seal` wants its `Wiki/` on a line of its own, above the 351 files of `root md`. Is that a new **Walk**?
> **Author:** No — a new **Source**, written in a **Config file** at its root: `Wiki`, walked as one level of files. The **Config file** at the root makes it a **Project** Source; written in the one at home, it would be **Global**, and its path taken from the home directory. Its files stay in `root md` as well — a **Source** reports its own directory.
>
> **Dev:** If the same skill name exists both globally and in the project, do we merge them?
> **Author:** No. Different **Scope** means a different **Source**, so both appear. Which one you are looking at must never get lost.

## Flagged ambiguities

- "skill" was used for two things — the unit Claude Code executes, and the row shown on our screen. Resolved: the row is an **Entry**; `skills` is used only as a **Source** name.
- "docs" was used both as a **Source** name and as a general word for documentation. Resolved: write "documentation" for the general sense. No **Source** is named `docs` since ADR-0015; `docs/` is only a directory, one of those a project's `root md` reads.
- "traversal" and "walk" were both used for the rule that decides what an **Entry** is inside a **Source** — ADR-0003 writes "traversal rule" in prose. Resolved: the term is **Walk**.
- "unreadable" was used both for a thing a **Walk** could not open and for anything a **Walk** passes over. Resolved: **Unreadable** is only what the Walk *could not look at*. What it looked at and rejected — a non-Markdown file, a link it will not follow, a path a `.gitignore` leaves out — is not unreadable; it is skipped.
- The line printed under a row during a search was called the "matched line", the "hit line", the "why line" and a "snippet". Resolved: it is the **Hit**.
- The relationship above says a **Bundle** has exactly one **Lead**, yet a directory with no Lead is still listed. Resolved: the relationship states the well-formed shape; a directory in a Bundle **Source** that is missing its **Lead** is still a **Bundle**, a malformed one.
