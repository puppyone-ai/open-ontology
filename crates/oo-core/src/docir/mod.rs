//! `oo.doc-ir/v1`: the document intermediate representation shared by the
//! document pipeline (parse, normalize, extract, compile) and the evaluator.
//!
//! A document is an ordered sequence of blocks. Every block carries a stable
//! anchor into the original file; anchors are the base of provenance and of
//! scope isolation (Super Ontology spec 10 §3.4, §3.5; decision D14).
//!
//! This module is types and pure checks only. Block ids are computed by the
//! pipeline (BLAKE3 lives there); here they are only shape-checked.

pub mod anchor;
#[cfg(kani)]
mod proofs;
pub mod validate;

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use anchor::{Anchor, AnchorError, a1_from_cell, a1_parse, is_speaker_label};
pub use validate::{IrViolation, needs_v11, validate};

/// Protocol tag written into every IR document.
pub const PROTOCOL: &str = "oo.doc-ir/v1";
/// v1.1 (spec 10 §17.7, D70): backward compatible; adds audio, video, image
/// and scanned-pdf documents. Every v1 document is still valid.
pub const PROTOCOL_V11: &str = "oo.doc-ir/v1.1";
/// Maximum blocks per document (`limits.max_blocks_per_file`).
pub const MAX_BLOCKS: usize = 200_000;
/// Maximum UTF-8 length of one block text, in characters.
pub const MAX_BLOCK_TEXT: usize = 65_536;
/// Maximum heading-path depth.
pub const MAX_HPATH: usize = 9;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DocFormat {
    Md,
    Txt,
    Csv,
    Xlsx,
    Docx,
    Pptx,
    Pdf,
    /// v1.1
    Audio,
    Video,
    Image,
    PdfScanned,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum ParserId {
    #[serde(rename = "oo-md")]
    OoMd,
    #[serde(rename = "oo-text")]
    OoText,
    #[serde(rename = "oo-csv")]
    OoCsv,
    #[serde(rename = "oo-xlsx")]
    OoXlsx,
    #[serde(rename = "docling")]
    Docling,
    /// v1.1
    #[serde(rename = "paddleocr")]
    PaddleOcr,
    /// v1.1
    #[serde(rename = "media")]
    Media,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParserInfo {
    pub id: ParserId,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_version: Option<String>,
    pub profile_sha256: String,
    /// v1.1: compute tier, models (name and sha256) and CPU instruction-set fingerprint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ParserProfile>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ComputeTier {
    #[serde(rename = "cpu")]
    Cpu,
    #[serde(rename = "gpu")]
    Gpu,
    #[serde(rename = "host-accel")]
    HostAccel,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParserModel {
    pub name: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParserProfile {
    pub tier: ComputeTier,
    pub models: Vec<ParserModel>,
    pub isa: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum TextEncoding {
    #[serde(rename = "utf-8")]
    Utf8,
    #[serde(rename = "utf-8-bom")]
    Utf8Bom,
    #[serde(rename = "gb18030")]
    Gb18030,
    #[serde(rename = "utf-16le")]
    Utf16Le,
    #[serde(rename = "utf-16be")]
    Utf16Be,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slide_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sheets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frontmatter: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding: Option<TextEncoding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_macros: Option<bool>,
    /// v1.1 media and image facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Speaker labels only (`S1`...); never a name or a voice feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub speakers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    Heading,
    Paragraph,
    ListItem,
    Quote,
    Code,
    Table,
    Caption,
    Frontmatter,
    SlideTitle,
    SlideNotes,
    SheetTable,
    Kv,
    PageHeader,
    PageFooter,
    Footnote,
    /// v1.1: one speaker's continuous speech.
    Utterance,
    /// v1.1: a video scene (merged frame text).
    Scene,
    FrameText,
    ImageText,
    ImageCaption,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum BlockFlag {
    MergedCellsExpanded,
    HeaderInferred,
    FormulaCached,
    FormulaNoCache,
    LowTextConfidence,
    TruncatedCell,
    /// v1.1: text produced by a generative model (captions); facts need review.
    ModelGenerated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableCell {
    pub r: u32,
    pub c: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cs: Option<u32>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableGrid {
    pub rows: u32,
    pub cols: u32,
    pub header_rows: u32,
    pub cells: Vec<TableCell>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Block {
    /// `blk_` + 20 lowercase hex characters (10 §3.4).
    pub id: String,
    pub ordinal: u32,
    pub kind: BlockKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    pub text: String,
    pub hpath: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub anchor: Anchor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<TableGrid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<BlockFlag>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrWarning {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
}

/// One parsed document in `oo.doc-ir/v1`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocIr {
    pub protocol: String,
    pub blob_b3: String,
    pub format: DocFormat,
    pub parser: ParserInfo,
    pub meta: DocMeta,
    pub blocks: Vec<Block>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<IrWarning>,
}

/// The JSON Schema shipped at `contracts/docs-ir/doc-ir.v1.schema.json`.
pub const SCHEMA_JSON: &str = include_str!("../../../../contracts/docs-ir/doc-ir.v1.schema.json");

#[cfg(test)]
mod tests;
