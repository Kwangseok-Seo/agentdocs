# agentdocs

The domain language of a viewer that gathers AI agent documentation scattered across a machine into one screen.

## Language

**Source**:
One place that holds documentation, occupying a single line in the left pane with a name and a count.
_Avoid_: folder, path, category

**Scope**:
Whether a Source belongs to the machine at large or to the repository you are currently in — `Global` or `Project`.
_Avoid_: level, range, context

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
> **Dev:** If the same skill name exists both globally and in the project, do we merge them?
> **Author:** No. Different **Scope** means a different **Source**, so both appear. Which one you are looking at must never get lost.

## Flagged ambiguities

- "skill" was used for two things — the unit Claude Code executes, and the row shown on our screen. Resolved: the row is an **Entry**; `skills` is used only as a **Source** name.
- "docs" was used both as a **Source** name and as a general word for documentation. Resolved: `docs` names the **Source** backed by a repository's `docs/` directory; otherwise write "documentation".
