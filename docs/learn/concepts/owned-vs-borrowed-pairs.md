# owned-vs-borrowed-pairs

Rust data types tend to arrive in pairs: **the one that owns** and **the one that borrows**. Confusing them fails to compile; telling them apart is how you learn to read a signature.

| Owning (holds the heap) | Borrowing (points only) | Borrowed to owned |
|---|---|---|
| `String` | `&str` | `.to_string()` |
| `PathBuf` | `&Path` | `.to_path_buf()` |
| `Vec<T>` | `&[T]` | `.to_vec()` |

## Why the return value must own

```rust
fn find_project_root(start: &Path, home: &Path) -> Option<PathBuf> {
    ...
    return Some(p.to_path_buf());   // borrowed &Path -> owning PathBuf
}
```

`p` is a borrowed reference. Returning it as-is would create a reference outliving its source, which trips the third rule in [[borrowing]] (`does not live long enough`). So we make a copy and hand over ownership with it.

## Why the arguments borrow

`fs::read_dir` accepts `&str`, `PathBuf`, and `&Path` alike, because it takes a generic argument bounded by the `AsRef<Path>` trait. Callers pass whatever they already hold, without converting first.

## The borrow that outlives everything: `&'static str`

```rust
fn reason(kind: io::ErrorKind) -> &'static str {
    match kind {
        io::ErrorKind::NotFound => "missing",
        ...
    }
}
```

A **string literal is compiled into the executable**, so it is alive for as long as the program is. `'static` is the name of that lifetime, and it is the honest return type for a function that only ever hands back literals.

Note what forces the annotation. Every other borrowed return in this codebase points at something an argument owns, so the compiler can work out how long it lasts. Here there is no reference among the arguments — `io::ErrorKind` is a plain value — so there is nothing to borrow *from* and the lifetime has to be named.

`-> String` would compile just as well and allocate a fresh copy, on every call, of text that never changes. Lifetimes in general arrived in M6 — [[lifetimes]]; this is the one case that paid for itself before then.

## Either one, decided at run time: `Cow`

Some text is sometimes borrowed and sometimes made. The Markdown renderer's rows are mostly pieces of the file, but a heading's `## ` or a list item's indent is text the renderer wrote itself. ratatui's `Span` holds either in one type:

```rust
// as it is for text; the real enum is generic over what it holds
enum Cow<'a, str> {
    Borrowed(&'a str),   // a window onto the file
    Owned(String),       // text made here
}
```

What is in each box decides what can be done with it. Cutting a piece in two, `markdown::part`:

```rust
fn part<'a>(text: &Cow<'a, str>, range: Range<usize>) -> Cow<'a, str> {
    match text {
        Cow::Borrowed(s) => Cow::Borrowed(&s[range]),
        Cow::Owned(s) => Cow::Owned(s[range].to_string()),
    }
}
```

A `Borrowed` box holds an arrow into the file, so a narrower arrow still points into the file and lives as long. An `Owned` box holds the letters themselves, so an arrow to them points into the box — and the box goes when the piece it belongs to goes. Written `Cow::Borrowed(&s[range])` in the `Owned` arm, it fails:

```
error[E0621]: explicit lifetime required in the type of `text`
    |         Cow::Owned(s) => Cow::Borrowed(&s[range]),
    |                          ^^^^^^^^^^^^^^^^^^^^^^^^ lifetime `'a` required
help: add explicit lifetime `'a` to the type of `text`
    | fn part<'a>(text: &'a Cow<'a, str>, range: Range<usize>) -> Cow<'a, str> {
```

Following the help compiles `part` and moves the failure to its callers — five errors there, E0597 once and E0515, *cannot return value referencing local data*, four times — because it asks the box itself to live as long as the file, which a piece being cut apart cannot promise. Only a few bytes are ever `Owned` — markers and indents — so copying them costs nothing worth counting.

The same picture answers what `clone()` and `&` do to a box:

```
the file "| key | value |"                        lives for 'a
   ▲
head's Cow::Borrowed(start = file + 2, len 3)      lives inside this function
   ▲
&span.content ─── an arrow to the box: gone when the function ends (E0597)

span.content.clone() ─── another box with the same arrow in it: points at the file
```

`clone()` makes one more of what is inside. Inside `Borrowed` is an arrow, so the clone is another arrow to the same place — measured, `heading.clone()` pointed at the same address as `heading`, still `Borrowed`. Inside `Owned` are letters, so the clone copies them.

## Pitfalls hit

**`Cow` took four wrong answers in a row**, each a different face of one question — what does this point at:

- `Cow::Borrowed(&s[range])` for the `Owned` arm (E0621 above).
- In review, the `Borrowed` arm was judged "risky too, only lucky so far".
- After the addresses were printed — the piece still pointing into the file once its span had been dropped — the reason given was "because it is a copy".
- In the next slice, `&span.content` to repeat a table's heading in every block: E0597. The compiler's own suggestion, `&*span.content`, was compiled too, and failed the same way.

Neither the printed addresses nor the 16 bytes of a `&str` settled it. The drawing of boxes and arrows did: *`&x` points at the box; `clone()` makes another box with the same thing inside.* Asked next what `&span.content` points at, the answer was the box in `head` — right.

## Related

[[ownership]] · [[borrowing]] · [[paths]] · [[result-and-errors]] · [[lifetimes]] · [[slices]]
