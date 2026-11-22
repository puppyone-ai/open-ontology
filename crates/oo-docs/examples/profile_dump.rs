//! The entities the rules (all of rules/1, profile extractors included) find in a folder of md, txt and code files, as JSON lines
//! `{"file","type","label","name"}`: the offline half of the profile measurement (PDFs need the parser sidecar and are measured live).
//!
//!   cargo run -p oo-docs --example profile_dump -- <dir>

use std::path::Path;

use oo_docs::extract::rules;

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
    let root = std::env::args().nth(1).expect("dir");
    let root = Path::new(&root);
    let mut files = Vec::new();
    walk(root, &mut files);
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
        let Ok(bytes) = std::fs::read(&f) else {
            continue;
        };
        let parsed = match ext.as_str() {
            "md" => oo_docs::parse::markdown::parse_markdown(&bytes),
            "pdf" | "docx" | "xlsx" | "pptx" | "png" | "jpg" => continue,
            _ => oo_docs::parse::text::parse_text(&bytes),
        };
        let Ok(n) = parsed else { continue };
        let x = rules::extract(&rel, &n.ir);
        for e in &x.entities {
            let ent = &e.entity;
            println!(
                "{}",
                serde_json::json!({"file": rel, "type": ent.ty.as_str(), "label": ent.thing_label, "name": ent.name})
            );
        }
    }
}
