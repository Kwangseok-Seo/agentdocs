use std::fs;
use std::env;
use std::path::{Path, PathBuf};

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

fn count_md(dir: &Path) -> usize {
    let mut n = 0;
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };

    for entry in entries {
        let path = entry.unwrap().path();

        if path.is_dir() {
            n += count_md(&path);
        } else if let Some(ext) = path.extension() {
            if ext == "md" {
                n += 1;
            }
        }
    }
    n
}

fn main() {
    let home = env::home_dir().unwrap();

    let sources = [
        ("skills", home.join(".claude").join("skills")),
        ("rules", home.join(".claude").join("rules")),
        ("agents", home.join(".claude").join("agents")),
        ("commands", home.join(".claude").join("commands")),
        ("agents/skills", home.join(".agents").join("skills")),
    ];

    for (name, path) in sources {
        if let Ok(dir) = fs::read_dir(&path) {
            println!("{}:{}", name, dir.count());    
        } else {
            println!("{}:(missing)", name);
        }
    }

    let cwd = env::current_dir().unwrap();
    match find_project_root(&cwd, &home) {
        Some(root) => {
            println!("PROJECT {}", root.display());

            let mut n = 0;
            for entry in fs::read_dir(&root).unwrap() {
                let path = entry.unwrap().path();
                if let Some(ext) = path.extension() {
                    if ext == "md" {
                        n += 1;
                    }
                }
            }
            println!(" root md:{n}");

            let docs = root.join("docs");
            if docs.is_dir() {
                println!(" docs:{}", count_md(&docs));
            }
        }
        None => println!("PROJECT  (outside any project)"),
    }
}
