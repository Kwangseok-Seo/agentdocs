# recursive-data

A type that holds values of its own type — a directory that holds directories. The value itself is always finite; what has to be solved is its **size**, which the compiler must know before anything is built.

## A type cannot hold itself directly

Every type has a size fixed at compile time: it is how much room a variable of that type takes. A variant that held a whole `Node` inside a `Node` would need room for a `Node`, which needs room for a `Node`, and so on:

```rust
enum Node { Leaf(String), Dir(String, Node) }
```
```
error[E0072]: recursive type `Node` has infinite size
2 | enum Node { Leaf(String), Dir(String, Node) }
  | ^^^^^^^^^                             ---- recursive without indirection
help: insert some indirection (e.g., a `Box`, `Rc`, or `&`) to break the cycle
  |
2 | enum Node { Leaf(String), Dir(String, Box<Node>) }
```

Writing `Option<Node>` to say "there may be none" gives the same E0072: an `Option` holds its value in place, so it is at least as large as what it holds. The loop does not have to be direct either. In agentdocs the way back to `Node` runs through `Entry` and a Bundle's `inside`, and the compiler follows it: with `Option<Tree>` in the middle of such a loop it reports *recursive types `Entry`, `Kind` and `Tree` have infinite size*.

## Indirection: hold an arrow, not the thing

The fix is to keep the children somewhere else — on the heap — and hold only where they are. An arrow is the same size however large, or however deep, the thing it points at:

```
Box<Node> = [ → ]              ──►  one Node
Vec<Node> = [ → | len | cap ]  ──►  Node Node Node …
```

Measured on 64-bit Windows:

```
Box<Node>                     8 bytes
Vec<Node>                    24 bytes
enum { Leaf(String), Dir(String, Box<Self>) }   32 bytes
enum { Leaf(String), Dir(String, Vec<Self>) }   48 bytes
```

`Box<T>` puts **one** `T` on the heap: the shape for a single child, like a linked list or `a + b`. `Vec<T>` puts **any number** side by side ([[vec]]), and a directory has any number of children, so agentdocs uses `Vec` and has no `Box` in its tree. It met `Box` once before, for a different reason: M5's `panic::set_hook(Box::new(…))` puts one closure on the heap, because a closure's type has no size the hook could know in advance ([[drop-and-unwinding]]).

```rust
pub enum Node {
    Entry(Entry),
    Dir { path: PathBuf, children: Vec<Node> },
    Unreadable { path: PathBuf, reason: io::ErrorKind },
}
```

## A level holds only the level below

A `Dir`'s `children` is the level right under it, and nothing further down. A grandchild is not in that `Vec`; it is in another `Vec`, held by the child. cli-maker's `docs/learn`, as the Walk builds it:

```
learn = Dir { children ──► ┌───────┬──────────────┬────────────────┐
                           │ index │ Dir concepts │ Dir milestones │   3, measured
                           └───────┴──────┬───────┴───────┬────────┘
                                children  │     children  │
                                          ▼               ▼
                             ┌───────────┬─────┐  ┌────┬────┬─────┐
                             │ borrowing │ …51 │  │ M0 │ M1 │ …12 │
                             └───────────┴─────┘  └────┴────┴─────┘
```

`children.len()` is 3, not 64. Reaching the 51 means following the arrow in the second box — which is what a function has to do.

## Functions over it call themselves

A type that holds itself is taken apart by a function that calls itself, and the shape is always the same: **stop at a leaf, and on a branch call yourself with its children.**

```rust
fn count(nodes: &[Node]) -> usize {
    nodes.iter().map(|node| match node {
        Node::Leaf(_) => 1,                            // a leaf: stop
        Node::Dir { children, .. } => count(children), // a branch: go down
    }).sum()
}
```

Over cli-maker's docs — `adr/` with 16, and `learn/` above — it returns 16 + (1 + 51 + 12) = 80. M1's `count_md` did the same sum walking the disk; this one walks the tree in memory. For `docs/a/b/c.md`, three calls are made, one per level, and only the third finds a leaf:

```
gather([Dir a])   follows a's arrow
gather([Dir b])   follows b's arrow
gather([c])       an Entry: pushed      → ["c"]
```

| function | what it does at a branch | where |
|---|---|---|
| `md_tree` | walks the subdirectory on disk and nests what it found as one `Dir` row | `source.rs` |
| `gather` | goes down into every directory, collecting Entries — but not into a Bundle, so a Bundle Source still counts its Bundles | `entry.rs` |
| `unread` | goes down into every directory and every Bundle that was walked, collecting what could not be read | `entry.rs` |
| `visible` | goes down only where the row is open | `tui.rs` |

`unread` and `visible` ask `Node::children()` where to go down, so a directory and a walked Bundle look the same to them, and a link standing in for a Bundle has nothing to go down into. The last three hand their results out through an argument, such as `out: &mut Vec<&'a Entry>`, and that needs a named lifetime — [[lifetimes]].

## Where it stops

A tree built in memory always ends, at its leaves. A walk over a disk ends only if the disk has no loop on the way down, and a link can make one: until M3, one junction pointing at its own parent turned a directory of two files into 128 rows. `md_tree` enters only what the entry itself says is a directory, never a link ([[file-types-and-links]]), so every tree it builds has a bottom.

## Pitfalls hit

- **A level taken for everything below it.** Asked what `gather` should do at a directory, the pick was to take the Entries among its children, one level only. In the repository that failed three tests and printed `docs:17` for cli-maker's 80: the 16 ADRs and `learn/index.md`. The same picture — `children` holds the whole subtree — gave two more wrong answers: `children.len()` to count directories, which printed 19 instead of 4, and one Entry expected from `docs/a/b/c.md`, where a one-level `gather` returns none. That picture was the code until this milestone: M6's `absorb` poured everything below into one list. Measuring `learn`'s three children and drawing a box whose arrows lead to more boxes settled it — the next two questions, and in slice 3 how many rows opening `learn/` adds, were right.

## Related

[[enums-and-data]] · [[vec]] · [[option-and-match]] · [[lifetimes]] · [[file-types-and-links]] · [[fs-read-dir]]
