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
| `Entry::doc(&self) -> Option<&Path>`, `Entry::supporting(&self) -> Vec<&Entry>` | `self` | 2 | the Entry |
| `Node::path(&self) -> &Path`, `Node::children(&self) -> Option<&[Node]>` | `self` | 2 | the Node |
| `Walked::entries(&self) -> Vec<&Entry>`, `Walked::unreadable(&self) -> Vec<(&Path, io::ErrorKind)>` | `self` | 2 | the Walked |
| `App::walked(&self)`, `App::entry(&self)`, `App::rows(&self) -> Vec<Row<'_>>`, `App::row(&self)`, `App::preview_lines(&self, width: u16) -> Vec<Line<'_>>` | `self` | 2 | the App |
| `names(walked: &Walked) -> Vec<&str>`, and in tests `inside(entry: &Entry)` and `hit(found: Option<Hit<'_>>)` | one | 2 | what was passed in |
| `Entry::first_hit(&self, terms: &[String]) -> Option<Hit<'_>>`, `Entry::own_hit(&self, terms: &[String]) -> Option<(usize, &str)>` | `self`, `terms` | **3** | the Entry |
| `gather<'a>`, `unread<'a>`, `visible<'a>` — they return nothing, and push borrows into an `out` argument | `'a`, declared on the function | none applies | named: the tree |
| `markdown::render(text: &str, width: u16) -> Vec<Line<'_>>` | `text` | 2 | the file |
| `listing::shown(text: &str) -> impl Iterator<Item = (&str, usize)>` | `text` | 2 | the file |
| `markdown::words(span: Span<'_>) -> Vec<Span<'_>>` | the one inside `Span` | 2 | the file |
| `markdown::part(text: &Cow<'a, str>, …) -> Cow<'a, str>` | the `&`, and the `'a` inside | none applies | named: the file, not the `Cow` |
| `Renderer::prefix(&self) -> (Vec<Span<'a>>, Vec<Span<'a>>)` | `self`, and the `'a` of `Renderer<'a>` | would be 3 | named: the file, not the Renderer |
| `markdown::wrap<'a>`, `whole_words<'a>`, `Rows::new`, `Rows::finish` | `'a`, declared on the function or on `Rows<'a>` | none needed | named: the file |
| `listing::reason(kind: io::ErrorKind) -> &'static str` | none | none applies | named: the binary |
| `highlight::for_language(language: &str) -> Option<HighlightLines<'static>>` | `language` | would be 2 | named: the theme, a `static` |

`prefix` shows what a rule would have done. Written `Vec<Span<'_>>`, rule 3 ties its rows to `&self`, and the Renderer is then still lent out when it adds those rows to its own `lines` — 7 errors, among them `E0502: cannot borrow self.lines as mutable because it is also borrowed as immutable`. Naming `'a` says the rows point into the file, which outlives the Renderer.

`for_language` is the same case the other way round. Written `HighlightLines<'_>`, rule 2 ties the highlighter to `language` — the word after a code fence — when what it holds an arrow to is the theme. The function still builds; its caller does not, since the word goes at the end of the `match` arm that read it and the highlighter is kept for the whole block: `E0597: info does not live long enough`. `'static` names the theme, which is a `static` and lasts as long as the program ([[statics]]).

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

## A borrow handed out through an argument

`gather` returns nothing. It pushes each Entry it finds into a list it was lent, and the borrows in that list point into the tree:

```rust
pub fn gather<'a>(nodes: &'a [Node], out: &mut Vec<&'a Entry>)
```

The rules only ever fill in *outputs*, and `out` is an input. Left out, rule 1 gives `nodes` and the Entries in `out` a lifetime each, and nothing says the second may point into the first:

```
error: lifetime may not live long enough
7 | fn gather(nodes: &[Node], out: &mut Vec<&Entry>) {
  |                  -                      - let's call the lifetime of this reference `'2`
  |                  let's call the lifetime of this reference `'1`
10|             Node::Entry(entry) => out.push(entry),
  |                                   ^^^^^^^^^^^^^^^ argument requires that `'1` must outlive `'2`
help: consider introducing a named lifetime parameter
7 | fn gather<'a>(nodes: &'a [Node], out: &mut Vec<&'a Entry>) {
```

One name on both says the Entries in `out` come from `nodes`. `unread` and `visible` are written the same way.

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

M7 added two more such types, both small. A `Hit<'a>` is the line that made a search keep an Entry: the line is a slice of a file's text, and `within: Option<&'a Entry>` says which supporting file it came from. A `Row<'a>` is one row of the Entries pane: a depth and a `&'a Node`. The screen's rows copy nothing out of the tree — they point into it, are made for one frame, and are gone before the tree could change.

M8's is a code block as the renderer reads it, and it holds two borrows of different lengths:

```rust
struct Code<'a> {
    highlighter: Option<HighlightLines<'static>>,  // an arrow to the theme, which never goes
    line: Cow<'a, str>,                            // the line so far, an arrow into the file
}
```

## `'static`, and which way a promise goes

`'static` promises that a thing lasts until the program ends. Only what is compiled into the binary can keep that — a string literal, or a `static`, whose place exists from the start of the run to its end ([[statics]]). A file's text is read at run time into an Entry's `String` and goes when the Entry goes. With `Renderer`'s `text` given `&'static str`:

```
error: lifetime may not live long enough
14 | pub fn render(text: &str) -> Vec<Line<'_>> {
   |               - let's call the lifetime of this reference `'1`
22 |         text,
   |         ^^^^ this usage requires that `'1` must outlive `'static`
```

A closure handed to another thread is asked for `'static` too — `thread::spawn` takes `F: FnOnce() -> T + Send + 'static` — since the thread may still be running when the function that made the closure has returned. A closure that borrows that function's variables cannot promise it; one that has taken them, with `move`, can ([[threads]]).

The other direction is free. The bar in front of a quote, `"│ "`, goes into a row of the file's pieces as it is: still a borrow of the binary, nothing copied (measured: `bar is Borrowed: "│ ", no String made`). A longer promise covers a shorter one. A shorter one cannot stand in for a longer, and in a `Vec<Span<'static>>` a single piece of the file is enough to refuse the whole vector.

## In this codebase

`render` returns rows that all borrow from the file. Across this machine's 558 files, 126,552 of the 126,554 pieces of text the parser handed back were slices of the file, not copies. The price is that no row may outlive the Entry whose text it shows, and none does: the preview builds its rows, draws them and drops them within one frame.

## Pitfalls hit

- **A lifetime read as how long something runs.** In the first quiz the file's text in `Renderer` was given `&'static str`, and a lifetime mistake was predicted to be a panic at run time. The build was refused, with the error above. Asked again in another shape — which of a table's two new fields needs `'a`, and when a lifetime mistake shows — both answers were right.
- **Declared but not used.** Before the quizzes, `struct Renderer<'a>` got its name while the field stayed `Vec<Span>`: E0106 on the field, and the help said what to write, `Vec<Span<'a>>`.
- **`links` borrows too.** Only `text` was picked as borrowing from the file. The address in `[ADR-0001](docs/adr/0001.md)` is text in the file, lent as `CowStr<'a>`.
- **Which way a literal fits.** Putting `"│ "` among the file's pieces was predicted to make the compiler copy it — it does not. In the review, `Vec<Span<'static>>` holding the bar *and* a piece of the file was picked as compiling alongside `Vec<Span<'a>>`; only the second does.
- **A visible `&` taken as a safe return (not settled).** In M7 the same wrong answer came four times in a row, among them: returning a slice kept in a local `Vec` picked as refused, and `&joined` — a `String` that `replace` made inside the function — picked as compiling. It is the other way round. What decides is where the arrow lands: `E0515: cannot return reference to local variable` when it points at something the function made and is about to drop, and no error when it points into what the caller lent — a `Vec` of slices is dropped as a box of arrows, and the text they point at lives on. A diagnostic question showed the criterion itself was missing, and a timeline of what `return` does, step by step, came after the fourth answer. The next question, `Node::path`, was answered right, but its wording said *arrow*; still open at the end of M7.

## Related

[[borrowing]] · [[slices]] · [[owned-vs-borrowed-pairs]] · [[structs]] · [[drop-and-unwinding]] · [[recursive-data]] · [[statics]] · [[threads]]
