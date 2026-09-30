# statics

A `static` is one value with one place in memory for the whole run of the program. A `let` is worked out when the program reaches its line, and goes at the end of its block; a `static` is worked out **by the compiler**, written into the binary, and is there before `main` starts and after it ends.

## What the compiler can work out

Only what needs nothing from the running program. Measured with one small program:

```rust
static NAME: &str = "agentdocs-static-demo";   // text: written into the .exe as it is
static LIMIT: usize = 30 * 1000;                // arithmetic: done by the compiler
```

`NAME`'s letters are in the `.exe` file before it ever runs — at byte 101,589 of 145,408. What needs the running program cannot go there:

```
static START: Instant = Instant::now();
error[E0015]: cannot call non-const associated function `Instant::now` in statics

static WORDS: Vec<String> = vec!["fn".to_string(), "let".to_string()];
error[E0010]: allocations are not allowed in statics
```

The first is the plainest case: *what time is it now* has no answer while the program is being compiled. The second asks for memory from a heap that does not exist yet. Only `const fn`s may be called — functions marked as ones the compiler can run itself.

## Made the first time it is used: `LazyLock`

```rust
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_nonewlines);
static THEME: LazyLock<Theme> = LazyLock::new(theme);
```

`LazyLock::new` is a `const fn`: all it does is put an empty box and a function to fill it side by side, which the compiler can write into the binary. The function runs the first time anything reads the static, once, and every later read gets the same value:

```
main starts: NAME = agentdocs-static-demo, LIMIT = 30000
nothing has touched WORDS yet
    ... WORDS is being made now          ← the first read
first use:  WORDS has 2 words
second use: WORDS has 2 words            ← not made again
```

*Lock* because two threads reading it for the first time at once must not both fill it: one waits for the other. In agentdocs, the languages are read the first time a preview holds a code block — 0.6 ms in a release build — and a preview without one never pays for them. (syntect is lazy a second time inside: each language turns its patterns into regular expressions the first time a block of it is drawn. Measured over this machine's files, the first pass took 622 ms and a second 340; the first Markdown block of the first pass, three lines, took 58 ms of it.)

## Why the theme has to be one

A highlighter holds an arrow to the theme it colours with, for as long as it lives — `HighlightLines<'a>` holds a `&'a Theme` — and a code block's highlighter lives in the renderer from the block's first line to its last, across many calls. The theme has to outlive all of that. Made inside the function that hands the highlighter out, it would go when the function returns:

```rust
pub fn for_language(language: &str) -> Option<HighlightLines<'static>> {
    let syntax = SYNTAXES.find_syntax_by_token(language)?;
    let theme = theme();
    Some(HighlightLines::new(syntax, &theme))
}
```
```
error[E0515]: cannot return value referencing local variable `theme`
    |     Some(HighlightLines::new(syntax, &theme))
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^------^^
    |     |                                |
    |     |                                `theme` is borrowed here
    |     returns a value referencing data owned by the current function
```

A static lives as long as the program, so an arrow to it is `&'static` — the longest a borrow can be ([[lifetimes]]). That is what the `'static` in `HighlightLines<'static>` says.

## Pitfalls hit

- **A static taken to be worked out as the program runs, like a `let`.** Asked which declaration of `THEME` compiles, the pick was `static THEME: Theme = theme();`. Built in the repository, it stopped at the E0015 above — whose last line reads `consider wrapping this expression in std::sync::LazyLock::new(|| ...)`: the compiler names the fix. A diagnostic question found the reason for the pick, and the program above, with its output, came next.
- **Calling a function and handing it over (not settled).** Asked again which of four statics compile, the picks were `LIMIT = 30 * 1000` and `START: Instant = Instant::now()`, and not `LazyLock::new(Instant::now)`. It is the other way round for the last two. With parentheses, `Instant::now()` is called on that line; without them, `Instant::now` is the function itself, handed to `LazyLock` to call later — as `LazyLock::new(theme)` is written in the code. When the value would arrive was answered right in the same round: the first time `START` is read. See [[closures]].

## Related

[[lifetimes]] · [[closures]] · [[state]] · [[external-crates]] · [[ownership]]
