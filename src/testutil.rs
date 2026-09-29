//! Fixtures shared by the test modules. Compiled only under `cargo test`.

use std::env;
use std::fs;
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

/// Hold `path` — a file or a directory — open so that nothing else can open
/// it while the returned handle lives: something this program cannot read,
/// which `std` has no other way to make. Windows lets a handle refuse to be
/// shared; elsewhere there is no such hold, and the test says so on stderr.
pub fn hold(path: &Path) -> Option<fs::File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_BACKUP_SEMANTICS, without which a directory cannot be
        // opened at all.
        let held = fs::OpenOptions::new().read(true).share_mode(0).custom_flags(0x0200_0000).open(path);
        if let Err(e) = &held {
            eprintln!("SKIPPED {}: cannot hold it open ({:?})", path.display(), e.kind());
        }
        held.ok()
    }
    #[cfg(not(windows))]
    {
        eprintln!("SKIPPED {}: no way to hold a path open on this platform", path.display());
        None
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
