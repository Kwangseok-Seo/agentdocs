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

## A call lends what is left of the dot

What a line borrows is decided at the call, in two steps: what is to the left of the dot, and what the method takes. `self.names.first()` lends `self.names`, one field. `self.first()` lends `self` — all of it, whatever `first` goes on to read — because its signature says `&self`. Each of these was built, holding the first borrow across the second line:

| holding | then | builds? |
|---|---|---|
| `&self.names[0]` | `self.scrolled.insert(…)` | yes — two different fields |
| `self.first()`, a `&self` method returning `&String` | `self.scrolled.insert(…)` | E0502 |
| `self.sources.first()` | `self.focus = 2` | yes |
| `self.walked()`, a `&self` method | `self.focus = 2` | E0506 |
| `code`, from `&mut self.code` | `self.current.push(…)` | yes — `push` lends `self.current` |
| `code`, from `&mut self.code` | `self.finish_line()`, a `&mut self` method | E0499 |

A method that hands back a reference keeps the loan open for as long as that reference is used: `self.first()` returns an arrow into `self` ([[lifetimes]], rule 3), so while `name` is in use, all of `self` is lent.

## The compiler reads the signature, not the body

`finish_line` never touches `self.code` — it moves the line being built into the finished rows, and changes `current`, `lines`, `containers` and `gapped` to do it. Called while `code` is held, it is refused all the same:

```
188 |                 if let Some(code) = &mut self.code {
    |                                     -------------- first mutable borrow occurs here
189 |                     self.finish_line();
    |                     ^^^^ second mutable borrow occurs here
190 |                     code.highlighter = None;
    |                     ---------------- first borrow later used here
```

`fn finish_line(&mut self)` says *I may change anything in `self`*, and that is all the caller is judged by. Each function is checked on its own: if a caller could lean on what a body happens to do today, changing the body tomorrow could break a caller somewhere else. The signature is the promise that stops that.

## Not which comes first — whether the bars overlap

| first borrow, still used later | a second borrow of the same thing | |
|---|---|---|
| read | read | fine |
| read | write | E0502 |
| write | read | E0502 |
| write | write | E0499 |

Order decides nothing by itself. `tick` asks the App which way to scroll, then takes the selection to change it:

```
let Some(rows) = self.autoscroll() else { return };      ━ all of self, read — ends here: rows is a number
let Some(selection) = &mut self.selection else { return }; ┬ self.selection, write
if selection.scroll_at.is_some_and(|at| now < at) { … }    │
selection.scroll_at = Some(now + AUTOSCROLL_EVERY);        ┴ its last use
self.scroll(rows);                                         ━ all of self, write
```

No two bars overlap. With the first two lines swapped, the write to `self.selection` is still in use when `self.autoscroll()` reads all of `self`: `E0502: cannot borrow *self as immutable because it is also borrowed as mutable`.

## Pitfalls hit

Writing `fs::read_dir(root)` moves `root` into the function, so the following `root.join("docs")` no longer compiles. Passing `&root` borrows it and keeps it usable afterwards.

In M7, the borrow was taken to last as long as its variable. Two snippets that compile — `contains`, then `remove` or `insert`, on one owned path, and a change to `self` after `rows.len()` — were predicted to be E0502. The explanation in slice 3 had said *"while `rows` is in use"*, which may have pushed the answer that way. A timetable, one bar per borrow from the line it starts to the line it is last used, brought the next answer right; but asked where `self.open.clear()` could go, the pick was the line before `rows[at]` — still a use — and not the line after it. The last use itself settled in slice 4: which of two versions of `load_frontmatter` stops at E0506 — the one that changes `self.name` while `doc`, borrowed from `self`, is still to be read — was answered right.

M8 met the same rules three more times, and each time the wrong answer had a different reason behind it:

- **A method taken to lend only what it reads.** In slice 1, `self.walked()` followed by `self.focus = 2` was picked as building, and `self.sources.first()` followed by the same line as not — the reverse of the table above. Asked again with three snippets, all three were picked as building; `self.row()` does not. A diagnostic question gave the reason: *`first()` is a method too* — there was no rule yet about what stands left of the dot. A drawing of the arrows, the addresses printed to show `self.row()`'s answer pointing into `self`, and the two steps above brought the next two answers right.
- **"Take the write borrow first and it is fine."** In slice 2 the pick for `tick` was the order above swapped — E0502 — and a second question was answered the same way. The explanation in slice 1 had been *a read held while writing is refused*, which may have been heard as a rule about order. The table of four directions came after.
- **The body, not the signature (not settled).** In slice 3, both versions of the end of a code block were picked as building; the one that uses `code` after `self.end_code_line()` is E0499. A diagnostic question gave the reason: the two change different things, so they cannot collide. The `finish_line` experiment above followed. Asked then which of four lines can stand between taking `&mut self.code` and using `code` again, the pick was `self.gapped = true` alone: the two methods on `self` were rightly left out, and so, wrongly, was `self.current.push(…)` — a method, but one whose dot has `self.current` to its left.

## Related

[[ownership]] · [[owned-vs-borrowed-pairs]] · [[fs-read-dir]] · [[lifetimes]] · [[impl-and-methods]] · [[state]]
