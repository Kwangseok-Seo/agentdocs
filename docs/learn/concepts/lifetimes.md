# lifetimes

A lifetime is **the stretch of code during which a borrow is valid**. It is bookkeeping the compiler keeps beside the types — not a value, not a clock. Nothing of it reaches the compiled program, so a mistake with one is always a refused build, never a crash at run time.

## What the bookkeeping prevents

```rust
let name;
{
    let text = String::from("dream");
    name = text.as_str();
}
println!("{name}");
```
```
error[E0597]: `text` does not live long enough
5 |         name = text.as_str();
  |                ^^^^ borrowed value does not live long enough
6 |     }
  |     - `text` dropped here while still borrowed
7 |     println!("{name}");
  |                ---- borrow later used here
```

`name` is a window onto `text`'s buffer ([[slices]]), and the buffer is freed at the `}`. In C this compiles and reads freed memory.

## A name says which input an output borrows from

```rust
fn longer(a: &str, b: &str) -> &str {
    if a.len() >= b.len() { a } else { b }
}
```
```
error[E0106]: missing lifetime specifier
  = help: this function's return type contains a borrowed value, but the signature
          does not say whether it is borrowed from `a` or `b`
```

`fn longer<'a>(a: &'a str, b: &'a str) -> &'a str` promises that the result lives no longer than both. Naming a lifetime lengthens nothing and changes nothing at run time; it connects an output to the inputs it may point into.

## Most are left out: three elision rules

1. Every reference among the inputs gets a lifetime of its own.
2. If there is exactly one input lifetime, every output gets it.
3. If one input is `&self` or `&mut self`, every output gets `self`'s.

Every function in this codebase that returns a borrow, against the rules:

| function | lifetimes among the inputs | rule | the output points into |
|---|---|---|---|
| `Entry::doc(&self) -> Option<&Path>` | `self` | 2 | the Entry |
| `App::walked(&self)`, `App::entry(&self)` | `self` | 2 | the App |
| `names(walked: &Walked) -> Vec<&str>` | `walked` | 2 | the Walked |
| `Entry::first_hit(&self, terms: &[String]) -> Option<(usize, &str)>` | `self`, `terms` | **3** | the Entry |
| `markdown::render(text: &str, width: u16) -> Vec<Line<'_>>` | `text` | 2 | the file |
| `listing::shown(text: &str) -> impl Iterator<Item = (&str, usize)>` | `text` | 2 | the file |
| `markdown::words(span: Span<'_>) -> Vec<Span<'_>>` | the one inside `Span` | 2 | the file |
| `markdown::part(text: &Cow<'a, str>, …) -> Cow<'a, str>` | the `&`, and the `'a` inside | none applies | named: the file, not the `Cow` |
| `Renderer::prefix(&self) -> (Vec<Span<'a>>, Vec<Span<'a>>)` | `self`, and the `'a` of `Renderer<'a>` | would be 3 | named: the file, not the Renderer |
| `markdown::wrap<'a>`, `whole_words<'a>`, `Rows::new`, `Rows::finish` | `'a`, declared on the function or on `Rows<'a>` | none needed | named: the file |
| `listing::reason(kind: io::ErrorKind) -> &'static str` | none | none applies | named: the binary |

`prefix` shows what a rule would have done. Written `Vec<Span<'_>>`, rule 3 ties its rows to `&self`, and the Renderer is then still lent out when it adds those rows to its own `lines` — 7 errors, among them `E0502: cannot borrow self.lines as mutable because it is also borrowed as immutable`. Naming `'a` says the rows point into the file, which outlives the Renderer.

`part` goes one step further. Its input carries two lifetimes — how long the `Cow` is lent for, and how long the text inside it lives — so no rule can choose, and the name ties the output to the text inside, which lets it outlive the `Cow` it was cut from. Why that holds for one variant and not the other is in [[owned-vs-borrowed-pairs]].

The rules read the signature, never the body:

```rust
fn first_term(&self, terms: &[String]) -> &str {
    &terms[0]
}
```
```
error: lifetime may not live long enough
  |     method was supposed to return data with lifetime `'2` but it is returning data with lifetime `'1`
```

Rule 3 tied the output to `self`, and the body, which returned something of `terms`, was held to that.

## A type that holds a borrow says so

```rust
struct Renderer<'a> {
    text: &'a str,                   // the whole file
    lines: Vec<Line<'a>>,            // the rows, made of the file's pieces
    links: Vec<Option<CowStr<'a>>>,  // an address is text in the file too
    styles: Vec<Style>,              // a colour borrows nothing
    …
}
```

`struct Renderer<'a>` declares a name, and each field that borrows uses it. `impl<'a> Renderer<'a>` does the same for a block of methods: the first `<'a>` declares, the second uses. Elision exists only in function signatures — a field has no inputs to infer from — so a borrowing field without a name is E0106, and a name given to a type that borrows nothing is E0107, *struct takes 0 lifetime arguments*.

A function that returns such a type may still leave the name out, but hiding it draws a warning:

```
warning: hiding a lifetime that's elided elsewhere is confusing
  | fn render(text: &str) -> Vec<Line> {
help: use `'_` for type paths
  | fn render(text: &str) -> Vec<Line<'_>> {
```

`'_` reads: fill this in by the rules, but show that a borrow is here.

## `'static`, and which way a promise goes

`'static` promises that a thing lasts until the program ends. Only what is compiled into the binary — a string literal — can keep that. A file's text is read at run time into an Entry's `String` and goes when the Entry goes. With `Renderer`'s `text` given `&'static str`:

```
error: lifetime may not live long enough
14 | pub fn render(text: &str) -> Vec<Line<'_>> {
   |               - let's call the lifetime of this reference `'1`
22 |         text,
   |         ^^^^ this usage requires that `'1` must outlive `'static`
```

The other direction is free. The bar in front of a quote, `"│ "`, goes into a row of the file's pieces as it is: still a borrow of the binary, nothing copied (measured: `bar is Borrowed: "│ ", no String made`). A longer promise covers a shorter one. A shorter one cannot stand in for a longer, and in a `Vec<Span<'static>>` a single piece of the file is enough to refuse the whole vector.

## In this codebase

`render` returns rows that all borrow from the file. Across this machine's 558 files, 126,552 of the 126,554 pieces of text the parser handed back were slices of the file, not copies. The price is that no row may outlive the Entry whose text it shows, and none does: the preview builds its rows, draws them and drops them within one frame.

## Pitfalls hit

- **A lifetime read as how long something runs.** In the first quiz the file's text in `Renderer` was given `&'static str`, and a lifetime mistake was predicted to be a panic at run time. The build was refused, with the error above. Asked again in another shape — which of a table's two new fields needs `'a`, and when a lifetime mistake shows — both answers were right.
- **Declared but not used.** Before the quizzes, `struct Renderer<'a>` got its name while the field stayed `Vec<Span>`: E0106 on the field, and the help said what to write, `Vec<Span<'a>>`.
- **`links` borrows too.** Only `text` was picked as borrowing from the file. The address in `[ADR-0001](docs/adr/0001.md)` is text in the file, lent as `CowStr<'a>`.
- **Which way a literal fits.** Putting `"│ "` among the file's pieces was predicted to make the compiler copy it — it does not. In the review, `Vec<Span<'static>>` holding the bar *and* a piece of the file was picked as compiling alongside `Vec<Span<'a>>`; only the second does.

## Related

[[borrowing]] · [[slices]] · [[owned-vs-borrowed-pairs]] · [[structs]] · [[drop-and-unwinding]]
