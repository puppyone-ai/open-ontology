//! Structural IR checks the JSON Schema cannot express (10 §3.4): unique
//! block ids, parents that exist and precede the child (so no cycles),
//! strictly increasing ordinals, anchor sanity, and size limits.

use std::collections::BTreeSet;

use super::{Block, DocIr, MAX_BLOCK_TEXT, MAX_BLOCKS, MAX_HPATH, PROTOCOL};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IrViolation {
    Protocol,
    BlobHash,
    TooManyBlocks,
    BadBlockId { ordinal: u32 },
    DuplicateBlockId { ordinal: u32 },
    OrdinalNotIncreasing { ordinal: u32 },
    ParentMissing { ordinal: u32 },
    TextTooLong { ordinal: u32 },
    HpathTooDeep { ordinal: u32 },
    BadLevel { ordinal: u32 },
    BadAnchor { ordinal: u32 },
    TableShape { ordinal: u32 },
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `blk_` followed by exactly 20 lowercase hex characters.
pub fn is_block_id(id: &str) -> bool {
    id.strip_prefix("blk_").is_some_and(|hex| is_hex(hex, 20))
}

fn check_block(block: &Block, out: &mut Vec<IrViolation>) {
    let ordinal = block.ordinal;
    if block.text.chars().count() > MAX_BLOCK_TEXT {
        out.push(IrViolation::TextTooLong { ordinal });
    }
    if block.hpath.len() > MAX_HPATH {
        out.push(IrViolation::HpathTooDeep { ordinal });
    }
    if block.level.is_some_and(|l| !(1..=9).contains(&l)) {
        out.push(IrViolation::BadLevel { ordinal });
    }
    if block.anchor.check().is_err() {
        out.push(IrViolation::BadAnchor { ordinal });
    }
    if let Some(table) = &block.table
        && table
            .cells
            .iter()
            .any(|cell| cell.r >= table.rows || cell.c >= table.cols)
    {
        out.push(IrViolation::TableShape { ordinal });
    }
}

/// Every violation found, in block order. Empty means the IR is well formed.
pub fn validate(ir: &DocIr) -> Vec<IrViolation> {
    let mut out = Vec::new();
    if ir.protocol != PROTOCOL {
        out.push(IrViolation::Protocol);
    }
    if !is_hex(&ir.blob_b3, 64) || !is_hex(&ir.parser.profile_sha256, 64) {
        out.push(IrViolation::BlobHash);
    }
    if ir.blocks.len() > MAX_BLOCKS {
        out.push(IrViolation::TooManyBlocks);
    }
    let mut seen = BTreeSet::new();
    let mut last: Option<u32> = None;
    for block in &ir.blocks {
        let ordinal = block.ordinal;
        if !is_block_id(&block.id) {
            out.push(IrViolation::BadBlockId { ordinal });
        }
        if last.is_some_and(|prev| prev >= ordinal) {
            out.push(IrViolation::OrdinalNotIncreasing { ordinal });
        }
        last = Some(ordinal);
        if let Some(parent) = &block.parent
            && !seen.contains(parent.as_str())
        {
            // Parents must appear earlier, which also rules out cycles.
            out.push(IrViolation::ParentMissing { ordinal });
        }
        if !seen.insert(block.id.as_str()) {
            out.push(IrViolation::DuplicateBlockId { ordinal });
        }
        check_block(block, &mut out);
    }
    out
}
