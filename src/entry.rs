use std::fs;
use std::path::{Path, PathBuf};

use crate::frontmatter::parse_frontmatter;

#[derive(Debug)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub kind: EntryKind,
    pub description: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug)]
pub enum EntryKind {
    File,
    /// A directory made of a Lead and the supporting files it carries. `inside`
    /// holds the rows below the Lead, and is `None` for a link standing in for
    /// a Bundle: it is listed, and what is inside it is not asked (ADR-0007).
    Bundle { lead: Option<PathBuf>, inside: Option<Vec<Node>> },
}

/// One row of what a Walk found: an Entry, or a directory with at least one
/// Entry somewhere below it.
///
/// A directory holds any number of rows, and a `Vec` is what holds them. It
/// also keeps a `Node` one fixed size: the rows live elsewhere, and a `Node`
/// carries only where they are. A `Node` held directly inside a `Node` would
/// never end, and the compiler refuses it (E0072) — as it does when the way
/// back to `Node` runs through `Entry` and a Bundle's `inside`.
#[derive(Debug)]
pub enum Node {
    Entry(Entry),
    Dir { path: PathBuf, children: Vec<Node> },
}

impl Node {
    /// Where this row is on disk: the directory, or the Entry's own path.
    pub fn path(&self) -> &Path {
        match self {
            Node::Entry(entry) => &entry.path,
            Node::Dir { path, .. } => path,
        }
    }

    /// The rows below this one: a directory's, or those of a Bundle that was
    /// walked into. A file has none, and neither has a link standing in for a
    /// Bundle — what is behind it was never asked.
    pub fn children(&self) -> Option<&[Node]> {
        match self {
            Node::Dir { children, .. } => Some(children),
            Node::Entry(Entry { kind: EntryKind::Bundle { inside: Some(rows), .. }, .. }) => Some(rows),
            Node::Entry(_) => None,
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

    /// Let the frontmatter name and describe the Entry. A file that cannot be
    /// read, or that carries no frontmatter, leaves the name taken from the
    /// filesystem in place.
    pub fn load_frontmatter(&mut self) {
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
    pub fn matches(&self, terms: &[String]) -> bool {
        let name = self.name.to_lowercase();
        let text = self.text.as_deref().unwrap_or("").to_lowercase();

        terms.iter().all(|t| text.contains(t) || name.contains(t))
    }

    /// The first line of the file that holds any of the terms, numbered from 1,
    /// so that a row can show why it matched. `None` when there is no text to
    /// look in, when only the name matched, or when a term that holds a line
    /// break was found in the text but, by definition, on no single line.
    pub fn first_hit(&self, terms: &[String]) -> Option<(usize, &str)> {
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

#[cfg(test)]
mod tests {
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
}
