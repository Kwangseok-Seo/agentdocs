# continuous-integration

A test suite proves something only about the machine it ran on. **Continuous integration** runs it somewhere else, every time the code changes: here GitHub Actions, on machines GitHub starts for each run and throws away after — **runners** — with Linux, macOS or Windows on them.

What runs is a **workflow**, a YAML file in `.github/workflows/`: on which event, on which runners, and the steps, each a shell command or an **action** — a step someone published, named by its repository. A **job** is the steps one runner takes; jobs run side by side unless one `needs` another.

```yaml
on:
  push:
    branches: [main]
  pull_request:

jobs:
  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
      - run: cargo clippy --all-targets --locked -- -D warnings
      - run: cargo test --locked
```

A **matrix** makes one job into several, one for each value: the same steps on three systems. `ci.yml` is that, and a fourth job that runs the tests on the oldest Rust `Cargo.toml` claims, `rust-version`, read out of the file rather than written a second time. The first run took 3 minutes on Linux, 1 on macOS, and 8¾ on Windows, all green.

## SKIPPED is a failure there

A test that cannot build its fixture — a link, a path it cannot read, a shell — says `SKIPPED` on stderr and passes, because on some machine that is the honest answer ([[testing]]). On these runners every fixture can be built, so `ci.yml` runs the tests with `--nocapture` and fails the job if `SKIPPED` appears: there it would mean a test had quietly stopped testing anything. The check was run against a log with the word and one without before it went in.

## The release workflow

`release.yml` runs on a tag, `v*`, as four jobs, each waiting on the one before:

```
build (×5 targets) ──▶ assemble ──▶ try-installers (×3 systems) ──▶ publish
```

`build` compiles each target and runs it ([[cross-compilation]]); `assemble` puts the archives, `SHA256SUMS` and the install scripts together; `try-installers` serves those files from the runner itself, in the layout GitHub serves a release in, and runs each install script against them with only the repository's address changed; `publish` makes the release, and is the only job allowed to write to the repository. A step that fails stops everything after it, so nothing reaches the release page that was not installed first.

## Pinning what runs

`actions/checkout@v7` names a tag, and a tag can be moved to other code by whoever owns the action. A commit cannot, so each action is named by commit, with its version in a comment. Every job starts with permission only to read; `publish` alone may write.

## What a run costs

On a private repository GitHub counts minutes, and a minute on Windows as two, on macOS as ten. One `ci.yml` run came to about 45 counted minutes; a release run, with two macOS builds and a macOS installer run, more. On a public repository none of it is counted.

## Pitfalls hit

- **A check stricter than what it checked.** The first release run, `v0.1.0-rc.1`, failed on Windows: the install script had installed and added its directory to `PATH`, and the workflow expected `PATH` to read `before;dir`. The runner's `PATH` had ended in `;`, and the script, which split it and joined it again, had dropped the empty entry. Nothing was published. The script now appends to the value as it is stored, and the check asks for the property — the old value, then the directory — rather than one spelling of it.
- **One failure cancelled another's answer.** A matrix stops its other jobs when one fails, by default, so the macOS installer run in that release was cancelled before it said anything. `try-installers` has `fail-fast: false`.

## Related

[[testing]] · [[cross-compilation]] · [[release-profiles]] · [[platform-differences]]
