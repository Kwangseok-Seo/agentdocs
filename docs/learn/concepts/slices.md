# slices

A slice is **a window onto part of something another value owns**: where it starts, and how long it is. A `&str` is those two numbers, 16 bytes on this machine, whatever the length of the text it shows. Taking one copies nothing; the window is only valid while the owner keeps the text where it is ([[lifetimes]]). `&[T]` is the same thing for a `Vec<T>` ([[vec]]).

## One window, one run

A window shows one stretch that runs without a gap. That decides whether a function can hand back a slice or must build something new:

| | what is left | returns |
|---|---|---|
| `"  x  ".trim()` | one run — the ends are gone | `&str` |
| `line.split_once(':')` | two runs, each one window | `Option<(&str, &str)>` |
| `&text[4..9]` | one run | `&str` |
| `"left\u{200b}right"` without the U+200B | two runs with a gap between | `String` |

The last is `listing::printable`, which drops what a terminal should not be given. Its pieces are windows onto the name:

```
text  = "cafe\u{301}\u{202e}x"      byte:  0  1  2  3  4 5  6 7 8  9
                                          [c][a][f][e ◌́][ RLO ][x]
shown(text)   "c" ──→ byte 0, 1 long        windows onto text, nothing new
              "e◌́" ─→ byte 3, 3 long
              "x" ──→ byte 9, 1 long
printable     a new String: c a f e ◌́ x    the gap at 6–8 means no single window can
                                           show 0–5 and 9 together: they are copied
```

Asked to build a `&str` from those pieces anyway, the compiler says exactly that:

```
error[E0277]: a value of type `&str` cannot be built from an iterator over elements of type `&str`
30 |     shown(s).map(|(g, _)| g).collect()
```

## A window outlives nothing

```rust
let first;
{
    let text = String::from("cafe\u{301}x");
    first = shown(&text).next().unwrap().0;
}   // text is dropped here
println!("first = {first:?}");
```
```
error[E0597]: `text` does not live long enough
```

With `.to_string()` after `.0`, the same code runs and prints `first = "c"`: a copy is a value of its own and needs nothing to stay.

## Ranges

`a..b` is from `a` up to but not including `b`; `a..=b` includes `b`; `..b` starts at the beginning and `a..` runs to the end. Two windows meet without a gap only if the second starts where the first stopped:

```
line = "key:value"      the colon is byte 3

&line[..3] + &line[4..]      "key" + "value"   the colon is in neither
&line[..=3] + &line[4..]     "key:" + "value"  every byte in one or the other
```

## Three kinds of position

```
text:             9       한          글
byte:             0       1  2  3     4  5  6     where a window may start or end
char_indices:     (0,'9') (1,'한')    (4,'글')    each character with its first byte
columns:          1       2           2           what a terminal gives it
```

A window is cut in **bytes**, and must be cut where a character begins — [[str-scanning]] shows the panic when it is not. A row is measured in **columns**. M6 met all three:

- pulldown-cmark cuts only beside ASCII marks — `#`, `*`, `` ` ``, a line break — and an ASCII byte always stands between two characters. Of 352,874 byte ranges it handed back for this machine's files, none began or ended inside one.
- The listing had counted **characters** to fit a description into 44 columns, and a Korean character takes two: the widest row in `cli-maker` was 108 columns, now 82. It now counts columns, one **grapheme** at a time — a character together with whatever joins it, such as an accent or the mark that makes `⚠` an emoji.
- The renderer cut a word too wide for its row one character at a time, then measured the row as a whole string. The two disagree on a grapheme: `⚠️` is two characters, one column counted singly and two as a string, so the row passed its edge; a flag is two characters and could be cut between them. Rows are now filled a grapheme at a time, and each grapheme is measured as the string it is.

## Pitfalls hit

- **The line break belongs to the line before it.** To find where the line holding byte `start` begins, `text[..start].find('\n')` was chosen over `rfind` (the first line break in the file, not the nearest), and `i` over `i + 1` (the break itself, not the character after it). The review — `text[..8].rfind('\n')` in `"one\ntwo\nthree"` — was right: `Some(7)`.
- **`"".strip_suffix('\n')` was predicted to be `Some("")`.** It is `None`: there is nothing to strip, and "nothing to strip" is what `None` says.
- **`..colon` leaves the colon out.** Cutting a line into `&line[..colon]` and `&line[colon + 1..]` loses the byte between them — a test printed `name a` where the file said `name: a`. The review, the two halves of `"key=value"` joined, was answered `"keyvalue"`: right.
- **A slice taken for a copy.** Asked for the type of `shown`'s pieces, `String` was chosen (E0271: *expected `(String, usize)`, found `(&str, usize)`*); asked why `printable` cannot return `&str`, "a `&str` cannot leave a function" — though `shown` itself returns them — and asked where the piece `"c"` points, "a new `String` on the heap". Three answers, one picture missing. The drawing above, with the bytes numbered and an arrow from each piece into the text, is what answered it: in review, where `split_once`'s `key` points, and which of four results cannot be a `&str`, were both right.

## Related

[[str-scanning]] · [[vec]] · [[lifetimes]] · [[owned-vs-borrowed-pairs]] · [[iterators]]
