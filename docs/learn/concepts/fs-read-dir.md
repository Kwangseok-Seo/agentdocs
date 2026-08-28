# fs-read-dir

`fs::read_dir(path)` yields an iterator over the entries of **one directory level**. It does not descend, and it does not care what kind of entry it hands you.

```rust
fs::read_dir(path)          -> Result<ReadDir>       // the path may not exist
   .unwrap()                -> ReadDir               // an iterator
for entry in ...            -> Result<DirEntry>      // each entry may fail too
   entry.unwrap().path()    -> PathBuf
```

Note the **two layers of `Result`**: opening the directory can fail, and so can reading a single entry inside it. See [[option-and-match]].

## What count() actually counts

`ReadDir` is an iterator, so `.count()` is available — but what it counts is **"entries in this directory"**, not the Entry defined in our glossary. Measured during M1:

| Path | `count()` | Wanted | |
|---|---:|---:|---|
| `.claude/rules` | 19 | 19 | matched by luck (only `.md` in there) |
| `.claude/skills` | 7 | 7 | matched by luck (only directories in there) |
| `cli-maker/docs` | **2** | **80** | wrong — one level holds just `adr/` and `learn/` |

A single `README.md` dropped into `skills` would have produced 8. **Passing tells you nothing on its own; you have to check that it fails when it should.**

## Descending with recursion

```rust
fn count_md(dir: &Path) -> usize {
    let mut n = 0;
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();

        if path.is_dir() {
            n += count_md(&path);                  // call itself
        } else if let Some(ext) = path.extension() {
            if ext == "md" { n += 1; }
        }
    }
    n                                              // no semicolon = the return value
}
```

Each level returns **the sum of its children plus what it found itself**. The trace over `cli-maker/docs` shows it plainly: `adr` 16 + (`concepts` 51 + `milestones` 12 + one file directly in `learn`) = 80.

Note `&path` and `&root` at the call sites — passing them by value would move them, per [[borrowing]].

## Pitfalls hit

`read_dir(...).unwrap()` panicked outright on repositories without a `docs/` directory, such as `session-seal`. **For a program that reads other people's files, a missing path is not a bug but the normal case**, so M1 guards it with `docs.is_dir()` and leaves proper handling to M3.

## Related

[[paths]] · [[option-and-match]] · [[borrowing]]
