# testing

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_directory_reads_as_missing() {
        assert_eq!(reason(io::ErrorKind::NotFound), "missing");
    }
}
```

Five things are happening in those six lines.

**`#[cfg(test)]` is conditional compilation.** The module does not exist during `cargo build` — it is compiled only by `cargo test`. Tests cost the shipped binary nothing, so there is no reason to keep them in a separate crate to "keep them out".

**`mod tests { }` is a module**, a namespace inside the file. A module does not automatically see its parent's items, hence **`use super::*`** — `super` is one level up, `*` is everything. Privacy is not in the way: an item private to the crate root is visible to its descendants, so tests reach `reason`, `md_files` and `Walked` without any of them being `pub`.

**`#[test]` marks a function the runner calls.** No arguments, no return value.

**A test fails by panicking.** `assert_eq!(a, b)` panics and prints both sides; `assert!(cond)` panics on false. Which means **`unwrap()` is correct inside a test** — the panic is the failure report. The same call that is a defect in the program is the right tool ten lines below it.

**The name is the report.** `cargo test` prints every name it runs, so they are written as sentences: `a_bundle_missing_its_lead_is_shown_rather_than_hidden`, not `test_bundle_2`.

## Fixtures on disk, without a crate

Walk functions need real directories. `std` is enough for that, as long as two tests never collide — the runner uses **parallel threads**.

```rust
fn scratch(name: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!("agentdocs-test-{}-{}", std::process::id(), name));
    let _ = fs::remove_dir_all(&dir);      // leftovers from a previous run
    fs::create_dir_all(&dir).unwrap();
    dir
}
```

The process id keeps two concurrent runs apart, the `name` keeps two tests apart, and clearing at the start (rather than at the end) survives a test that panicked before it could tidy up.

**Links can be built too**, which was assumed impossible for most of this milestone and is not:

```rust
#[cfg(windows)]
let made = std::os::windows::fs::symlink_dir(target, link);
#[cfg(unix)]
let made = std::os::unix::fs::symlink(target, link);
```

Windows grants that privilege to administrators and to accounts with Developer Mode enabled, so it can fail where the rest of the suite runs fine. A test that cannot build its fixture must not report success quietly: this one prints `SKIPPED` with the error kind to stderr and returns, and `cargo test -- --nocapture | grep -c SKIPPED` says whether that ever happened.

**A file or directory that refuses to be read** could not be built this way until M7. Removing your own read permission takes `icacls` on Windows and `chmod` on Unix, so from M3 the paths that report something unreadable had no automated test. On Windows there is another way: a handle can refuse to share. `testutil::hold` opens the path with `share_mode(0)` — and, for a directory, the flag without which one cannot be opened at all — and until that handle is dropped, anything else that opens the path gets *sharing violation*, os error 32, which the listing shows as `unreadable`. Since M7 those paths have tests on Windows; on other systems the same tests print `SKIPPED`.

## A green suite is not evidence

The interesting question about a test is not whether it passes. It is whether it can fail. Break the code on purpose and watch — nine mutations against 58 tests:

| mutation | tests red |
|---|---|
| `read_dir` failure swallowed again (the pre-M3 behaviour) | 3 |
| `short()` stops filtering control characters | 3 |
| an empty declared `name:` overwrites the filename anyway | 1 |
| `bundle_dirs` stops treating a link as Bundle-shaped | 2 |
| `md_files` goes back to requiring a plain file | 1 |
| `md_tree` asks about the target again, following links | 2 |
| the name column stops being filtered | 1 |
| a dangling link is dropped instead of listed | 1 |
| **`md_tree` stops counting an unreadable subdirectory** | **0** |

The last row is the finding, and it survived the milestone: nothing automated proves that a walk ever reaches `unreadable`. Read without that table, "58 passed" would have said "M3 is covered", and the one number this milestone invented would have been the one number nothing checked.

Five of those rows did not exist until a reviewer pointed out that **the entire link surface had no test** — the mutation reverting this milestone's own headline fix passed 48 of 48. The tests that now cover it were ruled out earlier on an assumption about privileges that turned out to be false.

## Pitfalls hit

- **`unwrap_err()` demands `Debug` from the *success* type.** `md_files(&dir).unwrap_err()` did not compile until `Walked`, `Entry` and `EntryKind` derived `Debug` — because to report an unexpected `Ok`, the macro has to print it. The requirement comes from the branch that is *not* supposed to happen.
- **The mutation harness failed silently and looked like a passing test.** The first attempt drove `sed -i 's|...|...|'` over a pattern containing `|`, so the substitution was rejected, the file was never modified, and the suite reported 48 passed. Read at face value that is "the suite has a second hole". A `diff` against the backup now runs before each mutation, and prints `!! NOT APPLIED` when nothing changed.
- **Tests can be written that pass on broken code.** The mutation table above is the only reason that claim is not being made here on faith; eight of the nine mutations were caught, one was not, and which was which was not predictable by reading.
- **A missing test is invisible to the tests.** The mutation battery only asks about behaviour somebody thought to mutate. The link surface had *no* coverage, and no amount of running the suite would have said so — it took a reviewer grepping for `symlink` and finding one occurrence, the production line itself.
- **Code that only `main` calls cannot be tested.** The control-character filter was correct and the name column still leaked, because the filtering happened in a `println!` argument inside `main`. Pulling the line into `fn row(entry: &Entry) -> String` — one function, no new behaviour — is the whole difference between a claim and a test.
- **A test that passes because the answer was already on screen.** M5's "clicking a scope heading selects nothing" first clicked the heading directly above the Source that was already selected, so a click that wrongly selected the row below still left the selection where the test expected it. A mutation passed it; moving each heading above a Source that was *not* selected made it red.
- **A test that feeds an input once, where the real thing feeds it many times.** M5's "a drag that began outside the preview selects nothing" sent one drag event and passed. A mutation that started a selection on the first drag event passed it too, since a real drag reports every cell it crosses and the test stopped after one. Sending two moves turned that mutation red.
- **A key whose meaning changed left two tests passing without testing what they are named for.** M8 made `Tab` go round three panes where it had gone between two. `a_row_stays_open_while_another_source_is_looked_at` pressed `Tab` twice to get back to the Sources, and now landed in the preview, where `j` and `k` scroll; the Source never changed, and the test still passed. Another tested that the tree's keys do nothing in the Sources pane from a fixture already in the Entries pane, so its one `Tab` went to the preview. Nothing was red. It came out by chance — a quiz option made one of them panic — and a mutation for each (another Source forgetting the open rows; `l` opening a row from the Sources) passed both until each test took the extra `Tab` and said which pane it was in. A mutation set written for the new code alone had not looked at them.
- **A line whose removal nobody could see.** Of slice 2's fourteen mutations, one that removed the line resetting the next scroll's time when the pointer came back inside the preview passed every test. No test could have caught it: with or without the line, the difference is under 30 ms and shows nowhere. The line was removed.
- **Mutations run against a copy whose own tests fail are all caught.** M8's review fixes added a test that failed on the fixed code — a click in the preview had handed it `k`, so `k` scrolled instead of moving up a row — and the first run reported twelve mutations of twelve caught, every one by that test. Nothing in the output looked wrong. The harness now runs the copy unmutated first, and stops if anything fails.
- **A guard tested on the input the author of the guard imagined.** Copying skipped a drag over blanks by checking `text.is_empty()`, and the test dragged within one blank row. The author dragged across two in a real terminal: the rows joined into `"\n"`, which is not empty, and "copied to clipboard" appeared. The rule had been written as *nothing but blanks* and coded as *the empty string*; `!text.trim().is_empty()` is the rule as written, and the two-row case is now in the test.

## A screen without a terminal

M5's screen is tested the way the listing is: by keeping judgement out of the parts that need the real thing. ratatui's `TestBackend` is a terminal made of memory, so a test can draw a frame at any size and read back every cell — its symbol, its colour, whether it is reversed. The loop hands keys and clicks to methods on `App` instead of acting on them itself, so a test can press `j` or click a cell without an event ever arriving. And the methods that depend on the time are handed the time, `release(now)` and `tick(now)`, so "two seconds later" is `start + COPIED_FOR` rather than a two-second wait — and eleven rows of a drag held below the preview are eleven calls to `tick`, 30 ms apart on paper, in no time at all. What stays outside — the loop's own wiring — is a few lines, checked in a real terminal.

## Related

[[result-and-errors]] · [[fs-read-dir]] · [[file-types-and-links]] · [[structs]] · [[event-loop]]
