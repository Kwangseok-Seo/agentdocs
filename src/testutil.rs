//! Fixtures shared by the test modules. Compiled only under `cargo test`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::entry::Entry;
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
