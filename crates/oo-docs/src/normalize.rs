//! Parser output to `oo.doc-ir/v1` (10 §4.5). Board WS-BE2.
//!
//! Parsers hand over an [`IrFragment`] (blocks without ids or ordinals).
//! [`finish`] splits oversize blocks at sentence ends, assigns ordinals and
//! content-derived block ids (3.4), validates with
//! `oo_core::docir::validate` and serializes with `canonical_json`, so the
//! same input and parser always give the same `ir_b3`.

use oo_core::docir::{
    Anchor, Block, BlockFlag, BlockKind, DocFormat, DocIr, DocMeta, IrViolation, MAX_BLOCK_TEXT,
    MAX_BLOCKS, PROTOCOL, ParserInfo, TableGrid, validate,
};
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

mod canonical;
#[cfg(test)]
mod tests;

pub use canonical::canonical_json;

/// A block before ids and ordinals exist.
#[derive(Clone, Debug, PartialEq)]
pub struct RawBlock {
    pub kind: BlockKind,
    pub level: Option<u8>,
    pub text: String,
    pub hpath: Vec<String>,
    /// Index of the parent block in the same fragment (must be smaller).
    pub parent: Option<usize>,
    pub anchor: Anchor,
    pub table: Option<TableGrid>,
    pub flags: Vec<BlockFlag>,
    /// `text` is a verbatim slice of the source starting at the anchor's
    /// `char_start`; lets oversize splitting give each chunk its own anchor.
    pub exact: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrWarning {
    pub code: String,
    /// Index into [`IrFragment::blocks`].
    pub block: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IrFragment {
    pub blob_b3: String,
    pub format: DocFormat,
    pub parser: ParserInfo,
    pub meta: DocMeta,
    pub blocks: Vec<RawBlock>,
    pub warnings: Vec<IrWarning>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NormalizeError {
    TooManyBlocks,
    /// `parse_failed(DOCS_PARSE_IR_INVALID)`: a parser defect.
    IrInvalid(Vec<IrViolation>),
}

/// A validated document with its canonical bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Normalized {
    pub ir: DocIr,
    /// `canonical_json` of the IR; the bytes stored under `cas/ir/`.
    pub canonical: String,
    /// BLAKE3 of `canonical`, hex.
    pub ir_b3: String,
}

/// `blk_` + first 20 hex characters of
/// `BLAKE3("oo.doc-ir/v1 block\0" ‖ canonical(anchor) ‖ "\0" ‖ NFC(text))`.
///
/// Same anchor and text always give the same id, across versions of a file.
pub fn block_id(anchor: &Anchor, text: &str) -> String {
    let anchor_json = serde_json::to_value(anchor).unwrap_or(Value::Null);
    let mut h = blake3::Hasher::new();
    h.update(b"oo.doc-ir/v1 block\0");
    h.update(canonical_json(&anchor_json).as_bytes());
    h.update(b"\0");
    let nfc: String = text.nfc().collect();
    h.update(nfc.as_bytes());
    let hex = h.finalize().to_hex();
    format!("blk_{}", &hex.as_str()[..20])
}

/// Split `text` into chunks of at most `max` characters. Cuts fall after
/// sentence-ending punctuation or a line break when one exists in the second
/// half of the window, otherwise at `max`. Concatenating the chunks gives back
/// `text`. `max` of 0 is treated as 1.
pub fn split_text(text: &str, max: usize) -> Vec<String> {
    let max = max.max(1);
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return vec![text.to_owned()];
    }
    let is_end = |c: char| matches!(c, '。' | '！' | '？' | '；' | '.' | '!' | '?' | ';' | '\n');
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let remaining = chars.len() - start;
        if remaining <= max {
            out.push(chars[start..].iter().collect());
            break;
        }
        let window_end = start + max;
        let min_cut = start + max / 2;
        let cut = (min_cut..window_end)
            .rev()
            .find(|i| is_end(chars[*i]))
            .map_or(window_end, |i| i + 1);
        out.push(chars[start..cut].iter().collect());
        start = cut;
    }
    out
}

/// Anchor for the chunk starting `offset` characters into an exact block.
fn refine_anchor(anchor: &Anchor, whole: &str, offset: usize, chunk: &str) -> Anchor {
    let Anchor::Text {
        line_start,
        char_start,
        ..
    } = anchor
    else {
        return anchor.clone();
    };
    let before: String = whole.chars().take(offset).collect();
    let newlines_before = before.bytes().filter(|b| *b == b'\n').count() as u32;
    let body = chunk.strip_suffix('\n').unwrap_or(chunk);
    let inside = body.bytes().filter(|b| *b == b'\n').count() as u32;
    let first = line_start + newlines_before;
    Anchor::Text {
        line_start: first,
        line_end: first + inside,
        char_start: char_start + offset as u64,
        char_end: char_start + (offset + chunk.chars().count()) as u64,
    }
}

/// Turn a fragment into validated canonical IR.
pub fn finish(fragment: IrFragment) -> Result<Normalized, NormalizeError> {
    let IrFragment {
        blob_b3,
        format,
        parser,
        meta,
        blocks: raw,
        warnings: raw_warnings,
    } = fragment;

    // Pass 1: split oversize blocks, remember where each source block went.
    let mut expanded: Vec<(RawBlock, Option<usize>)> = Vec::new();
    let mut first_new: Vec<usize> = Vec::with_capacity(raw.len());
    let mut split_marks: Vec<usize> = Vec::new();
    for block in raw {
        first_new.push(expanded.len());
        if block.text.chars().count() <= MAX_BLOCK_TEXT {
            let parent = block.parent;
            expanded.push((block, parent));
            continue;
        }
        split_marks.push(expanded.len());
        let chunks = split_text(&block.text, MAX_BLOCK_TEXT);
        let mut offset = 0usize;
        for (i, chunk) in chunks.into_iter().enumerate() {
            let anchor = if block.exact {
                refine_anchor(&block.anchor, &block.text, offset, &chunk)
            } else {
                block.anchor.clone()
            };
            offset += chunk.chars().count();
            let piece = RawBlock {
                text: chunk,
                anchor,
                table: if i == 0 { block.table.clone() } else { None },
                parent: block.parent,
                ..block.clone()
            };
            expanded.push((piece, block.parent));
        }
    }
    if expanded.len() > MAX_BLOCKS {
        return Err(NormalizeError::TooManyBlocks);
    }

    // Pass 2: ids and ordinals.
    let ids: Vec<String> = expanded
        .iter()
        .map(|(b, _)| block_id(&b.anchor, &b.text))
        .collect();
    let blocks: Vec<Block> = expanded
        .into_iter()
        .enumerate()
        .map(|(i, (b, parent))| Block {
            id: ids[i].clone(),
            ordinal: i as u32,
            kind: b.kind,
            level: b.level,
            text: b.text,
            hpath: b.hpath,
            parent: parent
                .and_then(|p| first_new.get(p))
                .map(|n| ids[*n].clone()),
            anchor: b.anchor,
            table: b.table,
            flags: b.flags,
        })
        .collect();

    let mut warnings: Vec<oo_core::docir::IrWarning> = raw_warnings
        .into_iter()
        .map(|w| oo_core::docir::IrWarning {
            code: w.code,
            block_id: w
                .block
                .and_then(|i| first_new.get(i))
                .map(|n| ids[*n].clone()),
        })
        .collect();
    for mark in split_marks {
        warnings.push(oo_core::docir::IrWarning {
            code: "DOCS_PARSE_BLOCK_SPLIT".to_owned(),
            block_id: Some(ids[mark].clone()),
        });
    }
    warnings.truncate(1000);

    let ir = DocIr {
        protocol: PROTOCOL.to_owned(),
        blob_b3,
        format,
        parser,
        meta,
        blocks,
        warnings,
    };
    seal(ir)
}

/// Validate and serialize an assembled IR.
pub fn seal(ir: DocIr) -> Result<Normalized, NormalizeError> {
    let violations = validate(&ir);
    if violations.contains(&IrViolation::TooManyBlocks) {
        return Err(NormalizeError::TooManyBlocks);
    }
    if !violations.is_empty() {
        return Err(NormalizeError::IrInvalid(violations));
    }
    let value = serde_json::to_value(&ir).map_err(|_| NormalizeError::IrInvalid(Vec::new()))?;
    let canonical = canonical_json(&value);
    let ir_b3 = blake3::hash(canonical.as_bytes()).to_hex().to_string();
    Ok(Normalized {
        ir,
        canonical,
        ir_b3,
    })
}
