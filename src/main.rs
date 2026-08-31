use std::fs;
use std::env;
use std::path::{Path, PathBuf};
use std::iter::Peekable;
use std::str::Lines;
use std::io;

enum Scope {
    Global,
    Project,
}

enum Walk {
    MarkdownFiles,
    BundleDirs,
    MarkdownTree,
}

#[derive(Debug)]
struct Entry {
    name: String,
    path: PathBuf,
    kind: EntryKind,
    description: Option<String>,
}

#[derive(Debug)]
enum EntryKind {
    File,
    Bundle { lead: Option<PathBuf> },
}

impl Entry {
    /// The file whose frontmatter describes this Entry: the file itself, or the
    /// Bundle's Lead. A Bundle without a Lead has nothing to read.
    fn doc(&self) -> Option<&Path> {
        match &self.kind {
            EntryKind::File => Some(&self.path),
            EntryKind::Bundle { lead: Some(lead) } => Some(lead),
            EntryKind::Bundle { lead: None } => None,
        }
    }

    /// Let the frontmatter name and describe the Entry. A file that cannot be
    /// read, or that carries no frontmatter, leaves the name taken from the
    /// filesystem in place.
    fn load_frontmatter(&mut self) {
        let Some(doc) = self.doc() else { return };
        let Ok(text) = fs::read_to_string(doc) else { return };

        let fm = parse_frontmatter(&text);
        if let Some(name) = fm.name {
            // An empty `name:` is not an identifier. A row with no name at all
            // cannot be told apart from any other row, so the name the
            // filesystem gave is better than what the file declared.
            if !name.trim().is_empty() {
                self.name = name;
            }
        }
        self.description = fm.description;
    }
}

struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
}

impl Frontmatter {
    /// Nothing was declared: every field absent.
    fn none() -> Self {
        Frontmatter { name: None, description: None }
    }
}

/// Read the YAML frontmatter that opens a Markdown file.
///
/// A field the file does not carry comes back as `None`, so that "absent" stays
/// distinguishable from "empty". A block that never closes counts as no
/// frontmatter at all, rather than letting the body be read as fields.
fn parse_frontmatter(text: &str) -> Frontmatter {
    // A byte-order mark sits invisibly in front of the opening delimiter and
    // would otherwise make the whole block unreadable.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    let mut lines = text.lines().peekable();

    let Some(first) = lines.next() else {
        return Frontmatter::none();                  // empty file
    };
    if first.trim_end() != "---" {
        return Frontmatter::none();                  // the file opens with something else
    }

    let mut fm = Frontmatter::none();

    while let Some(line) = lines.next() {
        if line.trim_end() == "---" {
            return fm;                               // closed: hand back what was gathered
        }

        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim();

        // ">" or "|", with an optional chomping indicator, says the value is not
        // on this line: it is the indented lines below, so take them now.
        let indicator = value.trim_end_matches(['-', '+']);

        let value = if indicator == ">" || indicator == "|" {
            fold(&mut lines)
        } else if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            // Strip one matching pair of quotes, and only a matching pair: a
            // value that merely ends in a quote has to keep it.
            value[1..value.len() - 1].to_string()
        } else {
            value.to_string()
        };

        match key.trim() {
            "name" => fm.name = Some(value),
            "description" => fm.description = Some(value),
            _ => {}                                  // model:, origin:, anything else
        }
    }

    Frontmatter::none()                              // the block never closed
}

/// Join the indented lines that carry a YAML block value into one line. The
/// block ends at the first line that is neither indented nor blank, and that
/// line is left for the caller to read as the next key.
fn fold(lines: &mut Peekable<Lines>) -> String {
    let mut out = String::new();

    while let Some(next) = lines.peek() {
        // Indented by anything at all, or blank. Testing for a space alone would
        // end the block on the first tab-indented line and lose the whole value.
        if !next.starts_with(char::is_whitespace) && !next.trim().is_empty() {
            break;                                   // a new key, or the closing ---
        }

        let Some(piece) = lines.next() else { break };
        let piece = piece.trim();
        if piece.is_empty() {
            continue;                                // a blank line inside the block
        }

        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(piece);
    }
    out
}

fn md_files(dir: &Path) -> io::Result<Walked> {
    let mut out = Walked::new();
    let read = fs::read_dir(dir)?;

    for item in read {
        let Ok(item) = item else { out.unreadable += 1; continue };
        let Ok(ft) = item.file_type() else { out.unreadable += 1; continue };
        let path = item.path();
        // A real directory is not a document, whatever it is named. A link is
        // left to `md_entry`, which decides by name: going to see what it points
        // at is walking into it, and a link is never walked into.
        if ft.is_dir() { continue };
        if let Some(entry) = md_entry(path) { out.entries.push(entry); }
    }
    Ok(out)
}

fn md_tree(dir: &Path) -> io::Result<Walked> {
    let mut out = Walked::new();
    let read = fs::read_dir(dir)?;

    for item in read {
        let Ok(item) = item else { out.unreadable += 1; continue };
        let Ok(ft) = item.file_type() else { out.unreadable += 1; continue };
        let path = item.path();
        if ft.is_dir() {
            match md_tree(&path) {
                Ok(sub) => out.absorb(sub),
                Err(_) => out.unreadable += 1,
            }
        } else if let Some(entry) = md_entry(path) {
            // Not a directory, so a candidate document — a link included, since
            // it is listed by name rather than followed.
            out.entries.push(entry);
        }
    }
    Ok(out)
}

fn bundle_dirs(dir: &Path) -> io::Result<Walked> {
    let mut out = Walked::new();
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

        out.entries.push(Entry {
            name: name.to_string_lossy().to_string(),
            path,
            kind: EntryKind::Bundle { lead },
            description: None,
        });
    }
    Ok(out)
}

struct Source {
    name: String,
    path: PathBuf,
    scope: Scope,
    walk: Walk,
}

impl Source {
    fn new(name: &str, path: PathBuf, scope: Scope, walk: Walk) -> Self {
        Source { name: name.to_string(), path: path, scope: scope, walk: walk }
    }

    fn entries(&self) -> io::Result<Walked> {
        let mut out = match self.walk {
            Walk::MarkdownFiles => md_files(&self.path),
            Walk::BundleDirs => bundle_dirs(&self.path),
            Walk::MarkdownTree => md_tree(&self.path),
        }?;

        for entry in &mut out.entries {
            entry.load_frontmatter();
        }
        Ok(out)
    }
}

fn find_project_root(start: &Path, home: &Path) -> Option<PathBuf> {
    // At or above the home directory there is no project
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

/// Drop the control characters from text that came out of somebody else's file.
/// An escape sequence would otherwise move the cursor or recolour the terminal,
/// and a carriage return would overwrite the row that had just been written.
///
/// Every field printed from a file goes through here, name as well as
/// description — the filter belongs to the source of the text, not to the
/// column it lands in.
fn printable(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

/// Fit a description onto one line. The cut counts characters, not bytes, so a
/// Korean description is never sliced through the middle of a character.
fn short(s: &str, width: usize) -> String {
    let text = printable(s);
    let mut out: String = text.chars().take(width).collect();
    if text.chars().count() > width {
        out.push('…');
    }
    out
}

fn reason(kind: io::ErrorKind) -> &'static str {
    match kind {
        io::ErrorKind::NotFound => "missing",
        io::ErrorKind::PermissionDenied => "permission denied",
        io::ErrorKind::NotADirectory => "not a directory",
        _ => "unreadable",
    }
}

/// What one Walk found: the Entries it could list, and how many things it could
/// not read. A count that hides its own blind spots is a wrong count.
#[derive(Debug)]
struct Walked {
    entries: Vec<Entry>,
    unreadable: usize,
}

impl Walked {
    fn new() -> Self {
        Walked { entries: Vec::new(), unreadable: 0 }
    }

    /// Fold a subdirectory's findings into this one.
    fn absorb(&mut self, other: Walked) {
        self.entries.extend(other.entries);
        self.unreadable += other.unreadable;
    }
}

/// A `.md` file becomes one Entry. Anything else is not a document.
fn md_entry(path: PathBuf) -> Option<Entry> {
    let ext = path.extension()?;
    if !ext.eq_ignore_ascii_case("md") {
        return None;
    }
    let name = path.file_stem()?.to_string_lossy().to_string();

    Some(Entry { name, path, kind: EntryKind::File, description: None })
}

/// One Entry's line on screen: the name it is known by, and as much of its
/// description as fits. Both halves come out of somebody else's file, so both
/// pass through `printable` — a function rather than two `println!` arms so that
/// a test can read the row the terminal would have been given.
fn row(entry: &Entry) -> String {
    match &entry.description {
        Some(text) => format!("    {:<32} {}", printable(&entry.name), short(text, 44)),
        None => format!("    {:<32} -", printable(&entry.name)),
    }
}

fn main() {
    let home = env::home_dir();
    let cwd = env::current_dir().ok();

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
                print!("  {}:{}", src.name, walked.entries.len());
                if walked.unreadable > 0 {
                    print!(" ({} unreadable)", walked.unreadable);
                }
                println!();
                for entry in &walked.entries {
                    println!("{}", row(entry));
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

    // ---------------------------------------------------------------- helpers

    /// A fresh directory under the system temp directory, named after the test
    /// that asked for it. Tests run on parallel threads, so two of them must
    /// never be handed the same path.
    fn scratch(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("agentdocs-test-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Write a file, creating the directories above it. `unwrap` is correct
    /// here: a fixture that cannot be built is a failing test, not a result.
    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn names(walked: &Walked) -> Vec<&str> {
        walked.entries.iter().map(|e| e.name.as_str()).collect()
    }

    // ------------------------------------------------------------------ reason

    #[test]
    fn an_absent_directory_reads_as_missing() {
        assert_eq!(reason(io::ErrorKind::NotFound), "missing");
    }

    #[test]
    fn a_forbidden_directory_says_it_was_forbidden() {
        assert_eq!(reason(io::ErrorKind::PermissionDenied), "permission denied");
    }

    #[test]
    fn a_file_where_a_directory_was_expected_says_so() {
        assert_eq!(reason(io::ErrorKind::NotADirectory), "not a directory");
    }

    #[test]
    fn a_failure_the_table_does_not_list_still_gets_a_word() {
        assert_eq!(reason(io::ErrorKind::TimedOut), "unreadable");
    }

    // ------------------------------------------------------------------- short

    #[test]
    fn a_description_that_fits_is_left_alone() {
        assert_eq!(short("hello", 10), "hello");
    }

    #[test]
    fn a_description_that_does_not_fit_is_cut_and_marked() {
        assert_eq!(short("hello", 3), "hel\u{2026}");
    }

    #[test]
    fn a_description_of_exactly_the_width_is_not_marked() {
        assert_eq!(short("hello", 5), "hello");
    }

    #[test]
    fn cutting_counts_characters_rather_than_bytes() {
        // Every one of these is three bytes in UTF-8. Cutting by bytes would
        // slice through the middle of a character and panic.
        assert_eq!(short("한국어입니다", 3), "한국어\u{2026}");
    }

    #[test]
    fn an_escape_sequence_in_somebody_elses_file_never_reaches_the_terminal() {
        assert_eq!(short("\u{1b}[31mred", 44), "[31mred");
        assert_eq!(short("a\tb", 44), "ab");
    }

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

    #[test]
    fn absorbing_adds_both_the_entries_and_the_blind_spots() {
        let mut a = Walked::new();
        a.entries.push(md_entry(PathBuf::from("one.md")).unwrap());
        a.unreadable = 1;

        let mut b = Walked::new();
        b.entries.push(md_entry(PathBuf::from("two.md")).unwrap());
        b.unreadable = 2;

        a.absorb(b);
        assert_eq!(a.entries.len(), 2);
        assert_eq!(a.unreadable, 3);
    }

    // ------------------------------------------------------- parse_frontmatter

    #[test]
    fn a_file_with_no_frontmatter_declares_nothing() {
        let fm = parse_frontmatter("# A title\n\nSome prose.\n");
        assert_eq!(fm.name, None);
        assert_eq!(fm.description, None);
    }

    #[test]
    fn an_empty_file_declares_nothing() {
        let fm = parse_frontmatter("");
        assert_eq!(fm.name, None);
        assert_eq!(fm.description, None);
    }

    #[test]
    fn a_closed_block_yields_its_fields() {
        let fm = parse_frontmatter("---\nname: alpha\ndescription: first skill\n---\nbody\n");
        assert_eq!(fm.name.as_deref(), Some("alpha"));
        assert_eq!(fm.description.as_deref(), Some("first skill"));
    }

    #[test]
    fn keys_the_viewer_does_not_use_are_ignored() {
        let fm = parse_frontmatter("---\nmodel: opus\norigin: elsewhere\n---\n");
        assert_eq!(fm.name, None);
        assert_eq!(fm.description, None);
    }

    #[test]
    fn a_block_that_never_closes_is_not_frontmatter() {
        // Without the guard the body below would be read as fields, which is a
        // silently wrong description rather than a crash.
        let fm = parse_frontmatter("---\nname: alpha\n\nbody line\n");
        assert_eq!(fm.name, None);
    }

    #[test]
    fn a_byte_order_mark_before_the_delimiter_is_ignored() {
        let fm = parse_frontmatter("\u{feff}---\nname: alpha\n---\n");
        assert_eq!(fm.name.as_deref(), Some("alpha"));
    }

    #[test]
    fn carriage_returns_do_not_break_the_delimiter() {
        let fm = parse_frontmatter("---\r\nname: alpha\r\n---\r\nbody\r\n");
        assert_eq!(fm.name.as_deref(), Some("alpha"));
    }

    #[test]
    fn a_folded_block_becomes_one_line() {
        let fm = parse_frontmatter("---\ndescription: >\n  one\n  two\n---\n");
        assert_eq!(fm.description.as_deref(), Some("one two"));
    }

    #[test]
    fn a_literal_block_becomes_one_line_too() {
        let fm = parse_frontmatter("---\ndescription: |\n  one\n  two\n---\n");
        assert_eq!(fm.description.as_deref(), Some("one two"));
    }

    #[test]
    fn a_chomping_indicator_is_still_a_block() {
        let fm = parse_frontmatter("---\ndescription: >-\n  one\n---\n");
        assert_eq!(fm.description.as_deref(), Some("one"));
    }

    #[test]
    fn a_blank_line_inside_a_block_does_not_end_it() {
        let fm = parse_frontmatter("---\ndescription: >\n  one\n\n  two\n---\n");
        assert_eq!(fm.description.as_deref(), Some("one two"));
    }

    #[test]
    fn a_block_ends_at_the_next_key_and_that_key_is_still_read() {
        let fm = parse_frontmatter("---\ndescription: >\n  one\nname: alpha\n---\n");
        assert_eq!(fm.description.as_deref(), Some("one"));
        assert_eq!(fm.name.as_deref(), Some("alpha"));
    }

    #[test]
    fn a_tab_indented_continuation_is_gathered() {
        // Testing for a leading space alone would end the block here and lose
        // the whole value.
        let fm = parse_frontmatter("---\ndescription: >\n\tone\n---\n");
        assert_eq!(fm.description.as_deref(), Some("one"));
    }

    #[test]
    fn a_matching_pair_of_quotes_is_stripped() {
        let fm = parse_frontmatter("---\ndescription: \"quoted value\"\n---\n");
        assert_eq!(fm.description.as_deref(), Some("quoted value"));
    }

    #[test]
    fn a_value_that_merely_ends_in_a_quote_keeps_it() {
        let fm = parse_frontmatter("---\ndescription: He said \"no\"\n---\n");
        assert_eq!(fm.description.as_deref(), Some("He said \"no\""));
    }

    #[test]
    fn a_declared_but_empty_value_is_not_the_same_as_an_absent_one() {
        let fm = parse_frontmatter("---\ndescription:\n---\n");
        assert_eq!(fm.description.as_deref(), Some(""));
        assert_eq!(fm.name, None);
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
        assert_eq!(walked.entries.len(), 0);
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
        assert_eq!(walked.entries.len(), 0);
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
        match &walked.entries[0].kind {
            EntryKind::Bundle { lead } => assert!(lead.is_none()),
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
        assert_eq!(walked.entries[0].description.as_deref(), Some("from the lead"));
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

    // --------------------------------------------------------------- printable

    #[test]
    fn a_control_character_in_a_name_never_reaches_the_terminal() {
        assert_eq!(printable("\u{1b}[31mRED\u{1b}[0m"), "[31mRED[0m");
        assert_eq!(printable("visible\rOVERWRITTEN"), "visibleOVERWRITTEN");
        assert_eq!(printable("a\tb"), "ab");
    }

    #[test]
    fn the_row_a_name_is_printed_on_carries_no_control_characters() {
        let mut entry = md_entry(PathBuf::from("notes.md")).unwrap();
        entry.name = "\u{1b}[31mRED".to_string();
        entry.description = Some("plain".to_string());

        let line = row(&entry);
        assert!(!line.chars().any(|c| c.is_control()), "row leaked a control character: {line:?}");
        assert!(line.contains("[31mRED"));
    }

    #[test]
    fn a_row_without_a_description_says_so_with_a_dash() {
        let entry = md_entry(PathBuf::from("notes.md")).unwrap();
        assert_eq!(row(&entry), format!("    {:<32} -", "notes"));
    }

    #[test]
    fn ordinary_text_passes_through_printable_unchanged() {
        assert_eq!(printable("session-retro"), "session-retro");
        assert_eq!(printable("한국어"), "한국어");
    }

    // -------------------------------------------------------------------links

    /// A directory link, or `false` when this account may not create one.
    /// Windows grants the privilege to administrators and to accounts with
    /// Developer Mode enabled. A test that cannot build its fixture says so on
    /// stderr rather than passing quietly — `cargo test -- --nocapture` shows it.
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

    fn link_file(target: &Path, link: &Path) -> bool {
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(target, link);
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(target, link);

        if let Err(e) = &made {
            eprintln!("SKIPPED {}: cannot create a file link ({:?})", link.display(), e.kind());
        }
        made.is_ok()
    }

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
        assert_eq!(walked.entries[0].description.as_deref(), Some("read past the link"));
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
        match &walked.entries[0].kind {
            EntryKind::Bundle { lead } => assert!(lead.is_none()),
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
