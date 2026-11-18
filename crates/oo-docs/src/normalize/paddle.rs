//! `ocr-parser` (PP-StructureV3) output to `oo.doc-ir/v1.1` (10 §17.6), for scanned pdf and standalone images.
//!
//! The sidecar sends pixel boxes plus the page size in points. For a scanned pdf the box moves to 1/100 pt
//! with a top-left origin (`px * pt / px_size * 100`) and every anchor carries `page_w`/`page_h` (D70).
//! A standalone image gives `image_text` blocks with `parent = null` and the box in image pixels.
//! Blocks whose mean recognition score is under 0.8 get `low_text_confidence`.

use oo_core::docir::{
    Anchor, BlockFlag, BlockKind, ComputeTier, DocFormat, DocMeta, ParserId, ParserInfo,
    ParserModel, ParserProfile,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{IrFragment, IrWarning, RawBlock};

const PROFILE: &str =
    "paddleocr-ppstructurev3/1;dpi=200;tables=text;orientation=off;bbox=1/100pt-top-left";
const LOW_CONFIDENCE: f64 = 0.8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaddleError {
    Shape(&'static str),
    Empty,
}

impl std::fmt::Display for PaddleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shape(w) => write!(f, "ocr output: {w}"),
            Self::Empty => f.write_str("no parser output"),
        }
    }
}

impl std::error::Error for PaddleError {}

fn profile_sha256() -> String {
    Sha256::digest(PROFILE.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn kind_of(name: &str) -> BlockKind {
    match name {
        "heading" => BlockKind::Heading,
        "table" => BlockKind::Table,
        "caption" => BlockKind::Caption,
        "page_header" => BlockKind::PageHeader,
        "page_footer" => BlockKind::PageFooter,
        "footnote" => BlockKind::Footnote,
        "code" => BlockKind::Code,
        _ => BlockKind::Paragraph,
    }
}

/// The weights directory of a model file path: `PaddlePaddle--X/inference.pdiparams` is model `X`.
fn model_name(file: &str) -> String {
    let dir = file.split('/').next().unwrap_or(file);
    dir.rsplit("--").next().unwrap_or(dir).to_owned()
}

fn px_to_hundredth_pt(v: i64, pt: f64, px: f64) -> i64 {
    ((v as f64) * pt / px * 100.0).round() as i64
}

/// `responses` are `POST /v1/parse` bodies of the ocr-parser (window of pdf pages, or one image).
pub fn assemble(
    format: DocFormat,
    blob_b3: String,
    mut responses: Vec<Value>,
) -> Result<IrFragment, PaddleError> {
    if responses.is_empty() {
        return Err(PaddleError::Empty);
    }
    if !matches!(format, DocFormat::PdfScanned | DocFormat::Image) {
        return Err(PaddleError::Shape("format is not pdf_scanned or image"));
    }
    responses.sort_by_key(|r| {
        r.get("pages")
            .and_then(|p| p.get(0))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    });
    let first = &responses[0];
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    let profile = first.get("profile");
    let models: Vec<ParserModel> = profile
        .and_then(|p| p.get("models"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|m| {
                    Some(ParserModel {
                        name: model_name(m.get("name")?.as_str()?),
                        sha256: m.get("sha256")?.as_str()?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let parser = ParserInfo {
        id: ParserId::PaddleOcr,
        version: text(first.get("parser").unwrap_or(&Value::Null), "version")
            .unwrap_or_else(|| "unknown".into())
            .chars()
            .take(64)
            .collect(),
        adapter_version: text(
            first.get("parser").unwrap_or(&Value::Null),
            "adapter_version",
        ),
        profile_sha256: profile_sha256(),
        profile: Some(ParserProfile {
            tier: ComputeTier::Cpu,
            models,
            isa: profile
                .and_then(|p| text(p, "isa"))
                .unwrap_or_else(|| "unknown".into()),
        }),
    };
    let mut blocks: Vec<RawBlock> = Vec::new();
    let mut warnings: Vec<IrWarning> = Vec::new();
    let mut page_count = 0u32;
    let mut size: Option<(u32, u32)> = None;
    let mut heading_stack: Vec<(u8, String)> = Vec::new();
    for r in &responses {
        page_count =
            page_count.max(r.get("page_count").and_then(Value::as_u64).unwrap_or(0) as u32);
        for w in r
            .get("warnings")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(code) = w.as_str().filter(|c| c.starts_with("DOCS_PARSE_"))
                && !warnings.iter().any(|x| x.code == code)
            {
                warnings.push(IrWarning {
                    code: code.to_owned(),
                    block: None,
                });
            }
        }
        for page in r
            .get("page_list")
            .and_then(Value::as_array)
            .ok_or(PaddleError::Shape("page_list"))?
        {
            let no = page.get("page").and_then(Value::as_u64).unwrap_or(1) as u32;
            let px_w = page.get("px_w").and_then(Value::as_f64).unwrap_or(0.0);
            let px_h = page.get("px_h").and_then(Value::as_f64).unwrap_or(0.0);
            if format == DocFormat::Image {
                size = Some((px_w as u32, px_h as u32));
            }
            let pt_w = page.get("pt_w").and_then(Value::as_f64);
            let pt_h = page.get("pt_h").and_then(Value::as_f64);
            for b in page
                .get("blocks")
                .and_then(Value::as_array)
                .ok_or(PaddleError::Shape("blocks"))?
            {
                let t = b.get("text").and_then(Value::as_str).unwrap_or("").trim();
                if t.is_empty() {
                    continue;
                }
                let bb: Vec<i64> = b
                    .get("bbox_px")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_i64).collect())
                    .unwrap_or_default();
                if bb.len() != 4 {
                    return Err(PaddleError::Shape("bbox_px"));
                }
                let (mut kind, anchor) = match (format, pt_w, pt_h) {
                    (DocFormat::PdfScanned, Some(w), Some(h)) if px_w > 0.0 && px_h > 0.0 => {
                        let kind = kind_of(b.get("kind").and_then(Value::as_str).unwrap_or(""));
                        let a = Anchor::Pdf {
                            page: no,
                            bbox: [
                                px_to_hundredth_pt(bb[0], w, px_w),
                                px_to_hundredth_pt(bb[1], h, px_h),
                                px_to_hundredth_pt(bb[2], w, px_w),
                                px_to_hundredth_pt(bb[3], h, px_h),
                            ],
                            charspan: None,
                            page_w: Some((w * 100.0).round() as i64),
                            page_h: Some((h * 100.0).round() as i64),
                        };
                        (kind, a)
                    }
                    (DocFormat::Image, _, _) => (
                        BlockKind::ImageText,
                        Anchor::Image {
                            parent: None,
                            image_index: None,
                            bbox: Some([bb[0], bb[1], bb[2], bb[3]]),
                        },
                    ),
                    _ => return Err(PaddleError::Shape("page size")),
                };
                let low = b.get("score").and_then(Value::as_f64).unwrap_or(1.0) < LOW_CONFIDENCE;
                let mut level = None;
                let hpath: Vec<String>;
                if kind == BlockKind::Heading {
                    let lvl = if b.get("label").and_then(Value::as_str) == Some("doc_title") {
                        1
                    } else {
                        2
                    };
                    while heading_stack.last().is_some_and(|(l, _)| *l >= lvl) {
                        heading_stack.pop();
                    }
                    hpath = heading_stack.iter().map(|(_, t)| t.clone()).collect();
                    heading_stack.push((lvl, t.chars().take(512).collect()));
                    level = Some(lvl);
                } else {
                    hpath = heading_stack.iter().map(|(_, t)| t.clone()).collect();
                }
                if format == DocFormat::Image {
                    kind = BlockKind::ImageText;
                    level = None;
                }
                blocks.push(RawBlock {
                    kind,
                    level,
                    text: t.to_owned(),
                    hpath: if format == DocFormat::Image {
                        Vec::new()
                    } else {
                        hpath.into_iter().take(oo_core::docir::MAX_HPATH).collect()
                    },
                    parent: None,
                    anchor,
                    table: None,
                    flags: if low {
                        vec![BlockFlag::LowTextConfidence]
                    } else {
                        Vec::new()
                    },
                    exact: false,
                });
            }
        }
    }
    if blocks.is_empty() {
        warnings.push(IrWarning {
            code: "DOCS_PARSE_EMPTY".to_owned(),
            block: None,
        });
    }
    let meta = DocMeta {
        title: blocks
            .iter()
            .find(|b| b.kind == BlockKind::Heading)
            .map(|b| b.text.chars().take(512).collect()),
        page_count: (format == DocFormat::PdfScanned && page_count > 0).then_some(page_count),
        width: size.map(|s| s.0).filter(|w| *w > 0),
        height: size.map(|s| s.1).filter(|h| *h > 0),
        ..DocMeta::default()
    };
    Ok(IrFragment {
        blob_b3,
        format,
        parser,
        meta,
        blocks,
        warnings,
    })
}
