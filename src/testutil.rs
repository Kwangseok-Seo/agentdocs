//! Fixtures shared by the test modules. Compiled only under `cargo test`.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::entry::{Entry, EntryKind, Hit, Node};
use crate::source::{Walked, md_entry};

/// A fresh directory under the system temp directory, named after the test
/// that asked for it. Tests run on parallel threads, so two of them must
/// never be handed the same path.
pub fn scratch(name: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!("agentdocs-test-{}-{}", std::process::id(), name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Write a file, creating the directories above it. `unwrap` is correct
/// here: a fixture that cannot be built is a failing test, not a result.
pub fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

pub fn names(walked: &Walked) -> Vec<&str> {
    walked.entries().into_iter().map(|e| e.name.as_str()).collect()
}

/// An Entry named `name`, as if its file had held `text`.
pub fn entry_with(name: &str, text: Option<&str>) -> Entry {
    let mut entry = md_entry(PathBuf::from(format!("{name}.md"))).unwrap();
    entry.text = text.map(|t| t.to_string());
    entry
}

/// A Bundle named `name` as if its Lead had held `text`, with `inside` as
/// the rows below its Lead — `None` for a link that was not walked into.
pub fn bundle_with(name: &str, text: Option<&str>, inside: Option<Vec<Node>>) -> Entry {
    let path = PathBuf::from(name);
    Entry {
        name: name.to_string(),
        kind: EntryKind::Bundle { lead: Some(path.join("SKILL.md")), inside },
        path,
        description: None,
        text: text.map(|t| t.to_string()),
    }
}

/// A Hit as three parts: the name of the supporting file it is in, or `""`
/// for the Entry's own file; the line's number; the line.
pub fn hit(found: Option<Hit<'_>>) -> Option<(&str, usize, &str)> {
    found.map(|h| (h.within.map_or("", |entry| entry.name.as_str()), h.number, h.line))
}

/// Search terms the way `main` hands them over: already lowercased.
pub fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|w| w.to_string()).collect()
}

/// A directory link, or `false` when this account may not create one.
/// Windows grants the privilege to administrators and to accounts with
/// Developer Mode enabled. A test that cannot build its fixture says so on
/// stderr rather than passing quietly — `cargo test -- --nocapture` shows it.
pub fn link_dir(target: &Path, link: &Path) -> bool {
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(target, link);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);

    if let Err(e) = &made {
        eprintln!("SKIPPED {}: cannot create a directory link ({:?})", link.display(), e.kind());
    }
    made.is_ok()
}

/// Make `path` something this program cannot read for as long as the
/// returned value lives, which `std` has no portable way to make: a file that
/// will not open, or a directory that will not be listed, though what is in
/// it can still be opened by name. On Windows a handle is held open that
/// refuses to be shared; on Unix the permissions are taken away — for a
/// directory, all but the one that lets a name inside it be looked up — and
/// given back when the value is dropped, so that the next run can clear the
/// scratch directory.
///
/// Whether it worked is found by trying, as a Walk would, and what the
/// trying met is `Held::kind`: the reason differs by platform. Where the
/// hold cannot be made the test says so on stderr and passes the case over —
/// as it must for root, who reads whatever the permissions say.
pub fn hold(path: &Path) -> Option<Held> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_BACKUP_SEMANTICS, without which a directory cannot be
        // opened at all.
        let file = match fs::OpenOptions::new().read(true).share_mode(0).custom_flags(0x0200_0000).open(path) {
            Ok(file) => file,
            Err(e) => {
                eprintln!("SKIPPED {}: cannot hold it open ({:?})", path.display(), e.kind());
                return None;
            }
        };
        let kind = refused(path)?;
        Some(Held { kind, _file: file })
    }
    #[cfg(unix)]
    {
        take_away(path, if path.is_dir() { 0o100 } else { 0 })
    }
    #[cfg(not(any(windows, unix)))]
    {
        eprintln!("SKIPPED {}: no way to hold a path on this platform", path.display());
        None
    }
}

/// A directory shut entirely: not listed, and nothing in it looked up by
/// name either — which Unix permissions can do and a Windows handle cannot.
#[cfg(unix)]
pub fn shut(path: &Path) -> Option<Held> {
    take_away(path, 0)
}

/// What `hold` and `shut` return: while it lives, the path cannot be read.
pub struct Held {
    pub kind: io::ErrorKind,
    #[cfg(windows)]
    _file: fs::File,
    #[cfg(unix)]
    given_back: (PathBuf, u32),
}

#[cfg(unix)]
impl Drop for Held {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let (path, mode) = &self.given_back;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(*mode));
    }
}

/// Leave only `mode` of `path`'s permissions, keeping the rest to give back.
#[cfg(unix)]
fn take_away(path: &Path, mode: u32) -> Option<Held> {
    use std::os::unix::fs::PermissionsExt;
    let kept = match fs::metadata(path) {
        Ok(meta) => meta.permissions().mode(),
        Err(e) => {
            eprintln!("SKIPPED {}: cannot read its permissions ({:?})", path.display(), e.kind());
            return None;
        }
    };
    if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(mode)) {
        eprintln!("SKIPPED {}: cannot take its permissions away ({:?})", path.display(), e.kind());
        return None;
    }
    // Made before the trying, so that a hold that did not take is given back.
    let mut held = Held { kind: io::ErrorKind::Other, given_back: (path.to_path_buf(), kept) };
    held.kind = refused(path)?;
    Some(held)
}

/// What reading `path` meets, as a Walk reads it: a directory listed, a file
/// opened. `None`, said on stderr, when it reads after all.
fn refused(path: &Path) -> Option<io::ErrorKind> {
    let tried = if path.is_dir() { fs::read_dir(path).map(drop) } else { fs::File::open(path).map(drop) };
    match tried {
        Err(e) => Some(e.kind()),
        Ok(()) => {
            eprintln!("SKIPPED {}: it reads all the same — run as root?", path.display());
            None
        }
    }
}

pub fn link_file(target: &Path, link: &Path) -> bool {
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(target, link);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);

    if let Err(e) = &made {
        eprintln!("SKIPPED {}: cannot create a file link ({:?})", link.display(), e.kind());
    }
    made.is_ok()
}
