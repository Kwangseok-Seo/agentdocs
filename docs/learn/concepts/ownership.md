# ownership

Every value has **exactly one owner**. When the owner goes out of scope the value is dropped; when the value is handed to someone else the ownership **moves**, and the old name becomes invalid at that instant.

## Why the rule exists

A `String` keeps a (pointer, length, capacity) triple on the stack and the actual bytes on the heap. If `let b = a` were a plain copy, **two pointers would address the same heap block**, and both would free it when they went out of scope — a double free.

Languages differ in how they avoid this:

| Language | Approach | Price |
|---|---|---|
| Go | a garbage collector cleans up later | GC at runtime |
| Python | reference counting | a count bump on every hand-off |
| C | the programmer keeps track | crashes or silent corruption when wrong |
| Rust | **exactly one owner, enforced** | you argue with the compiler; zero runtime cost |

## The `Copy` exception

Types that live entirely on the stack, such as `i32`, implement the `Copy` trait and are duplicated instead of moved. That is why `let b = a; println!("{a}")` compiles for an integer and not for a `String`. The error says so directly:

```
move occurs because `a` has type `String`, which does not implement the `Copy` trait
```

## Function arguments move too

`f(cwd)` hands ownership to the function; using `cwd` afterwards is `borrow of moved value`. To avoid that, pass a reference — see [[borrowing]] — as in `f(&cwd)`.

## Pitfalls hit

In M7, the key that opens or closes a row was written `if !self.open.insert(path) { self.open.remove(&path); }`. The logic was right — `insert` answers `false` when the path was already in the set — but `insert` takes the `PathBuf` itself, so on the next line `path` is gone: `E0382: borrow of moved value: path`. The compiler's help, `insert(path.clone())`, passes all 229 tests. The version kept, `if !self.open.remove(&path) { self.open.insert(path); }`, only lends `path` until the last line, which hands it over, and needs no copy.

## Related

[[borrowing]] · [[owned-vs-borrowed-pairs]] · [[mutability]]
