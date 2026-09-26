use std::iter::Peekable;
use std::str::Lines;

#[derive(Default)]
pub struct Frontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Read the YAML frontmatter that opens a Markdown file.
///
/// A field the file does not carry comes back as `None`, so that "absent" stays
/// distinguishable from "empty". A block that never closes counts as no
/// frontmatter at all, rather than letting the body be read as fields.
pub fn parse_frontmatter(text: &str) -> Frontmatter {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
