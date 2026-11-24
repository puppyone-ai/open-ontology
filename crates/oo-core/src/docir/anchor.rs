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
        /// v1.1: page width and height in 1/100 pt (same unit as `bbox`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        page_w: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        page_h: Option<i64>,
    },
    /// csv (`sheet = null`) and xlsx: A1 range.
    Cells {
        sheet: Option<String>,
        range: String,
    },
    /// v1.1 audio / video span on the source file's time axis, milliseconds.
    /// `speaker` is a per-file label (`S1`, `S2`...); `word_start`/`word_end`
    /// index the ASR words of the file.
    Media {
        t_start_ms: u64,
        t_end_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        speaker: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        word_start: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        word_end: Option<u32>,
    },
    /// v1.1 one video frame at `t_ms`; `bbox` is `[x0,y0,x1,y1]` in frame pixels.
    Frame {
        t_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bbox: Option<[i64; 4]>,
    },
    /// v1.1 text found inside an image. With `parent = null` the image is a
    /// standalone file and `bbox` is in image pixels. Inside a pdf the parent
    /// is the pdf anchor of the picture and `bbox` is already converted to
    /// page coordinates (1/100 pt, top-left origin). A docx image has the
    /// docx paragraph anchor as parent and an `image_index` (1-based), no bbox.
    Image {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<Box<Anchor>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        image_index: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bbox: Option<[i64; 4]>,
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
    #[error("speaker label must look like S1, S2...")]
    BadSpeaker,
    #[error("image parent must be a pdf, slide or docx anchor")]
    BadImageParent,
    #[error("page size must be positive")]
    BadPageSize,
}

/// Speaker labels are file-local: `S` and a positive number of up to 3 digits.
pub fn is_speaker_label(label: &str) -> bool {
    label.strip_prefix('S').is_some_and(|n| {
        (1..=3).contains(&n.len()) && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit())
    })
}

impl Anchor {
    /// True for the anchor kinds that exist only from `oo.doc-ir/v1.1`.
    pub fn is_v11(&self) -> bool {
        match self {
            Anchor::Media { .. } | Anchor::Frame { .. } | Anchor::Image { .. } => true,
            Anchor::Pdf { page_w, page_h, .. } => page_w.is_some() || page_h.is_some(),
            _ => false,
        }
    }
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
                page_w,
                page_h,
            } => {
                if *page == 0 {
                    return Err(AnchorError::ZeroBased);
                }
                if page_w.is_some_and(|w| w <= 0) || page_h.is_some_and(|h| h <= 0) {
                    return Err(AnchorError::BadPageSize);
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
            Anchor::Media {
                t_start_ms,
                t_end_ms,
                speaker,
                word_start,
                word_end,
            } => {
                if t_start_ms > t_end_ms {
                    return Err(AnchorError::Reversed);
                }
                if let (Some(s), Some(e)) = (word_start, word_end)
                    && s > e
                {
                    return Err(AnchorError::Reversed);
                }
                if speaker.as_deref().is_some_and(|s| !is_speaker_label(s)) {
                    return Err(AnchorError::BadSpeaker);
                }
            }
            Anchor::Frame { bbox, .. } => check_bbox(bbox)?,
            Anchor::Image {
                parent,
                image_index,
                bbox,
            } => {
                if *image_index == Some(0) {
                    return Err(AnchorError::ZeroBased);
                }
                check_bbox(bbox)?;
                if let Some(parent) = parent {
                    if !matches!(
                        **parent,
                        Anchor::Pdf { .. } | Anchor::Slide { .. } | Anchor::Docx { .. }
                    ) {
                        return Err(AnchorError::BadImageParent);
                    }
                    parent.check()?;
                }
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
            Anchor::Media {
                t_start_ms,
                t_end_ms,
                speaker,
                ..
            } => {
                // Start rounds down and end rounds up so the span still covers the words.
                let start = hms(t_start_ms / 1000);
                let end = hms(t_end_ms.div_ceil(1000));
                let at = speaker
                    .as_deref()
                    .map(|s| format!("@{s}"))
                    .unwrap_or_default();
                if start == end {
                    format!("#t={start}{at}")
                } else {
                    format!("#t={start}-{end}{at}")
                }
            }
            Anchor::Frame { t_ms, .. } => format!("#f={}", hms(t_ms / 1000)),
            Anchor::Image { image_index, .. } => match image_index {
                Some(n) => format!("#img{n}"),
                None => "#img".to_owned(),
            },
        }
    }
}

fn check_bbox(bbox: &Option<[i64; 4]>) -> Result<(), AnchorError> {
    match bbox {
        Some(b) if b[0] > b[2] || b[1] > b[3] => Err(AnchorError::Reversed),
        _ => Ok(()),
    }
}

/// `HH:MM:SS` for whole seconds.
fn hms(secs: u64) -> String {
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
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

/// Longest A1 cell: three letters and seven row digits.
pub const MAX_A1: usize = 10;

/// Allocation-free kernel of [`a1_from_cell`] (so the KP2 proofs reach every
/// byte): writes the A1 text of (col, row) into `out` and returns its length,
/// `None` outside the A..ZZZ / 1..9_999_999 range.
pub fn a1_write(col: u32, row: u32, out: &mut [u8; MAX_A1]) -> Option<usize> {
    if col == 0 || col > 18_278 || row == 0 || row > 9_999_999 {
        return None;
    }
    let mut letters = [0u8; 3];
    let mut nl = 0;
    let mut n = col;
    while n > 0 {
        let rem = (n - 1) % 26;
        letters[nl] = b'A' + rem as u8;
        nl += 1;
        n = (n - 1) / 26;
    }
    let mut len = 0;
    while nl > 0 {
        nl -= 1;
        out[len] = letters[nl];
        len += 1;
    }
    let mut digits = [0u8; 7];
    let mut nd = 0;
    let mut r = row;
    while r > 0 {
        digits[nd] = b'0' + (r % 10) as u8;
        nd += 1;
        r /= 10;
    }
    while nd > 0 {
        nd -= 1;
        out[len] = digits[nd];
        len += 1;
    }
    Some(len)
}

/// Column (1-based) and row (1-based) to A1 notation, e.g. (3, 9) -> "C9".
/// Returns `None` outside the A..ZZZ / 1..9_999_999 range.
///
/// Formal: KP2 (on [`a1_write`])
pub fn a1_from_cell(col: u32, row: u32) -> Option<String> {
    let mut out = [0u8; MAX_A1];
    let n = a1_write(col, row, &mut out)?;
    String::from_utf8(out[..n].to_vec()).ok()
}

/// Kernel of the cell parser: one A1 cell as bytes.
pub fn a1_cell_bytes(s: &[u8]) -> Option<(u32, u32)> {
    let mut split = 0;
    while split < s.len() && !s[split].is_ascii_digit() {
        split += 1;
    }
    if split == s.len() {
        return None;
    }
    let (letters, digits) = s.split_at(split);
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let mut col = 0u32;
    for &b in letters {
        if !b.is_ascii_uppercase() {
            return None;
        }
        col = col * 26 + u32::from(b - b'A' + 1);
    }
    if digits.len() > 7 || digits[0] == b'0' {
        return None;
    }
    let mut row = 0u32;
    for &b in digits {
        if !b.is_ascii_digit() {
            return None;
        }
        row = row * 10 + u32::from(b - b'0');
    }
    Some((col, row))
}

/// Kernel of [`a1_parse`]: `A1` or `A1:C9` as bytes.
pub fn a1_parse_bytes(range: &[u8]) -> Option<((u32, u32), (u32, u32))> {
    let mut colon = 0;
    while colon < range.len() && range[colon] != b':' {
        colon += 1;
    }
    let (a, b) = if colon < range.len() {
        (
            a1_cell_bytes(&range[..colon])?,
            a1_cell_bytes(&range[colon + 1..])?,
        )
    } else {
        let cell = a1_cell_bytes(range)?;
        (cell, cell)
    };
    (a.0 <= b.0 && a.1 <= b.1).then_some((a, b))
}

/// Parse `A1` or `A1:C9` into ((col, row), (col, row)), requiring the second
/// corner to be at or below/right of the first.
///
/// Formal: KP2 (on [`a1_parse_bytes`])
pub fn a1_parse(range: &str) -> Option<((u32, u32), (u32, u32))> {
    a1_parse_bytes(range.as_bytes())
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
