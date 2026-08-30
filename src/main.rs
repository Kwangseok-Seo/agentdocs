use std::fs;
use std::env;
use std::path::{Path, PathBuf};
use std::iter::Peekable;
use std::str::Lines;

enum Scope {
    Global,
    Project,
}

enum Walk {
    MarkdownFiles,
    BundleDirs,
    MarkdownTree,
}

struct Entry {
    name: String,
    path: PathBuf,
    kind: EntryKind,
    description: Option<String>,
}

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

fn md_files(dir: &Path) -> Vec<Entry> {
    let mut out = Vec::new();

    let Ok(read) = fs::read_dir(dir) else {
        return out;
    };

    for item in read {
        let path = item.unwrap().path();
        // A directory may be named `notes.md` too, and it is not a document.
        // Only a known directory is rejected: anything unreadable stays listed.
        if path.is_dir() { continue };
        let Some(ext) = path.extension() else { continue };
        // Case-insensitive, because the filesystems this runs on are.
        if !ext.eq_ignore_ascii_case("md") { continue };
        let Some(stem) = path.file_stem() else { continue };

        out.push(Entry {
            name: stem.to_string_lossy().to_string(),
            path,
            kind: EntryKind::File,
            description: None,
        });
    }
    out
}

fn md_tree(dir: &Path) -> Vec<Entry> {
    let mut out = md_files(dir);

    let Ok(read) = fs::read_dir(dir) else { return out };
    for item in read {
        let path = item.unwrap().path();
        if path.is_dir() {
            out.extend(md_tree(&path));
        }
    }
    out
}

fn bundle_dirs(dir: &Path) -> Vec<Entry> {
    let mut out = Vec::new();

    let Ok(read) = fs::read_dir(dir) else {
        return out;
    };

    for item in read {
        let path = item.unwrap().path();
        if !path.is_dir() { continue };
        let Some(name) = path.file_name() else { continue };

        let lead_path = path.join("SKILL.md");
        let lead = if lead_path.is_file() { Some(lead_path) } else { None };

        out.push(Entry {
            name: name.to_string_lossy().to_string(),
            path,
            kind: EntryKind::Bundle { lead },
            description: None,
        });
    }
    out
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

    fn entries(&self) -> Vec<Entry> {
        let mut out = match self.walk {
            Walk::MarkdownFiles => md_files(&self.path),
            Walk::BundleDirs => bundle_dirs(&self.path),
            Walk::MarkdownTree => md_tree(&self.path),
        };

        for entry in &mut out {
            entry.load_frontmatter();
        }
        out
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

/// Fit a description onto one line. The cut counts characters, not bytes, so a
/// Korean description is never sliced through the middle of a character.
///
/// Control characters are dropped rather than printed: this text comes out of
/// somebody else's file, and an escape sequence in it would otherwise move the
/// cursor or recolour the terminal.
fn short(s: &str, width: usize) -> String {
    let mut out: String = s.chars().filter(|c| !c.is_control()).take(width).collect();
    if s.chars().filter(|c| !c.is_control()).count() > width {
        out.push('…');
    }
    out
}

fn main() {
    let home = env::home_dir().unwrap();

    let mut sources = vec![
        Source::new("skills", home.join(".claude").join("skills"), Scope::Global, Walk::BundleDirs),
        Source::new("rules", home.join(".claude").join("rules"), Scope::Global, Walk::MarkdownFiles),
        Source::new("agents", home.join(".claude").join("agents"), Scope::Global, Walk::MarkdownFiles),
        Source::new("commands", home.join(".claude").join("commands"), Scope::Global, Walk::MarkdownFiles),
        Source::new("agents/skills", home.join(".agents").join("skills"), Scope::Global, Walk::BundleDirs),
    ];

    let cwd = env::current_dir().unwrap();
    let root = find_project_root(&cwd, &home);

    if let Some(r) = &root {
        sources.push(Source::new("root md", r.clone(), Scope::Project, Walk::MarkdownFiles));
        sources.push(Source::new("docs", r.join("docs"), Scope::Project, Walk::MarkdownTree));
    }

    let project_header = match &root {
        Some(r) => format!("PROJECT {}", r.display()),
        None => String::from("PROJECT (outside any project)"),
    };

    println!("GLOBAL");
    
    let mut project_shown = false;
    for src in &sources {
        if let Scope::Project = src.scope {
            if !project_shown {
                println!("{project_header}");
                project_shown = true;
            }
        }

        if !src.path.is_dir() {
            println!("  {}:(missing)", src.name);
            continue;
        }
        let entries = src.entries();
        println!("  {}:{}", src.name, entries.len());

        for entry in &entries {
            match &entry.description {
                Some(text) => println!("    {:<32} {}", entry.name, short(text, 44)),
                None => println!("    {:<32} -", entry.name),
            }
        }
    }

    if !project_shown {
        println!("{project_header}");
    }
    
}
