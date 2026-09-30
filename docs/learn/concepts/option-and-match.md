# option-and-match

`Option<T>` puts "there may or may not be a value" **into the type**. It is either `Some(value)` or `None`.

```rust
env::home_dir()     -> Option<PathBuf>    // some environments have no home
p.parent()          -> Option<&Path>      // a root has no parent
path.extension()    -> Option<&OsStr>     // some files have no extension
```

Go's `(value, ok)` and Python's `None` serve the same purpose, but Rust makes it **impossible to use the value before unwrapping it**. "I forgot it could be None" stops being a category of bug.

## Three ways to open one

```rust
// 1. match — every arm must be handled; omit one and it will not compile
match p.parent() {
    Some(up) => p = up,
    None => return None,
}

// 2. if let — the short form when only one arm is interesting
if let Some(ext) = path.extension() {
    if ext == "md" { n += 1; }
}

// 3. unwrap — asserting it cannot be None. Panics when wrong
let home = env::home_dir().unwrap();
```

The **exhaustiveness** of `match` is the point: forgetting a case becomes a compile error rather than a runtime surprise.

## `if let … else` is not checked

An `if let` asks about one shape, and its `else` takes everything else, whatever that turns out to be. So when a variant is added, the compiler points at every `match` that does not name it, and at no `if let`. M7 added `Node::Unreadable`; with its arms taken out again, a build stops three times:

```
error[E0004]: non-exhaustive patterns: `&Node::Unreadable { .. }` not covered   (Node::children)
error[E0004]: non-exhaustive patterns: `&Node::Unreadable { .. }` not covered   (gather)
error[E0004]: non-exhaustive patterns: `&Node::Unreadable { .. }` not covered   (retain, in supporting)
```

`unread`, written as `if let Node::Unreadable … else if let Some(rows) = node.children()`, builds either way. There that is right — *anything else* is what it means — but it is also a place a new variant passes through unannounced. A `match` with `_ =>` is the same.

## Two kinds of nothing

A Bundle's `inside` is `Option<Vec<Node>>`, and its two empty values say different things:

| what happened | `inside` | on this machine |
|---|---|---|
| the directory was not read — a link stands in for it ([ADR-0007](../../adr/0007-links-are-listed-not-followed.md)) | `None` | `grill-with-docs` in `~/.claude/skills` |
| it was read, and holds nothing but its Lead | `Some` of an empty `Vec` | `dream` |
| it was read, and holds rows | `Some` of rows | `session-retro` |

`None` does not say the directory is empty. It says there is no result, because the question was never asked. The screen draws the first as `grill-with-docs (link)` and the second as plain `dream`, and neither opens. As a pattern, the empty case is `Some(rows) if rows.is_empty()`: `Some([])` is refused, since `[]` matches an array or a slice and this is a `Vec` — `E0529: expected an array or slice, found Vec<Node>`.

## A question asked of what may not be there

`is_some_and` and `is_none_or` put a yes-or-no question to what an `Option` holds. Their names are easy to read the wrong way; written out as the `match` each one stands for, the only difference is the answer written into the `None` arm:

```rust
// code.is_some_and(|c| !c.line.is_empty())   // code.is_none_or(|c| !c.line.is_empty())
match code {                                  match code {
    None => false,                                None => true,
    Some(c) => !c.line.is_empty(),                Some(c) => !c.line.is_empty(),
}                                             }
```

Run over the three states a code block can be in at its end:

```
code         is_some_and  is_none_or
None         false        true
Some("")     false        false
Some("x")    true         true
```

`is_some_and` is *there is one, and it is so*; `is_none_or` is *there is none, or it is so*. The code uses both: the end of a code block asks `is_some_and` whether a line is left to finish, and `tick` asks whether the time to scroll has not come yet with `selection.scroll_at.is_some_and(|at| now < at)` — no time set means scroll now.

## `Result` has the same shape

`Result<T, E>` is `Ok(value)` or `Err(error)`, and it opens exactly the same way — which is why learning `Option` first pays for both.

```rust
// if let, when the failure has its own thing to say
if let Ok(dir) = fs::read_dir(&path) {
    println!("{}:{}", name, dir.count());
} else {
    println!("{}:(missing)", name);
}

// let ... else — the inverted form: bind on success, or leave
let Ok(entries) = fs::read_dir(dir) else {
    return 0;
};
```

`let ... else` keeps the success path at the outer indentation instead of pushing it inside a block, and the `else` arm must diverge — `return`, `continue`, `break`, or `panic!`. It cannot fall through, because there would be nothing bound to fall through with.

Inside a loop, `continue` is what makes this a **guard clause**. One gate is a matter of taste; two or more, and the difference shows:

```rust
// if let — the real work sinks
for item in read {
    if let Some(ext) = path.extension() {
        if ext == "md" {
            if let Some(stem) = path.file_stem() {
                out.push(...);                                  // five levels deep
            }
        }
    }
}

// let ... else — the gates line up and the work stays flat
for item in read {
    let Some(ext) = path.extension() else { continue };
    if ext != "md" { continue; }
    let Some(stem) = path.file_stem() else { continue };

    out.push(...);                                              // two levels
}
```

The two gates are not equally real, and it is worth knowing which is which. `extension()` returns `None` for any file without a dot — `README`, `.gitignore` — and a planted extensionless file is exactly what made M1 miscount. `file_stem()` returning `None` needs a path ending in `..`, which `read_dir` never produces. It is still written as a gate rather than an `unwrap()`, because M1 already owes a debt of unwraps to M3 and there is no reason to add to it.

**`(missing)` rather than `0` is the point of the first form.** Printing `0` would make "the directory is empty" and "the directory does not exist" identical on screen, which is exactly the `exit 0` wrong answer this project treats as worse than a crash. Two different facts have to look different.

## Pitfalls hit — what unwrap throws away

`.unwrap()` discards the *context* of the failure. When one of the five source paths was missing during M1, the message read:

```
called `Result::unwrap()` on an `Err` value: Os { code: 3, kind: NotFound }
```

**Which of the five is absent is nowhere in it.** With five sources you find it by eye; once a config file can add more, you cannot. That is why error handling is its own milestone (M3) rather than a detail — see [[fs-read-dir]] for the paths that fail.

## Pitfalls hit — `None` read as "nothing in it"

For a link standing in for a Bundle, `inside` was picked as `Some([])` four times across M7's slices 2 and 3, and a table, an execution trace and a drawing did not change the answer. A diagnostic question showed why: `None` was being read in its everyday sense, *there is nothing inside*. What settled it was the table above, put as a question of whether there is a result at all, followed by a prediction on the real screen — with the empty case written as the pattern for a link, `(link)` lands on `dream`, `get-api-docs` and `to-html` instead of `grill-with-docs`. That was answered right.

## Pitfalls hit — `None` answering *no* to every question

The same reading came back in M8. In slice 1, `Option<&String>` was read as a result that says yes or no, not as an arrow that may be missing. In slice 2, asked what `is_none_or(|at| now >= at)` gives for no time set, a time to come and a time gone, the answer was right for the two times and wrong for none — *false*; and the guard that has to scroll when no time is set was given `is_some_and`, which then never scrolls: four tests failed, the preview staying where it was — `• 1` where `• 2` was expected, `• 8` where `• 6` was. Asked why `is_none_or` had been picked before, the answer was that its name has *none* in it. A table of the three states did not settle it.

The `match` each one stands for did, in slice 3. A diagnostic question asked only for the value of `None.is_none_or(…)` — *true* — and a review question over `None`, `Some(3)` and `Some(9)` was answered right. The wrong answer in that slice was to a question that asked whether a function *is called* when the value is true; it is, and returns at once, so the question had asked about an effect, not the value.

## Pitfalls hit — expecting `if let … else` to be checked (not settled)

Asked which of four functions stop at E0004 once `Unreadable` exists, the pick included `unread`, the `if let`, and left out two of the three `match`es above. Asked again with three small functions, `if let Node::Dir { .. } = node { true } else { false }` was again picked as stopping; it builds. Still open at the end of M7.

## Related

[[ownership]] · [[paths]] · [[fs-read-dir]] · [[enums-and-data]] · [[recursive-data]]
