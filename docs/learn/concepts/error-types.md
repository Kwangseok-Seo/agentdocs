# error-types

Until M10 every failure in the program was an `io::Error`. Reading a config file can fail two ways — the file will not read, or it reads and is not a config file — and the second is a `toml::de::Error`. Two error types that have to travel one channel, a function's `Result`, need a type of their own that holds either ([[result-and-errors]]):

```rust
pub enum Problem {
    Read(io::Error),          // it would not open, or would not read as text
    Parse(toml::de::Error),   // it was read, and does not hold what a config file holds
}
```

It is M9's `Message` again: one type for two kinds of thing ([[enums-and-data]]).

## By hand: a variant is a function

Slice 1 turned each error into a `Problem` where it arose. `map_err(f)` leaves an `Ok` alone and turns `Err(e)` into `Err(f(e))`, and a variant that holds data is itself a function from that data to the enum — `Some` is one from a value to `Some(value)`:

```
[1, 2].map(Some)                          = [Some(1), Some(2)]
"7".parse::<i32>().map_err(Wrapped::Bad)  = Ok(7)
"x".parse::<i32>().map_err(Wrapped::Bad)  = Err(Bad(ParseIntError { kind: InvalidDigit }))
```

```rust
let file: File = toml::from_str(&text).map_err(Problem::Parse)?;
```

Handed over by its name, without parentheses, as M8 and M9 handed functions over ([[closures]]). `Problem::Parse()` is a call with nothing in it — E0061, *this enum variant takes 1 argument but 0 arguments were supplied* — and `|e| Problem::Read(e)` puts a `toml::de::Error` where an `io::Error` goes — E0308.

## What makes a type an error

`std::error::Error` is a trait, and asks for two others: `Debug`, how a programmer sees the value, and `Display`, the words a person reads. Its own method, `source()`, says which error, if any, caused this one. And `?` turns one error type into another through `From` — `return Err(From::from(e))` — so `impl From<io::Error> for Problem` is what lets a bare `?` turn an `io::Error` into a `Problem`.

## thiserror writes them

Those three `impl`s follow from the enum, so a derive writes them ([[traits]]):

```rust
#[derive(Debug, thiserror::Error)]
pub enum Problem {
    #[error(transparent)]
    Read(#[from] io::Error),
    #[error(transparent)]
    Parse(#[from] toml::de::Error),
    #[error("`order` names `{0}`, and no Source here is called that")]
    Order(String),
}
```

- `#[error("…")]` is the variant's `Display`; `{0}` is what it holds.
- `#[error(transparent)]` hands the held error's words and its `source()` through unchanged: the parser's message *is* the message.
- `#[from]` writes the `From`, and with it `map_err` goes:

```rust
let text = match fs::read_to_string(dir.join(FILE)) {
    Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
    read => read?,
};
let file: File = toml::from_str(&text)?;
```

A file that is not there is not a problem, so that one `Err` is taken out first; `read?` hands back the text, or turns any other `io::Error` into `Problem::Read`.

`Order`, the third variant, came with slice 3 and holds neither of the other two. `listing::unused` matched on the variants to choose the row's word, and the build stopped there until the new one had a word too:

```
error[E0004]: non-exhaustive patterns: `&config::Problem::Order(_)` not covered
```

That was then the one place in the program that had to know — the compiler found it, as a `match` is checked and an `if let` is not ([[option-and-match]]). Two more `match`es name every variant now — what is printed under the row, and what the preview shows — since a file that will not read stopped being shown in the system's words, below; a fourth variant would stop the build at all three.

## Whose words

A parse error says itself what went wrong, in the crate's own words, the same on every machine: those are printed under the file's row. A file that will not read says it in the system's words, in the machine's language, and so is shown as everything else that would not read is, by a word of the program's own — `(unreadable)`, `(permission denied)` ([[result-and-errors]]). `Read` is still `transparent`: its `Display` is the system's, kept for whatever prints the error itself, and the screen and the listing do not.

## `From` and `Into`

Each `impl From<A> for B` also gives `Into<B>` for `A`, free. `Source::new` takes its name as `impl Into<String>`: a `&str` from the built-in table is copied once, and a `String` read out of a config file is moved in as it is — where `name: &str` had copied that `String` again, as M2's log foresaw.

## Related

[[result-and-errors]] · [[enums-and-data]] · [[traits]] · [[closures]] · [[serde]] · [[toml]]
