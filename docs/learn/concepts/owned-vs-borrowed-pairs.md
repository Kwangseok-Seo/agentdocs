# owned-vs-borrowed-pairs

Rust data types tend to arrive in pairs: **the one that owns** and **the one that borrows**. Confusing them fails to compile; telling them apart is how you learn to read a signature.

| Owning (holds the heap) | Borrowing (points only) | Borrowed to owned |
|---|---|---|
| `String` | `&str` | `.to_string()` |
| `PathBuf` | `&Path` | `.to_path_buf()` |
| `Vec<T>` | `&[T]` | `.to_vec()` |

## Why the return value must own

```rust
fn find_project_root(start: &Path, home: &Path) -> Option<PathBuf> {
    ...
    return Some(p.to_path_buf());   // borrowed &Path -> owning PathBuf
}
```

`p` is a borrowed reference. Returning it as-is would create a reference outliving its source, which trips the third rule in [[borrowing]] (`does not live long enough`). So we make a copy and hand over ownership with it.

## Why the arguments borrow

`fs::read_dir` accepts `&str`, `PathBuf`, and `&Path` alike, because it takes a generic argument bounded by the `AsRef<Path>` trait. Callers pass whatever they already hold, without converting first.

## The borrow that outlives everything: `&'static str`

```rust
fn reason(kind: io::ErrorKind) -> &'static str {
    match kind {
        io::ErrorKind::NotFound => "missing",
        ...
    }
}
```

A **string literal is compiled into the executable**, so it is alive for as long as the program is. `'static` is the name of that lifetime, and it is the honest return type for a function that only ever hands back literals.

Note what forces the annotation. Every other borrowed return in this codebase points at something an argument owns, so the compiler can work out how long it lasts. Here there is no reference among the arguments — `io::ErrorKind` is a plain value — so there is nothing to borrow *from* and the lifetime has to be named.

`-> String` would compile just as well and allocate a fresh copy, on every call, of text that never changes. Lifetimes in general arrive in M6; this is the one case that already pays for itself.

## Related

[[ownership]] · [[borrowing]] · [[paths]] · [[result-and-errors]]
