//! The program as people run it: the binary cargo built, started in a home
//! and a project made for each test, with its output going to a pipe — so it
//! prints the listing (ADR-0008). What `main` puts together — the source
//! table, the config files, the project root, the headings — is checked here
//! and nowhere else, on every platform the tests run on.

use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A fresh directory for one test, under the system temp directory.
fn scratch(name: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!("agentdocs-cli-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    // macOS keeps its temp directory behind a link, /var to /private/var, and
    // the program's current directory comes back with the link resolved: the
    // home handed to it is written the same way, or no project is found below
    // it. On Windows the resolved form is a `\\?\` path, which is not how a
    // directory is given to a program.
    #[cfg(unix)]
    let dir = fs::canonicalize(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A home with one skill and two rules, and below it a repository with a
/// `CLAUDE.md` and one document.
fn home(name: &str) -> PathBuf {
    let home = scratch(name);
    write(
        &home.join(".claude/skills/alpha/SKILL.md"),
        "---\nname: alpha\ndescription: the first skill\n---\n# alpha\nmentions ADR here\n",
    );
    write(&home.join(".claude/rules/M10.md"), "# ten\n");
    write(&home.join(".claude/rules/M2.md"), "# two\n");
    fs::create_dir_all(home.join("proj/.git")).unwrap();
    write(&home.join("proj/CLAUDE.md"), "# project\n");
    write(&home.join("proj/docs/guide.md"), "# guide\n");
    home
}

/// What the program printed and its exit code, run in `cwd` with `home` as
/// its home directory — where each platform's `env::home_dir` looks — and
/// with `words` after it.
fn run(home: &Path, cwd: &Path, words: &[&str]) -> (Vec<String>, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_agentdocs"))
        .args(words)
        .current_dir(cwd)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    (text.lines().map(String::from).collect(), out.status.code().unwrap())
}

/// An Entry's row, as the listing prints it.
fn row(name: &str, description: &str) -> String {
    format!("    {name:<32} {description}")
}

#[test]
fn a_bare_command_into_a_pipe_lists_every_source_and_finds_the_project_above() {
    let home = home("bare");
    let (lines, code) = run(&home, &home.join("proj/docs"), &[]);
    assert_eq!(code, 0);
    assert_eq!(
        lines,
        [
            "GLOBAL".to_string(),
            "  skills:1".to_string(),
            row("alpha", "the first skill"),
            "  rules:2".to_string(),
            row("M2", "-"),
            row("M10", "-"),
            "  agents:(missing)".to_string(),
            "  commands:(missing)".to_string(),
            "  agents/skills:(missing)".to_string(),
            format!("PROJECT {}", home.join("proj").display()),
            "  root md:1".to_string(),
            row("CLAUDE", "-"),
            "  docs:1".to_string(),
            row("guide", "-"),
        ]
    );
}

#[test]
fn words_keep_the_entries_that_hold_them_and_say_where() {
    let home = home("words");
    let (lines, code) = run(&home, &home.join("proj"), &["ADR"]);
    assert_eq!(code, 0);
    assert_eq!(lines[..4], ["GLOBAL", "  skills:1/1", row("alpha", "the first skill").as_str(), "      6: mentions ADR here"]);
    assert_eq!(lines[4], "  rules:0/2");
}

#[test]
fn at_home_or_outside_it_there_is_no_project() {
    let home = home("outside");
    let elsewhere = scratch("outside-elsewhere");
    for cwd in [&home, &elsewhere] {
        let (lines, code) = run(&home, cwd, &[]);
        assert_eq!(code, 0);
        assert_eq!(lines.last().map(String::as_str), Some("PROJECT (outside any project)"), "{}", cwd.display());
    }
}

/// A link at `link` to the directory `target`, or `false`, said on stderr,
/// where this account may not make one.
fn link_dir(target: &Path, link: &Path) -> bool {
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(target, link);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);
    if let Err(e) = &made {
        eprintln!("SKIPPED {}: cannot create a directory link ({:?})", link.display(), e.kind());
    }
    made.is_ok()
}

#[test]
fn a_home_reached_through_a_link_still_has_projects_below_it() {
    // The system gives the current directory with every link resolved — and
    // FreeBSD's /home is a link — so it is not below a home written with one.
    let real = home("linked-home");
    let link = scratch("linked-home-link").join("home");
    if !link_dir(&real, &link) {
        return;
    }
    let (lines, code) = run(&link, &link.join("proj"), &[]);
    assert_eq!(code, 0);
    assert_eq!(lines[1], "  skills:1");
    let heading = lines.iter().find(|line| line.starts_with("PROJECT")).unwrap();
    assert!(heading.ends_with("proj"), "{heading}");
}

#[test]
fn the_config_file_at_home_adds_global_sources_and_orders_them() {
    let home = home("config-home");
    write(&home.join("notes/one.md"), "# one\n");
    write(
        &home.join(".agentdocs.toml"),
        "order = [\"notes\"]\n\n[[source]]\nname = \"notes\"\npath = \"notes\"\nwalk = \"markdown-files\"\n",
    );
    let (lines, code) = run(&home, &home.join("proj"), &[]);
    assert_eq!(code, 0);
    assert_eq!(lines[..4], ["GLOBAL", "  notes:1", row("one", "-").as_str(), "  skills:1"]);
}

#[test]
fn a_config_file_at_the_root_that_cannot_be_used_is_a_row_where_its_sources_would_be() {
    let home = home("config-broken");
    write(&home.join("proj/.agentdocs.toml"), "[[source]]\nname = \"s\"\npath = \"s\"\nwalk = \"tree\"\n");
    let (lines, code) = run(&home, &home.join("proj"), &[]);
    assert_eq!(code, 0);
    let at = lines.iter().position(|line| line == "  .agentdocs.toml:(invalid)").expect("no row for the file");
    assert_eq!(lines[at - 2..at], ["  docs:1".to_string(), row("guide", "-")]);
    assert_eq!(lines[at + 1], "    TOML parse error at line 4, column 8");
}

#[test]
fn a_link_back_up_the_tree_is_listed_and_not_walked_into() {
    // M1 found a junction under `docs/` pointing at `docs/` itself: two files
    // were counted as 128.
    let home = home("loop");
    write(&home.join("proj/docs/second.md"), "# second\n");
    let docs = home.join("proj/docs");
    if !link_dir(&docs, &docs.join("again")) {
        return;
    }
    let (lines, code) = run(&home, &home.join("proj"), &[]);
    assert_eq!(code, 0);
    assert_eq!(lines[lines.len() - 3..], ["  docs:2".to_string(), row("guide", "-"), row("second", "-")]);
}

#[test]
fn a_reader_that_stops_early_is_not_a_failure() {
    // More than a pipe holds, so that the program is still writing when the
    // reader goes: it must stop and exit 0, not panic.
    let home = home("pipe");
    for n in 0..3000 {
        write(&home.join(format!(".claude/rules/rule-{n}.md")), "");
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentdocs"))
        .current_dir(home.join("proj"))
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut first = [0u8; 6];
    child.stdout.take().unwrap().read_exact(&mut first).unwrap();
    assert_eq!(&first, b"GLOBAL");
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
}

#[test]
fn version_and_help_say_what_the_program_is() {
    let home = home("version");
    let (lines, code) = run(&home, &home, &["--version"]);
    assert_eq!((lines, code), (vec![format!("agentdocs {}", env!("CARGO_PKG_VERSION"))], 0));

    let (lines, code) = run(&home, &home, &["--help"]);
    assert_eq!(code, 0);
    assert!(lines[0].starts_with(&format!("agentdocs {} — ", env!("CARGO_PKG_VERSION"))), "{lines:?}");
    assert_eq!(lines.last().map(String::as_str), Some(env!("CARGO_PKG_REPOSITORY")));
}
