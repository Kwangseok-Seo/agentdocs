# iterators

An iterator is a value that hands out items **one at a time**, and in Rust that is a single trait with a single method you must write:

```rust
trait Iterator {
    type Item;                                  // what it hands out
    fn next(&mut self) -> Option<Self::Item>;   // one item, or None when it is done
    // ...and everything else
}
```

The end is signalled with the same `Option` as [[option-and-match]]. `next` takes `&mut self` because handing out an item moves an internal position forward — which is why an iterator you drive by hand is always `let mut`.

"Everything else" is the point. In the standard library of rustc 1.94 the trait declares **76 methods, and `next` is the only one without a body**. Write `next` for your own type and `map`, `filter`, `sum`, `find` and the rest arrive with it:

```rust
struct Countdown { left: u32 }

impl Iterator for Countdown {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        if self.left == 0 { return None; }
        self.left -= 1;
        Some(self.left + 1)
    }
}

Countdown { left: 6 }.filter(|n| n % 2 == 0).collect()   // [6, 4, 2]
Countdown { left: 4 }.sum()                               // 10
Countdown { left: 9 }.find(|n| *n < 3)                    // Some(2)
```

How a trait can carry method bodies is [[traits]].

## `for` is a loop around `next`

`for s in names.iter() { .. }` is `while let Some(s) = it.next() { .. }` with the bookkeeping hidden. `parse_frontmatter` is written in the unhidden form, `while let Some(line) = lines.next()`, because a `for` would have moved the iterator into the loop and it had to stay reachable to be lent to `fold` ([[str-scanning]]).

`for` does not demand an iterator; it asks the value to *turn into* one. `Result` agrees to — one item on `Ok`, none on `Err` — which is why a `for` over a `Result` compiled into a loop around the wrong thing in M3 ([[result-and-errors]]).

## Adapters and consumers

| Kind | Examples | What it does | Returns |
|---|---|---|---|
| **adapter** | `map` `filter` `skip` `enumerate` `filter_map` | wraps another iterator; does nothing by itself | another iterator |
| **consumer** | `collect` `count` `all` `any` `find` `min` | calls `next` until it has its answer | a value |

**Adapters alone do nothing.** A `map` whose closure prints, with no consumer after it, printed nothing at all. The compiler says so:

```
warning: unused `Map` that must be used
  = note: iterators are lazy and do nothing unless consumed
help: use `let _ = ...` to ignore the resulting value
```

The `help:` line silences the warning and changes nothing else: the iterator still never runs. It is advice about the warning, not about the intent.

**A consumer pulls one item through the whole chain before asking for the next.** With a `print` in both closures:

```
  map    alpha
  filter ALPHA
  map    beta
  filter BETA
  map    gamma
  filter GAMMA
  ["ALPHA", "GAMMA"]
```

Not "every `map`, then every `filter`". `collect` asks `filter` for an item, `filter` asks `map`, `map` asks the array, and one item travels up before the next request goes down — so no intermediate list is ever built.

## Stopping early

`all`, `any` and `find` stop as soon as the answer is known. Counting the calls:

```
    contains "adr"?
  "검증 절차만 있는 문서" -> false, checked 1 of 2
    contains "adr"?
    contains "검증"?
  "adr 과 검증을 둘 다 말하는 문서" -> true, checked 2 of 2
```

And on nothing at all:

```
all on [] = true (closure called 0 times), any on [] = false
```

`all` is really asking *"is there anything that fails?"* — with nothing to check, nothing fails. That one fact removed two branches from the search: with no terms, `matches` keeps every Entry and `any` in `first_hit` finds no line, so neither needs an `if` for a run without arguments to list what M3 listed.

## `collect` is told what to build

```rust
fn search_terms(args: impl Iterator<Item = String>) -> Vec<String> {
    args.skip(1)
        .filter(|a| !a.trim().is_empty())
        .map(|a| a.to_lowercase())
        .collect()
}
```

`collect` can build a `Vec`, a `String`, a map and more; the return type — or a `let` annotation — picks one. `printable` collects `char`s into a `String`; `listing` collects `&Entry`s into a `Vec<&Entry>`, which borrows the Entries rather than moving them out of the `Walked` they live in.

## `filter_map`: map and filter in one pass

The closure returns an `Option`; `Some(x)` passes through as `x`, `None` is dropped. In `around`, one step per term, measured:

```rust
terms.iter()
    .filter_map(|t| lower.find(t.as_str()).map(|at| (at, t.len())))
    .min()
```
```
line = "검증 절차는 ADR 에 적는다",  terms = ["adr", "검증", "memory"]

  adr      find -> Some(17)   map -> Some((17, 3))
  검증       find -> Some(0)    map -> Some((0, 6))
  memory   find -> None       map -> None            <- dropped
  filter_map + min -> Some((0, 6))
```

The inner `.map` is not the iterator's — it is `Option::map`, which changes the value inside a `Some` and leaves a `None` alone. Same name, different type, same idea. `min` on tuples compares the first field first, so it picks the earliest start. `find` answers in **bytes**: `"adr"` starts at byte 17, which is character 7.

## In this codebase

| Chain | Where |
|---|---|
| `skip` → `filter` → `map` → `collect` | `search_terms` |
| `all` | `Entry::matches` |
| `enumerate` → `find`, then `Option::map` | `Entry::first_hit` |
| `filter` → `collect` | `listing` |
| `filter_map` → `min`, then `chars` → `skip` → `collect` | `around` |
| `map` → `sum` over one character's lowercase form | `chars_before` |
| `filter` → `collect` | `printable` (since M3) |

## Pitfalls hit

- **An adapter is called, not written.** Asked to add two adapters to a chain, the question back was *how an adapter is implemented*. None is: `skip` and `map` are methods every iterator already has, reached with a dot. The chain is four method calls on one expression, and the line breaks are only for reading.
- **An empty term is not an empty list of terms.** `all` over no terms is true on purpose. But `""` *as a term* is found inside every text, and so is on line 1 of every file. A reviewer ran `agentdocs "" adr` — as an unset shell variable would — and the right rows were kept, but each showed `1: ---`, a line without `adr` in it, as the reason it matched; `agentdocs ""` alone kept every row and turned every count into `N/N`. `search_terms` now drops arguments that are empty or only whitespace, so `agentdocs ""` means what no arguments mean.

## Related

[[closures]] · [[traits]] · [[option-and-match]] · [[str-scanning]] · [[vec]]
