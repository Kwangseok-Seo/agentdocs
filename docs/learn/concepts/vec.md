# vec

`Vec<T>` is a growable, heap-allocated list that **owns** its elements. An array `[T; N]` cannot grow, because its length is part of its type.

```rust
let a = [1, 2, 3];       // type [i32; 3] — a 4-element array is a different type
let mut v = vec![1, 2, 3];
v.push(4);               // fine
```

M1 kept the source table in an array: five sources, always five. M2 needs a `Vec`, because the project rows are appended only when `find_project_root` returns `Some` — the length is not known until the program runs.

## What a Vec actually is

```
size_of::<Vec<i32>>()    = 24        three words, whatever T is
size_of::<Vec<String>>() = 24
size_of::<[i32; 5]>()    = 20        the elements themselves, inline
size_of::<[i32; 50]>()   = 200
```

```
stack                         heap
+--------------+             +----+----+----+----+
| ptr  --------+------------>| 10 | 20 | 30 | ?? |
| len      3   |             +----+----+----+----+
| cap      4   |
+--------------+
```

`String` has the identical layout — it is a `Vec<u8>` carrying a guarantee that the bytes are valid UTF-8, which is why this page and [[str-scanning]] describe the same machine.

## push, and the buffer that moves

Nine pushes onto a fresh `Vec`, printing length, capacity and the address of the heap buffer:

```
 push   len   cap  buffer address
    -     0     0  0x4                  Vec::new() allocates nothing
    0     1     4  0x2c760e96940
    1     2     4  0x2c760e96940
    2     3     4  0x2c760e96940
    3     4     4  0x2c760e96940
    4     5     8  0x2c760e975d0        full: new buffer, everything copied
    5     6     8  0x2c760e975d0
    6     7     8  0x2c760e975d0
    7     8     8  0x2c760e975d0
    8     9    16  0x2c760e91010        full again
```

An empty `Vec` costs nothing. When it fills, capacity doubles: a fresh buffer is allocated, the elements are copied over, and **the old address stops being valid**. `Vec::with_capacity(n)` skips the reallocations when the count is known up front.

## Which is why this cannot compile

```rust
let mut v = vec![10, 20, 30];
let first = &v[0];       // points into the heap buffer
v.push(40);              // that buffer may be freed right here
println!("{first}");
```

```
error[E0502]: cannot borrow `v` as mutable because it is also borrowed as immutable
  |     let first = &v[0];
  |                  - immutable borrow occurs here
  |     v.push(40);
  |     ^^^^^^^^^^ mutable borrow occurs here
```

This is iterator invalidation, moved from "undefined behaviour at runtime" to "will not build" — the exact bug the exclusivity rule in [[borrowing]] exists to stop.

## Three ways to walk one

| Written | Element type | Afterwards |
|---|---|---|
| `for x in v` | `T` (owned) | `v` is **gone** — moved |
| `for x in &v` | `&T` | `v` still usable |
| `for x in &mut v` | `&mut T` | `v` still usable, elements editable |

All three appear in this codebase: `&sources` to print the table, `&mut out` inside `Source::entries` so that each `Entry` can load its own frontmatter, and the moving form inside `out.extend(md_tree(&path))`, which swallows the returned `Vec` whole.

Using `v` after the moving form gives a message that also prescribes the fix:

```
error[E0382]: borrow of moved value: `v`
  |     for s in v {
  |              - `v` moved due to this implicit call to `.into_iter()`
help: consider iterating over a slice of the `Vec<String>`'s content to avoid moving into the `for` loop
  |     for s in &v {
  |              +
```

Reach for `&v` by default; move only when consuming is the intent. Note that the move only *bites* if the `Vec` is used later — code that never touches it again compiles either way, which makes the habit worth having rather than the error worth waiting for.

## Indexing

```rust
v.get(9)    // Option<&T> — None when out of range
v[9]        // panic: index out of bounds: the len is 3 but the index is 9  (exit 101)
```

## The borrowed half is `&[T]`

| Owning | Borrowing |
|---|---|
| `String` | `&str` |
| `PathBuf` | `&Path` |
| `Vec<T>` | `&[T]` |

Take `&[T]` in a signature, not `&Vec<T>`: the slice accepts arrays and slices too, while `&Vec<T>` accepts only a `Vec`. Same rule as [[owned-vs-borrowed-pairs]], one type further along.

## Pitfalls hit — returning the thing you are building

`count_md` returned a `usize`; `md_files` had to return `Vec<Entry>`, and the shape read as unfamiliar until the two were laid side by side. It is the same accumulator with a different type in it:

| `count_md` | `md_files` |
|---|---|
| `let mut n = 0;` | `let mut out = Vec::new();` |
| `else { return 0; }` | `else { return out; }` |
| `n += 1;` | `out.push(Entry { .. });` |
| `n` on the last line | `out` on the last line |

The last row is the part that reads strangely at first: **a block's final expression, written without a semicolon, is its value.** Add a semicolon there and the function returns `()`, with the error `expected Vec<Entry>, found ()`.

`Vec::new()` needs no type annotation here, because the return type of the function tells the compiler what is being collected.

## Related

[[ownership]] · [[borrowing]] · [[owned-vs-borrowed-pairs]] · [[structs]] · [[str-scanning]]
