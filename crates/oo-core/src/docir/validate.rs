//! Structural IR checks the JSON Schema cannot express (10 §3.4): unique
//! block ids, parents that exist and precede the child (so no cycles),
//! strictly increasing ordinals, anchor sanity, and size limits.

use std::collections::BTreeSet;

use super::{
    Block, BlockFlag, BlockKind, DocFormat, DocIr, MAX_BLOCK_TEXT, MAX_BLOCKS, MAX_HPATH, PROTOCOL,
    PROTOCOL_V11, ParserId, is_speaker_label,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IrViolation {
    Protocol,
    BlobHash,
    TooManyBlocks,
    BadBlockId {
        ordinal: u32,
    },
    DuplicateBlockId {
        ordinal: u32,
    },
    OrdinalNotIncreasing {
        ordinal: u32,
    },
    ParentMissing {
        ordinal: u32,
    },
    TextTooLong {
        ordinal: u32,
    },
    HpathTooDeep {
        ordinal: u32,
    },
    BadLevel {
        ordinal: u32,
    },
    BadAnchor {
        ordinal: u32,
    },
    TableShape {
        ordinal: u32,
    },
    /// A v1 document uses something that exists only from v1.1.
    V11InV1 {
        ordinal: Option<u32>,
    },
    BadSpeakerLabel,
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `blk_` followed by exactly 20 lowercase hex characters.
pub fn is_block_id(id: &str) -> bool {
    id.strip_prefix("blk_").is_some_and(|hex| is_hex(hex, 20))
}

fn doc_header_is_v11(ir: &DocIr) -> bool {
    matches!(
        ir.format,
        DocFormat::Audio | DocFormat::Video | DocFormat::Image | DocFormat::PdfScanned
    ) || matches!(ir.parser.id, ParserId::PaddleOcr | ParserId::Media)
        || ir.parser.profile.is_some()
        || ir.meta.duration_ms.is_some()
        || !ir.meta.speakers.is_empty()
        || ir.meta.codec.is_some()
        || ir.meta.width.is_some()
        || ir.meta.height.is_some()
}

/// True when the document uses anything that exists only from v1.1; such a
/// document must carry `oo.doc-ir/v1.1`, every other one stays `oo.doc-ir/v1`
/// byte for byte.
pub fn needs_v11(ir: &DocIr) -> bool {
    doc_header_is_v11(ir) || ir.blocks.iter().any(block_is_v11)
}

fn block_is_v11(block: &Block) -> bool {
    matches!(
        block.kind,
        BlockKind::Utterance
            | BlockKind::Scene
            | BlockKind::FrameText
            | BlockKind::ImageText
            | BlockKind::ImageCaption
    ) || block.flags.contains(&BlockFlag::ModelGenerated)
        || block.anchor.is_v11()
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
    if ir.protocol != PROTOCOL && ir.protocol != PROTOCOL_V11 {
        out.push(IrViolation::Protocol);
    }
    if ir.protocol == PROTOCOL && doc_header_is_v11(ir) {
        out.push(IrViolation::V11InV1 { ordinal: None });
    }
    if ir.meta.speakers.iter().any(|s| !is_speaker_label(s)) {
        out.push(IrViolation::BadSpeakerLabel);
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
        if ir.protocol == PROTOCOL && block_is_v11(block) {
            out.push(IrViolation::V11InV1 {
                ordinal: Some(ordinal),
            });
        }
        check_block(block, &mut out);
    }
    out
}
