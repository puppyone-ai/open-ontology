//! Plain text parser (`oo-text`): one `paragraph` block per run of non-blank
//! lines (10 §4.4, §3.5). `hpath` is always empty.

use oo_core::docir::{BlockKind, DocFormat, DocMeta, ParserId};

use super::decode::decode_text;
use super::lines::LineIndex;
use super::{ParseError, blob_b3, parser_info};
use crate::normalize::{IrFragment, IrWarning, RawBlock};

const PROFILE: &str =
    "oo-text/1;paragraphs=blank-line;offsets=scalar-half-open;encodings=utf-8,utf-8-bom,utf-16-bom";

pub fn parse_text_fragment(bytes: &[u8]) -> Result<IrFragment, ParseError> {
    let decoded = decode_text(bytes)?;
    let text = decoded.text.as_str();
    let index = LineIndex::new(text);
    let mut blocks = Vec::new();

    // (start, end) byte range of the current paragraph, trimmed at both ends.
    let mut cur: Option<(usize, usize)> = None;
    let mut pos = 0usize;
    for line in text.split_inclusive('\n') {
        let line_start = pos;
        pos += line.len();
        let body = line.trim_end_matches(['\n', '\r']);
        let trimmed_start = body.trim_start();
        if trimmed_start.trim_end().is_empty() {
            if let Some((s, e)) = cur.take() {
                blocks.push(paragraph(&index, text, s, e));
            }
            continue;
        }
        let s = line_start + (body.len() - trimmed_start.len());
        let e = line_start + body.trim_end().len();
        cur = Some(match cur {
            Some((first, _)) => (first, e),
            None => (s, e),
        });
    }
    if let Some((s, e)) = cur {
        blocks.push(paragraph(&index, text, s, e));
    }

    let mut warnings = Vec::new();
    if blocks.is_empty() {
        warnings.push(IrWarning {
            code: "DOCS_PARSE_EMPTY".to_owned(),
            block: None,
        });
    }
    Ok(IrFragment {
        blob_b3: blob_b3(bytes),
        format: DocFormat::Txt,
        parser: parser_info(ParserId::OoText, PROFILE),
        meta: DocMeta {
            encoding: Some(decoded.encoding),
            ..DocMeta::default()
        },
        blocks,
        warnings,
    })
}

fn paragraph(index: &LineIndex<'_>, text: &str, start: usize, end: usize) -> RawBlock {
    let slice = &text[start..end];
    // CRLF files are normalized to LF in the block text, so the text is no
    // longer a verbatim source slice and oversize splitting cannot refine
    // the anchors per chunk.
    let exact = !slice.contains('\r');
    RawBlock {
        kind: BlockKind::Paragraph,
        level: None,
        text: slice.replace("\r\n", "\n"),
        hpath: Vec::new(),
        parent: None,
        anchor: index.anchor(start, end),
        table: None,
        flags: Vec::new(),
        exact,
    }
}

/// Parse a `.txt` file to canonical IR.
pub fn parse_text(bytes: &[u8]) -> Result<crate::normalize::Normalized, ParseError> {
    Ok(crate::normalize::finish(parse_text_fragment(bytes)?)?)
}
