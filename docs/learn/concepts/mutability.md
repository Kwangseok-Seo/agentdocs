# mutability

Rust bindings are **immutable by default**. Changing one requires opting in with `let mut`. This is the opposite of Go and Python.

```rust
let n = 0;
n += 1;              // error: cannot assign twice to immutable variable

let mut n = 0;
n += 1;              // fine
```

## Why immutable is the default

When "this does not change" is the default, both the compiler and the reader reason more easily, and the places that do change stand out because `mut` is written there. The mutable-borrow rule in [[borrowing]] (`&mut` may exist only alone) grows from the same root.

## Pitfalls hit — a leading `_` silences the compiler's help

An underscore prefix means *"I know I am not using this; do not warn me."*

```rust
let _home_path = home.join(".claude").join("skills");   // built, never used — and no warning
```

During M1 this value was constructed and never read, and the compiler said nothing. Without the underscore it would have reported `warning: unused variable`. **Adding `_` out of habit throws away help you were about to get.**

## Shadowing

Re-declaring the same name is an idiom here, not a mistake:

```rust
let entry = entry.unwrap();   // entry (Result) opened, rebound as entry (DirEntry)
```

It saves inventing a name like `entry_unwrapped` for "the same thing, unwrapped." Unlike `mut`, this creates a **new** binding, so the type is free to change.

## Related

[[ownership]] · [[borrowing]]
