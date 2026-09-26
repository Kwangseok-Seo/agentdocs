mod entry;
mod frontmatter;
mod listing;
mod source;
#[cfg(test)]
mod testutil;

use std::env;

use crate::listing::{listing, reason};
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

fn main() {
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

    match &home {
        Some(_) => println!("GLOBAL"),
        None => println!("GLOBAL (home directory unknown)"),
    }

    let project_header = match (&root, &cwd) {
        (Some(r), _) => format!("PROJECT {}", r.display()),
        (None, Some(_)) => String::from("PROJECT (outside any project)"),
        (None, None) => String::from("PROJECT (current directory unknown)"),
    };
    
    let mut project_shown = false;
    for src in &sources {
        if let Scope::Project = src.scope {
            if !project_shown {
                println!("{project_header}");
                project_shown = true;
            }
        }

        match src.entries() {
            Ok(walked) => {
                for line in listing(&src.name, &walked, &terms) {
                    println!("{line}");
                }
            }
            Err(e) => println!("  {}:({})", src.name, reason(e.kind())),
        }
    }

    if !project_shown {
        println!("{project_header}");
    }
    
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
