use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::entry::{Entry, EntryKind, Node};

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
        let Ok(item) = item else { out.unreadable += 1; continue };
        let Ok(ft) = item.file_type() else { out.unreadable += 1; continue };
        let path = item.path();
        // A real directory is not a document, whatever it is named. A link is
        // left to `md_entry`, which decides by name: going to see what it points
        // at is walking into it, and a link is never walked into.
        if ft.is_dir() { continue };
        if let Some(entry) = document(path) { out.nodes.push(Node::Entry(entry)); }
    }
    Ok(out)
}

fn md_tree(dir: &Path) -> io::Result<Walked> {
    let mut out = Walked::default();
    let read = fs::read_dir(dir)?;

    for item in read {
        let Ok(item) = item else { out.unreadable += 1; continue };
        let Ok(ft) = item.file_type() else { out.unreadable += 1; continue };
        let path = item.path();
        if ft.is_dir() {
            match md_tree(&path) {
                Ok(sub) => out.nest(path, sub),
                Err(_) => out.unreadable += 1,
            }
        } else if let Some(entry) = document(path) {
            // Not a directory, so a candidate document — a link included, since
            // it is listed by name rather than followed.
            out.nodes.push(Node::Entry(entry));
        }
    }
    Ok(out)
}

fn bundle_dirs(dir: &Path) -> io::Result<Walked> {
    let mut out = Walked::default();
    let read = fs::read_dir(dir)?;

    for item in read {
        let Ok(item) = item else { out.unreadable += 1; continue };
        let Ok(ft) = item.file_type() else { out.unreadable += 1; continue };
        let path = item.path();
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
        // asked at all — the entry itself says which it is (ADR-0007).
        let inside = if ft.is_dir() {
            match supporting(&path, lead.as_deref()) {
                Ok(sub) => {
                    out.unreadable += sub.unreadable;
                    Some(sub.nodes)
                }
                Err(_) => {
                    out.unreadable += 1;
                    Some(Vec::new())
                }
            }
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
        entry.load_frontmatter();
        out.nodes.push(Node::Entry(entry));
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
        Node::Dir { .. } => true,
    });
    Ok(walked)
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

/// What one Walk found: the Entries it could list, in the shape of the
/// directories it found them in, and how many things it could not read. A
/// count that hides its own blind spots is a wrong count.
#[derive(Debug, Default)]
pub struct Walked {
    pub nodes: Vec<Node>,
    pub unreadable: usize,
}

impl Walked {
    /// Keep a subdirectory's findings as one row that holds them. A directory
    /// with no Entry anywhere below it is not a row, but what could not be
    /// read down there still counts.
    fn nest(&mut self, path: PathBuf, sub: Walked) {
        self.unreadable += sub.unreadable;
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
}

/// Add every Entry among `nodes` to `out`, going down into each directory.
fn gather<'a>(nodes: &'a [Node], out: &mut Vec<&'a Entry>) {
    for node in nodes {
        match node {
            Node::Entry(entry) => out.push(entry),
            Node::Dir { children, .. } => gather(children, out),
        }
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
/// frontmatter where it has one.
fn document(path: PathBuf) -> Option<Entry> {
    let mut entry = md_entry(path)?;
    entry.load_frontmatter();
    Some(entry)
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
    /// Entry by its name. Each level is sorted, since the order `read_dir`
    /// gives is the platform's.
    fn outline(nodes: &[Node], depth: usize) -> Vec<String> {
        let mut level: Vec<(String, Vec<String>)> = nodes
            .iter()
            .map(|node| match node {
                Node::Entry(entry) => (entry.name.clone(), Vec::new()),
                Node::Dir { path, children } => (
                    format!("{}/", path.file_name().unwrap().to_string_lossy()),
                    outline(children, depth + 1),
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

    #[test]
    fn nesting_keeps_a_subdirectory_as_one_row_and_adds_its_blind_spots() {
        let mut a = Walked::default();
        a.nodes.push(found("one"));
        a.unreadable = 1;

        let mut b = Walked::default();
        b.nodes.push(found("two"));
        b.unreadable = 2;

        a.nest(PathBuf::from("sub"), b);
        assert_eq!(outline(&a.nodes, 0), vec!["one", "sub/", "  two"]);
        assert_eq!(a.unreadable, 3);
    }

    #[test]
    fn a_subdirectory_with_nothing_listed_is_no_row_but_its_blind_spots_count() {
        let mut a = Walked::default();
        a.nodes.push(found("one"));

        let b = Walked { nodes: Vec::new(), unreadable: 2 };
        a.nest(PathBuf::from("sub"), b);
        assert_eq!(outline(&a.nodes, 0), vec!["one"]);
        assert_eq!(a.unreadable, 2);
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
            unreadable: 0,
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
        assert_eq!(walked.unreadable, 0);
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
        assert_eq!(walked.unreadable, 0);
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
        assert_eq!(walked.entries()[0].first_hit(&words(&["검증"])), Some((3, "body says 검증")));
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
        assert_eq!(entry.first_hit(&words(&["adr"])), Some((5, "mentions adr \u{fffd} here")));
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
        assert_eq!(walked.unreadable, 0);
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
