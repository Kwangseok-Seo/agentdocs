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

## Related

[[ownership]] · [[borrowing]] · [[paths]]
