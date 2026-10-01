# paths

A path is not a string. It is handled as a `PathBuf` (owning) / `&Path` (borrowing) pair — see [[owned-vs-borrowed-pairs]] — and both assembly and comparison go through dedicated methods.

## Why strings do not do

- **Separators differ per platform.** `join` inserts `\` on Windows and `/` on Unix. Since the whole point of this project is a single binary that runs everywhere, this is not negotiable.
- **`starts_with` compares component by component.** As a string, `C:\Users\adman2` would "start with" `C:\Users\adman` and be wrong. We decide the home-directory ceiling with exactly this call, so it matters.

## What M1 used

```rust
home.join(".claude").join("skills")   // assembly; chainable, takes &self so the original survives
p.parent()                            // one level up -> Option<&Path>
p.join(".git").exists()               // existence check
path.extension()                      // extension -> Option<&OsStr>
path.is_dir()                         // directory?
p.to_path_buf()                       // borrowed -> owned
root.display()                        // for printing
```

## A path as written, and the path it leads to (M11)

`starts_with`, `==` and `parent` work on a path's parts as written, and never ask the file system where they lead. `fs::canonicalize` does: it follows every link and returns the path that is left — on Windows with a `\\?\` in front, which is why the program only uses it when the path as written has failed. Unix reports the current directory already resolved, so with a home written through a link no directory was ever below it ([[platform-differences]]).

## Why there is no `Display`

`Path` cannot be printed with `{}`. On some operating systems a path is not valid UTF-8, so Rust declines to implement `Display` for it. Printing goes through `.display()`.

## Pitfalls hit

**A variable does not come alive inside a string literal.** This produced a `NotFound` panic:

```rust
let home_path = home.join(".claude").join("skills");
("skills", "home_path/.claude/skills")   // <- parsed as nine literal characters
```

Rust is not Python's f-string or a JS template literal. To place a value in a string you need `format!` — and for a path you use `join` instead of building a string at all.

One more: `extension()` returns `None` for `.gitignore`. A leading dot makes the whole thing a name, not an extension — see [[option-and-match]] for why that is an `Option` in the first place.

## Related

[[owned-vs-borrowed-pairs]] · [[option-and-match]] · [[fs-read-dir]] · [[platform-differences]]
