# result-and-errors

`Result<T, E>` is [[option-and-match]]'s shape with one difference that changes everything: **the empty side carries a reason.**

```rust
enum Option<T>    { Some(T), None }
enum Result<T, E> { Ok(T),   Err(E) }
```

`None` can only say "there is no value". `Err(e)` says *why*. When `fs::read_dir` fails, the difference between "the directory is not there" and "you are not allowed to look" survives the return — and a viewer that discards it prints `0` for both.

Rust has **no exceptions**. There is no `throw`, no `catch`, nothing that leaves a function by a hidden door. An error is an ordinary value that a function returns, which means failure is written **in the signature** and no caller can be unaware of it. `Result` is also `#[must_use]`: receive one and ignore it, and the compiler says so.

## The four ways an error can end

| Ending | Written as | Means |
|---|---|---|
| **panic** | `.unwrap()`, `.expect("...")` | "this cannot fail" — and the program dies when it does |
| **swallow** | `let Ok(x) = .. else { return }`, `.unwrap_or(d)` | fall back to a default; **the reason is gone** |
| **handle** | `match e.kind() { .. }` | look at *which* failure it was and act |
| **propagate** | `?` | "not my decision" — hand it to the caller |

Only the first two need no thought, which is why code that has never been made to face failure is full of them. Choosing between the last two is the actual design work: **the reason must end up where something can be done with it.**

## `?`

```rust
fn md_files(dir: &Path) -> io::Result<Walked> {
    let read = fs::read_dir(dir)?;
    //                          ^ Ok(v)  -> the expression is v
    //                            Err(e) -> return Err(From::from(e)) immediately
    ...
    Ok(out)                   // the success path now has to wear the wrapper too
}
```

- `?` is allowed **only in a function that returns `Result`** (or `Option` — see below). You cannot bolt it onto a function that promises a bare value; you have to declare the failure first.
- On `Err` it converts through `From`, which is what lets one function propagate several error types. With a single error type there is nothing to convert, and no reason yet for a crate like `thiserror`.
- **`io::Result<T>` is an alias for `Result<T, io::Error>`**, defined in `std::io`. Most of the standard library's file operations return it.

### `?` works on `Option` too

Same rule, same shape — the function must return `Option`.

```rust
fn md_entry(path: PathBuf) -> Option<Entry> {
    let ext = path.extension()?;               // None -> return None
    if !ext.eq_ignore_ascii_case("md") { return None; }
    let name = path.file_stem()?.to_string_lossy().to_string();
    Some(Entry { name, path, kind: EntryKind::File, description: None })
}
```

Three `let ... else { continue }` guards collapse into two question marks. Note the order: `file_stem()` **borrows** `path`, and the borrow has to be finished before `path` is moved into the `Entry` — swapping those two lines produces `borrow of moved value`.

### Propagation is contagious, and that is the feature

Adding one `?` to the smallest function broke every caller above it at once:

```
md_files now returns io::Result<..>
  → md_tree:   `out` is a Result       E0599 no method `extend`
               `return out`             E0308 expected Vec, found Result
  → entries:   one match arm differs    E0308 match arms have incompatible types
```

In a language with exceptions the middle functions would say nothing and pass the error through untouched. Here every one of them is made to answer. The discomfort is the mechanism: `agents:0` existed because a middle function was allowed to stay silent.

## An error that is not a failure: the reader left

`println!` does not return a `Result`. When writing fails it panics — `failed printing to stdout` — which is the right trade for a quick program and the wrong one for the listing. `agentdocs | Select-Object -First 5` (or `| head`) reads what it wants and closes the pipe, the next line cannot be written, and until M5 that meant a panic after perfectly good output. Reading the first 40 bytes and closing the pipe ended in exit 101 with the panic on stderr.

`writeln!(out, …)` does the same writing and hands the failure back as an `io::Result`, so `?` can carry it up to a place that decides what it means:

```rust
match write_listing(out, global, project, sources, terms) {
    Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
    other => other,
}
```

It is the table at the top in two functions: `write_listing` **propagates** every failure with `?`, and `print_listing` **handles** the one it can decide about. The guard after the pattern (`if …`) picks that kind of `Err` out: a reader that left has seen all it asked for, so that is success. `other => other` passes everything else through untouched — a disk that is full is still an error. The same test now ends in exit 0 with nothing on stderr. On Windows the failure arrives as OS error 232, "the pipe is being closed", and Rust files it under `BrokenPipe`; that it does is shown by the exit code, since any other kind would have come back as an error.

## Turning a reason into words

`e.kind()` and `e` are not interchangeable. Measured on this machine:

| | `e.kind()` | `e` |
|---|---|---|
| directory forbidden | `PermissionDenied` | `액세스가 거부되었습니다. (os error 5)` |
| directory absent | `NotFound` | `지정된 경로를 찾을 수 없습니다. (os error 3)` |
| a file, not a directory | `NotADirectory` | `디렉터리 이름이 올바르지 않습니다. (os error 267)` |

`Display` hands back **the operating system's message in the machine's language**. Printing it makes the program speak Korean here and English elsewhere, and no test can pin the result. `ErrorKind` is Rust's own enum and reads the same everywhere, so the program translates it into words it owns:

```rust
fn reason(kind: io::ErrorKind) -> &'static str {
    match kind {
        io::ErrorKind::NotFound => "missing",
        io::ErrorKind::PermissionDenied => "permission denied",
        io::ErrorKind::NotADirectory => "not a directory",
        _ => "unreadable",
    }
}
```

`io::ErrorKind` is `#[non_exhaustive]`: the standard library reserves the right to add variants, so **`_` is required here**, not lazy. On the return type see [[owned-vs-borrowed-pairs]].

## `.ok()` — deliberately dropping the reason

```rust
let cwd = env::current_dir().ok();     // Result<PathBuf, io::Error> -> Option<PathBuf>
```

This is a swallow, chosen on purpose: `env::home_dir()` returns a bare `Option` and has no reason to give, so carrying one for `current_dir` alone would only force the two to be handled differently. **What is dropped is the reason, not the fact** — the screen still says `PROJECT (current directory unknown)`.

## Pitfalls hit

- **The compiler's `help:` suggested undoing the milestone.** Three of the four errors from the first `?` ended with `help: consider using Result::expect ... panicking if the value is a Err`. Taking all three would have compiled and reinstated the panics the milestone existed to remove. The compiler knows the types do not line up; it does not know what you are trying to do.
- **`Result` is iterable, so a wrong loop still compiled into something.** With `entries` accidentally left as a `Result`, `for entry in &entries` did not fail with "not an iterator" — `Result` yields one item on `Ok` and none on `Err`, so `entry` became `&Vec<Entry>` and the error read `no field 'name' on type '&Vec<Entry>'`. The loop was not dead; it was running once around the wrong thing.
- **In a recursive function the compiler believes the signature, not the body.** With `md_tree`'s body rewritten but its return type still `io::Result<Vec<Entry>>`, the error landed on `out.absorb(sub)` twelve lines below — because the type of the recursive call comes from the *declaration*. The error surfaces where the contradiction shows, not where the cause is.
- **Swallowing and handling look identical.** `let Ok(item) = item else { continue }` and `let Ok(item) = item else { out.unreadable += 1; continue }` are the same construct; the difference is entirely in whether the `else` body records anything. `let ... else` was never the problem — an empty `else` was.

## Related

[[option-and-match]] · [[fs-read-dir]] · [[file-types-and-links]] · [[testing]] · [[owned-vs-borrowed-pairs]] · [[drop-and-unwinding]]
