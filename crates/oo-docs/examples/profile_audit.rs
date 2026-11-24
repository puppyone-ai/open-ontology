//! Profile audit of a folder: how many files each profile gets, by path and format (and by content for md and txt).
//!
//!   cargo run -p oo-docs --example profile_audit -- <dir> [--list <profile>]

use std::collections::BTreeMap;
use std::path::Path;

use oo_docs::profile::{Profile, classify, classify_path};

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .collect();
    entries.sort_by_key(std::fs::DirEntry::path);
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = Path::new(&args[0]);
    let list = args
        .iter()
        .position(|a| a == "--list")
        .and_then(|i| args.get(i + 1))
        .and_then(|p| Profile::parse(p));
    let mut files = Vec::new();
    walk(root, &mut files);
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for f in files {
        let rel = f
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let ext = f
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let bytes = std::fs::read(&f).unwrap_or_default();
        let profile = match ext.as_str() {
            "md" | "markdown" | "mdx" => oo_docs::parse::markdown::parse_markdown(&bytes)
                .ok()
                .map(|n| classify(&rel, &n.ir).profile),
            "txt" | "rs" | "py" | "ts" | "tsx" | "js" | "go" | "json" | "toml" | "yaml" | "yml"
            | "sh" | "log" => oo_docs::parse::text::parse_text(&bytes)
                .ok()
                .map(|n| classify(&rel, &n.ir).profile),
            "csv" => Some(classify_path(&rel, Some("csv")).profile),
            "xlsx" => Some(classify_path(&rel, Some("xlsx")).profile),
            "docx" | "pptx" | "pdf" => Some(classify_path(&rel, Some(&ext)).profile),
            "wav" | "m4a" | "mp3" => Some(classify_path(&rel, Some("audio")).profile),
            "png" | "jpg" | "jpeg" => Some(classify_path(&rel, Some("image")).profile),
            _ => Some(classify_path(&rel, Some("unknown")).profile),
        };
        let Some(profile) = profile else { continue };
        *counts.entry(profile.as_str()).or_default() += 1;
        if list == Some(profile) {
            println!("{} {rel}", profile.as_str());
        }
    }
    println!("{counts:?}");
}
