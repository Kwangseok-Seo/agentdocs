# borrowing

Looking at a value without taking ownership of it. `&T` borrows immutably, `&mut T` borrows mutably.

> **Many readers, or one writer — never both. And a borrow may not outlive what it borrows from.**

## The three rules, and what each one prevents

All three were put to the compiler directly during M1.

| Experiment | Error | What it prevented |
|---|---|---|
| `let b = &a;` then use `a` | (compiles) | nothing — many readers are safe |
| use a reference after its target left scope | `E0597: s does not live long enough` | **reading a dead address** (a dangling pointer) |
| hold `&v[0]` across `v.push(...)` | `E0502: cannot borrow as mutable because it is also borrowed as immutable` | **the ground moving while you read it** |

The third one matters most. A `Vec` that runs out of capacity allocates a larger heap block and moves its contents, so the address you were holding now points at nobody's house. The same code written in Go **compiles and runs**, quietly reading the old array — not a crash, but a wrong answer with `exit 0`. Rust stops it at compile time. See [[fs-read-dir]] for where this shows up in our own code.

## The convention

**Borrow in the arguments, own in the return.** There is no reason to seize ownership just to look at something, and the caller of a returning function needs to become the owner.

```rust
fn find_project_root(start: &Path, home: &Path) -> Option<PathBuf>
//                          ^^^^^ borrowed         ^^^^^^^ owned
```

## A borrow lasts until its last use

Not until the end of the block, and not for as long as the variable exists. The screen's rows borrow the tree the Walk built, and the App holds both the tree and the set of open rows, so a method that takes `&self` lends out **all** of `self`:

```rust
let rows = self.rows();                      // rows ──▶ self (all of it, as the compiler sees it)
let Some(row) = rows.get(at) else { return };
self.open.remove(row.node.path());           // E0502: self.open is borrowed as immutable
```

`self.open` is a different field from the tree, but `rows()` took `&self`, and `row.node.path()` is still in use while `remove` runs. Copying the path out first ends the borrow:

```rust
let path = row.node.path().to_path_buf();    // an owned copy — the last use of rows
self.open.remove(&path);                     // compiles
```

`row.node.path().clone()` does not do it: the path is a `&Path`, and cloning a reference makes another reference to the same place — the same E0502. Where the last use falls is the whole question. After `let n = rows.len();` a line that changes `self` compiles as long as `rows` is not touched again below it, and `rows[at]` counts as touching it.

## Pitfalls hit

Writing `fs::read_dir(root)` moves `root` into the function, so the following `root.join("docs")` no longer compiles. Passing `&root` borrows it and keeps it usable afterwards.

In M7, the borrow was taken to last as long as its variable. Two snippets that compile — `contains`, then `remove` or `insert`, on one owned path, and a change to `self` after `rows.len()` — were predicted to be E0502. The explanation in slice 3 had said *"while `rows` is in use"*, which may have pushed the answer that way. A timetable, one bar per borrow from the line it starts to the line it is last used, brought the next answer right; but asked where `self.open.clear()` could go, the pick was the line before `rows[at]` — still a use — and not the line after it. The last use itself settled in slice 4: which of two versions of `load_frontmatter` stops at E0506 — the one that changes `self.name` while `doc`, borrowed from `self`, is still to be read — was answered right.

## Related

[[ownership]] · [[owned-vs-borrowed-pairs]] · [[fs-read-dir]] · [[lifetimes]]
