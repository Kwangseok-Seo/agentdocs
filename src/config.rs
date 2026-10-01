//! The Sources a person adds by writing them down, and the order the Sources
//! are shown in. `.agentdocs.toml` in the home directory speaks for the global
//! ones; the same file at a project's root, for that project's. Each is read
//! once, as the program starts.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::source::{Scope, Source, Walk};

/// The config file's name, at home and at a project's root alike.
pub const FILE: &str = ".agentdocs.toml";

/// What one config file holds: the names of the Sources to show first, and
/// its `[[source]]` tables, in the order they are written. A file with
/// neither — empty, or only comments — changes nothing.
///
/// A key that has no place here is refused rather than passed over: written
/// `[[sources]]`, one letter too many, the file would otherwise add nothing
/// and say nothing.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    order: Vec<String>,
    #[serde(default)]
    source: Vec<Row>,
}

/// One `[[source]]` table: a row of the source table, written by hand.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    name: String,
    path: PathBuf,
    walk: Walk,
}

/// Why a config file that is there could not be used: the error that stopped
/// it, which says itself what went wrong — `transparent` hands its words and
/// its own cause through unchanged. `from` lets `?` turn either error into a
/// `Problem`.
#[derive(Debug, thiserror::Error)]
pub enum Problem {
    /// It would not open, or would not read as text.
    #[error(transparent)]
    Read(#[from] io::Error),
    /// It was read, and does not hold what a config file holds.
    #[error(transparent)]
    Parse(#[from] toml::de::Error),
    /// It holds what a config file holds, and its `order` names a Source
    /// that its scope does not have.
    #[error("`order` names `{0}`, and no Source here is called that")]
    Order(String),
}

/// Add the Sources written in the config file in `dir` to `sources`, the
/// ones `scope` already has, each with its path taken from `dir` — or from
/// `home` when written `~/…` — and then put them all in the file's order:
/// the Sources it names first, as it names them, and the rest after, as they
/// came. With no such file nothing changes; a file that is there but cannot
/// be used changes nothing either, and says why instead.
pub fn add(dir: &Path, home: &Path, scope: Scope, sources: &mut Vec<Source>) -> Result<(), Problem> {
    let path = dir.join(FILE);
    // Not found is no file only when nothing at all is there: a link whose
    // target is gone is not found either, and is something — as a Walk lists
    // one rather than dropping it.
    let text = match fs::read_to_string(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound && fs::symlink_metadata(&path).is_err() => return Ok(()),
        read => read?,
    };
    let file: File = toml::from_str(&text)?;

    let written: Vec<Source> = file
        .source
        .into_iter()
        .map(|row| {
            let path = match row.path.strip_prefix("~") {
                Ok(rest) => home.join(rest),
                Err(_) => dir.join(&row.path),
            };
            Source::new(row.name, path, scope, row.walk)
        })
        .collect();
    let known = |name: &String| sources.iter().chain(&written).any(|src| src.name == *name);
    if let Some(name) = file.order.iter().find(|name| !known(name)) {
        return Err(Problem::Order(name.clone()));
    }

    sources.extend(written);
    // Sorting is stable: Sources that sort the same keep the order they came in.
    let place = |src: &Source| file.order.iter().position(|name| *name == src.name).unwrap_or(file.order.len());
    sources.sort_by_key(place);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    /// A directory holding a config file that says `text`.
    fn config(name: &str, text: &str) -> PathBuf {
        let dir = scratch(name);
        write(&dir.join(FILE), text);
        dir
    }

    /// What a Walk of each Source finds, by name: enough to tell its path and
    /// its Walk apart from any other's.
    fn found(sources: &[Source]) -> Vec<(String, Vec<String>)> {
        sources
            .iter()
            .map(|s| {
                let names = match s.entries() {
                    Ok(walked) => names(&walked).iter().map(|n| n.to_string()).collect(),
                    Err(e) => vec![format!("({:?})", e.kind())],
                };
                (s.name.clone(), names)
            })
            .collect()
    }

    /// What the config file in `dir` adds to a scope that has no Sources yet.
    fn sources(dir: &Path, home: &Path, scope: Scope) -> Result<Vec<Source>, Problem> {
        let mut sources = Vec::new();
        add(dir, home, scope, &mut sources).map(|()| sources)
    }

    fn parse_error(result: Result<Vec<Source>, Problem>) -> String {
        match result {
            Err(Problem::Parse(e)) => e.to_string(),
            Err(other) => panic!("not a parse problem: {other}"),
            Ok(sources) => panic!("parsed, {} Sources", sources.len()),
        }
    }

    /// Sources named `names`, each standing for a default of the scope.
    fn defaults(dir: &Path, names: &[&str]) -> Vec<Source> {
        names.iter().map(|name| Source::new(*name, dir.join(name), Scope::Project, Walk::MarkdownFiles)).collect()
    }

    fn named(sources: &[Source]) -> Vec<&str> {
        sources.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn the_sources_order_names_come_first_as_named_and_the_rest_keep_their_places() {
        let dir = config(
            "config-order",
            "order = [\"wiki\", \"docs\"]\n\n[[source]]\nname = \"wiki\"\npath = \"Wiki\"\nwalk = \"markdown-files\"\n",
        );
        let mut sources = defaults(&dir, &["root md", "docs", "notes"]);
        add(&dir, &dir, Scope::Project, &mut sources).ok().unwrap();
        assert_eq!(named(&sources), ["wiki", "docs", "root md", "notes"]);
    }

    #[test]
    fn sources_the_order_does_not_name_keep_their_places_however_many() {
        // Under twenty, an unstable sort happens to keep them too; past that
        // it need not.
        let dir = config("config-order-many", "order = [\"s50\"]\n");
        let names: Vec<String> = (0..100).map(|i| format!("s{i}")).collect();
        let mut sources = defaults(&dir, &names.iter().map(String::as_str).collect::<Vec<_>>());
        add(&dir, &dir, Scope::Project, &mut sources).ok().unwrap();

        let mut expected = vec!["s50".to_string()];
        expected.extend(names.iter().filter(|n| *n != "s50").cloned());
        assert_eq!(named(&sources), expected);
    }

    #[test]
    fn an_order_alone_puts_the_scopes_own_sources_in_order() {
        let dir = config("config-order-only", "order = [\"notes\"]\n");
        let mut sources = defaults(&dir, &["root md", "docs", "notes"]);
        add(&dir, &dir, Scope::Project, &mut sources).ok().unwrap();
        assert_eq!(named(&sources), ["notes", "root md", "docs"]);
    }

    #[test]
    fn an_order_naming_no_source_here_is_refused_and_changes_nothing() {
        let dir = config(
            "config-order-unknown",
            "order = [\"docs\", \"wkii\"]\n\n[[source]]\nname = \"wiki\"\npath = \"Wiki\"\nwalk = \"markdown-files\"\n",
        );
        let mut sources = defaults(&dir, &["root md", "docs"]);
        match add(&dir, &dir, Scope::Project, &mut sources) {
            Err(problem @ Problem::Order(_)) => {
                assert_eq!(problem.to_string(), "`order` names `wkii`, and no Source here is called that")
            }
            other => panic!("expected an order problem, got {:?}", other.err()),
        }
        assert_eq!(named(&sources), ["root md", "docs"]);
    }

    #[test]
    fn an_order_written_below_a_source_table_belongs_to_that_table_and_is_refused() {
        // TOML gives every key after `[[source]]` to that table, up to the
        // next header — so `order` there is a field no Source has.
        let dir = config(
            "config-order-late",
            "[[source]]\nname = \"wiki\"\npath = \"Wiki\"\nwalk = \"markdown-files\"\norder = [\"wiki\"]\n",
        );
        let error = parse_error(sources(&dir, &dir, Scope::Project));
        assert!(error.contains("unknown field `order`, expected one of `name`, `path`, `walk`"), "{error}");
    }

    #[test]
    fn each_source_table_is_a_source_in_the_order_written() {
        let dir = config(
            "config-two",
            "[[source]]\nname = \"wiki\"\npath = \"Wiki\"\nwalk = \"markdown-files\"\n\n\
             [[source]]\nname = \"sessions\"\npath = \"Sessions\"\nwalk = \"markdown-tree\"\n",
        );
        write(&dir.join("Wiki").join("index.md"), "# index\n");
        write(&dir.join("Sessions").join("agentdocs").join("one.md"), "# one\n");

        let sources = sources(&dir, &dir, Scope::Project).ok().unwrap();
        assert_eq!(
            found(&sources),
            [("wiki".to_string(), vec!["index".to_string()]), ("sessions".to_string(), vec!["one".to_string()])]
        );
        assert!(sources.iter().all(|s| matches!(s.scope, Scope::Project)));
    }

    #[test]
    fn the_walk_named_is_the_walk_taken() {
        // One directory, two ways: a Bundle directory under `bundle-dirs`, a
        // directory of documents nobody walks into under `markdown-files`.
        let dir = config(
            "config-walks",
            "[[source]]\nname = \"as bundles\"\npath = \"s\"\nwalk = \"bundle-dirs\"\n\n\
             [[source]]\nname = \"as files\"\npath = \"s\"\nwalk = \"markdown-files\"\n",
        );
        write(&dir.join("s").join("tool").join("SKILL.md"), "---\nname: tool\n---\n");
        write(&dir.join("s").join("loose.md"), "# loose\n");

        let sources = sources(&dir, &dir, Scope::Global).ok().unwrap();
        assert_eq!(
            found(&sources),
            [("as bundles".to_string(), vec!["tool".to_string()]), ("as files".to_string(), vec!["loose".to_string()])]
        );
    }

    #[test]
    fn a_path_is_taken_from_the_files_directory_or_from_home_after_a_tilde() {
        let home = scratch("config-home");
        let dir = config(
            "config-tilde",
            "[[source]]\nname = \"here\"\npath = \"notes\"\nwalk = \"markdown-files\"\n\n\
             [[source]]\nname = \"home\"\npath = \"~/notes\"\nwalk = \"markdown-files\"\n",
        );
        write(&dir.join("notes").join("near.md"), "# near\n");
        write(&home.join("notes").join("far.md"), "# far\n");

        let sources = sources(&dir, &home, Scope::Project).ok().unwrap();
        assert_eq!(
            found(&sources),
            [("here".to_string(), vec!["near".to_string()]), ("home".to_string(), vec!["far".to_string()])]
        );
    }

    #[test]
    fn an_absolute_path_is_kept_as_written() {
        let elsewhere = scratch("config-elsewhere");
        write(&elsewhere.join("far.md"), "# far\n");
        let toml_path = elsewhere.display().to_string().replace('\\', "/");
        let dir = config(
            "config-absolute",
            &format!("[[source]]\nname = \"far\"\npath = \"{toml_path}\"\nwalk = \"markdown-files\"\n"),
        );

        let sources = sources(&dir, &dir, Scope::Global).ok().unwrap();
        assert_eq!(found(&sources), [("far".to_string(), vec!["far".to_string()])]);
    }

    #[test]
    fn no_file_and_an_empty_file_both_add_nothing() {
        let none = scratch("config-none");
        assert!(sources(&none, &none, Scope::Global).ok().unwrap().is_empty());

        let empty = config("config-empty", "# nothing here yet\n");
        assert!(sources(&empty, &empty, Scope::Global).ok().unwrap().is_empty());
    }

    #[test]
    fn a_walk_that_does_not_exist_is_refused_with_the_ones_that_do() {
        let dir = config("config-badwalk", "[[source]]\nname = \"s\"\npath = \"s\"\nwalk = \"tree\"\n");
        let error = parse_error(sources(&dir, &dir, Scope::Global));
        assert!(error.contains("line 4"), "{error}");
        assert!(error.contains("unknown variant `tree`, expected one of `markdown-files`, `bundle-dirs`, `markdown-tree`"), "{error}");
    }

    #[test]
    fn a_key_with_no_place_is_refused_not_passed_over() {
        // Passed over, `[[sources]]` would add nothing and say nothing.
        let dir = config("config-plural", "[[sources]]\nname = \"s\"\npath = \"s\"\nwalk = \"markdown-tree\"\n");
        let error = parse_error(sources(&dir, &dir, Scope::Global));
        assert!(error.contains("unknown field `sources`"), "{error}");

        let dir = config("config-extra", "[[source]]\nname = \"s\"\npath = \"s\"\nwalk = \"markdown-tree\"\nhidden = true\n");
        let error = parse_error(sources(&dir, &dir, Scope::Global));
        assert!(error.contains("unknown field `hidden`"), "{error}");
    }

    #[test]
    fn a_row_without_its_walk_is_refused() {
        let dir = config("config-nowalk", "[[source]]\nname = \"s\"\npath = \"s\"\n");
        let error = parse_error(sources(&dir, &dir, Scope::Global));
        assert!(error.contains("missing field `walk`"), "{error}");
    }

    #[test]
    fn text_that_is_not_toml_is_refused() {
        let dir = config("config-syntax", "[[source]]\nname = \"s\npath = \"s\"\n");
        let error = parse_error(sources(&dir, &dir, Scope::Global));
        assert!(error.contains("line 2"), "{error}");
    }

    #[test]
    fn a_file_that_will_not_read_as_text_is_a_read_problem() {
        let dir = scratch("config-binary");
        fs::write(dir.join(FILE), [0xff, 0xfe, 0x00]).unwrap();
        match sources(&dir, &dir, Scope::Global) {
            Err(Problem::Read(e)) => assert_eq!(e.kind(), io::ErrorKind::InvalidData),
            _ => panic!("expected a read problem"),
        }
    }

    #[test]
    fn a_link_whose_target_is_gone_is_a_read_problem_not_no_file() {
        // A dotfile manager links `~/.agentdocs.toml` to a file it keeps
        // elsewhere. With that file gone something is still there, and taken
        // for no file at all it would add nothing and say nothing.
        let dir = scratch("config-dangling");
        let target = dir.join("kept").join("agentdocs.toml");
        write(&target, "[[source]]\nname = \"s\"\npath = \"s\"\nwalk = \"markdown-tree\"\n");
        if !link_file(&target, &dir.join(FILE)) {
            return;
        }
        fs::remove_file(&target).unwrap();
        match sources(&dir, &dir, Scope::Global) {
            Err(Problem::Read(e)) => assert_eq!(e.kind(), io::ErrorKind::NotFound),
            Err(other) => panic!("expected a read problem, got {other}"),
            Ok(sources) => panic!("taken for no file: {} Sources", sources.len()),
        }
    }

    #[test]
    fn a_file_held_open_is_a_read_problem() {
        let dir = config("config-held", "");
        let Some(_held) = hold(&dir.join(FILE)) else { return };
        match sources(&dir, &dir, Scope::Global) {
            Err(Problem::Read(e)) => assert_ne!(e.kind(), io::ErrorKind::NotFound),
            _ => panic!("expected a read problem"),
        }
    }
}
