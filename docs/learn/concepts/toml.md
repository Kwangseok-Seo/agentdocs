# toml

TOML is the format of `Cargo.toml`, and of agentdocs' config files. It is read here by the `toml` crate, into types that [[serde]] fills.

- `key = "value"` is one field. Strings are quoted; `true`, numbers and `[ … ]` arrays are not. `#` starts a comment that runs to the end of the line.
- `[name]` starts a **table**: the lines below it, up to the next header, are its fields.
- `[[name]]` adds a table to an **array** of tables called `name`. Written twice, there are two. This is what a `Vec` of structs is read from.
- The same array can be written on one line: `source = [ { name = "wiki", … }, { … } ]`. Read into `File`, the two spellings gave the same value.

```toml
order = ["Wiki"]

[[source]]
name = "Sessions"
path = "Sessions"
walk = "markdown-tree"

[[source]]
name = "Wiki"
path = "Wiki"
walk = "markdown-files"
```

## A key belongs to the header above it

Every key after a header belongs to that table, until the next header. `order` written below a `[[source]]` is not the file's `order` but a field of that Source, which has none of that name:

```
unknown field `order`, expected one of `name`, `path`, `walk`
```

So `order` comes first, before any table. The README says so, and a test holds the message.

## One table is not an array of them

`File.source` is a `Vec`. `[source]`, one table, and `source = { … }`, one inline table, are both maps where a sequence is wanted:

```
TOML parse error at line 1, column 1
  |
1 | [source]
  | ^^^^^^^^
invalid type: map, expected a sequence
```

## What an error carries

Every error the crate gives has the line and column it stopped at and the line itself, and its `Display` draws them, as above. agentdocs prints those words as they are rather than counting lines itself, which could disagree with the parser's count. A file saved with a byte-order mark before its first line, or with `\r\n` line ends, read as it would without — both were tried.

## Related

[[serde]] · [[error-types]] · [[external-crates]] · [[str-scanning]]
