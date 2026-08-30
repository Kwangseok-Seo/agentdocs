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

## Related

[[ownership]] · [[paths]] · [[fs-read-dir]]
