mod entry;
mod frontmatter;
mod listing;
mod source;
mod tui;
#[cfg(test)]
mod testutil;

use std::env;
use std::io::{self, IsTerminal, Write};

use crate::listing::{failed, listing};
use crate::source::{Scope, Source, Walk, find_project_root};

/// The words to search for: every argument after the program's own path,
/// lowercased once here so that matching compares like with like. Any iterator
/// of `String`s will do — `env::args()` in `main`, a plain array in a test.
///
/// An argument that is empty or only whitespace is not a term. The empty
/// string is inside every text: one stray `""` — an unset shell variable is
/// enough — would keep every Entry on its own, and beside a real term would
/// point each kept Entry at its first line.
fn search_terms(args: impl Iterator<Item = String>) -> Vec<String> {
    args.skip(1)
        .filter(|a| !a.trim().is_empty())
        .map(|a| a.to_lowercase())
        .collect()
}

fn main() -> io::Result<()> {
    let home = env::home_dir();
    let cwd = env::current_dir().ok();

    let terms = search_terms(env::args());

    let mut sources = Vec::new();
    if let Some(home) = &home {
        sources.push(Source::new("skills", home.join(".claude").join("skills"), Scope::Global, Walk::BundleDirs));
        sources.push(Source::new("rules", home.join(".claude").join("rules"), Scope::Global, Walk::MarkdownFiles));
        sources.push(Source::new("agents", home.join(".claude").join("agents"), Scope::Global, Walk::MarkdownFiles));
        sources.push(Source::new("commands", home.join(".claude").join("commands"), Scope::Global, Walk::MarkdownFiles));
        sources.push(Source::new("agents/skills", home.join(".agents").join("skills"), Scope::Global, Walk::BundleDirs));
    }

    let root = match (&cwd, &home) {
        (Some(cwd), Some(home)) => find_project_root(cwd, home),
        _ => None,
    };

    if let Some(r) = &root {
        sources.push(Source::new("root md", r.clone(), Scope::Project, Walk::MarkdownFiles));
        sources.push(Source::new("docs", r.join("docs"), Scope::Project, Walk::MarkdownTree));
    }

    let global_header = match &home {
        Some(_) => String::from("GLOBAL"),
        None => String::from("GLOBAL (home directory unknown)"),
    };

    let project_header = match (&root, &cwd) {
        (Some(r), _) => format!("PROJECT {}", r.display()),
        (None, Some(_)) => String::from("PROJECT (outside any project)"),
        (None, None) => String::from("PROJECT (current directory unknown)"),
    };

    // Only a bare `agentdocs` typed at a terminal opens the screen. With words,
    // or with output going to a pipe or a file, the listing is printed —
    // ADR-0008.
    if terms.is_empty() && io::stdout().is_terminal() {
        // The pane has room for the project's directory name, not its path.
        let project_title = match &root {
            Some(r) => format!("PROJECT {}", r.file_name().unwrap_or(r.as_os_str()).to_string_lossy()),
            None => project_header,
        };
        let app = tui::App::new(sources, global_header, project_title);
        return tui::open(app);
    }

    print_listing(&mut io::stdout().lock(), &global_header, &project_header, &sources, &terms)
}

/// Print the listing to `out`. Whoever reads it may stop early — `| head`,
/// `| Select-Object -First 5` — and has then seen all they asked for: the
/// rest goes unwritten, and that is not a failure.
fn print_listing(
    out: &mut impl Write,
    global: &str,
    project: &str,
    sources: &[Source],
    terms: &[String],
) -> io::Result<()> {
    match write_listing(out, global, project, sources, terms) {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

/// The listing itself: the global heading, then every Source with its
/// Entries, and the project heading just before the first project Source —
/// or last, when there is none. The first write that fails ends it.
fn write_listing(
    out: &mut impl Write,
    global: &str,
    project: &str,
    sources: &[Source],
    terms: &[String],
) -> io::Result<()> {
    writeln!(out, "{global}")?;

    let mut project_shown = false;
    for src in sources {
        if let Scope::Project = src.scope {
            if !project_shown {
                writeln!(out, "{project}")?;
                project_shown = true;
            }
        }

        match src.entries() {
            Ok(walked) => {
                for line in listing(&src.name, &walked, terms) {
                    writeln!(out, "{line}")?;
                }
            }
            Err(e) => writeln!(out, "{}", failed(&src.name, &e))?,
        }
    }

    if !project_shown {
        writeln!(out, "{project}")?;
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    // ------------------------------------------------------------ search_terms

    /// Command-line arguments as `env::args()` would hand them over, the
    /// program's own path first.
    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter().map(|a| a.to_string()).collect::<Vec<_>>().into_iter()
    }

    #[test]
    fn the_programs_own_path_is_not_a_search_term() {
        assert_eq!(search_terms(args(&["target\\debug\\agentdocs.exe", "adr"])), vec!["adr"]);
    }

    #[test]
    fn search_terms_are_lowercased() {
        // Without this, `agentdocs ADR` finds nothing anywhere and exits 0.
        assert_eq!(search_terms(args(&["agentdocs", "ADR", "검증"])), vec!["adr", "검증"]);
    }

    #[test]
    fn no_arguments_means_no_search_terms() {
        assert!(search_terms(args(&["agentdocs"])).is_empty());
    }

    #[test]
    fn a_quoted_phrase_stays_one_term() {
        assert_eq!(search_terms(args(&["agentdocs", "Error Handling"])), vec!["error handling"]);
    }

    #[test]
    fn an_empty_or_blank_argument_is_not_a_term() {
        // Found in review: `agentdocs "" adr` showed `1: ---` as the reason
        // for every match, and `agentdocs ""` kept every Entry, because "" is
        // inside every text.
        assert_eq!(search_terms(args(&["agentdocs", "", "adr", "   "])), vec!["adr"]);
        assert!(search_terms(args(&["agentdocs", ""])).is_empty());
    }

    // ----------------------------------------------------------------- listing

    /// A global Source holding one rule, and a project Source that is not there.
    fn two_sources(name: &str) -> Vec<Source> {
        let dir = scratch(name);
        write(&dir.join("rules").join("one.md"), "# one\n");
        vec![
            Source::new("rules", dir.join("rules"), Scope::Global, Walk::MarkdownFiles),
            Source::new("docs", dir.join("docs"), Scope::Project, Walk::MarkdownTree),
        ]
    }

    fn written(sources: &[Source]) -> Vec<String> {
        let mut out = Vec::new();
        write_listing(&mut out, "GLOBAL", "PROJECT here", sources, &[]).unwrap();
        String::from_utf8(out).unwrap().lines().map(String::from).collect()
    }

    #[test]
    fn the_project_heading_comes_just_before_the_first_project_source() {
        let lines = written(&two_sources("main-order"));
        assert_eq!(lines[..2], ["GLOBAL", "  rules:1"]);
        assert_eq!(lines[lines.len() - 2..], ["PROJECT here", "  docs:(missing)"]);
    }

    #[test]
    fn two_project_sources_share_one_project_heading() {
        // `main` pushes two project Sources, `root md` and then `docs`; with one,
        // a heading written before every project Source looks the same.
        let mut sources = two_sources("main-twoproject");
        let root = scratch("main-twoproject-root");
        sources.insert(1, Source::new("root md", root, Scope::Project, Walk::MarkdownFiles));

        let lines = written(&sources);
        assert_eq!(lines[lines.len() - 3..], ["PROJECT here", "  root md:0", "  docs:(missing)"]);
    }

    #[test]
    fn with_no_project_source_the_project_heading_comes_last() {
        let mut sources = two_sources("main-noproject");
        sources.pop();
        assert_eq!(written(&sources).last().map(String::as_str), Some("PROJECT here"));
    }

    /// Takes `room` bytes and then refuses with `kind` — the way a pipe does
    /// once `| head` has read enough and gone.
    struct StopsAfter {
        room: usize,
        kind: io::ErrorKind,
    }

    impl Write for StopsAfter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.room == 0 {
                return Err(io::Error::from(self.kind));
            }
            let taken = buf.len().min(self.room);
            self.room -= taken;
            Ok(taken)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_reader_that_stops_early_is_not_an_error() {
        let mut out = StopsAfter { room: 10, kind: io::ErrorKind::BrokenPipe };
        let result = print_listing(&mut out, "GLOBAL", "PROJECT here", &two_sources("main-pipe"), &[]);
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn any_other_failure_to_write_still_is() {
        let mut out = StopsAfter { room: 10, kind: io::ErrorKind::PermissionDenied };
        let result = print_listing(&mut out, "GLOBAL", "PROJECT here", &two_sources("main-denied"), &[]);
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    }
}
