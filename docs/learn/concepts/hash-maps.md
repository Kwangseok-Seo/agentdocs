# hash-maps

A `HashMap<K, V>` holds values under keys, and finds the value for a key without looking through the rest. A `HashSet<K>` is the same thing with the values left out: it only says whether a key is in it. Neither keeps an order — nothing here walks through one, so nothing needs one.

## Two on the screen, both keyed by path

```rust
pub struct App {
    open: HashSet<PathBuf>,              // M7: which rows of the tree show what is below them
    scrolled: HashMap<PathBuf, usize>,   // M8: how far down each file's preview has been scrolled
    …
}
```

Scrolling one file and then looking at another has to leave the first where it was, and come back to it there. So the number is kept per file, and the file is named by its path — as M7 already named an open row.

What the other keys do was run before the quiz that asked for this one:

| kept as | what happens | measured |
|---|---|---|
| `usize` — one number for the screen | every file shares it: drawing a short file in between holds it to that file's end, and the long one comes back at its top | 2 tests fail, `• 1` where `• 6` was expected |
| `HashMap<usize, usize>`, by row number | a directory opened above the file moves it to another row, and it forgets | the test that opens a row above fails |
| `HashMap<PathBuf, usize>`, by path | a file keeps its place, whatever happens to the rows around it | 269 pass |

A row number is where something is *drawn*, and that moves. A path is what it *is*.

## Asking, and asking to change

```rust
let top = self.scrolled.entry(path).or_default();   // &mut usize: the one there, or a new 0
*top = top.saturating_add_signed(rows);

self.scrolled.get(path).copied().unwrap_or(0)        // Option<&usize> → Option<usize> → usize
```

`entry` finds the slot for a key and `or_default` fills it with `0` when it is empty, handing back a `&mut` to what is there either way — one lookup where `get` then `insert` would take two. `get` answers with an arrow into the map, `Option<&usize>`, since the value stays in the map; `copied()` makes it a number of its own, so nothing is borrowed from the map afterwards.

A `HashSet`'s `insert` and `remove` each answer whether anything changed, which makes a toggle two lines:

```rust
if !self.open.remove(&path) {   // it was not open…
    self.open.insert(path);     // …so open it
}
```

## What a key needs

A key has to be hashed and compared, so its type implements `Hash` and `Eq` — `PathBuf`, `String` and the integers all do. Two paths are the same key when they are equal as `Path`s, which compares part by part and case by case: on Windows, `C:\Docs\a.md` and `C:\docs\a.md` name one file and are two keys. Nothing on this machine reaches a file by two spellings of its path.

## Related

[[state]] · [[vec]] · [[traits]] · [[paths]] · [[borrowing]]
