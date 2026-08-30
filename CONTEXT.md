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

**Entry**:
One listable, readable unit. Either a single file or one Bundle.
_Avoid_: item, document, file

**Bundle**:
A directory made of one Lead file and the supporting files it carries.
_Avoid_: package, skill folder

**Lead**:
The file that represents a Bundle, supplying its name and preview (for example `SKILL.md`).
_Avoid_: main file, index, root document

## Relationships

- A **Source** has exactly one **Scope**
- A **Source** has exactly one **Walk**
- A **Source** holds zero or more **Entries**
- An **Entry** is either a single file or one **Bundle**
- A **Bundle** has exactly one **Lead** and zero or more supporting files

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
> **Dev:** If the same skill name exists both globally and in the project, do we merge them?
> **Author:** No. Different **Scope** means a different **Source**, so both appear. Which one you are looking at must never get lost.

## Flagged ambiguities

- "skill" was used for two things — the unit Claude Code executes, and the row shown on our screen. Resolved: the row is an **Entry**; `skills` is used only as a **Source** name.
- "docs" was used both as a **Source** name and as a general word for documentation. Resolved: `docs` names the **Source** backed by a repository's `docs/` directory; otherwise write "documentation".
- "traversal" and "walk" were both used for the rule that decides what an **Entry** is inside a **Source** — ADR-0003 writes "traversal rule" in prose. Resolved: the term is **Walk**.
- The relationship above says a **Bundle** has exactly one **Lead**, yet a directory with no Lead is still listed. Resolved: the relationship states the well-formed shape; a directory in a Bundle **Source** that is missing its **Lead** is a malformed **Bundle**, and it is shown rather than hidden, because what is on disk is what the viewer reports.
