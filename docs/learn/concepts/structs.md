# structs

A `struct` gives every field a **name and a type**. It is what you reach for the moment a tuple stops being obvious.

```rust
struct Source {
    name: String,
    path: PathBuf,
    scope: Scope,
    walk: Walk,
}

let s = Source { name: String::from("rules"), path: p, scope: Scope::Global, walk: Walk::MarkdownFiles };
println!("{}", s.name);
```

- Field types are **never inferred**. A `let` inside a function may skip its type; a field may not, because the field list is the contract other code reads.
- Construction is **by field name**, so the order does not matter. That is the whole point: a four-slot tuple is ordered, and `("skills", path, Global, BundleDirs)` says nothing about which slot is which.
- **Field init shorthand**: when the variable and the field share a name, `path: path` shortens to `path`.

## Where the tuple gave out

M1 carried the source table as `("skills", path)` pairs and read them back with `for (name, path) in sources`. Two slots is fine. M2 needed two more — the [[enums-and-data]] values `Scope` and `Walk` — and at four slots, swapping the last two produces code that still compiles and is wrong. Naming them removes that class of mistake entirely.

## A struct owns its fields

`name: String` means this `Source` **owns** that string, not a view of someone else's ([[ownership]]). Ownership then nests: dropping a `Vec<Source>` drops each `Source`, which drops its `String` and its `PathBuf`, which each free their heap buffer. One root going out of scope cleans up the whole tree.

Holding `name: &str` instead is possible but drags in a lifetime annotation, and it would mean the `Source` cannot outlive whatever it borrowed from — no good once names arrive from a parsed config file.

## Choosing the argument types of a constructor

The struct owns all four fields, so `Source::new` has to end up with four owned values. What each argument should be falls out of one question: **can the caller hand over ownership, and does it still want the value afterwards?**

| Situation | Take it as | Why |
|---|---|---|
| The caller cannot give ownership (a `"literal"`) | `&str`, copy inside | There is no owned `String` at the call site to give |
| The caller holds it but is done with it (`home.join(..)`) | by value | Moving is free; taking `&Path` would allocate a second copy |
| The value is smaller than a pointer (`Scope`) | by value | The reference would be larger than the thing it points at |
| The caller must keep using it | `&T` | |

Measured on 64-bit Windows:

| Value | bytes | Reference | bytes |
|---|---:|---|---:|
| `Scope` | 1 | `&Scope` | 8 |
| `String` | 24 | `&str` | 16 |
| `PathBuf` | 32 | `&Path` | 16 |

Taking `&str` rather than `String` also widens what a caller may pass — a literal, a `&String`, the result of `format!` — which is why "accept `&str`, not `String`" is the standing convention. See [[owned-vs-borrowed-pairs]].

## Pitfalls hit — the loop body kept speaking the tuple's language

Replacing the tuple array with `Vec<Source>` and changing the loop header produced three errors at once, all the same mistake:

```
error[E0425]: cannot find value `name` in this scope
error[E0423]: expected value, found built-in attribute `path`
```

`for (name, path) in sources` had been *unpacking* the tuple into two variables; `for src in &sources` hands over one value and the fields have to be reached through it. The translation is `name` to `src.name` and `&path` to `&src.path` — nothing more.

(`path` reporting as a *built-in attribute* rather than an unknown name is a red herring: Rust has a `#[path]` attribute and the compiler guessed at it.)

## Pitfalls hit — the compiler's `help:` can point at the wrong thing

Writing `scope: Scope` before the type existed produced a helpful-looking suggestion:

```
help: consider importing this struct
  |
1 + use std::thread::Scope;
```

`std::thread::Scope` is the handle for scoped threads and has nothing to do with this program. The compiler matched on the *name* alone. Taking that suggestion would have compiled and meant something else entirely.

## Related

[[enums-and-data]] · [[impl-and-methods]] · [[ownership]] · [[owned-vs-borrowed-pairs]] · [[vec]]
