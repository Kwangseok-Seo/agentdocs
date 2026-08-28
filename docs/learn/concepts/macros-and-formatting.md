# macros-and-formatting

A `!` after a name marks a **macro**. It is not a function; it expands into code at compile time.

```rust
println!("{}", dir.count());
//        ^^^^  ^^^^^^^^^^^
//   format string   value to interpolate
```

## Why the first argument must be a literal

Because the compiler **takes that string apart at compile time** and checks that the number of `{}` placeholders matches the number and types of the values after it. C's `printf` has no such check, which is where format-string vulnerabilities come from; Rust accepts only literals and rules the whole class out.

## Short forms and their limit

```rust
println!("{name}")            // a bare variable name may go inside the braces (stable since Rust 1.58, independent of edition)
println!("{}", dir.count())   // a method call may not — use the comma form
println!("{n:>5}")            // width 5, right-aligned
println!("{name:<9}")         // width 9, left-aligned
println!("{home:?}")          // Debug output, for types without Display (see [[paths]])
```

## Pitfalls hit

Putting the value straight into the parentheses produced `error: format argument must be a string literal`:

```rust
println!(dir.count());          // no
println!({"dir.count()"});      // no — a block is not a literal, and it would print the text anyway
println!("{}", dir.count());    // yes
```

## Related

[[paths]] · [[fs-read-dir]]
