# serde

serde turns text into Rust values, and values into text, without either side knowing the other. The work is split three ways:

```
.agentdocs.toml (text)
      │  the toml crate knows TOML: [[…]], "…", # comments          — the format
      ▼
"an array called source, holding two tables, each with name, path and walk"
      │  serde: the traits the two sides meet at
      │  #[derive(Deserialize)]: our side of them, written by the compiler
      ▼
File { source: [Row, Row] }                                         — the shape
```

The format is a crate per format — `toml`, `serde_json`, and so on — and the shape is our types. `#[derive(Deserialize)]` is an `impl` the compiler writes, as `derive` always is ([[traits]]): it walks the type's fields and asks the format for each by name. Which type to make is said where the value lands:

```rust
let file: File = toml::from_str(&text)?;
```

Run on two `[[source]]` tables ([[toml]]), with `Debug` derived for the printing:

```
=> Ok(File { source: [Row { name: "wiki", path: "Wiki", walk: MarkdownFiles },
                      Row { name: "sessions", path: "Sessions", walk: MarkdownTree }] })
```

`path` is a `PathBuf` and `walk` an enum, and neither needed anything written for it: serde already knows how to make a `PathBuf` from a string, and a derived enum takes the variant's name.

## The names come from the enum

`#[serde(rename_all = "kebab-case")]` on `Walk` says how a variant's name is written in the file: lower case, words joined by `-`. `MarkdownTree` is `markdown-tree`. Without it, or with another case, the words change — and the error says which words it would take:

```
walk = "markdown-tree"   no rename_all              => unknown variant `markdown-tree`, expected one of `MarkdownFiles`, `BundleDirs`, `MarkdownTree`
walk = "markdown-tree"   rename_all = "snake_case"  => unknown variant `markdown-tree`, expected one of `markdown_files`, `bundle_dirs`, `markdown_tree`
```

So the list of words a config file may use is not written down anywhere but in the enum. A variant added — `MarkdownFilesDeep`, in a copy — was taken as `markdown-files-deep` with no other line changed, and the error for a wrong word listed four. A list kept by hand beside the enum would have been one more place to forget.

## A field that may be missing, and one that should not be there

- **Missing.** A field the text does not have is an error — `missing field `walk``. A field that may be left out takes `#[serde(default)]`, and then its type's default: an empty `Vec` for `source`, so a file holding only a comment adds nothing. Without the attribute that file was refused with `missing field `source``.
- **Not there to be had.** By default a key the type has no field for is **passed over without a word**. `#[serde(deny_unknown_fields)]` makes it an error:

  ```
  hidden = true, no deny_unknown_fields  => Ok(… { name: "wiki", path: "Wiki", walk: MarkdownFiles } …)   — hidden is gone
  hidden = true, deny_unknown_fields     => unknown field `hidden`, expected one of `name`, `path`, `walk`
  ```

  On `File` this is what catches `[[sources]]`, one letter too many: passed over, the file would add no Sources and say nothing, exit 0 — run in a copy without the attribute, that is exactly what it did. The expected names come from the fields too: when M10 gave `File` a second field, `order`, the same mistake's message became `expected `order` or `source``, and the test that held the old words failed until it was told the new ones.

## Related

[[toml]] · [[traits]] · [[error-types]] · [[external-crates]] · [[enums-and-data]]
