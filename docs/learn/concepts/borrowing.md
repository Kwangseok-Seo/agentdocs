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

## Pitfalls hit

Writing `fs::read_dir(root)` moves `root` into the function, so the following `root.join("docs")` no longer compiles. Passing `&root` borrows it and keeps it usable afterwards.

## Related

[[ownership]] · [[owned-vs-borrowed-pairs]] · [[fs-read-dir]]
