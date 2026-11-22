//! Code (D94): no content extraction. A source file, a manifest or a lock file is read for structure only: the project it belongs
//! to (`attr.scope`, and a binding hint to an existing `Project` of that name) and, for CODEOWNERS, the owners it names
//! (`attr.owners`). No module, symbol or dependency entity is made.

use oo_core::docir::DocIr;

use super::{attr, attr_enum, is_codeowners, project_dir};
use crate::extract::model::{Collector, EntityRef, EntityType, EvidenceRef, Predicate};

/// Owners named in a CODEOWNERS file: `@user`, `@org/team` and e-mail addresses, in order of first appearance.
pub fn owners(ir: &DocIr) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for b in &ir.blocks {
        for line in b.text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            for tok in line.split_whitespace().skip(1) {
                if (tok.starts_with('@') || tok.contains('@')) && !out.iter().any(|o| o == tok) {
                    out.push(tok.to_owned());
                }
            }
        }
    }
    out
}

pub(crate) fn extract(path: &str, ir: &DocIr, siblings: &[String], col: &mut Collector) {
    let Some(first) = ir.blocks.first() else {
        return;
    };
    let ev = EvidenceRef::from_block(first);
    let doc = EntityRef::new(EntityType::Document, path);
    attr_enum(col, &doc, "class", "code", "profile.code", 10000, &ev);
    let dir = project_dir(path, siblings);
    attr(
        col,
        &doc,
        "scope",
        if dir.is_empty() { "/" } else { &dir },
        "profile.code",
        10000,
        &ev,
    );
    if let Some(name) = dir.rsplit('/').next().filter(|n| !n.is_empty()) {
        col.hint(
            &doc,
            Predicate::About,
            EntityType::Project,
            name,
            8000,
            "profile.code",
            &ev,
        );
    }
    if is_codeowners(path) {
        let list = owners(ir);
        if !list.is_empty() {
            attr(
                col,
                &doc,
                "owners",
                &list.join(", "),
                "profile.codeowners",
                9500,
                &ev,
            );
        }
    }
}
