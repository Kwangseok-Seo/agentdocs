# closures

A closure is a function written where it is used, without a name: `|arguments| expression`, or `|arguments| { statements; final expression }`. The argument types can almost always be left out, because whatever receives the closure says what it will be handed.

```rust
fn not_control(c: &char) -> bool { !c.is_control() }

"a\tb".chars().filter(not_control).collect()          // "ab"
"a\tb".chars().filter(|c| !c.is_control()).collect()  // "ab" — printable's actual form
```

A named function is accepted in the same place. So why the closure?

## The one thing a function cannot do

A closure can use the variables around it. A function cannot, even one declared inside another function:

```rust
let text = String::from("adr notes");
fn has(t: &str) -> bool { text.contains(t) }
```
```
error[E0434]: can't capture dynamic environment in a fn item
  |         text.contains(t)
  |         ^^^^
  = help: use the `|| { ... }` closure form instead
```

A nested `fn` is still a free-standing item; the local `text` does not exist for it. This is also why `Entry::matches` can be one line — the closure handed to `all` uses `name` and `text` from the method body without taking them as arguments:

```rust
terms.iter().all(|t| text.contains(t) || name.contains(t))
```

## How a closure holds what it uses

A useful and accurate picture: **a closure is an unnamed struct whose fields are the things it captured**, with a method to call it. The compiler chooses each field's kind from the body, and always the weakest that works.

| The body… | Captured as | Seen in the demo |
|---|---|---|
| only reads it | `&` borrow | `text` — readable again after the closure ran |
| changes it | `&mut` borrow | `checked += 1` — readable again after the closure ran |
| takes it | by value (a move) | `move` forces it, and a closure run on another thread needs it ([[threads]]) |

Because these are borrows, [[borrowing]]'s rules apply to them for as long as the closure is alive:

```rust
let mut checked = 0;
let mut check = |t: &str| { checked += 1; text.contains(t) };
println!("{checked}");
check("adr");
```
```
error[E0502]: cannot borrow `checked` as immutable because it is also borrowed as mutable
  |     let mut check = |t: &str| { checked += 1; text.contains(t) };
  |                     ---------   ------- first borrow occurs due to use of `checked` in closure
  |                     |
  |                     mutable borrow occurs here
  |     println!("{checked}");
  |                ^^^^^^^ immutable borrow occurs here
  |     check("adr");
  |     ----- mutable borrow later used here
```

Swap the last two lines and it compiles and prints `1`: the borrow ends at the closure's last use, the same early-ending borrow as in [[impl-and-methods]].

## Three capture kinds, three traits

| Trait | The closure… | Can be called |
|---|---|---|
| `Fn` | only reads what it captured | any number of times |
| `FnMut` | changes what it captured | any number of times, one at a time |
| `FnOnce` | gives away what it captured | once |

Functions that take a closure name the weakest kind they can live with. `filter`, `all` and `any` take `FnMut`, which is why a closure that counts its own calls could be handed to `all`. How a function says "any type, as long as it is `FnMut`" is [[traits]].

## Not only for iterators

`fs::metadata(&path).map(|m| m.is_dir())` in `bundle_dirs` hands a closure to `Result::map`, and `first_hit` hands one to `Option::map`. A closure is just a value you can pass; [[iterators]] are where it shows up most.

## A function's name, without the parentheses

Where a closure is taken, a named function can go instead, written without its parentheses:

```rust
static THEME: LazyLock<Theme> = LazyLock::new(theme);             // not theme()
info.split_whitespace().next().and_then(highlight::for_language)   // not for_language(…)
```

`theme()` calls the function on that line and hands over what it returns. `theme` is the function itself, handed over for the receiver to call when it chooses — `LazyLock` the first time the theme is read, `and_then` only if there is a word to look up. `LazyLock::new(theme)` and `LazyLock::new(|| theme())` do the same thing.

M9 hands over two more: `chosen(env::var_os)`, which calls it once for `VISUAL` and, only if that finds nothing, again for `EDITOR` ([[processes]]); and `read_input(event::read, …)`, which calls it once for every event, on another thread ([[threads]]). When the parentheses are written is when it runs, which a function that prints shows:

```
① let f = fake_read;
② let n = fake_read();
    -> fake_read runs
③ twice(fake_read)
  inside twice
    -> fake_read runs
    -> fake_read runs
```

## Taking a closure, and handing one back

A parameter can be any closure of a kind, written `impl` and the trait ([[traits]]): `read: impl FnMut() -> io::Result<Event>` in `read_input`, `lookup: impl Fn(&'static str) -> Option<OsString>` in `chosen`. So can a return type. `tell` builds the closure notify will call and hands it back, with the promises notify asks for added — that it can go to another thread, and borrows nothing:

```rust
fn tell(to_loop: Sender<Message>) -> impl FnMut(notify::Result<notify::Event>) + Send + 'static {
    move |event| { … }
}
```

A test can call what `tell` returns the way notify would, with a report made up for the purpose, and see what the loop is told ([[channels]]).

## Pitfalls hit

- **Calling and handing over told apart by the parentheses.** In M8, `static START: Instant = Instant::now();` was picked as compiling and `static START: LazyLock<Instant> = LazyLock::new(Instant::now);` as not. The first calls `now` while the program is compiled, which cannot be done (E0015); the second hands `now` over, and compiles. See [[statics]]. In M9's slice 1 it came back as `chosen(env::var_os())`, picked as the one form that compiles: E0061 — the call is missing its argument — and E0277, since what it would return is not something to call. Two questions found how the parentheses were being read: `env::var_os` as the function itself, which is right, and `env::var_os()` as a way of writing that something is a function, as documentation does. Then a function that prints showed when each runs, and in slice 2 `read_input(event::read, …)` was picked over `event::read()` at the first attempt. Settled.
- **An argument is worked out before the call, whether it is used or not (not settled).** With `VISUAL` set, so that no default is needed, `Some(…).unwrap_or(fallback())` and `.unwrap_or_else(fallback)` were both picked as never running `fallback`. The first runs it — its argument is worked out on that line, before `unwrap_or` is called, though `unwrap_or` then throws it away; only the second hands `fallback` over to be called if it is needed. It has not been asked again: M10 ran without quizzes, at the author's request.
- **Moving a captured value out of a closure called more than once.** M10's `config::add` makes a Source of each row in `.map(|row| … Source::new(row.name, path, scope, row.walk))`, and `scope` came from outside. `map` calls its closure once a row, so the closure is `FnMut`, and handing `scope` to `Source::new` by value would move it out on the first call, leaving nothing for the second — the assistant's first build said so:

  ```
  error[E0507]: cannot move out of `scope`, a captured variable in an `FnMut` closure
  note: if `source::Scope` implemented `Clone`, you could clone the value
  ```

  `Scope` is a tag with no data, so it now derives `Clone, Copy`, and each call copies it. Deriving only `Clone`, or writing `move |row|`, gave the same E0507; `&scope` gave E0308, since `Source::new` wants a `Scope`.

## Related

[[iterators]] · [[traits]] · [[borrowing]] · [[impl-and-methods]] · [[statics]] · [[threads]] · [[channels]] · [[processes]]
