use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::entry::{Entry, EntryKind, Node, gather, unread};

pub enum Scope {
    Global,
    Project,
}

pub enum Walk {
    MarkdownFiles,
    BundleDirs,
    MarkdownTree,
}

fn md_files(dir: &Path) -> io::Result<Walked> {
    let mut out = Walked::default();
    let read = fs::read_dir(dir)?;

    for item in read {
        let Some((path, ft)) = described(dir, item, &mut out) else { continue };
        // A real directory is not a document, whatever it is named. A link is
        // left to `md_entry`, which decides by name: going to see what it points
        // at is walking into it, and a link is never walked into.
        if ft.is_dir() { continue };
        if let Some(node) = document(path) { out.nodes.push(node); }
    }
    Ok(out)
}

fn md_tree(dir: &Path) -> io::Result<Walked> {
    let mut out = Walked::default();
    let read = fs::read_dir(dir)?;

    for item in read {
        let Some((path, ft)) = described(dir, item, &mut out) else { continue };
        if ft.is_dir() {
            // A directory named with a leading dot is hidden by convention, and
            // what a tool leaves in one — `.pytest_cache/README.md` — is not
            // documentation. Passed over like a `.txt`, so not shown either.
            if path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.')) { continue };
            match md_tree(&path) {
                Ok(sub) => out.nest(path, sub),
                Err(e) => out.nodes.push(unreadable(path, &e)),
            }
        } else if let Some(node) = document(path) {
            // Not a directory, so a candidate document — a link included, since
            // it is listed by name rather than followed.
            out.nodes.push(node);
        }
    }
    Ok(out)
}

fn bundle_dirs(dir: &Path) -> io::Result<Walked> {
    let mut out = Walked::default();
    let read = fs::read_dir(dir)?;

    for item in read {
        let Some((path, ft)) = described(dir, item, &mut out) else { continue };
        // A Bundle is a directory, and a link standing in for one is listed
        // without being walked into. A link to a *file* is not a Bundle, and
        // asking where a link points is reading rather than walking — but a link
        // whose target is gone cannot answer, and is listed rather than dropped,
        // because something is there.
        let bundle_shaped = ft.is_dir()
            || (ft.is_symlink() && fs::metadata(&path).map(|m| m.is_dir()).unwrap_or(true));
        if !bundle_shaped { continue };
        let Some(name) = path.file_name() else { continue };

        let lead_path = path.join("SKILL.md");
        let lead = if lead_path.is_file() { Some(lead_path) } else { None };

        // Only a directory is walked into. What a link stands in for is not
        // asked at all — the entry itself says which it is (ADR-0007). A
        // directory that will not open holds one row saying so.
        let inside = if ft.is_dir() {
            Some(match supporting(&path, lead.as_deref()) {
                Ok(sub) => sub.nodes,
                Err(e) => vec![unreadable(path.clone(), &e)],
            })
        } else {
            None
        };

        let mut entry = Entry {
            name: name.to_string_lossy().to_string(),
            path,
            kind: EntryKind::Bundle { lead, inside },
            description: None,
            text: None,
        };
        // The Lead is what names and shows a Bundle. A Bundle whose Lead will
        // not open has nothing to show, and is shown as unreadable.
        match entry.load_frontmatter() {
            Ok(()) => out.nodes.push(Node::Entry(entry)),
            Err(e) => out.nodes.push(unreadable(entry.path, &e)),
        }
    }
    Ok(out)
}

/// What a Bundle holds besides its Lead, as a tree. The Lead is the Bundle's
/// own row, so it is taken out — from the top level only: a `SKILL.md` further
/// down belongs to a directory inside the Bundle, and is a supporting file like
/// any other.
fn supporting(dir: &Path, lead: Option<&Path>) -> io::Result<Walked> {
    let mut walked = md_tree(dir)?;
    walked.nodes.retain(|node| match node {
        Node::Entry(entry) => Some(entry.path.as_path()) != lead,
        Node::Dir { .. } | Node::Unreadable { .. } => true,
    });
    Ok(walked)
}

/// One item of a directory being read, as its path and what kind of thing it
/// is — or, when the system will not say, a row in `out` for it and `None`.
/// An item that will not describe itself has no name of its own, so its row
/// is named by the directory it was found in.
fn described(dir: &Path, item: io::Result<fs::DirEntry>, out: &mut Walked) -> Option<(PathBuf, fs::FileType)> {
    let item = match item {
        Ok(item) => item,
        Err(e) => {
            out.nodes.push(unreadable(dir.to_path_buf(), &e));
            return None;
        }
    };
    match item.file_type() {
        Ok(ft) => Some((item.path(), ft)),
        Err(e) => {
            out.nodes.push(unreadable(item.path(), &e));
            None
        }
    }
}

/// The row for something at `path` that could not be looked at, and why.
fn unreadable(path: PathBuf, e: &io::Error) -> Node {
    Node::Unreadable { path, reason: e.kind() }
}

pub struct Source {
    pub name: String,
    path: PathBuf,
    pub scope: Scope,
    walk: Walk,
}

impl Source {
    pub fn new(name: &str, path: PathBuf, scope: Scope, walk: Walk) -> Self {
        Source { name: name.to_string(), path: path, scope: scope, walk: walk }
    }

    pub fn entries(&self) -> io::Result<Walked> {
        match self.walk {
            Walk::MarkdownFiles => md_files(&self.path),
            Walk::BundleDirs => bundle_dirs(&self.path),
            Walk::MarkdownTree => md_tree(&self.path),
        }
    }
}

pub fn find_project_root(start: &Path, home: &Path) -> Option<PathBuf> {
    // At the home directory, or anywhere not below it, there is no project
    if start == home || !start.starts_with(home) {
        return None;
    }

    let mut p = start;
    while p != home {
        if p.join(".git").exists() || p.join("CLAUDE.md").exists() {
            return Some(p.to_path_buf());        // found a marker
        }
        match p.parent() {
            Some(up) => p = up,
            None => break,
        }
    }

    Some(start.to_path_buf())                    // nothing found: the current directory is the root
}

/// What one Walk found: the Entries it could list and the things it could not
/// read, in the shape of the directories it found them in. A count that hides
/// its own blind spots is a wrong count, and one that names none of them
/// leaves the reader to find them.
#[derive(Debug, Default)]
pub struct Walked {
    pub nodes: Vec<Node>,
}

impl Walked {
    /// Keep a subdirectory's findings as one row that holds them. A directory
    /// with nothing below it — no Entry, and nothing that could not be read —
    /// is not a row.
    fn nest(&mut self, path: PathBuf, sub: Walked) {
        if !sub.nodes.is_empty() {
            self.nodes.push(Node::Dir { path, children: sub.nodes });
        }
    }

    /// Every Entry, with each directory's in the place the directory stands —
    /// the order the Walk found them in.
    pub fn entries(&self) -> Vec<&Entry> {
        let mut out = Vec::new();
        gather(&self.nodes, &mut out);
        out
    }

    /// Where each thing that could not be read is, and why, wherever in the
    /// tree it was found — the order the Walk found them in.
    pub fn unreadable(&self) -> Vec<(&Path, io::ErrorKind)> {
        let mut out = Vec::new();
        unread(&self.nodes, &mut out);
        out
    }
}

/// A `.md` file becomes one Entry. Anything else is not a document.
pub fn md_entry(path: PathBuf) -> Option<Entry> {
    let ext = path.extension()?;
    if !ext.eq_ignore_ascii_case("md") {
        return None;
    }
    let name = path.file_stem()?.to_string_lossy().to_string();

    Some(Entry { name, path, kind: EntryKind::File, description: None, text: None })
}

/// A `.md` file as a Walk lists it: an Entry, named and described by its
/// frontmatter where it has one — or, when the file will not open, a row
/// saying so. Anything else is not a document.
fn document(path: PathBuf) -> Option<Node> {
    let mut entry = md_entry(path)?;
    Some(match entry.load_frontmatter() {
        Ok(()) => Node::Entry(entry),
        Err(e) => unreadable(entry.path, &e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listing::reason;
    use crate::testutil::*;

    // ---------------------------------------------------------------- md_entry

    #[test]
    fn a_markdown_file_becomes_an_entry_named_by_its_stem() {
        let entry = md_entry(PathBuf::from("notes.md")).unwrap();
        assert_eq!(entry.name, "notes");
    }

    #[test]
    fn the_extension_is_matched_without_regard_to_case() {
        assert!(md_entry(PathBuf::from("NOTES.MD")).is_some());
        assert!(md_entry(PathBuf::from("notes.Md")).is_some());
    }

    #[test]
    fn a_file_that_is_not_markdown_is_not_an_entry() {
        assert!(md_entry(PathBuf::from("notes.txt")).is_none());
    }

    #[test]
    fn a_file_without_an_extension_is_not_an_entry() {
        assert!(md_entry(PathBuf::from("README")).is_none());
    }

    // ------------------------------------------------------------------ Walked

    /// The tree as indented lines: a directory by its name and a slash, an
    /// Entry by its name, and what could not be read by its name and why. Each
    /// level is sorted, since the order `read_dir` gives is the platform's.
    fn outline(nodes: &[Node], depth: usize) -> Vec<String> {
        let mut level: Vec<(String, Vec<String>)> = nodes
            .iter()
            .map(|node| match node {
                Node::Entry(entry) => (entry.name.clone(), Vec::new()),
                Node::Dir { path, children } => (
                    format!("{}/", path.file_name().unwrap().to_string_lossy()),
                    outline(children, depth + 1),
                ),
                Node::Unreadable { path, reason: why } => (
                    format!("{} ({})", path.file_name().unwrap().to_string_lossy(), reason(*why)),
                    Vec::new(),
                ),
            })
            .collect();
        level.sort();
        level
            .into_iter()
            .flat_map(|(name, below)| std::iter::once(format!("{}{name}", "  ".repeat(depth))).chain(below))
            .collect()
    }

    fn found(name: &str) -> Node {
        Node::Entry(md_entry(PathBuf::from(format!("{name}.md"))).unwrap())
    }

    fn refused(path: &str) -> Node {
        Node::Unreadable { path: PathBuf::from(path), reason: io::ErrorKind::PermissionDenied }
    }

    #[test]
    fn nesting_keeps_a_subdirectory_as_one_row_with_its_blind_spots_inside() {
        let mut a = Walked::default();
        a.nodes.push(found("one"));
        a.nodes.push(refused("locked.md"));

        let mut b = Walked::default();
        b.nodes.push(found("two"));
        b.nodes.push(refused("sub/deep"));

        a.nest(PathBuf::from("sub"), b);
        assert_eq!(
            outline(&a.nodes, 0),
            vec!["locked.md (permission denied)", "one", "sub/", "  deep (permission denied)", "  two"]
        );
        assert_eq!(a.unreadable().len(), 2);
    }

    #[test]
    fn a_subdirectory_with_nothing_below_it_is_no_row() {
        let mut a = Walked::default();
        a.nodes.push(found("one"));
        a.nest(PathBuf::from("sub"), Walked::default());
        assert_eq!(outline(&a.nodes, 0), vec!["one"]);
    }

    #[test]
    fn a_subdirectory_holding_only_what_could_not_be_read_is_a_row() {
        // It is where the blind spot is, and the screen shows it there.
        let mut a = Walked::default();
        let b = Walked { nodes: vec![refused("sub/secret")] };
        a.nest(PathBuf::from("sub"), b);
        assert_eq!(outline(&a.nodes, 0), vec!["sub/", "  secret (permission denied)"]);
    }

    #[test]
    fn what_could_not_be_read_is_found_inside_directories_and_walked_bundles() {
        let mut alpha = md_entry(PathBuf::from("alpha.md")).unwrap();
        alpha.kind = EntryKind::Bundle { lead: None, inside: Some(vec![refused("alpha/forms.md")]) };
        let walked = Walked {
            nodes: vec![
                refused("top.md"),
                Node::Dir { path: PathBuf::from("sub"), children: vec![found("b"), refused("sub/deep")] },
                Node::Entry(alpha),
            ],
        };
        let where_: Vec<String> = walked.unreadable().iter().map(|(path, _)| path.display().to_string()).collect();
        assert_eq!(where_, vec!["top.md", "sub/deep", "alpha/forms.md"]);
        assert_eq!(names(&walked), vec!["b", "alpha"]);
    }

    #[test]
    fn every_entry_is_gathered_where_its_directory_stands() {
        let walked = Walked {
            nodes: vec![
                found("a"),
                Node::Dir {
                    path: PathBuf::from("sub"),
                    children: vec![
                        found("b"),
                        Node::Dir { path: PathBuf::from("sub/deep"), children: vec![found("c")] },
                    ],
                },
                found("d"),
            ],
        };
        assert_eq!(names(&walked), vec!["a", "b", "c", "d"]);
    }

    // --------------------------------------------------------------- md_files

    #[test]
    fn a_directory_that_cannot_be_opened_fails_rather_than_reporting_zero() {
        // The bug this milestone exists for: an unreadable directory used to
        // come back as an empty one.
        let dir = scratch("absent").join("nope");
        let err = md_files(&dir).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn an_empty_directory_reports_zero() {
        let dir = scratch("empty");
        let walked = md_files(&dir).unwrap();
        assert_eq!(walked.entries().len(), 0);
        assert_eq!(walked.unreadable().len(), 0);
    }

    #[test]
    fn only_markdown_files_are_counted() {
        let dir = scratch("mixed");
        write(&dir.join("kept.md"), "# kept");
        write(&dir.join("skipped.txt"), "not a document");
        write(&dir.join("README"), "no extension");
        let walked = md_files(&dir).unwrap();
        assert_eq!(names(&walked), vec!["kept"]);
    }

    #[test]
    fn a_directory_named_like_a_document_is_not_one() {
        let dir = scratch("oops");
        fs::create_dir_all(dir.join("oops.md")).unwrap();
        let walked = md_files(&dir).unwrap();
        assert_eq!(walked.entries().len(), 0);
    }

    #[test]
    fn one_level_only_is_read() {
        let dir = scratch("onelevel");
        write(&dir.join("top.md"), "# top");
        write(&dir.join("sub").join("below.md"), "# below");
        let walked = md_files(&dir).unwrap();
        assert_eq!(names(&walked), vec!["top"]);
    }

    // ---------------------------------------------------------------- md_tree

    #[test]
    fn a_tree_gathers_markdown_from_every_level() {
        let dir = scratch("tree");
        write(&dir.join("a.md"), "# a");
        write(&dir.join("sub").join("b.md"), "# b");
        write(&dir.join("sub").join("deep").join("c.md"), "# c");
        write(&dir.join("sub").join("notes.txt"), "ignored");

        let walked = md_tree(&dir).unwrap();
        let mut found = names(&walked);
        found.sort();
        assert_eq!(found, vec!["a", "b", "c"]);
        assert_eq!(walked.unreadable().len(), 0);
    }

    #[test]
    fn a_tree_keeps_the_directories_it_found_its_entries_in() {
        let dir = scratch("tree-shape");
        write(&dir.join("a.md"), "# a");
        write(&dir.join("sub").join("b.md"), "# b");
        write(&dir.join("sub").join("deep").join("c.md"), "# c");
        write(&dir.join("sub").join("notes.txt"), "ignored");
        write(&dir.join("text-only").join("notes.txt"), "no document here");
        fs::create_dir_all(dir.join("empty")).unwrap();

        // `text-only/` and `empty/` hold no Entry, so they are no row.
        let walked = md_tree(&dir).unwrap();
        assert_eq!(outline(&walked.nodes, 0), vec!["a", "sub/", "  b", "  deep/", "    c"]);
    }

    #[test]
    fn a_tree_passes_over_a_hidden_directory_without_counting_it() {
        // `session-retro/.pytest_cache/README.md` on this machine: the one
        // Markdown file in a hidden directory anywhere in the corpus.
        let dir = scratch("tree-hidden");
        write(&dir.join("a.md"), "# a");
        write(&dir.join(".pytest_cache").join("README.md"), "# generated");

        let walked = md_tree(&dir).unwrap();
        assert_eq!(outline(&walked.nodes, 0), vec!["a"]);
        assert_eq!(walked.unreadable().len(), 0);
    }

    #[test]
    fn a_tree_whose_own_directory_is_absent_fails() {
        let dir = scratch("treeabsent").join("nope");
        assert_eq!(md_tree(&dir).unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    // ------------------------------------------------------------ bundle_dirs

    #[test]
    fn a_bundle_directory_is_one_entry_however_many_files_it_holds() {
        let dir = scratch("bundles");
        write(&dir.join("alpha").join("SKILL.md"), "---\nname: alpha\n---\n");
        write(&dir.join("alpha").join("REFERENCE.md"), "# supporting");
        write(&dir.join("alpha").join("examples").join("one.md"), "# example");

        let walked = bundle_dirs(&dir).unwrap();
        assert_eq!(names(&walked), vec!["alpha"]);
    }

    #[test]
    fn a_bundle_missing_its_lead_is_shown_rather_than_hidden() {
        let dir = scratch("noleads");
        fs::create_dir_all(dir.join("beta")).unwrap();
        let walked = bundle_dirs(&dir).unwrap();
        assert_eq!(names(&walked), vec!["beta"]);
        match &walked.entries()[0].kind {
            EntryKind::Bundle { lead, .. } => assert!(lead.is_none()),
            EntryKind::File => panic!("a directory became a file entry"),
        }
    }

    #[test]
    fn a_loose_file_among_bundles_is_not_a_bundle() {
        let dir = scratch("loose");
        write(&dir.join("stray-notes.txt"), "loose");
        write(&dir.join("alpha").join("SKILL.md"), "---\nname: alpha\n---\n");
        let walked = bundle_dirs(&dir).unwrap();
        assert_eq!(names(&walked), vec!["alpha"]);
    }

    /// The rows below a Bundle's Lead, or `None` where it was not walked into.
    fn inside(entry: &Entry) -> Option<&[Node]> {
        match &entry.kind {
            EntryKind::Bundle { inside, .. } => inside.as_deref(),
            EntryKind::File => panic!("{} is a file, not a Bundle", entry.name),
        }
    }

    #[test]
    fn a_bundle_holds_its_supporting_files_as_a_tree_without_its_lead() {
        let dir = scratch("bundle-inside");
        let alpha = dir.join("alpha");
        write(&alpha.join("SKILL.md"), "---\nname: alpha\n---\n");
        write(&alpha.join("REFERENCE.md"), "# supporting");
        write(&alpha.join("examples").join("one.md"), "# example");
        write(&alpha.join("scripts").join("run.py"), "print()");
        write(&alpha.join("notes.txt"), "not a document");

        // The supporting files are rows below the Bundle, not Entries of the
        // Source: the count stays one.
        let walked = bundle_dirs(&dir).unwrap();
        assert_eq!(names(&walked), vec!["alpha"]);
        let rows = inside(walked.entries()[0]).unwrap();
        assert_eq!(outline(rows, 0), vec!["REFERENCE", "examples/", "  one"]);
    }

    #[test]
    fn a_lead_further_down_is_a_supporting_file() {
        // `synced/` on this machine holds eight skills, each with its own
        // `SKILL.md` two levels down. Only the Bundle's own Lead is its row.
        let dir = scratch("bundle-nested-lead");
        let alpha = dir.join("alpha");
        write(&alpha.join("SKILL.md"), "---\nname: alpha\n---\n");
        write(&alpha.join("pdf").join("SKILL.md"), "---\nname: pdf\n---\n");
        write(&alpha.join("pdf").join("forms.md"), "# forms");

        let walked = bundle_dirs(&dir).unwrap();
        let rows = inside(walked.entries()[0]).unwrap();
        assert_eq!(outline(rows, 0), vec!["pdf/", "  forms", "  pdf"]);
    }

    #[test]
    fn a_bundle_without_a_lead_holds_every_document() {
        let dir = scratch("bundle-inside-nolead");
        write(&dir.join("beta").join("README.md"), "# beta");

        let walked = bundle_dirs(&dir).unwrap();
        let rows = inside(walked.entries()[0]).unwrap();
        assert_eq!(outline(rows, 0), vec!["README"]);
    }

    #[test]
    fn a_bundle_holding_only_its_lead_has_nothing_below_it() {
        let dir = scratch("bundle-inside-leadonly");
        write(&dir.join("dream").join("SKILL.md"), "---\nname: dream\n---\n");

        let walked = bundle_dirs(&dir).unwrap();
        assert_eq!(inside(walked.entries()[0]).map(|rows| rows.len()), Some(0));
    }

    // --------------------------------------------------------- Source::entries

    #[test]
    fn the_lead_may_rename_the_row_the_directory_named() {
        let dir = scratch("renamed");
        write(
            &dir.join("renamed-dir").join("SKILL.md"),
            "---\nname: actual-name\ndescription: from the lead\n---\n",
        );

        let src = Source::new("skills", dir, Scope::Global, Walk::BundleDirs);
        let walked = src.entries().unwrap();
        assert_eq!(names(&walked), vec!["actual-name"]);
        assert_eq!(walked.entries()[0].description.as_deref(), Some("from the lead"));
    }

    #[test]
    fn an_empty_declared_name_leaves_the_filesystems_name_alone() {
        let dir = scratch("emptyname");
        write(&dir.join("keep-me").join("SKILL.md"), "---\nname:   \n---\n");

        let src = Source::new("skills", dir, Scope::Global, Walk::BundleDirs);
        let walked = src.entries().unwrap();
        assert_eq!(names(&walked), vec!["keep-me"]);
    }

    #[test]
    fn a_source_whose_directory_is_absent_reports_the_reason() {
        let src = Source::new(
            "gone",
            scratch("srcabsent").join("nope"),
            Scope::Global,
            Walk::MarkdownFiles,
        );
        assert_eq!(reason(src.entries().unwrap_err().kind()), "missing");
    }

    // ------------------------------------------------------ find_project_root

    #[test]
    fn a_directory_holding_a_marker_is_the_root() {
        let home = scratch("root-here");
        let proj = home.join("proj");
        fs::create_dir_all(proj.join(".git")).unwrap();
        assert_eq!(find_project_root(&proj, &home), Some(proj));
    }

    #[test]
    fn the_marker_is_looked_for_above_as_well() {
        let home = scratch("root-above");
        let proj = home.join("proj");
        let deep = proj.join("src").join("inner");
        fs::create_dir_all(&deep).unwrap();
        write(&proj.join("CLAUDE.md"), "# project");
        assert_eq!(find_project_root(&deep, &home), Some(proj));
    }

    #[test]
    fn without_a_marker_the_current_directory_is_the_root() {
        let home = scratch("root-none");
        let here = home.join("loose").join("deeper");
        fs::create_dir_all(&here).unwrap();
        assert_eq!(find_project_root(&here, &home), Some(here));
    }

    #[test]
    fn the_home_directory_itself_is_not_a_project() {
        let home = scratch("root-athome");
        assert_eq!(find_project_root(&home, &home), None);
    }

    #[test]
    fn a_directory_outside_home_is_not_a_project() {
        let home = scratch("root-outside");
        let elsewhere = scratch("root-elsewhere");
        assert_eq!(find_project_root(&elsewhere, &home), None);
    }

    // ------------------------------------------------------------------ search

    #[test]
    fn a_word_in_the_body_is_found_after_a_walk() {
        // The text has to survive `load_frontmatter`, which used to read the
        // file and drop it.
        let dir = scratch("search-body");
        write(&dir.join("rule.md"), "# Title\n\nbody says 검증\n");

        let src = Source::new("rules", dir, Scope::Global, Walk::MarkdownFiles);
        let walked = src.entries().unwrap();
        assert!(walked.entries()[0].matches(&words(&["검증"])));
        assert_eq!(hit(walked.entries()[0].first_hit(&words(&["검증"]))), Some(("", 3, "body says 검증")));
    }

    #[test]
    fn a_bundle_without_a_lead_has_no_text_to_search() {
        let dir = scratch("search-nolead");
        fs::create_dir_all(dir.join("beta")).unwrap();

        let src = Source::new("skills", dir, Scope::Global, Walk::BundleDirs);
        let walked = src.entries().unwrap();
        assert_eq!(walked.entries()[0].text, None);
        assert!(walked.entries()[0].matches(&words(&["beta"])));
        assert!(!walked.entries()[0].matches(&words(&["anything"])));
    }

    #[test]
    fn a_file_with_a_byte_that_is_not_utf8_is_still_read_and_searched() {
        // Found in review: one such byte made the whole file unreadable as a
        // string, and it dropped out of search without a trace.
        let dir = scratch("search-notutf8");
        fs::write(
            dir.join("broken.md"),
            b"---\nname: broken\ndescription: still read\n---\nmentions adr \xff here\n",
        )
        .unwrap();

        let src = Source::new("rules", dir, Scope::Global, Walk::MarkdownFiles);
        let walked = src.entries().unwrap();
        let entry = &walked.entries()[0];
        assert_eq!(entry.description.as_deref(), Some("still read"));
        assert!(entry.matches(&words(&["adr"])));
        assert_eq!(hit(entry.first_hit(&words(&["adr"]))), Some(("", 5, "mentions adr \u{fffd} here")));
    }

    #[test]
    fn a_word_in_a_supporting_file_is_found_after_a_walk() {
        let dir = scratch("search-supporting");
        write(&dir.join("alpha").join("SKILL.md"), "---\nname: alpha\n---\nlead\n");
        write(&dir.join("alpha").join("examples").join("one.md"), "intro\nmentions adr\n");

        let src = Source::new("skills", dir, Scope::Global, Walk::BundleDirs);
        let walked = src.entries().unwrap();
        let entry = walked.entries()[0];
        assert!(entry.matches(&words(&["adr"])));
        assert_eq!(hit(entry.first_hit(&words(&["adr"]))), Some(("one", 2, "mentions adr")));
    }

    // ------------------------------------------------------ what cannot be read
    //
    // Each of these holds a file or a directory open so that the Walk cannot
    // open it, and gives up — saying so on stderr — where that is impossible.

    #[test]
    fn a_document_that_will_not_open_is_a_row_saying_so() {
        // Before M7 it was an Entry with no text: its row read `-`, as if it
        // had no description, and a search passed it over without a word.
        let dir = scratch("unread-file");
        write(&dir.join("kept.md"), "# kept");
        write(&dir.join("locked.md"), "# locked");
        let Some(_held) = hold(&dir.join("locked.md")) else { return };

        let walked = md_files(&dir).unwrap();
        assert_eq!(names(&walked), vec!["kept"]);
        let unread = walked.unreadable();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].0, dir.join("locked.md"));
        assert_eq!(reason(unread[0].1), "unreadable");
    }

    #[test]
    fn a_subdirectory_that_will_not_open_is_a_row_where_it_was_found() {
        // Until M7 no test reached this: the count it added to was checked by
        // a fixture script outside the repository.
        let dir = scratch("unread-subdir");
        write(&dir.join("a.md"), "# a");
        write(&dir.join("sub").join("b.md"), "# b");
        write(&dir.join("sub").join("secret").join("c.md"), "# c");
        let Some(_held) = hold(&dir.join("sub").join("secret")) else { return };

        let walked = md_tree(&dir).unwrap();
        assert_eq!(outline(&walked.nodes, 0), vec!["a", "sub/", "  b", "  secret (unreadable)"]);
        assert_eq!(walked.unreadable().len(), 1);
    }

    #[test]
    fn a_bundle_whose_lead_will_not_open_is_a_row_saying_so() {
        let dir = scratch("unread-lead");
        write(&dir.join("alpha").join("SKILL.md"), "---\nname: alpha\n---\n");
        write(&dir.join("alpha").join("REFERENCE.md"), "# supporting");
        let Some(_held) = hold(&dir.join("alpha").join("SKILL.md")) else { return };

        // The Lead is what names and shows a Bundle.
        let walked = bundle_dirs(&dir).unwrap();
        assert_eq!(outline(&walked.nodes, 0), vec!["alpha (unreadable)"]);
    }

    #[test]
    fn a_supporting_file_that_will_not_open_is_a_row_inside_its_bundle() {
        let dir = scratch("unread-supporting");
        write(&dir.join("alpha").join("SKILL.md"), "---\nname: alpha\n---\n");
        write(&dir.join("alpha").join("REFERENCE.md"), "# supporting");
        write(&dir.join("alpha").join("FORMS.md"), "# forms");
        let Some(_held) = hold(&dir.join("alpha").join("FORMS.md")) else { return };

        let walked = bundle_dirs(&dir).unwrap();
        let rows = inside(walked.entries()[0]).unwrap();
        assert_eq!(outline(rows, 0), vec!["FORMS.md (unreadable)", "REFERENCE"]);
        assert_eq!(walked.unreadable().len(), 1);
    }

    #[test]
    fn a_bundle_directory_that_will_not_open_holds_a_row_saying_so() {
        let dir = scratch("unread-bundle");
        write(&dir.join("alpha").join("SKILL.md"), "---\nname: alpha\n---\n");
        write(&dir.join("alpha").join("REFERENCE.md"), "# supporting");
        let Some(_held) = hold(&dir.join("alpha")) else { return };

        let walked = bundle_dirs(&dir).unwrap();
        assert_eq!(names(&walked), vec!["alpha"]);
        let rows = inside(walked.entries()[0]).unwrap();
        assert_eq!(outline(rows, 0), vec!["alpha (unreadable)"]);
        assert_eq!(walked.unreadable().len(), 1);
    }

    // -------------------------------------------------------------------links

    #[test]
    fn a_markdown_file_that_is_a_link_is_still_a_document() {
        let dir = scratch("link-md");
        write(&dir.join("real.md"), "# real");
        if !link_file(&dir.join("real.md"), &dir.join("linked.md")) {
            return;
        }

        let walked = md_files(&dir).unwrap();
        let mut found = names(&walked);
        found.sort();
        assert_eq!(found, vec!["linked", "real"]);
        assert_eq!(walked.unreadable().len(), 0);
    }

    #[test]
    fn a_linked_directory_is_not_walked_into() {
        let dir = scratch("link-escape");
        write(&dir.join("tree").join("own.md"), "# own");
        write(&dir.join("outside").join("foreign.md"), "# foreign");
        if !link_dir(&dir.join("outside"), &dir.join("tree").join("shortcut")) {
            return;
        }

        // `foreign` lives behind the link and belongs to the directory the link
        // points at, not to the one being walked.
        let walked = md_tree(&dir.join("tree")).unwrap();
        assert_eq!(names(&walked), vec!["own"]);
    }

    #[test]
    fn a_directory_linked_to_itself_does_not_loop() {
        let dir = scratch("link-cycle");
        let tree = dir.join("tree");
        write(&tree.join("a.md"), "# a");
        if !link_dir(&tree, &tree.join("self")) {
            return;
        }

        // Following this link once costs nothing; following it every time turned
        // a two-file directory into 128 rows before M3.
        let walked = md_tree(&tree).unwrap();
        assert_eq!(names(&walked), vec!["a"]);
    }

    #[test]
    fn a_bundle_reached_through_a_link_keeps_its_lead() {
        let dir = scratch("link-bundle");
        let skills = dir.join("skills");
        fs::create_dir_all(&skills).unwrap();
        write(
            &dir.join("target").join("SKILL.md"),
            "---\nname: through-a-link\ndescription: read past the link\n---\n",
        );
        if !link_dir(&dir.join("target"), &skills.join("linked")) {
            return;
        }

        let src = Source::new("skills", skills, Scope::Global, Walk::BundleDirs);
        let walked = src.entries().unwrap();
        assert_eq!(names(&walked), vec!["through-a-link"]);
        assert_eq!(walked.entries()[0].description.as_deref(), Some("read past the link"));
    }

    #[test]
    fn a_bundle_reached_through_a_link_is_not_walked_into() {
        // `grill-with-docs` on this machine is such a link: its Lead is read,
        // and its two supporting files are listed under the Source that owns
        // the directory, not under this one.
        let dir = scratch("link-bundle-inside");
        let skills = dir.join("skills");
        fs::create_dir_all(&skills).unwrap();
        write(&dir.join("target").join("SKILL.md"), "---\nname: linked\n---\n");
        write(&dir.join("target").join("FORMAT.md"), "# supporting");
        if !link_dir(&dir.join("target"), &skills.join("linked")) {
            return;
        }

        let walked = bundle_dirs(&skills).unwrap();
        assert!(inside(walked.entries()[0]).is_none());
    }

    #[test]
    fn a_bundle_reached_through_a_link_is_searched_by_its_lead_alone() {
        // What is behind the link is searched through the Source that owns
        // it — `agents/skills`, for `grill-with-docs` on this machine.
        let dir = scratch("link-bundle-search");
        let skills = dir.join("skills");
        fs::create_dir_all(&skills).unwrap();
        write(&dir.join("target").join("SKILL.md"), "---\nname: linked\n---\nlead\n");
        write(&dir.join("target").join("FORMAT.md"), "mentions adr\n");
        if !link_dir(&dir.join("target"), &skills.join("linked")) {
            return;
        }

        let walked = bundle_dirs(&skills).unwrap();
        assert!(walked.entries()[0].matches(&words(&["lead"])));
        assert!(!walked.entries()[0].matches(&words(&["adr"])));
    }

    #[test]
    fn a_bundle_whose_link_target_is_gone_is_still_listed() {
        let dir = scratch("link-dangling");
        let skills = dir.join("skills");
        fs::create_dir_all(&skills).unwrap();
        let target = dir.join("target");
        write(&target.join("SKILL.md"), "---\nname: ghost\n---\n");
        if !link_dir(&target, &skills.join("ghost")) {
            return;
        }
        fs::remove_dir_all(&target).unwrap();

        // `path.is_dir()` answers false here, which is how this row used to
        // disappear from the count without leaving anything behind.
        let walked = bundle_dirs(&skills).unwrap();
        assert_eq!(names(&walked), vec!["ghost"]);
        match &walked.entries()[0].kind {
            EntryKind::Bundle { lead, inside } => assert!(lead.is_none() && inside.is_none()),
            EntryKind::File => panic!("a dangling directory link became a file entry"),
        }
    }

    #[test]
    fn a_link_to_a_file_is_not_a_bundle() {
        let dir = scratch("link-not-bundle");
        let skills = dir.join("skills");
        write(&skills.join("real").join("SKILL.md"), "---\nname: real\n---\n");
        write(&dir.join("loose.md"), "# loose");
        if !link_file(&dir.join("loose.md"), &skills.join("posing")) {
            return;
        }

        let walked = bundle_dirs(&skills).unwrap();
        assert_eq!(names(&walked), vec!["real"]);
    }
}
