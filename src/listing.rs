use std::io;

use crate::entry::Entry;
use crate::source::Walked;

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

pub fn reason(kind: io::ErrorKind) -> &'static str {
    match kind {
        io::ErrorKind::NotFound => "missing",
        io::ErrorKind::PermissionDenied => "permission denied",
        io::ErrorKind::NotADirectory => "not a directory",
        _ => "unreadable",
    }
}

/// A walked Source's line: its name and how many Entries it holds — how many
/// were kept out of the whole, when searching — and how many things could not
/// be read. The listing and the screen both print it, so there is one wording.
pub fn heading(name: &str, walked: &Walked, terms: &[String]) -> String {
    let total = walked.entries.len();
    let mut heading = if terms.is_empty() {
        format!("  {name}:{total}")
    } else {
        let kept = walked.entries.iter().filter(|e| e.matches(terms)).count();
        format!("  {name}:{kept}/{total}")
    };
    if walked.unreadable > 0 {
        heading.push_str(&format!(" ({} unreadable)", walked.unreadable));
    }
    heading
}

/// A Source that could not be walked at all: its name and why.
pub fn failed(name: &str, err: &io::Error) -> String {
    format!("  {}:({})", name, reason(err.kind()))
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
pub fn listing(name: &str, walked: &Walked, terms: &[String]) -> Vec<String> {
    let mut lines = vec![heading(name, walked, terms)];
    for entry in walked.entries.iter().filter(|e| e.matches(terms)) {
        lines.push(row(entry));
        // Out of somebody else's file, so through `around` and with it
        // `printable`, like every other field on screen.
        if let Some((n, line)) = entry.first_hit(terms) {
            lines.push(format!("      {n}: {}", around(line.trim(), terms, 60)));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use crate::source::md_entry;
    use crate::testutil::*;

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
}
