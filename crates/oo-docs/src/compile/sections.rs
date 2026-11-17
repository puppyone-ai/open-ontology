//! Section digests (10 §16.1, D53): `section_digest = sha256(canonical bytes of
//! the section)`. A section runs from a heading line (`#` to `###`) to the
//! next heading line; its key is the heading text. Equal keys in one file get
//! `#2`, `#3`, ... so the map is total.

use std::collections::BTreeMap;

use super::brief::FactUse;
use super::pack::{PackFile, sha256_hex};
use super::render::entity_file_name;
use super::visible::VisibleGraph;
use crate::extract::model::EntityType;

fn brief_title(key: &str) -> &'static str {
    match key {
        "people" => "相关的人",
        "state" => "当前状态与决定",
        "tried" => "已试过但走不通",
        "preferences" => "主人的偏好",
        _ => "其他相关",
    }
}

/// `(section, fact_key, section_digest)` for `share_section_use`: where each
/// used fact sits in BRIEF and in the entity files of both its ends.
pub fn uses(
    g: &VisibleGraph,
    files: &[PackFile],
    brief_uses: &[FactUse],
) -> Vec<(String, String, String)> {
    let digests: BTreeMap<&str, BTreeMap<String, String>> = files
        .iter()
        .filter(|f| f.path.ends_with(".md") && !f.path.starts_with("refs/"))
        .map(|f| (f.path.as_str(), section_digests(f.as_str())))
        .collect();
    let mut out: Vec<(String, String, String)> = Vec::new();
    for u in brief_uses.iter().filter(|u| u.section != "detail") {
        let title = brief_title(u.section);
        if let Some(d) = digests.get("BRIEF.md").and_then(|m| m.get(title)) {
            out.push((format!("BRIEF.md#{title}"), u.fact_key.clone(), d.clone()));
        }
    }
    let home = |key: &str| -> Option<String> {
        let e = g.entity(key)?;
        Some(match e.ty {
            EntityType::Person => format!("people/{}", entity_file_name(e)),
            EntityType::Project => format!("projects/{}", entity_file_name(e)),
            EntityType::Decision => "decisions.md".to_owned(),
            _ => "terms.md".to_owned(),
        })
    };
    for f in g.facts() {
        for end in std::iter::once(&f.subject).chain(f.object.iter()) {
            let (Some(path), Some(e)) = (home(end), g.entity(end)) else {
                continue;
            };
            let stem = path.strip_suffix(".md").unwrap_or(&path).to_owned();
            let hit = digests
                .iter()
                .filter(|(p, _)| **p == path || p.starts_with(&format!("{stem}/_page/")))
                .find_map(|(p, m)| m.get(&e.name).map(|d| (*p, d.clone())));
            if let Some((p, d)) = hit {
                out.push((format!("{p}#{}", e.name), f.key.clone(), d));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn heading(line: &str) -> Option<&str> {
    let t = line.trim_start_matches('#');
    let n = line.len() - t.len();
    ((1..=3).contains(&n) && t.starts_with(' ')).then(|| t.trim())
}

/// Digests of every section of a Markdown file.
pub fn section_digests(text: &str) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut cur: Option<(String, String)> = None;
    let flush = |cur: &mut Option<(String, String)>, out: &mut BTreeMap<String, String>| {
        if let Some((name, body)) = cur.take() {
            let mut key = name.clone();
            let mut i = 2;
            while out.contains_key(&key) {
                key = format!("{name}#{i}");
                i += 1;
            }
            out.insert(key, sha256_hex(body.trim_end().as_bytes()));
        }
    };
    for line in text.lines() {
        if let Some(h) = heading(line) {
            flush(&mut cur, &mut out);
            cur = Some((h.to_owned(), format!("{line}\n")));
        } else if let Some((_, body)) = cur.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    flush(&mut cur, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_at_headings_and_numbers_duplicates() {
        let d = section_digests("> AI\n\n# A\nx\n## B\ny\n## B\nz\n");
        assert_eq!(d.len(), 3);
        assert!(d.contains_key("A") && d.contains_key("B") && d.contains_key("B#2"));
        assert_ne!(d["B"], d["B#2"]);
        let again = section_digests("> AI\n\n# A\nx\n## B\ny\n## B\nz\n");
        assert_eq!(d, again);
        let changed = section_digests("> AI\n\n# A\nx\n## B\nY\n## B\nz\n");
        assert_ne!(d["B"], changed["B"]);
        assert_eq!(d["A"], changed["A"]);
    }
}
