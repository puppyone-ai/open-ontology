//! Anchors: where a block lives in its original file (10 §3.5).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Location of a block inside the original document. The JSON form is a
/// `oneOf` tagged by `kind`; every variant rejects unknown fields.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Anchor {
    /// md / txt: 1-based closed line range, Unicode scalar offsets into the
    /// decoded file (newlines included).
    Text {
        line_start: u32,
        line_end: u32,
        char_start: u64,
        char_end: u64,
    },
    /// docx: 0-based body paragraph range; table paragraphs use
    /// `table_index` + `cell_range` (`R3C1` or `R3C1:R4C2`).
    Docx {
        para_start: u32,
        para_end: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        table_index: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cell_range: Option<String>,
    },
    /// pptx: 1-based slide, optional `p:cNvPr/@id` shape and paragraph range.
    Slide {
        slide: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shape_id: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        para_start: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        para_end: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notes: Option<bool>,
    },
    /// pdf: 1-based page; bbox in 1/100 pt, top-left origin, `[x0,y0,x1,y1]`.
    Pdf {
        page: u32,
        bbox: [i64; 4],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        charspan: Option<[u64; 2]>,
    },
    /// csv (`sheet = null`) and xlsx: A1 range.
    Cells {
        sheet: Option<String>,
        range: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum AnchorError {
    #[error("anchor range is reversed")]
    Reversed,
    #[error("anchor line or page numbers are 1-based")]
    ZeroBased,
    #[error("A1 range is malformed")]
    BadA1,
    #[error("docx cell range is malformed")]
    BadCellRange,
    #[error("sheet name is longer than 128 characters")]
    SheetTooLong,
}

impl Anchor {
    /// Structural checks that the JSON Schema cannot express.
    pub fn check(&self) -> Result<(), AnchorError> {
        match self {
            Anchor::Text {
                line_start,
                line_end,
                char_start,
                char_end,
            } => {
                if *line_start == 0 || *line_end == 0 {
                    return Err(AnchorError::ZeroBased);
                }
                if line_start > line_end || char_start > char_end {
                    return Err(AnchorError::Reversed);
                }
            }
            Anchor::Docx {
                para_start,
                para_end,
                cell_range,
                ..
            } => {
                if para_start > para_end {
                    return Err(AnchorError::Reversed);
                }
                if let Some(range) = cell_range {
                    rc_parse(range).ok_or(AnchorError::BadCellRange)?;
                }
            }
            Anchor::Slide {
                slide,
                para_start,
                para_end,
                ..
            } => {
                if *slide == 0 {
                    return Err(AnchorError::ZeroBased);
                }
                if let (Some(s), Some(e)) = (para_start, para_end)
                    && s > e
                {
                    return Err(AnchorError::Reversed);
                }
            }
            Anchor::Pdf {
                page,
                bbox,
                charspan,
            } => {
                if *page == 0 {
                    return Err(AnchorError::ZeroBased);
                }
                if bbox[0] > bbox[2] || bbox[1] > bbox[3] {
                    return Err(AnchorError::Reversed);
                }
                if let Some([s, e]) = charspan
                    && s > e
                {
                    return Err(AnchorError::Reversed);
                }
            }
            Anchor::Cells { sheet, range } => {
                if sheet.as_ref().is_some_and(|s| s.chars().count() > 128) {
                    return Err(AnchorError::SheetTooLong);
                }
                a1_parse(range).ok_or(AnchorError::BadA1)?;
            }
        }
        Ok(())
    }

    /// Provenance fragment without the leading path: `#L12-L18`, `#R12`,
    /// `#'Sheet 1'!A1:C9`, `#¶12-14`, `#T2!R3C1` (tables 1-based), `#s3`, `#p5`.
    /// A single-line or single-paragraph range is written once (`#L12`).
    pub fn fragment(&self) -> String {
        match self {
            Anchor::Text {
                line_start,
                line_end,
                ..
            } => {
                if line_start == line_end {
                    format!("#L{line_start}")
                } else {
                    format!("#L{line_start}-L{line_end}")
                }
            }
            Anchor::Cells { sheet: None, range } => match a1_parse(range) {
                Some(((_, r0), (_, r1))) if r0 == r1 => format!("#R{r0}"),
                _ => format!("#{range}"),
            },
            Anchor::Cells {
                sheet: Some(sheet),
                range,
            } => {
                format!("#{}!{range}", quote_sheet(sheet))
            }
            Anchor::Docx {
                table_index: Some(t),
                cell_range: Some(cells),
                ..
            } => {
                format!("#T{t}!{cells}")
            }
            Anchor::Docx {
                para_start,
                para_end,
                ..
            } => {
                if para_start == para_end {
                    format!("#¶{para_start}")
                } else {
                    format!("#¶{para_start}-{para_end}")
                }
            }
            Anchor::Slide { slide, .. } => format!("#s{slide}"),
            Anchor::Pdf { page, .. } => format!("#p{page}"),
        }
    }
}

/// Quote a sheet name the way spreadsheets do when it is not plain
/// alphanumeric: wrap in single quotes and double inner quotes.
fn quote_sheet(name: &str) -> String {
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        name.to_owned()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

/// Column (1-based) and row (1-based) to A1 notation, e.g. (3, 9) -> "C9".
/// Returns `None` outside the A..ZZZ / 1..9_999_999 range.
pub fn a1_from_cell(col: u32, row: u32) -> Option<String> {
    if col == 0 || col > 18_278 || row == 0 || row > 9_999_999 {
        return None;
    }
    let mut letters = Vec::new();
    let mut n = col;
    while n > 0 {
        let rem = (n - 1) % 26;
        letters.push(b'A' + rem as u8);
        n = (n - 1) / 26;
    }
    letters.reverse();
    Some(format!("{}{row}", String::from_utf8(letters).ok()?))
}

fn a1_cell(s: &str) -> Option<(u32, u32)> {
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = s.split_at(split);
    if letters.is_empty() || letters.len() > 3 || !letters.bytes().all(|b| b.is_ascii_uppercase()) {
        return None;
    }
    if digits.is_empty()
        || digits.len() > 7
        || digits.starts_with('0')
        || !digits.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let col = letters
        .bytes()
        .fold(0u32, |acc, b| acc * 26 + u32::from(b - b'A' + 1));
    Some((col, digits.parse().ok()?))
}

/// Parse `A1` or `A1:C9` into ((col, row), (col, row)), requiring the second
/// corner to be at or below/right of the first.
pub fn a1_parse(range: &str) -> Option<((u32, u32), (u32, u32))> {
    let (a, b) = match range.split_once(':') {
        Some((a, b)) => (a1_cell(a)?, a1_cell(b)?),
        None => {
            let cell = a1_cell(range)?;
            (cell, cell)
        }
    };
    (a.0 <= b.0 && a.1 <= b.1).then_some((a, b))
}

fn rc_cell(s: &str) -> Option<(u32, u32)> {
    let rest = s.strip_prefix('R')?;
    let (r, c) = rest.split_once('C')?;
    if r.is_empty()
        || c.is_empty()
        || !r.bytes().all(|b| b.is_ascii_digit())
        || !c.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    Some((r.parse().ok()?, c.parse().ok()?))
}

fn rc_parse(range: &str) -> Option<((u32, u32), (u32, u32))> {
    let (a, b) = match range.split_once(':') {
        Some((a, b)) => (rc_cell(a)?, rc_cell(b)?),
        None => {
            let cell = rc_cell(range)?;
            (cell, cell)
        }
    };
    (a.0 <= b.0 && a.1 <= b.1).then_some((a, b))
}
