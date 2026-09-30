use std::env;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;

/// The editor when neither `$VISUAL` nor `$EDITOR` names one: herdr's on
/// Windows, and the one POSIX promises everywhere else.
#[cfg(windows)]
const FALLBACK: &str = "notepad";
#[cfg(not(windows))]
const FALLBACK: &str = "vi";

/// The command that opens `path` in the editor this environment names.
pub fn command(path: &Path) -> Command {
    let editor = chosen(env::var_os);
    #[cfg(windows)]
    let command = through_cmd(&editor, path);
    #[cfg(not(windows))]
    let command = through_sh(&editor, path);
    command
}

/// The editor `lookup` finds in `$VISUAL`, or else in `$EDITOR` — the order
/// git and herdr look in — passing over a variable that holds only blanks;
/// or else the fallback.
fn chosen(lookup: impl Fn(&'static str) -> Option<OsString>) -> OsString {
    ["VISUAL", "EDITOR"]
        .into_iter()
        .filter_map(lookup)
        .find(|name| !name.to_string_lossy().trim().is_empty())
        .unwrap_or_else(|| OsString::from(FALLBACK))
}

// The editor is run by the platform's shell, so that what works at a prompt
// works here: a name only the shell finds — `code` is `code.cmd`, which
// `Command` does not look for — and words after it, as in `code --wait`. The
// path never goes through the shell's reading of a line, so that nothing in a
// file's name can change what is run.

/// On Windows the path is handed over in a variable, which cmd expands once
/// and does not read again: a `%`, `^` or `&` in the name stays as it is.
/// `/d` leaves out any AutoRun commands, and `/v:off` makes `!` only a
/// character. cmd takes the first and last quotes off what follows `/c`.
#[cfg(windows)]
fn through_cmd(editor: &OsStr, path: &Path) -> Command {
    use std::os::windows::process::CommandExt;

    let mut line = OsString::from("\"");
    line.push(editor);
    line.push(" \"%AGENTDOCS_FILE%\"\"");

    let mut command = Command::new("cmd");
    command.raw_arg("/d /v:off /c").raw_arg(line).env("AGENTDOCS_FILE", path);
    command
}

/// Elsewhere the path is `$1`, as git hands it to `$EDITOR`: quoted, sh reads
/// it as one word and looks no further into it. Compiled on Windows too, for
/// its tests, which run it with Git's sh where there is one.
#[cfg(any(not(windows), test))]
fn through_sh(editor: &OsStr, path: &Path) -> Command {
    let mut line = editor.to_owned();
    line.push(" \"$1\"");

    let mut command = Command::new("sh");
    command.arg("-c").arg(line).arg("sh").arg(path);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Stdio;

    /// An environment holding `visual` and `editor`, where `None` is unset.
    fn with(visual: Option<&str>, editor: Option<&str>) -> OsString {
        chosen(|name| match name {
            "VISUAL" => visual.map(OsString::from),
            "EDITOR" => editor.map(OsString::from),
            _ => None,
        })
    }

    #[test]
    fn visual_comes_before_editor() {
        assert_eq!(with(Some("hx"), Some("vim")), "hx");
        assert_eq!(with(None, Some("vim")), "vim");
        assert_eq!(with(Some("hx"), None), "hx");
    }

    #[test]
    fn a_variable_holding_only_blanks_is_not_set() {
        assert_eq!(with(Some(" "), Some("vim")), "vim");
        assert_eq!(with(Some(""), Some("\t")), FALLBACK);
        assert_eq!(with(None, None), FALLBACK);
    }

    /// Files whose names a shell would read something into, if it were given
    /// them to read. `PATH`, `OS` and `HOME` are variables that are set.
    fn awkward(dir: &Path) -> Vec<PathBuf> {
        let names = ["plain.md", "a space & more.md", "100%PATH%.md", "bang!OS!.md", "caret^x.md", "dollar$HOME.md"];
        names
            .iter()
            .map(|name| {
                let path = dir.join(name);
                write(&path, "before\n");
                path
            })
            .collect()
    }

    // Each "editor" appends a line to the file it is given: if the path came
    // through changed, the line lands somewhere else. Each runs where the
    // test made its files, so that a file it makes up is counted there.

    #[cfg(windows)]
    #[test]
    fn cmd_hands_the_editor_the_path_as_it_is() {
        let dir = scratch("editor-cmd");
        for path in awkward(&dir) {
            let status = through_cmd(OsStr::new("echo edited >>"), &path).current_dir(&dir).status().unwrap();
            assert!(status.success(), "{}: {status}", path.display());
            let text = fs::read_to_string(&path).unwrap();
            assert!(text.contains("edited"), "{}: {text:?}", path.display());
        }
        // Nothing was written beside them, under a name cmd made up.
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 6);
    }

    #[test]
    fn sh_hands_the_editor_the_path_as_it_is() {
        if Command::new("sh").arg("-c").arg("true").status().is_err() {
            eprintln!("SKIPPED: no sh to run");
            return;
        }
        // The path is a word handed to a program, as it is to an editor —
        // and not where output goes, which sh does not split into words.
        let dir = scratch("editor-sh");
        for path in awkward(&dir) {
            let editor = OsStr::new("echo edited | tee -a");
            let status = through_sh(editor, &path).current_dir(&dir).stdout(Stdio::null()).status().unwrap();
            assert!(status.success(), "{}: {status}", path.display());
            let text = fs::read_to_string(&path).unwrap();
            assert!(text.contains("edited"), "{}: {text:?}", path.display());
        }
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 6);
    }

    #[cfg(windows)]
    #[test]
    fn an_editor_cmd_cannot_find_ends_in_failure_not_in_an_error() {
        // cmd itself starts, says it cannot find the name, and exits 1.
        let dir = scratch("editor-missing");
        let path = dir.join("x.md");
        write(&path, "");
        let status = through_cmd(OsStr::new("agentdocs-no-such-editor"), &path)
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(1));
    }
}
