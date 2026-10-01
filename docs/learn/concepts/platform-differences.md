# platform-differences

The same source compiled for another system is a program that meets another system. The standard library smooths over most of the differences and passes some straight through. M11 ran the suite on Linux, in a container on this machine, and on macOS and Windows on GitHub's ([[continuous-integration]]); each row below was found that way, and none of them had shown on Windows in ten milestones.

| | Windows | Linux, macOS |
|---|---|---|
| the order `read_dir` gives | NTFS: by name | ext4: by hash — Rust promises no order at all |
| a path its owner cannot read, made for a test | a handle held that refuses to share: `unreadable` | the permissions taken away: `permission denied` — and root reads it anyway |
| a directory that will not be listed | the files in it can still be opened by name | without its search permission, they cannot even be looked up |
| `env::current_dir()` | the path as it was given | every link in it resolved |
| `env::home_dir()` | reads `USERPROFILE` | reads `HOME` |
| making a watcher | — | Linux: fails once the user holds the system's limit of inotify instances — 8192 in the container, of which a probe could take 8190 |
| watching one more directory | — | Linux: refused once the user holds the system's limit of inotify watches. macOS watches through FSEvents, which was not tried at either limit |

## The order a directory is read in

`read_dir` hands over entries in whatever order the file system keeps them ([[fs-read-dir]]). NTFS keeps them by name, so every listing here came out alphabetical, and one test assumed it without saying so. On ext4 it failed: `long.md` came before `a/`. Each Walk now sorts each directory's rows as a file manager does — ignoring case, and reading a run of digits as a number, so that `M2` comes before `M10` — with `sort_by` and a comparison written for it, which reads the digits with a `Peekable` ([[iterators]]). Here the listing came out byte for byte the same in 69 of 75 runs; in the other six, `M10` moved after `M9`.

## A link the system resolves for you

On Unix the current directory is reported with every link in it followed. With `HOME` written through a link — FreeBSD's `/home` is one — the current directory is never below home *as written*: `starts_with` compares the parts of a path, not where they lead ([[paths]]). Every project was `(outside any project)`, exit 0. `fs::canonicalize` resolves the home as well, and the project root is looked for below that when the first look finds nothing. macOS's temporary directory is under such a link, `/var` to `/private/var`, which is where it was met: the integration tests' home had to be resolved before any test could find a project in it on macOS ([[testing]]).

## Pitfalls hit

- **A test that had only ever run on one file system.** `a_file_keeps_where_it_was_left_when_rows_open_above_it` opened `a/` above `long.md` — the first two rows on NTFS, not on ext4. Every other test of order already sorted what it compared, saying why; this one had nothing to sort, and read the screen.
- **A fixture that made a different situation on each system.** On Windows a held directory cannot be listed, but its `SKILL.md` can still be read; with its permissions gone on Unix, it can be neither. Three Bundle tests that needed the Lead readable failed on Linux. The Unix hold now leaves a directory the one permission that lets a name inside it be looked up, which is the Windows situation; the situation Windows cannot make — a directory shut entirely — got a test of its own, and that test found the next item.
- **A wrong answer only Unix could give.** `Path::is_file` answers `false` for a path it was not allowed to look at, just as for one that is not there. A Bundle whose directory was shut was listed as having no Lead, and its preview said `(this Bundle has no SKILL.md)`. `fs::metadata` keeps the error, and only `NotFound` means none; a Lead that could not be looked at is tried, and the row says `permission denied`.
- **The reason a test expected was the platform's.** Tests wrote `unreadable`, Windows' word for a sharing violation; Unix's is `permission denied`. The fixture now tries the path as a Walk would, and hands the test the error it met.

## Related

[[cross-compilation]] · [[continuous-integration]] · [[testing]] · [[fs-read-dir]] · [[paths]] · [[file-types-and-links]]
