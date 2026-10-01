# release-profiles

A **profile** is how `cargo` builds, and `release` is the one `--release` uses — so it is what people download. [[external-crates]] has the two profiles cargo ships with, and the one line M8 changed in `dev`. The settings of `release` trade three things against each other: how large the file is, how long it takes to build, and what happens when the program panics.

## The settings, and what each traded here

`[profile.release]` in `Cargo.toml` changes the build of this package and of every dependency. Each setting can also be tried without touching the file, through an environment variable named after it — `CARGO_PROFILE_RELEASE_LTO=fat cargo build --release` — which is how six were built from clean and measured before one was written down:

| profile | Windows | Linux, musl | build, Windows | a search |
|---|--:|--:|--:|--:|
| the defaults | 3.93 MB | 5.40 MB | 11.8 s | 32.7 ms |
| `strip = true` | 3.93 MB | 4.44 MB | 11.8 s | 33.3 ms |
| `lto = "thin"`, stripped | 4.03 MB | | 13.2 s | 35.2 ms |
| `lto = "fat"`, `codegen-units = 1`, stripped | 3.60 MB | 3.83 MB | 40.0 s | 33.8 ms |
| that, and `opt-level = "s"` | 2.91 MB | | 38.8 s | 63.0 ms |
| that, and `panic = "abort"` | 3.04 MB | 3.66 MB | 42.0 s | 33.8 ms |

The search is the binary run in `cli-maker` with `검증`, the median of five. Reading the files is most of it, which is why only one setting moved it.

- **`strip`** takes the symbol table out — the names of functions, kept for a debugger and for the names a panic's backtrace prints. On Linux that was 18% of the file. On Windows it changed nothing: Microsoft's linker keeps symbols in a separate `.pdb` file, which is never shipped.
- **`lto`**, link-time optimisation, lets the optimiser see across crates at the end rather than one crate at a time, so code a dependency carries and nothing calls can go. `"thin"` is the cheaper kind, and here it made the file *larger*. `"fat"`, with **`codegen-units = 1`** — the crate optimised as one piece instead of sixteen in parallel — made it 8% smaller on Windows and 29% on Linux than the defaults, and took three times as long to build.
- **`opt-level`** is how hard the compiler works, and toward what: `3`, the default, for speed; `"s"` and `"z"` for size. `"s"` saved another 0.7 MB and made a search twice as slow: lowercasing and matching text ([[str-scanning]]) is the work a search does, and it had been compiled for size.

M11 kept the fourth row:

```toml
[profile.release]
lto = "fat"
codegen-units = 1
strip = true
```

Forty seconds is paid once per release on GitHub's machines, and by nobody who runs the program.

## `panic = "abort"`, and why not

With `panic = "unwind"`, the default, a panic walks back up the stack, running each value's `Drop` on the way ([[drop-and-unwinding]]). With `"abort"` the process ends where it panicked, and the code that does the unwinding is left out of the file — 0.56 MB on Windows, 0.17 MB on Linux. The panic hook still runs first, so the screen would still hand the terminal back: that is done by a hook, not a `Drop`. But a `Drop` written later to clean something up would be skipped on the way out, without a word; 4% of the Linux binary did not pay for that.

## Related

[[external-crates]] · [[drop-and-unwinding]] · [[cross-compilation]] · [[continuous-integration]]
