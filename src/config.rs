//! The Sources a person adds by writing them down. `.agentdocs.toml` in the
//! home directory holds global ones; the same file at a project's root holds
//! that project's. Each is read once, as the program starts.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::source::{Scope, Source, Walk};

/// The config file's name, at home and at a project's root alike.
pub const FILE: &str = ".agentdocs.toml";

/// What one config file holds: its `[[source]]` tables, in the order they are
/// written. A file with none — empty, or only comments — adds nothing.
///
/// A key that has no place here is refused rather than passed over: written
/// `[[sources]]`, one letter too many, the file would otherwise add nothing
/// and say nothing.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
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
}

/// The Sources written in the config file in `dir`, each of `scope`, with its
/// path taken from `dir` — or from `home` when written `~/…`. None when there
/// is no such file; a file that is there but cannot be used adds none of its
/// Sources, and says why instead.
pub fn sources(dir: &Path, home: &Path, scope: Scope) -> Result<Vec<Source>, Problem> {
    let text = match fs::read_to_string(dir.join(FILE)) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        read => read?,
    };
    let file: File = toml::from_str(&text)?;

    let sources = file
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
    Ok(sources)
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

    fn parse_error(result: Result<Vec<Source>, Problem>) -> String {
        match result {
            Err(Problem::Parse(e)) => e.to_string(),
            Err(Problem::Read(e)) => panic!("read, not parsed: {e}"),
            Ok(sources) => panic!("parsed, {} Sources", sources.len()),
        }
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
    fn a_file_held_open_is_a_read_problem() {
        let dir = config("config-held", "");
        let Some(_held) = hold(&dir.join(FILE)) else { return };
        match sources(&dir, &dir, Scope::Global) {
            Err(Problem::Read(e)) => assert_ne!(e.kind(), io::ErrorKind::NotFound),
            _ => panic!("expected a read problem"),
        }
    }
}
