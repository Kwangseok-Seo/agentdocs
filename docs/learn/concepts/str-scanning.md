# str-scanning

Reading structured text out of a `&str` — the frontmatter parser is the whole of M2's text handling, and it needs five ideas.

| To do this | Use | Gives back |
|---|---|---|
| read a file into memory | `fs::read_to_string(path)` | `Result<String>` |
| walk it line by line | `text.lines()` | an iterator of `&str` |
| trim whitespace | `s.trim()`, `s.trim_end()` | `&str` |
| split at the **first** separator | `s.split_once(':')` | `Option<(&str, &str)>` |
| look at the next item without taking it | `iter.peekable()`, then `.peek()` | `Option<&Item>` |

`trim`, `split_once` and friends **allocate nothing**. They hand back a `&str` pointing into the original `String` ([[owned-vs-borrowed-pairs]]). Ownership is only taken at the end, with `.to_string()`, when the value has to outlive the text it came from.

## `lines()` already handles CRLF

```
LF   first line == "---" ? true
CRLF first line == "---" ? true
```

`lines()` splits on the newline and strips a trailing carriage return, so a Windows-authored file needs no special case. This is not hypothetical: one of the Lead files on this machine uses CRLF while the rest use LF.

## `split_once` rather than `split`

```
description: Args: (없음)=현재 프로젝트; 사용 시점: dream

split(':')  [1]  ->  Some(" Args")                       the value is chopped
split_once(':')  ->  Some(("description", " Args: ..."))  the value survives
```

Values contain colons. `split_once` cuts once, at the first separator, and returns the rest untouched — and because it returns an `Option`, a line with no colon at all falls out through `let ... else { continue }` ([[option-and-match]]).

## `peek` — looking without consuming

A YAML block value lives on the lines *after* its key:

```yaml
description: >
  first line of the block
  second line joined by a space
name: key-after-block
```

Reaching `description: >` means pulling in the indented lines that follow. The loop has to stop at `name:` **and leave that line unread**, so the outer loop can still parse it as a key. `next()` cannot do this — it consumes what it looks at. `peekable()` wraps the iterator in a `Peekable`, whose `peek()` returns a reference to the next item without advancing.

```rust
while let Some(next) = lines.peek() {
    if !next.starts_with(' ') && !next.trim().is_empty() {
        break;                                   // a new key, or the closing delimiter
    }
    let Some(piece) = lines.next() else { break };
    ...
}
```

Note what the stop condition is **not**: a list of the things that can end a block. "Neither indented nor blank" covers the closing delimiter, the next key and stray body text in one rule, and does not need extending when a new case turns up.

Note also `while let Some(line) = lines.next()` in the caller rather than `for line in lines`. `for` **moves** the iterator into the loop ([[vec]]), and the iterator has to stay reachable so it can be lent to the function that gathers the block.

## Characters and bytes are not the same count

Cutting a description to fit a terminal line is where this bites.

```
"세션 회고 — 직전 발행 rule"      18 characters / 36 bytes

s[.. 9]  on a character boundary? false
s[..10]  on a character boundary? true
s[..11]  on a character boundary? false
s[..12]  on a character boundary? false
```

```rust
&s[..10]   // "세션 회"  — happens to land on a boundary
&s[..11]   // panic: byte index 11 is not a char boundary;
           //        it is inside '고' (bytes 10..13)      exit 101
```

`&s[..n]` indexes **bytes**. A Korean character is three bytes in UTF-8, so roughly one offset in three is legal, and which ones depends entirely on the text. Counting characters instead removes the question:

```rust
fn short(s: &str, width: usize) -> String {
    let mut out: String = s.chars().take(width).collect();
    if s.chars().count() > width {
        out.push('…');
    }
    out
}
```

## A lowercase copy is not the same length

Case-insensitive search lowercases both sides, which quietly produces a second string — and a position found in that copy is a position in the copy, not in the original:

```
"İ": 1 char / 2 bytes  ->  "i\u{307}": 2 chars / 3 bytes
"A": 1 char / 1 bytes  ->  "a": 1 chars / 1 bytes
"Σ": 1 char / 2 bytes  ->  "σ": 1 chars / 2 bytes
"ΣΑΣ".to_lowercase()   ->  "σας"          the last Σ depends on its neighbours
```

The dotted capital I becomes an `i` and a combining dot: one character turns into two, and two bytes into three. So in M4's `around` a byte offset found by searching the lowercase line is walked back to the original **one character at a time**, adding up how long each character becomes once lowercased (`chars_before`). Summing per character is exact for byte lengths even with `Σ`, whose lowercase depends on context but is two bytes either way.

## Pitfalls hit — the demonstration that proved nothing

The first attempt to show this panic used `&s[..10]` on a Korean string, and it **printed happily**: byte 10 landed on a boundary by luck. Had the point been made from that run, the conclusion drawn would have been the opposite of the truth.

Which is the shape of the bug itself. `&s[..n]` is not reliably wrong — it is wrong for *some* strings at *some* offsets, so it survives every test written with English text and fails the first time a description arrives in another language. Four of the descriptions in this machine's corpus are Korean.

## Pitfalls hit — converting bytes to characters in the wrong string

M4's first `around` did convert its byte offsets to characters, as the section above demands — but it counted the characters of the **lowercase copy** and then skipped that many in the original. Every test passed, because in every test the two strings had the same length. A reviewer fed it forty `İ`s before the term: the window skipped past the end of a 48-character line and showed `…` with nothing after it, and with a different count it landed in filler text that did not contain the term at all — a wrong reason for a match, at exit 0. The byte-versus-character lesson had been learned; the assumption hiding one step further along had not been named.

## Related

[[owned-vs-borrowed-pairs]] · [[option-and-match]] · [[vec]] · [[macros-and-formatting]] · [[iterators]]
