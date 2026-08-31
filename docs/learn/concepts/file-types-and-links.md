# file-types-and-links

"Is this a directory?" is two different questions, and a symbolic link is where they come apart.

```rust
path.is_dir()          // about the TARGET  — follows the link
item.file_type()       // about the ENTRY   — does not follow the link
```

`Path::is_dir()` opens whatever the path finally points at. A link is not a thing to it, only a road to a thing. `DirEntry::file_type()` reports the row in the directory itself, which is why it can say "this is a link" at all.

## What each one answers

Measured against three real directory entries — a plain directory, a junction whose target had been deleted, and a junction pointing back at its own parent:

| entry | `path.is_dir()` | `file_type()` | `metadata()` (follows) |
|---|---|---|---|
| plain directory | `true` | `is_dir=true`, `is_symlink=false` | `is_dir=true` |
| **dangling** link | **`false`** | `is_dir=false`, **`is_symlink=true`** | `Err(NotFound)` |
| link to its own parent | **`true`** | `is_dir=false`, **`is_symlink=true`** | `is_dir=true` |

The middle row and the bottom row are opposite failures produced by the same boolean. `false` for a broken link means a listing that filters on `is_dir()` **drops the row entirely** — silent under-counting. `true` for a self-referential link means a recursive walk **descends into it** — silent over-counting.

There is a third meaning hiding in the same word, and it is the one that made the second failure survivable. Measured on the cycle above, the walk stops at depth 64:

```
depth 64: path len 384
    path.is_dir()  = false
    fs::read_dir() = Err kind=FilesystemLoop os=Some(1921)
```

`false` there does not mean "not a directory" and does not mean "gone". It means **the system gave up resolving the link** — Windows stops after a fixed number of reparse-point hops (`ERROR_CANT_RESOLVE_FILENAME`), which `std` reports as `io::ErrorKind::FilesystemLoop`. Note the path length: 384 characters, well past the classic 260-character limit, so it is the hop count that ends this and not the length. Note also which line the code reached — the walk branched on `path.is_dir()` and never called `read_dir` at that depth, so there was not even an error to swallow. One boolean, three meanings, and the run finished at exit 0 with a plausible number.

`file_type()` separates all three because it never asks the target anything. Its cost is documented per platform: *"On Windows and most Unix platforms this function is free (no extra system calls needed), but some Unix platforms may require the equivalent call to `symlink_metadata` to learn about the target file type."* So it is not merely more truthful than `path.is_dir()` here, it is also never more expensive — `is_dir()` always has to go and look.

## `is_file()` excludes links too, and that is easy to miss

`FileType`'s three questions are **mutually exclusive** — the standard library documents that of `is_dir`, `is_file` and `is_symlink`, *"only zero or one of these tests may pass."* A link is a link, so `is_file()` is `false` for one even when it points at a perfectly readable file, and a walk that admits documents with `if !ft.is_file() { continue }` **silently drops every linked Markdown file** — not listed, not counted unreadable.

The gate that holds is `if ft.is_dir() { continue }`: reject what is certainly not a document, and let the name decide the rest. Deciding by the target would mean following the link.

## Windows: junctions are links

`mklink /J` makes a **junction**, which needs no administrator rights, unlike a directory symbolic link. Rust does not make you care: a junction reports `is_symlink() == true`, exactly as a symbolic link does. Note that a **hard link** is a different thing — it is a second name for the same file, indistinguishable from a plain file, and it reports `is_file()`.

## Three more ways to ask

```rust
fs::metadata(&path)            // follows links; Err(NotFound) if the target is gone
fs::symlink_metadata(&path)    // does not follow; describes the link itself
path.canonicalize()            // resolves to the real path — a Result, since it can fail
```

`canonicalize` is what a "have I been here already?" set would be built on, and it does prove a cycle: on the self-referential junction it returned the parent's own path. agentdocs does not use it — see [ADR-0007](../../adr/0007-links-are-listed-not-followed.md) for why not following links at all is a shorter rule than following them carefully.

## Pitfalls hit

- **A Bundle that was a broken link vanished without a trace.** `bundle_dirs` filtered on `!path.is_dir()`, so a dangling junction produced no row at all — not even the `-` that a Bundle with no Lead gets. The count said 2 where 3 directory entries existed, and nothing was printed on any stream. This is under-counting, the *opposite* direction from the cycle below, and one boolean produced both.
- **A junction pointing at its own directory turned two files into 128 rows.** The recursion did not hang, which is what made it survivable and therefore easy to miss.
- **The reason it stopped was written down before it was measured, and the write-up was wrong.** The first draft of this page said the path length limit made `read_dir` fail and that the swallowed error hid it. Probing the actual boundary showed neither half: the limit is the reparse-point hop count, not the path length (384 characters at the cutoff), and the code never reached a `read_dir` error at all, because `path.is_dir()` had already answered `false`. A mechanism that sounds right and produces the observed number is still a guess until something prints it.
- **The escape does not announce itself either.** A junction under a project's `docs/` pointing into `~/.claude` listed a global rules file as a project document. Nothing about the row said where it came from.
- **Fixing the directory case opened a file-shaped hole.** Replacing `path.is_dir()` with `ft.is_file()` in the file walks was correct about directories and wrong about links: a symlinked `.md` file stopped being listed at all. A review caught it by reading [ADR-0007](../../adr/0007-links-are-listed-not-followed.md) against the code — the record said links are listed, and only the Bundle walk had been taught that.

## Related

[[fs-read-dir]] · [[paths]] · [[result-and-errors]] · [[option-and-match]]
