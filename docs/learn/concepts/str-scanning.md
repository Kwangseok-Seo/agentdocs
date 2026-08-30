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

## Pitfalls hit — the demonstration that proved nothing

The first attempt to show this panic used `&s[..10]` on a Korean string, and it **printed happily**: byte 10 landed on a boundary by luck. Had the point been made from that run, the conclusion drawn would have been the opposite of the truth.

Which is the shape of the bug itself. `&s[..n]` is not reliably wrong — it is wrong for *some* strings at *some* offsets, so it survives every test written with English text and fails the first time a description arrives in another language. Four of the descriptions in this machine's corpus are Korean.

## Related

[[owned-vs-borrowed-pairs]] · [[option-and-match]] · [[vec]] · [[macros-and-formatting]]
