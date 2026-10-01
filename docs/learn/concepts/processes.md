# processes

A process is another program, running on its own. It shares no memory with this one, the system starts it, and when it ends it hands back one number, its **exit code**. M9's `e` runs one — the editor — and what this program can do with it comes to three things: say what to run with which arguments, wait for it, and read how it ended.

## `Command`: put together, then run one of three ways

```rust
use std::process::Command;

let status = Command::new("edit")
    .arg(path)              // one argument, handed over whole — never split
    .env("NAME", "value")   // seen by the child only
    .status();              // run it, and wait until it ends
```

| Method | Waits | The child's keyboard and screen | Returns |
|---|---|---|---|
| `.status()` | until it ends | **ours, handed over** | `io::Result<ExitStatus>` |
| `.output()` | until it ends | captured and handed back | `io::Result<Output>` |
| `.spawn()` | no | ours | `io::Result<Child>`, to `.wait()` on later |

An editor needs `.status()`. Tried with Windows 11's `edit`: under `.output()` it ended after 11 ms with `Error 0x80070006` — the handle is invalid — having no keyboard or screen to talk to; under `.spawn()` the call came back after 4 ms, with the editor still running and the screen about to be drawn over it. While `.status()` waits, this program waits on that line, and its loop does not turn.

## Two ways to fail, and one that is not a failure

`.status()` returns `Err(e)` when the program could not be started at all — `NotFound`, when there is no such program. A program that started has ended somehow, and an exit code other than 0 is **not** an `Err`: it is `Ok(status)` with `status.success()` false. `?` lets it through ([[result-and-errors]]). `status.code()` is an `Option<i32>`, since a Unix program killed by a signal has no code.

```
Command::new("agentdocs-no-such-editor").status()     -> Err(NotFound)
cmd /c  "agentdocs-no-such-editor …"   .status()       -> Ok(exit code: 1)
```

## A program is not a line typed at a shell

`Command::new` asks the system to start a program, and no shell reads anything on the way. What a shell would have done is not done:

```
Command::new("code")        -> Err(NotFound)     — though `code` works at a PowerShell prompt
Command::new("code.cmd")    -> exit 0
Command::new("edit")        -> exit 0
Command::new("code --wait") -> Err(NotFound)     — looked for as one name
```

On Windows, `std` adds only `.exe` to a name that has no extension, and VS Code's `code` is a batch file, `code.cmd`; a shell tries each extension in `PATHEXT`. Splitting `code --wait` into a name and a word is a shell's work too: `.arg()` hands over what it is given, whole.

`$EDITOR` is written for a shell to read, so agentdocs hands it to one — and keeps the file's path out of the line the shell reads ([ADR-0010](../../adr/0010-the-editor-runs-through-the-shell.md)). cmd is given `"<EDITOR> "%AGENTDOCS_FILE%""` with the path in that variable, which it expands once and does not read again; sh is given `<EDITOR> "$1"`, with the path as `$1`. With the path on cmd's line, a file named `100%PATH%.md` reached the editor with the machine's `PATH` inside its name, and cmd exited 0. cmd's own quoting is not what `.arg()` produces, so the line goes in through `raw_arg`, which only Windows has, from `std::os::windows::process::CommandExt`.

## Environment variables are `OsString`s

`env::var_os("EDITOR")` returns `Option<OsString>`. What the system holds need not be UTF-8, for the same reason a path is not a `String` ([[paths]]); `env::var` returns `Err` for a value that is not. `chosen` is handed the function that looks a name up instead of calling `env::var_os` itself, so its tests can give it a pretend environment ([[closures]]).

## Code for one platform: `cfg`

```rust
#[cfg(windows)]
const FALLBACK: &str = "notepad";
#[cfg(not(windows))]
const FALLBACK: &str = "vi";

#[cfg(any(not(windows), test))]
fn through_sh(editor: &OsStr, path: &Path) -> Command { … }
```

`#[cfg(…)]` keeps an item, or a statement, only where its condition holds; elsewhere it is not compiled at all, and the compiler checks nothing in it. `through_sh` is compiled everywhere but Windows — and on Windows for the tests, which run it with Git's `sh`. That is also all it has been run with: no Unix machine was at hand in M9.

## The terminal belongs to one program at a time

The screen keeps the terminal in raw mode, on the alternate screen, and reporting the mouse ([[event-loop]]). An editor in the same terminal sets those as it needs them, so the screen puts all three back first and sets them up again afterwards, each time in the order `open` uses. Then it draws everything afresh with `terminal.clear()`: ratatui sends only the cells it thinks changed, and after the editor it would think none had — 25 bytes, where a first frame sends 1,306, to a screen that came back blank.

## Pitfalls hit

- **Three layers taken for two.** `edit()` returns `io::Result<io::Result<ExitStatus>>`. The outer `Err` is the terminal not handed over or not taken back, and ends the screen; the inner one is an editor that could not be started; and an `ExitStatus` that is not a success is neither. The explanation before the quiz had called `.status()`'s two layers *outer* and *inner*, `edit()` wraps one more round them, and the answer given put those two words on the wrong pair. Settled in slice 1, once `?` was written out as `match … Err(e) => return Err(e)` ([[result-and-errors]]).

## Related

[[result-and-errors]] · [[event-loop]] · [[paths]] · [[closures]] · [[threads]]
