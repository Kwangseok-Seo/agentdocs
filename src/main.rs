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
    text: Option<String>,
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
        let Ok(bytes) = fs::read(doc) else { return };

        // One byte that is not UTF-8 — a quote pasted from another encoding —
        // would make `read_to_string` refuse the whole file, and the file would
        // silently drop out of every search. Decoded lossily, only that byte is
        // lost: it shows as `�`.
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        };

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
        self.text = Some(text);
    }

    /// Whether every search term appears somewhere in this Entry: in its name,
    /// or in the text of the file behind it. With no terms at all, every Entry
    /// matches — `all` of nothing is true.
    fn matches(&self, terms: &[String]) -> bool {
        let name = self.name.to_lowercase();
        let text = self.text.as_deref().unwrap_or("").to_lowercase();

        terms.iter().all(|t| text.contains(t) || name.contains(t))
    }

    /// The first line of the file that holds any of the terms, numbered from 1,
    /// so that a row can show why it matched. `None` when there is no text to
    /// look in, when only the name matched, or when a term that holds a line
    /// break was found in the text but, by definition, on no single line.
    fn first_hit(&self, terms: &[String]) -> Option<(usize, &str)> {
        let text = self.text.as_deref()?;

        text.lines()
            .enumerate()
            .find(|(_, line)| {
                let l = line.to_lowercase();
                terms.iter().any(|t| l.contains(t))
            })
            .map(|(i, line)| (i + 1, line))
    }
}

#[derive(Default)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
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
        return Frontmatter::default();               // empty file
    };
    if first.trim_end() != "---" {
        return Frontmatter::default();               // the file opens with something else
    }

    let mut fm = Frontmatter::default();

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

    Frontmatter::default()                              // the block never closed
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
        if let Some(entry) = md_entry(path) { out.entries.push(entry); }
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

        out.entries.push(Entry {
            name: name.to_string_lossy().to_string(),
            path,
            kind: EntryKind::Bundle { lead },
            description: None,
            text: None,
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

/// The stretch of a matched line worth showing. A line whose earliest term
/// already fits is shown from its start; otherwise it is shown from a little
/// before that term, so the reason for the match is never the part cut off.
fn around(line: &str, terms: &[String], width: usize) -> String {
    let lower = line.to_lowercase();

    // Where the earliest term starts in `lower`, and how long it is, in bytes.
    let earliest: Option<(usize, usize)> = terms
        .iter()
        .filter_map(|t| lower.find(t.as_str()).map(|at| (at, t.len())))
        .min();
    let Some((at, len)) = earliest else {
        return short(line, width);
    };

    // `at` and `len` are bytes of `lower`; the screen shows characters of `line`.
    if chars_before(line, at + len) <= width {
        return short(line, width);
    }
    let skip = chars_before(line, at).saturating_sub(10);
    let rest: String = line.chars().skip(skip).collect();
    format!("…{}", short(&rest, width))
}

/// How many characters of `line` start before byte `at` of its lowercase form.
///
/// Counting the characters of the lowercase copy instead would assume that
/// lowercasing keeps every character one character long, and it does not: `İ`
/// becomes `i` followed by a combining dot. So the original is walked one
/// character at a time, adding up how long each becomes once lowercased. (The
/// one character `str::to_lowercase` treats by context, a final `Σ`, lowercases
/// to two bytes either way.)
fn chars_before(line: &str, at: usize) -> usize {
    let mut lower_bytes = 0;
    let mut count = 0;
    for c in line.chars() {
        if lower_bytes >= at {
            break;
        }
        lower_bytes += c.to_lowercase().map(char::len_utf8).sum::<usize>();
        count += 1;
    }
    count
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
#[derive(Debug, Default)]
struct Walked {
    entries: Vec<Entry>,
    unreadable: usize,
}

impl Walked {
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

    Some(Entry { name, path, kind: EntryKind::File, description: None, text: None })
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

/// The lines one walked Source puts on screen: a heading with its count, then a
/// row for each Entry the search kept, each followed by the line that made it
/// match. Without search terms this is exactly the listing printed before M4 —
/// every Entry, the plain count, no matched lines. A function for the same
/// reason as `row`: a test can read what the terminal would have been given.
fn listing(name: &str, walked: &Walked, terms: &[String]) -> Vec<String> {
    let hits: Vec<&Entry> = walked.entries.iter()
        .filter(|e| e.matches(terms))
        .collect();

    // Searching shows how many of the whole were kept.
    let mut heading = if terms.is_empty() {
        format!("  {}:{}", name, walked.entries.len())
    } else {
        format!("  {}:{}/{}", name, hits.len(), walked.entries.len())
    };
    if walked.unreadable > 0 {
        heading.push_str(&format!(" ({} unreadable)", walked.unreadable));
    }

    let mut lines = vec![heading];
    for entry in hits {
        lines.push(row(entry));
        // Out of somebody else's file, so through `around` and with it
        // `printable`, like every other field on screen.
        if let Some((n, line)) = entry.first_hit(terms) {
            lines.push(format!("      {n}: {}", around(line.trim(), terms, 60)));
        }
    }
    lines
}

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
        let mut a = Walked::default();
        a.entries.push(md_entry(PathBuf::from("one.md")).unwrap());
        a.unreadable = 1;

        let mut b = Walked::default();
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

    // ------------------------------------------------------------------ search

    /// An Entry named `name`, as if its file had held `text`.
    fn entry_with(name: &str, text: Option<&str>) -> Entry {
        let mut entry = md_entry(PathBuf::from(format!("{name}.md"))).unwrap();
        entry.text = text.map(|t| t.to_string());
        entry
    }

    /// Search terms the way `main` hands them over: already lowercased.
    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn with_no_terms_every_entry_matches() {
        // `all` of nothing is true, which is what keeps a run without
        // arguments identical to the listing before M4.
        assert!(entry_with("alpha", Some("anything")).matches(&words(&[])));
        assert!(entry_with("beta", None).matches(&words(&[])));
    }

    #[test]
    fn every_term_has_to_appear_somewhere() {
        assert!(!entry_with("notes", Some("adr only")).matches(&words(&["adr", "검증"])));
        assert!(entry_with("notes", Some("adr and 검증")).matches(&words(&["adr", "검증"])));
    }

    #[test]
    fn one_term_in_the_name_and_another_in_the_text_is_a_match() {
        assert!(entry_with("dream", Some("mentions adr")).matches(&words(&["dream", "adr"])));
    }

    #[test]
    fn matching_ignores_the_case_of_the_entry() {
        let entry = entry_with("README", Some("Uses ADR"));
        assert!(entry.matches(&words(&["readme"])));
        assert!(entry.matches(&words(&["adr"])));
    }

    #[test]
    fn an_entry_without_text_is_matched_by_its_name_alone() {
        let entry = entry_with("alpha", None);
        assert!(entry.matches(&words(&["alpha"])));
        assert!(!entry.matches(&words(&["beta"])));
    }

    #[test]
    fn the_first_line_holding_a_term_is_numbered_from_one() {
        let entry = entry_with("notes", Some("intro\nsee ADR here\nADR again"));
        assert_eq!(entry.first_hit(&words(&["adr"])), Some((2, "see ADR here")));
    }

    #[test]
    fn any_single_term_is_enough_for_a_line() {
        let entry = entry_with("notes", Some("intro\n검증 only\nadr only"));
        assert_eq!(entry.first_hit(&words(&["adr", "검증"])), Some((2, "검증 only")));
    }

    #[test]
    fn with_no_terms_there_is_no_line_to_show() {
        // `any` of nothing is false: no line is printed under any row.
        let entry = entry_with("notes", Some("intro\nbody"));
        assert_eq!(entry.first_hit(&words(&[])), None);
    }

    #[test]
    fn an_entry_without_text_has_no_line() {
        assert_eq!(entry_with("alpha", None).first_hit(&words(&["alpha"])), None);
    }

    #[test]
    fn a_match_in_the_name_alone_has_no_line() {
        // Seen in the real corpus: `0005-http-error-surface` has "error" in
        // its file name and only the Korean word for it in its text.
        let entry = entry_with("http-error-surface", Some("에러 표면"));
        assert!(entry.matches(&words(&["error"])));
        assert_eq!(entry.first_hit(&words(&["error"])), None);
    }

    #[test]
    fn a_term_spanning_a_line_break_matches_but_shows_no_line() {
        // The whole text holds "part\nsecond"; no single line can.
        let entry = entry_with("notes", Some("first part\nsecond part"));
        assert!(entry.matches(&words(&["part\nsecond"])));
        assert_eq!(entry.first_hit(&words(&["part\nsecond"])), None);
    }

    #[test]
    fn a_word_in_the_body_is_found_after_a_walk() {
        // The text has to survive `load_frontmatter`, which used to read the
        // file and drop it.
        let dir = scratch("search-body");
        write(&dir.join("rule.md"), "# Title\n\nbody says 검증\n");

        let src = Source::new("rules", dir, Scope::Global, Walk::MarkdownFiles);
        let walked = src.entries().unwrap();
        assert!(walked.entries[0].matches(&words(&["검증"])));
        assert_eq!(walked.entries[0].first_hit(&words(&["검증"])), Some((3, "body says 검증")));
    }

    #[test]
    fn a_bundle_without_a_lead_has_no_text_to_search() {
        let dir = scratch("search-nolead");
        fs::create_dir_all(dir.join("beta")).unwrap();

        let src = Source::new("skills", dir, Scope::Global, Walk::BundleDirs);
        let walked = src.entries().unwrap();
        assert_eq!(walked.entries[0].text, None);
        assert!(walked.entries[0].matches(&words(&["beta"])));
        assert!(!walked.entries[0].matches(&words(&["anything"])));
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
        let entry = &walked.entries[0];
        assert_eq!(entry.description.as_deref(), Some("still read"));
        assert!(entry.matches(&words(&["adr"])));
        assert_eq!(entry.first_hit(&words(&["adr"])), Some((5, "mentions adr \u{fffd} here")));
    }

    // ------------------------------------------------------------------ around

    #[test]
    fn a_line_whose_term_fits_is_shown_from_its_start() {
        let line = "# 아키텍처 결정 기록 (ADR)";
        assert_eq!(around(line, &words(&["adr"]), 60), line);
    }

    #[test]
    fn a_term_past_the_width_is_brought_into_view_with_ten_characters_before_it() {
        let line = format!("{} adr tail", "x".repeat(100));
        assert_eq!(around(&line, &words(&["adr"]), 60), "…xxxxxxxxx adr tail");
    }

    #[test]
    fn whether_a_term_fits_is_counted_in_characters_not_bytes() {
        // 33 characters but 93 bytes: a byte count would move a line that fits.
        let line = format!("{}adr", "가".repeat(30));
        assert_eq!(around(&line, &words(&["adr"]), 60), line);
    }

    #[test]
    fn how_far_to_skip_is_counted_in_characters_not_bytes() {
        // The term starts at character 70 and byte 210. Skipping 200 characters
        // would skip the term along with everything else.
        let line = format!("{}adr", "가".repeat(70));
        assert_eq!(around(&line, &words(&["adr"]), 60), format!("…{}adr", "가".repeat(10)));
    }

    #[test]
    fn the_earliest_term_decides_where_the_window_starts() {
        let line = format!("{} early {} late", "x".repeat(80), "y".repeat(40));
        let shown = around(&line, &words(&["late", "early"]), 60);
        assert!(shown.starts_with("…xxxxxxxxx early"), "window missed the earliest term: {shown:?}");
    }

    #[test]
    fn a_line_without_any_term_falls_back_to_the_plain_cut() {
        let line = "x".repeat(100);
        assert_eq!(around(&line, &words(&["zzz"]), 60), short(&line, 60));
    }

    #[test]
    fn a_control_character_in_a_shown_line_never_reaches_the_terminal() {
        let near = "\u{1b}[31m adr";
        let far = format!("{}\u{1b}[31m adr", "x".repeat(100));
        for line in [near, far.as_str()] {
            let shown = around(line, &words(&["adr"]), 60);
            assert!(!shown.chars().any(|c| c.is_control()), "leaked a control character: {shown:?}");
            assert!(shown.contains("adr"));
        }
    }

    #[test]
    fn a_position_in_the_lowercase_copy_is_walked_back_to_the_original() {
        // `İ` lowercases to `i` plus a combining dot: three bytes, two
        // characters. In "İadr" the term starts at byte 3 of the lowercase
        // copy, which is after one character of the original.
        assert_eq!(chars_before("İadr", 3), 1);
        assert_eq!(chars_before("xadr", 1), 1);
        assert_eq!(chars_before("adr", 0), 0);
    }

    #[test]
    fn a_character_that_lowercases_longer_does_not_empty_the_window() {
        // Found in review: this line fits, but counting the lowercase copy put
        // the term at character 80 of a 48-character line, and all that was
        // left to show was `…`.
        let line = format!("{}adr tail", "İ".repeat(40));
        assert_eq!(around(&line, &words(&["adr"]), 60), line);
    }

    #[test]
    fn a_character_that_lowercases_longer_does_not_move_the_window_off_the_term() {
        // Found in review: the window landed in the filler, showing text that
        // did not contain the term as the reason for the match.
        let line = format!("{}adr {}", "İ".repeat(29), "FILLERTEXTNOMATCHHERE".repeat(4));
        let shown = around(&line, &words(&["adr"]), 60);
        assert!(shown.contains("adr"), "window missed the term: {shown:?}");
    }

    #[test]
    fn the_window_moves_by_characters_of_the_original_line() {
        let line = format!("{}adr", "İ".repeat(70));
        assert_eq!(around(&line, &words(&["adr"]), 60), format!("…{}adr", "İ".repeat(10)));
    }

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

    /// Two Entries: `alpha` mentions ADR in its first line, `beta` does not.
    fn two_entries(unreadable: usize) -> Walked {
        let mut alpha = entry_with("alpha", Some("ADR notes\nmore"));
        alpha.description = Some("first".to_string());
        let beta = entry_with("beta", Some("nothing here"));
        Walked { entries: vec![alpha, beta], unreadable }
    }

    #[test]
    fn without_terms_the_listing_is_the_one_printed_before_m4() {
        let walked = two_entries(1);
        assert_eq!(
            listing("rules", &walked, &words(&[])),
            vec![
                "  rules:2 (1 unreadable)".to_string(),
                row(&walked.entries[0]),
                row(&walked.entries[1]),
            ]
        );
    }

    #[test]
    fn a_search_shows_what_it_kept_out_of_the_whole_and_why() {
        let walked = two_entries(0);
        assert_eq!(
            listing("rules", &walked, &words(&["adr"])),
            vec![
                "  rules:1/2".to_string(),
                row(&walked.entries[0]),
                "      1: ADR notes".to_string(),
            ]
        );
    }

    #[test]
    fn a_search_that_keeps_nothing_still_shows_the_source() {
        let walked = two_entries(1);
        assert_eq!(listing("rules", &walked, &words(&["zzz"])), vec!["  rules:0/2 (1 unreadable)"]);
    }

    #[test]
    fn the_matched_line_is_brought_into_view() {
        let long = format!("{} adr tail", "x".repeat(100));
        let walked = Walked { entries: vec![entry_with("gamma", Some(&long))], unreadable: 0 };
        let lines = listing("docs", &walked, &words(&["adr"]));
        assert_eq!(lines[2], "      1: …xxxxxxxxx adr tail");
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
