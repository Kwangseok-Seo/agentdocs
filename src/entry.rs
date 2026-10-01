use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::frontmatter::parse_frontmatter;

#[derive(Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub kind: EntryKind,
    pub description: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum EntryKind {
    File,
    /// A directory made of a Lead and the supporting files it carries. `inside`
    /// holds the rows below the Lead, and is `None` for a link standing in for
    /// a Bundle: it is listed, and what is inside it is not asked (ADR-0007).
    Bundle { lead: Option<PathBuf>, inside: Option<Vec<Node>> },
}

/// One row of what a Walk found: an Entry, a directory with something below
/// it, or something the Walk could not look at.
///
/// A directory holds any number of rows, and a `Vec` is what holds them. It
/// also keeps a `Node` one fixed size: the rows live elsewhere, and a `Node`
/// carries only where they are. A `Node` held directly inside a `Node` would
/// never end, and the compiler refuses it (E0072) — as it does when the way
/// back to `Node` runs through `Entry` and a Bundle's `inside`.
#[derive(Debug, PartialEq)]
pub enum Node {
    Entry(Entry),
    Dir { path: PathBuf, children: Vec<Node> },
    /// Something the Walk found and could not look at, where it found it: a
    /// document that would not open, a directory that would not open, or an
    /// entry the system would not describe (ADR-0006).
    Unreadable { path: PathBuf, reason: io::ErrorKind },
}

impl Node {
    /// Where this row is on disk: the directory, the Entry's own path, or
    /// where the thing that could not be read is.
    pub fn path(&self) -> &Path {
        match self {
            Node::Entry(entry) => &entry.path,
            Node::Dir { path, .. } => path,
            Node::Unreadable { path, .. } => path,
        }
    }

    /// The rows below this one: a directory's, or those of a Bundle that was
    /// walked into. A file has none, and neither has a link standing in for a
    /// Bundle — what is behind it was never asked — nor anything that could
    /// not be read.
    pub fn children(&self) -> Option<&[Node]> {
        match self {
            Node::Dir { children, .. } => Some(children),
            Node::Entry(Entry { kind: EntryKind::Bundle { inside: Some(rows), .. }, .. }) => Some(rows),
            Node::Entry(_) | Node::Unreadable { .. } => None,
        }
    }
}

impl Entry {
    /// The file whose frontmatter describes this Entry: the file itself, or the
    /// Bundle's Lead. A Bundle without a Lead has nothing to read.
    pub fn doc(&self) -> Option<&Path> {
        match &self.kind {
            EntryKind::File => Some(&self.path),
            EntryKind::Bundle { lead: Some(lead), .. } => Some(lead),
            EntryKind::Bundle { lead: None, .. } => None,
        }
    }

    /// Let the frontmatter name and describe the Entry. A file that carries no
    /// frontmatter leaves the name taken from the filesystem in place; one that
    /// cannot be read is an error, for the Walk to show as unreadable.
    pub fn load_frontmatter(&mut self) -> io::Result<()> {
        let Some(doc) = self.doc() else { return Ok(()) };
        let bytes = fs::read(doc)?;

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
        Ok(())
    }

    /// The supporting files of a Bundle that was walked into, in the order the
    /// Walk found them. A file has none, and neither has a link standing in
    /// for a Bundle: what is behind it was never asked (ADR-0007).
    pub fn supporting(&self) -> Vec<&Entry> {
        let mut out = Vec::new();
        if let EntryKind::Bundle { inside: Some(rows), .. } = &self.kind {
            gather(rows, &mut out);
        }
        out
    }

    /// Whether every search term appears somewhere in this Entry: in its name,
    /// in the text of the file behind it, or — in a Bundle — in the name or
    /// text of any of its supporting files. With no terms at all, every Entry
    /// matches — `all` of nothing is true.
    pub fn matches(&self, terms: &[String]) -> bool {
        let supporting = self.supporting();
        terms.iter().all(|t| self.holds(t) || supporting.iter().any(|entry| entry.holds(t)))
    }

    /// Whether `term` is in this Entry's name or in the text of its own file.
    fn holds(&self, term: &str) -> bool {
        self.name.to_lowercase().contains(term)
            || self.text.as_deref().is_some_and(|text| text.to_lowercase().contains(term))
    }

    /// The first line that holds any of the terms, so that a row can show why
    /// it matched: in the Entry's own file, or else in the first of its
    /// supporting files that has one. `None` when there is no text to look in,
    /// when only a name matched, or when a term that holds a line break was
    /// found in a text but, by definition, on no single line.
    pub fn first_hit(&self, terms: &[String]) -> Option<Hit<'_>> {
        if let Some((number, line)) = self.own_hit(terms) {
            return Some(Hit { within: None, number, line });
        }
        self.supporting().into_iter().find_map(|entry| {
            let (number, line) = entry.own_hit(terms)?;
            Some(Hit { within: Some(entry), number, line })
        })
    }

    /// The first line of this Entry's own file that holds any of the terms,
    /// numbered from 1.
    fn own_hit(&self, terms: &[String]) -> Option<(usize, &str)> {
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

/// The first line of an Entry that holds a search term.
#[derive(Debug)]
pub struct Hit<'a> {
    /// The supporting file the line is in, or `None` for the Entry's own file.
    pub within: Option<&'a Entry>,
    /// Where the line is in its file, counted from 1.
    pub number: usize,
    pub line: &'a str,
}

/// Add every Entry among `nodes` to `out`, going down into each directory —
/// each directory's Entries in the place the directory stands.
pub fn gather<'a>(nodes: &'a [Node], out: &mut Vec<&'a Entry>) {
    for node in nodes {
        match node {
            Node::Entry(entry) => out.push(entry),
            Node::Dir { children, .. } => gather(children, out),
            Node::Unreadable { .. } => {}
        }
    }
}

/// Add where each thing among `nodes` that could not be read is, and why, to
/// `out`, going down into each directory and into each Bundle that was walked
/// into.
pub fn unread<'a>(nodes: &'a [Node], out: &mut Vec<(&'a Path, io::ErrorKind)>) {
    for node in nodes {
        if let Node::Unreadable { path, reason } = node {
            out.push((path, *reason));
        } else if let Some(rows) = node.children() {
            unread(rows, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    // ------------------------------------------------------------------ search

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
        assert_eq!(hit(entry.first_hit(&words(&["adr"]))), Some(("", 2, "see ADR here")));
    }

    #[test]
    fn any_single_term_is_enough_for_a_line() {
        let entry = entry_with("notes", Some("intro\n검증 only\nadr only"));
        assert_eq!(hit(entry.first_hit(&words(&["adr", "검증"]))), Some(("", 2, "검증 only")));
    }

    #[test]
    fn with_no_terms_there_is_no_line_to_show() {
        // `any` of nothing is false: no line is printed under any row.
        let entry = entry_with("notes", Some("intro\nbody"));
        assert_eq!(hit(entry.first_hit(&words(&[]))), None);
    }

    #[test]
    fn an_entry_without_text_has_no_line() {
        assert_eq!(hit(entry_with("alpha", None).first_hit(&words(&["alpha"]))), None);
    }

    #[test]
    fn a_match_in_the_name_alone_has_no_line() {
        // Seen in the real corpus: `0005-http-error-surface` has "error" in
        // its file name and only the Korean word for it in its text.
        let entry = entry_with("http-error-surface", Some("에러 표면"));
        assert!(entry.matches(&words(&["error"])));
        assert_eq!(hit(entry.first_hit(&words(&["error"]))), None);
    }

    #[test]
    fn a_term_spanning_a_line_break_matches_but_shows_no_line() {
        // The whole text holds "part\nsecond"; no single line can.
        let entry = entry_with("notes", Some("first part\nsecond part"));
        assert!(entry.matches(&words(&["part\nsecond"])));
        assert_eq!(hit(entry.first_hit(&words(&["part\nsecond"]))), None);
    }

    // ------------------------------------------------------- search in a Bundle

    /// `alpha`, a Bundle whose Lead holds `lead`, with two supporting files:
    /// `REFERENCE` holding `reference`, and below it, in `examples/`, `one`
    /// holding `one`.
    fn alpha(lead: &str, reference: &str, one: &str) -> Entry {
        bundle_with(
            "alpha",
            Some(lead),
            Some(vec![
                Node::Entry(entry_with("REFERENCE", Some(reference))),
                Node::Dir {
                    path: PathBuf::from("alpha/examples"),
                    children: vec![Node::Entry(entry_with("one", Some(one)))],
                },
            ]),
        )
    }

    #[test]
    fn a_term_in_a_supporting_file_keeps_its_bundle() {
        let bundle = alpha("lead", "reference", "mentions adr");
        assert!(bundle.matches(&words(&["adr"])));
        assert!(!bundle.matches(&words(&["zzz"])));
    }

    #[test]
    fn each_term_may_be_in_a_different_file_of_a_bundle() {
        let bundle = alpha("mentions adr", "reference", "mentions 검증");
        assert!(bundle.matches(&words(&["adr", "검증"])));
        assert!(!bundle.matches(&words(&["adr", "zzz"])));
    }

    #[test]
    fn a_supporting_files_name_is_searched_as_its_text_is() {
        let bundle = alpha("lead", "reference", "example");
        assert!(bundle.matches(&words(&["one"])));
    }

    #[test]
    fn a_line_of_the_leads_own_comes_before_any_supporting_file() {
        let bundle = alpha("lead mentions adr", "adr too", "adr as well");
        assert_eq!(hit(bundle.first_hit(&words(&["adr"]))), Some(("", 1, "lead mentions adr")));
    }

    #[test]
    fn a_line_from_a_supporting_file_says_which_file_it_is_in() {
        // `REFERENCE` comes before `one`, which is further down the tree.
        let bundle = alpha("lead", "intro\nadr here", "adr as well");
        assert_eq!(hit(bundle.first_hit(&words(&["adr"]))), Some(("REFERENCE", 2, "adr here")));
        let bundle = alpha("lead", "reference", "x\ny\nadr down here");
        assert_eq!(hit(bundle.first_hit(&words(&["adr"]))), Some(("one", 3, "adr down here")));
    }

    #[test]
    fn a_bundle_matched_by_a_supporting_files_name_alone_has_no_line() {
        let bundle = alpha("lead", "reference", "example");
        assert_eq!(hit(bundle.first_hit(&words(&["one"]))), None);
    }
}
