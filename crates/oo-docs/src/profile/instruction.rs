//! Instruction documents (D94): AGENTS.md, CLAUDE.md, README, SOPs, style guides, ADRs.
//!
//! They put no entities into the ontology. The file is attached, as an instruction (a working convention), to the project or
//! directory it governs: `attr.class = instruction` and `attr.scope = <project directory>` on its own Document, and a binding
//! hint to a `Project` of that name when one already exists. When the file is shared with an Agent, its original text goes into the
//! BRIEF section 工作约定 (`compile::brief`).

use oo_core::docir::DocIr;

use super::{attr, attr_enum, project_dir};
use crate::extract::model::{Collector, EntityRef, EntityType, EvidenceRef, Predicate};

pub(crate) fn extract(path: &str, ir: &DocIr, siblings: &[String], col: &mut Collector) {
    let Some(first) = ir.blocks.first() else {
        return;
    };
    let ev = EvidenceRef::from_block(first);
    let doc = EntityRef::new(EntityType::Document, path);
    attr_enum(
        col,
        &doc,
        "class",
        "instruction",
        "profile.instruction",
        10000,
        &ev,
    );
    let dir = project_dir(path, siblings);
    attr(
        col,
        &doc,
        "scope",
        if dir.is_empty() { "/" } else { &dir },
        "profile.instruction",
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
            "profile.instruction",
            &ev,
        );
    }
}
