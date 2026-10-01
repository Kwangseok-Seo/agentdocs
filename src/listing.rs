use std::io;
use std::path::Path;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::config::{self, Problem};
use crate::entry::Entry;
use crate::source::Walked;

/// The pieces of `text` a terminal would show, each with the columns it takes.
///
/// A piece is a grapheme: a character together with whatever joins it, like an
/// accent or the mark that makes `⚠` an emoji. Two kinds are left out. One
/// holding a control character: an escape sequence would move the cursor or
/// recolour the terminal, and a carriage return would overwrite the row just
/// written. And one taking no room of its own, like a right-to-left override
/// or a zero-width space, which change how the rest is laid out while showing
/// nothing. That is the rule ratatui follows as it writes the preview.
fn shown(text: &str) -> impl Iterator<Item = (&str, usize)> {
    text.graphemes(true)
        .map(|g| (g, g.width()))
        .filter(|&(g, width)| width > 0 && !g.contains(char::is_control))
}

/// Text that came out of somebody else's file, as a terminal may be given it.
///
/// Every field printed from a file goes through here, name as well as
/// description, on the screen as well as in the listing — the filter belongs
/// to the source of the text, not to the place it lands.
pub fn printable(s: &str) -> String {
    shown(s).map(|(g, _)| g).collect()
}

/// How many columns of a terminal `s` takes once it is printable.
fn columns(s: &str) -> usize {
    shown(s).map(|(_, w)| w).sum()
}

/// Fit a description onto `width` columns of one line. A Korean character
/// takes two columns, so the room is added up piece by piece rather than
/// counted — and the cut never falls inside a character.
fn short(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for (g, w) in shown(s) {
        used += w;
        if used > width {
            out.push('…');
            break;
        }
        out.push_str(g);
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

    // `at` and `len` are bytes of `lower`; the terminal shows columns of `line`.
    let through_term: String = line.chars().take(chars_before(line, at + len)).collect();
    if columns(&through_term) <= width {
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
    let entries = walked.entries();
    let total = entries.len();
    let mut heading = if terms.is_empty() {
        format!("  {name}:{total}")
    } else {
        let kept = entries.iter().filter(|e| e.matches(terms)).count();
        format!("  {name}:{kept}/{total}")
    };
    let unreadable = walked.unreadable().len();
    if unreadable > 0 {
        heading.push_str(&format!(" ({unreadable} unreadable)"));
    }
    heading
}

/// The line for something a Walk could not read: where it is, in full — the
/// listing has no tree to show where a name sits — and why. A path is made
/// of somebody else's names, so it passes through `printable` too.
fn unread_row(path: &Path, why: io::ErrorKind) -> String {
    format!("    {} ({})", printable(&path.to_string_lossy()), reason(why))
}

/// A Source that could not be walked at all: its name and why.
pub fn failed(name: &str, err: &io::Error) -> String {
    format!("  {}:({})", name, reason(err.kind()))
}

/// A config file that could not be used: its name and why, in the place of
/// the Sources it would have added. The listing and the screen both print it.
pub fn unused(problem: &Problem) -> String {
    let why = match problem {
        Problem::Read(e) => reason(e.kind()),
        Problem::Parse(_) | Problem::Order(_) => "invalid",
    };
    format!("  {}:({why})", config::FILE)
}

/// What stopped a config file being used, one line of the listing per line
/// it says, under the file's row. The parser quotes the line of the file it
/// stopped at, so it passes through `printable` like anything else out of a
/// file.
pub fn unused_said(problem: &Problem) -> Vec<String> {
    problem.to_string().lines().map(|line| format!("    {}", printable(line))).collect()
}

/// One Entry's line on screen: the name it is known by, and as much of its
/// description as fits. Both halves come out of somebody else's file, so both
/// pass through `printable` — a function rather than two `println!` arms so that
/// a test can read the row the terminal would have been given.
fn row(entry: &Entry) -> String {
    let name = printable(&entry.name);
    // `{:<32}` would pad to 32 characters, and a Korean name is twice as wide.
    let pad = " ".repeat(32usize.saturating_sub(columns(&name)));
    match &entry.description {
        Some(text) => format!("    {name}{pad} {}", short(text, 44)),
        None => format!("    {name}{pad} -"),
    }
}

/// The lines one walked Source puts on screen: a heading with its count, then a
/// row for each Entry the search kept, each followed by the line that made it
/// match, and last a row for each thing that could not be read — searched or
/// not, since what could not be read could not be ruled out either. Without
/// search terms and with nothing unreadable, this is exactly the listing
/// printed before M4. A function for the same reason as `row`: a test can read
/// what the terminal would have been given.
pub fn listing(name: &str, walked: &Walked, terms: &[String]) -> Vec<String> {
    let mut lines = vec![heading(name, walked, terms)];
    for entry in walked.entries().into_iter().filter(|e| e.matches(terms)) {
        lines.push(row(entry));
        // Out of somebody else's file, so through `around` and with it
        // `printable`, like every other field on screen. A line from one of a
        // Bundle's supporting files is marked with the name its row has on
        // screen: its path can run to a hundred characters, and a Bundle can
        // hold eight files called `SKILL.md`.
        if let Some(hit) = entry.first_hit(terms) {
            let at = match hit.within {
                None => hit.number.to_string(),
                Some(file) => format!("{}:{}", printable(&file.name), hit.number),
            };
            lines.push(format!("      {at}: {}", around(hit.line.trim(), terms, 60)));
        }
    }
    for (path, why) in walked.unreadable() {
        lines.push(unread_row(path, why));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use crate::entry::Node;
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
    fn cutting_counts_columns_rather_than_bytes_or_characters() {
        // Every one of these is three bytes in UTF-8 and two columns on
        // screen. Cutting by bytes would slice through the middle of a
        // character and panic; counting characters would print twice the room.
        assert_eq!(short("한국어입니다", 6), "한국어\u{2026}");
    }

    #[test]
    fn a_wide_character_that_would_cross_the_width_is_left_out_whole() {
        assert_eq!(short("한국어", 5), "한국\u{2026}");
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

    #[test]
    fn a_character_that_takes_no_room_of_its_own_never_reaches_the_terminal() {
        // A right-to-left override and a zero-width space.
        assert_eq!(printable("left\u{202e}right\u{200b}gap"), "leftrightgap");
    }

    #[test]
    fn a_character_that_joins_the_one_before_it_is_kept() {
        // An accent written as its own character, the mark that makes a
        // warning sign an emoji, and the joiner inside a two-person emoji.
        for text in ["cafe\u{301}", "\u{26a0}\u{fe0f} note", "\u{1f469}\u{200d}\u{1f4bb}"] {
            assert_eq!(printable(text), text);
        }
    }

    #[test]
    fn a_korean_name_is_padded_to_the_same_column_as_any_other() {
        let mut entry = md_entry(PathBuf::from("notes.md")).unwrap();
        entry.name = "세션회고".to_string();
        entry.description = Some("x".to_string());
        // Four characters, eight columns: 24 spaces to reach column 32.
        assert_eq!(row(&entry), format!("    세션회고{} x", " ".repeat(24)));
    }

    #[test]
    fn a_korean_description_is_cut_at_44_columns() {
        let mut entry = md_entry(PathBuf::from("notes.md")).unwrap();
        entry.description = Some("가".repeat(30));
        assert_eq!(row(&entry), format!("    {:<32} {}\u{2026}", "notes", "가".repeat(22)));
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
    fn whether_a_term_fits_is_counted_in_columns_not_bytes() {
        // 59 columns but 87 bytes: a byte count would move a line that fits.
        let line = format!("{}adr", "가".repeat(28));
        assert_eq!(around(&line, &words(&["adr"]), 60), line);
    }

    #[test]
    fn a_term_that_ends_on_the_last_column_still_fits() {
        // 56 + 1 + 3 = 60 columns: the term ends exactly where the room does.
        let line = format!("{}xadr", "가".repeat(28));
        assert_eq!(around(&line, &words(&["adr"]), 60), line);
    }

    #[test]
    fn a_term_past_the_width_in_columns_is_brought_into_view() {
        // 33 characters but 63 columns: a character count would call it a fit
        // and cut the term off the end.
        let line = format!("{}adr", "가".repeat(30));
        assert_eq!(around(&line, &words(&["adr"]), 60), format!("…{}adr", "가".repeat(10)));
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
    /// With `locked`, a file between them that could not be read.
    fn two_entries(locked: bool) -> Walked {
        let mut alpha = entry_with("alpha", Some("ADR notes\nmore"));
        alpha.description = Some("first".to_string());
        let beta = entry_with("beta", Some("nothing here"));
        let mut nodes = vec![Node::Entry(alpha), Node::Entry(beta)];
        if locked {
            let path = PathBuf::from("rules").join("locked.md");
            nodes.insert(1, Node::Unreadable { path, reason: io::ErrorKind::PermissionDenied });
        }
        Walked { nodes }
    }

    #[test]
    fn without_terms_the_listing_is_the_one_printed_before_m4() {
        let walked = two_entries(false);
        assert_eq!(
            listing("rules", &walked, &words(&[])),
            vec![
                "  rules:2".to_string(),
                format!("    {:<32} first", "alpha"),
                format!("    {:<32} -", "beta"),
            ]
        );
    }

    #[test]
    fn what_could_not_be_read_is_counted_and_then_named_where_it_is() {
        let walked = two_entries(true);
        let locked = PathBuf::from("rules").join("locked.md");
        assert_eq!(
            listing("rules", &walked, &words(&[])),
            vec![
                "  rules:2 (1 unreadable)".to_string(),
                format!("    {:<32} first", "alpha"),
                format!("    {:<32} -", "beta"),
                format!("    {} (permission denied)", locked.display()),
            ]
        );
    }

    #[test]
    fn a_path_that_could_not_be_read_never_carries_a_control_character() {
        let path = PathBuf::from("\u{1b}[31mlocked.md");
        let walked = Walked { nodes: vec![Node::Unreadable { path, reason: io::ErrorKind::Other }] };
        assert_eq!(listing("rules", &walked, &words(&[]))[1], "    [31mlocked.md (unreadable)");
    }

    #[test]
    fn a_search_shows_what_it_kept_out_of_the_whole_and_why() {
        let walked = two_entries(false);
        assert_eq!(
            listing("rules", &walked, &words(&["adr"])),
            vec![
                "  rules:1/2".to_string(),
                format!("    {:<32} first", "alpha"),
                "      1: ADR notes".to_string(),
            ]
        );
    }

    #[test]
    fn a_search_that_keeps_nothing_still_shows_the_source() {
        let walked = two_entries(false);
        assert_eq!(listing("rules", &walked, &words(&["zzz"])), vec!["  rules:0/2"]);
    }

    #[test]
    fn a_search_still_names_what_could_not_be_read() {
        // What could not be read could not be ruled out either.
        let walked = two_entries(true);
        let locked = PathBuf::from("rules").join("locked.md");
        assert_eq!(
            listing("rules", &walked, &words(&["zzz"])),
            vec!["  rules:0/2 (1 unreadable)".to_string(), format!("    {} (permission denied)", locked.display())]
        );
    }

    #[test]
    fn the_matched_line_is_brought_into_view() {
        let long = format!("{} adr tail", "x".repeat(100));
        let walked = Walked { nodes: vec![Node::Entry(entry_with("gamma", Some(&long)))] };
        let lines = listing("docs", &walked, &words(&["adr"]));
        assert_eq!(lines[2], "      1: …xxxxxxxxx adr tail");
    }

    #[test]
    fn a_line_from_a_supporting_file_is_marked_with_its_name() {
        let reference = entry_with("REFERENCE", Some("intro\nsee ADR here"));
        let alpha = bundle_with("alpha", Some("lead"), Some(vec![Node::Entry(reference)]));
        let walked = Walked { nodes: vec![Node::Entry(alpha)] };
        assert_eq!(
            listing("skills", &walked, &words(&["adr"])),
            vec![
                "  skills:1/1".to_string(),
                format!("    {:<32} -", "alpha"),
                "      REFERENCE:2: see ADR here".to_string(),
            ]
        );
    }

    #[test]
    fn the_mark_is_the_name_the_row_has_not_the_file_name() {
        // Its frontmatter renamed this supporting file, as it renames its row.
        let mut reference = entry_with("REFERENCE", Some("see ADR here"));
        reference.name = "renamed".to_string();
        let alpha = bundle_with("alpha", Some("lead"), Some(vec![Node::Entry(reference)]));
        let walked = Walked { nodes: vec![Node::Entry(alpha)] };
        assert_eq!(listing("skills", &walked, &words(&["adr"]))[2], "      renamed:1: see ADR here");
    }

    #[test]
    fn the_name_marking_a_line_never_carries_a_control_character() {
        let reference = entry_with("\u{1b}[31mREF", Some("adr"));
        let alpha = bundle_with("alpha", Some("lead"), Some(vec![Node::Entry(reference)]));
        let walked = Walked { nodes: vec![Node::Entry(alpha)] };
        assert_eq!(listing("skills", &walked, &words(&["adr"]))[2], "      [31mREF:1: adr");
    }

    #[test]
    fn a_tree_is_counted_and_listed_through_its_directories() {
        // `docs/` as the Walk now keeps it: `learn/` holds `index` and the
        // directory `concepts/`, which holds `slices`. The listing still reads
        // as one list, in the order the Walk found them.
        let walked = Walked {
            nodes: vec![
                Node::Entry(entry_with("README", None)),
                Node::Dir {
                    path: PathBuf::from("learn"),
                    children: vec![
                        Node::Entry(entry_with("index", None)),
                        Node::Dir {
                            path: PathBuf::from("learn/concepts"),
                            children: vec![Node::Entry(entry_with("slices", None))],
                        },
                    ],
                },
                Node::Entry(entry_with("SPEC", None)),
            ],
        };
        assert_eq!(
            listing("docs", &walked, &words(&[])),
            vec![
                "  docs:4".to_string(),
                format!("    {:<32} -", "README"),
                format!("    {:<32} -", "index"),
                format!("    {:<32} -", "slices"),
                format!("    {:<32} -", "SPEC"),
            ]
        );
    }
}
