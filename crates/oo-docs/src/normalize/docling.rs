//! DoclingDocument JSON to `oo.doc-ir/v1` (10 §3.5, §4.5), for pdf, docx, pptx.
//!
//! Reading order is the depth-first order of `body.children`. Anchors:
//!
//! * pdf: page and bbox from `prov[0]`; the bbox moves from points with a
//!   bottom-left origin to 1/100 pt with a top-left origin (`y' = h - y`).
//! * pptx: `prov[0].page_no` is the slide; notes are `content_layer: notes`;
//!   paragraphs are counted per slide and layer.
//! * docx: Docling gives no provenance, so the sidecar also reads
//!   `word/document.xml` (`ooxml`: body paragraphs in order, table positions)
//!   and each Docling text item is matched, in order and by normalized text,
//!   to a body paragraph ordinal. A paragraph that cannot be matched keeps
//!   the last matched ordinal and a warning. Docx tables get one block for the
//!   grid and one `kv` block per data row (`#T1!R2C1:R2C3`, tables 1-based).
//!
//! Windows of a PDF are merged in page order; ordinals come from `finish`.

use std::collections::BTreeMap;

use oo_core::docir::{
    Anchor, BlockKind, DocFormat, DocMeta, MAX_HPATH, ParserId, ParserInfo, TableCell, TableGrid,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use super::{IrFragment, IrWarning, RawBlock};

const PROFILE: &str = "docling/1;ocr=off;table_structure=on;pictures=off;window=20;bbox=1/100pt-top-left;docx-ordinals=ooxml";
/// Table text rendering cap, same as the sheet tables.
const TABLE_TEXT_CHARS: usize = 16_000;
const MAX_CELL_CHARS: usize = 32_768;
/// How far past the last matched paragraph a docx text item may be found.
const DOCX_LOOKAHEAD: usize = 64;
const MAX_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DoclingError {
    /// A required part of the document JSON is missing.
    Shape(&'static str),
    /// No window responses.
    Empty,
}

impl std::fmt::Display for DoclingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shape(w) => write!(f, "docling document: {w}"),
            Self::Empty => f.write_str("no parser output"),
        }
    }
}

impl std::error::Error for DoclingError {}

/// One `POST /v1/parse` response, kept as JSON.
#[derive(Clone, Debug)]
pub struct WindowResponse {
    pub parser: Value,
    pub page_count: u32,
    pub pages: Option<(u32, u32)>,
    pub doc: Value,
    /// Docx only: `{"paragraphs":[{"text","style","level"}],"tables":[{"before","rows","cols"}]}`.
    pub ooxml: Option<Value>,
    pub warnings: Vec<String>,
}

impl WindowResponse {
    pub fn from_value(mut v: Value) -> Self {
        let u = |x: &Value| x.as_u64().unwrap_or(0) as u32;
        let pages = v
            .get("pages")
            .and_then(Value::as_array)
            .and_then(|a| Some((u(a.first()?), u(a.get(1)?))));
        WindowResponse {
            parser: v.get("parser").cloned().unwrap_or(Value::Null),
            page_count: v.get("page_count").map_or(0, u),
            pages,
            doc: v["doc"].take(),
            ooxml: v.get_mut("ooxml").map(Value::take).filter(|o| !o.is_null()),
            warnings: v
                .get("warnings")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|w| w.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

fn profile_sha256() -> String {
    let d = Sha256::digest(PROFILE.as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn norm(s: &str) -> String {
    let nfkc: String = s.nfkc().collect();
    nfkc.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---- tree walk -------------------------------------------------------------------

/// `#/texts/3` -> that node.
fn resolve<'a>(doc: &'a Value, r: &str) -> Option<&'a Value> {
    let path = r.strip_prefix("#/")?;
    let mut cur = doc;
    for part in path.split('/') {
        cur = match cur {
            Value::Array(a) => a.get(part.parse::<usize>().ok()?)?,
            Value::Object(_) => cur.get(part)?,
            _ => return None,
        };
    }
    Some(cur)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layer {
    Header,
    Body,
    Notes,
    Footer,
}

struct Item<'a> {
    node: &'a Value,
    is_table: bool,
    layer: Layer,
    seq: usize,
}

fn walk<'a>(doc: &'a Value, node: &'a Value, layer: Layer, out: &mut Vec<Item<'a>>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    let Some(children) = node.get("children").and_then(Value::as_array) else {
        return;
    };
    for child in children {
        let Some(r) = child.get("$ref").and_then(Value::as_str) else {
            continue;
        };
        let Some(n) = resolve(doc, r) else { continue };
        let own_layer = match n.get("content_layer").and_then(Value::as_str) {
            Some("notes") => Layer::Notes,
            _ => layer,
        };
        if r.starts_with("#/texts/") {
            out.push(Item {
                node: n,
                is_table: false,
                layer: own_layer,
                seq: out.len(),
            });
            // Section headers own the items below them (docx, md-like trees).
            walk(doc, n, own_layer, out, depth + 1);
        } else if r.starts_with("#/tables/") {
            out.push(Item {
                node: n,
                is_table: true,
                layer: own_layer,
                seq: out.len(),
            });
            // A table caption is a child text of the table.
            walk(doc, n, own_layer, out, depth + 1);
        } else {
            // groups, pictures (captions), inline groups
            walk(doc, n, own_layer, out, depth + 1);
        }
    }
}

fn collect<'a>(doc: &'a Value, pdf: bool) -> Vec<Item<'a>> {
    let mut items = Vec::new();
    if let Some(body) = doc.get("body") {
        walk(doc, body, Layer::Body, &mut items, 0);
    }
    if let Some(furniture) = doc.get("furniture") {
        let start = items.len();
        walk(doc, furniture, Layer::Body, &mut items, 0);
        // Page furniture: header before, footer after the page body.
        for it in &mut items[start..] {
            it.layer = match it.node.get("label").and_then(Value::as_str) {
                Some("page_footer") => Layer::Footer,
                _ => Layer::Header,
            };
        }
    }
    if pdf {
        let page = |it: &Item| first_prov(it.node).map_or(0, |p| p.0);
        items.sort_by_key(|it| {
            let order = match it.layer {
                Layer::Header => 0,
                Layer::Footer => 2,
                _ => 1,
            };
            (page(it), order, it.seq)
        });
    }
    items
}

struct Prov {
    bbox: Option<[f64; 4]>,
    origin_bottom_left: bool,
    charspan: Option<[u64; 2]>,
}

fn first_prov(node: &Value) -> Option<(u32, Prov)> {
    let p = node.get("prov")?.as_array()?.first()?;
    let page = p.get("page_no")?.as_u64()? as u32;
    let bbox = p.get("bbox").and_then(|b| {
        let f = |k: &str| b.get(k).and_then(Value::as_f64);
        Some([f("l")?, f("t")?, f("r")?, f("b")?])
    });
    let origin = p
        .get("bbox")
        .and_then(|b| b.get("coord_origin"))
        .and_then(Value::as_str)
        .unwrap_or("BOTTOMLEFT");
    let charspan = p
        .get("charspan")
        .and_then(Value::as_array)
        .and_then(|a| Some([a.first()?.as_u64()?, a.get(1)?.as_u64()?]));
    Some((
        page,
        Prov {
            bbox,
            origin_bottom_left: origin.eq_ignore_ascii_case("BOTTOMLEFT"),
            charspan,
        },
    ))
}

/// Points, bottom-left origin (or top-left) to 1/100 pt, top-left origin.
fn pdf_bbox(p: &Prov, page_h: f64) -> [i64; 4] {
    let Some([l, t, r, b]) = p.bbox else {
        return [0, 0, 0, 0];
    };
    let (y0, y1) = if p.origin_bottom_left {
        (page_h - t, page_h - b)
    } else {
        (t, b)
    };
    let q = |v: f64| (v * 100.0).round() as i64;
    let (x0, x1) = (q(l.min(r)), q(l.max(r)));
    let (y0, y1) = (q(y0.min(y1)), q(y0.max(y1)));
    [x0, y0, x1, y1]
}

fn page_height(doc: &Value, page: u32) -> f64 {
    doc.get("pages")
        .and_then(|p| p.get(page.to_string()))
        .and_then(|p| p.get("size"))
        .and_then(|s| s.get("height"))
        .and_then(Value::as_f64)
        .unwrap_or(842.0)
}

// ---- blocks -----------------------------------------------------------------------

struct Builder {
    format: DocFormat,
    blocks: Vec<RawBlock>,
    warnings: Vec<IrWarning>,
    heading_stack: Vec<(u8, String)>,
    title: Option<String>,
}

impl Builder {
    fn hpath(&self) -> Vec<String> {
        self.heading_stack
            .iter()
            .take(MAX_HPATH)
            .map(|(_, t)| clip(t, 512))
            .collect()
    }

    fn warn(&mut self, code: &str) {
        let block = self.blocks.len().checked_sub(1);
        self.warnings.push(IrWarning {
            code: code.to_owned(),
            block,
        });
    }

    fn push_heading(&mut self, level: u8, text: &str) {
        while self.heading_stack.last().is_some_and(|(l, _)| *l >= level) {
            self.heading_stack.pop();
        }
        self.heading_stack.push((level, text.to_owned()));
    }
}

fn label(node: &Value) -> &str {
    node.get("label").and_then(Value::as_str).unwrap_or("")
}

fn text_of(node: &Value) -> String {
    node.get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .replace("\r\n", "\n")
        .trim()
        .to_owned()
}

fn kind_of(label: &str) -> BlockKind {
    match label {
        "list_item" | "checkbox_selected" | "checkbox_unselected" => BlockKind::ListItem,
        "caption" => BlockKind::Caption,
        "footnote" => BlockKind::Footnote,
        "page_header" => BlockKind::PageHeader,
        "page_footer" => BlockKind::PageFooter,
        "code" => BlockKind::Code,
        _ => BlockKind::Paragraph,
    }
}

struct TableData {
    rows: u32,
    cols: u32,
    header_rows: u32,
    cells: Vec<TableCell>,
}

fn table_data(node: &Value) -> Option<TableData> {
    let data = node.get("data")?;
    let rows = data.get("num_rows")?.as_u64()? as u32;
    let cols = data.get("num_cols")?.as_u64()? as u32;
    let mut cells = Vec::new();
    let mut header_rows = 0u32;
    for c in data.get("table_cells")?.as_array()? {
        let g = |k: &str| c.get(k).and_then(Value::as_u64).map(|v| v as u32);
        let (Some(r), Some(col)) = (g("start_row_offset_idx"), g("start_col_offset_idx")) else {
            continue;
        };
        if r >= rows || col >= cols {
            continue;
        }
        let rs = g("end_row_offset_idx")
            .map(|e| e.saturating_sub(r))
            .filter(|n| *n > 1);
        let cs = g("end_col_offset_idx")
            .map(|e| e.saturating_sub(col))
            .filter(|n| *n > 1);
        let header = c.get("column_header").and_then(Value::as_bool) == Some(true);
        if header {
            header_rows = header_rows.max(g("end_row_offset_idx").unwrap_or(r + 1));
        }
        cells.push(TableCell {
            r,
            c: col,
            rs,
            cs,
            text: clip(
                c.get("text").and_then(Value::as_str).unwrap_or("").trim(),
                MAX_CELL_CHARS,
            ),
            header: header.then_some(true),
        });
    }
    cells.sort_by_key(|c| (c.r, c.c));
    Some(TableData {
        rows,
        cols,
        header_rows: header_rows.min(rows),
        cells,
    })
}

fn render_table(t: &TableData) -> String {
    let mut out = String::new();
    let mut row = u32::MAX;
    for c in &t.cells {
        if c.r != row {
            if !out.is_empty() {
                out.push('\n');
            }
            row = c.r;
        } else {
            out.push_str(" | ");
        }
        out.push_str(&c.text.replace('\n', " "));
        if out.chars().count() > TABLE_TEXT_CHARS {
            return clip(&out, TABLE_TEXT_CHARS);
        }
    }
    out
}

/// One window's blocks into the builder.
fn add_window(b: &mut Builder, w: &WindowResponse, docx_state: &mut DocxState) {
    let doc = &w.doc;
    let pdf = b.format == DocFormat::Pdf;
    let items = collect(doc, pdf);
    let mut slide_para: BTreeMap<(u32, bool), u32> = BTreeMap::new();
    let mut table_no = 0u32;
    for it in items {
        let node = it.node;
        let prov = first_prov(node);
        if it.is_table {
            table_no += 1;
            add_table(
                b,
                w,
                node,
                prov.as_ref(),
                table_no,
                docx_state,
                &mut slide_para,
                it.layer,
            );
            continue;
        }
        let text = text_of(node);
        if text.is_empty() {
            continue;
        }
        let lab = label(node);
        let mut kind = kind_of(lab);
        let mut level = None;
        if lab == "title" || lab == "section_header" {
            let raw = node
                .get("level")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .max(1) as u8;
            // A document title is level 1; sections nest under it.
            let lv = if lab == "title" {
                1
            } else {
                raw + u8::from(b.title.is_some())
            };
            let lv = lv.min(9);
            if b.format == DocFormat::Pptx && lab == "title" {
                kind = BlockKind::SlideTitle;
            } else {
                kind = BlockKind::Heading;
                level = Some(lv);
            }
            if lab == "title" && b.title.is_none() {
                b.title = Some(clip(&text, 512));
            }
        }
        let anchor = match b.format {
            DocFormat::Pdf => {
                let Some((page, p)) = prov else {
                    b.warn("DOCS_PARSE_NO_PROVENANCE");
                    continue;
                };
                Anchor::Pdf {
                    page,
                    bbox: pdf_bbox(&p, page_height(doc, page)),
                    charspan: p.charspan,
                    page_w: None,
                    page_h: None,
                }
            }
            DocFormat::Pptx => {
                let slide = prov.as_ref().map_or(1, |(p, _)| *p).max(1);
                let notes = it.layer == Layer::Notes;
                let n = slide_para.entry((slide, notes)).or_insert(0);
                let idx = *n;
                *n += 1;
                if notes {
                    kind = BlockKind::SlideNotes;
                }
                Anchor::Slide {
                    slide,
                    shape_id: None,
                    para_start: Some(idx),
                    para_end: Some(idx),
                    notes: notes.then_some(true),
                }
            }
            _ => {
                let (s, e) = docx_state.align(b, &text, w.ooxml.as_ref());
                Anchor::Docx {
                    para_start: s,
                    para_end: e,
                    table_index: None,
                    cell_range: None,
                }
            }
        };
        if let Some(lv) = level {
            b.push_heading(lv, &text);
        }
        let hpath = b.hpath();
        b.blocks.push(RawBlock {
            kind,
            level,
            text,
            hpath,
            parent: None,
            anchor,
            table: None,
            flags: Vec::new(),
            exact: false,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn add_table(
    b: &mut Builder,
    w: &WindowResponse,
    node: &Value,
    prov: Option<&(u32, Prov)>,
    table_no: u32,
    docx_state: &mut DocxState,
    slide_para: &mut BTreeMap<(u32, bool), u32>,
    layer: Layer,
) {
    let Some(t) = table_data(node) else {
        b.warn("DOCS_PARSE_TABLE_SKIPPED");
        return;
    };
    if t.cells.is_empty() {
        return;
    }
    let doc = &w.doc;
    let text = render_table(&t);
    let hpath = b.hpath();
    let grid = |t: &TableData| TableGrid {
        rows: t.rows,
        cols: t.cols,
        header_rows: t.header_rows,
        cells: t.cells.clone(),
    };
    let anchor = match b.format {
        DocFormat::Pdf => {
            let Some((page, p)) = prov else {
                b.warn("DOCS_PARSE_NO_PROVENANCE");
                return;
            };
            Anchor::Pdf {
                page: *page,
                bbox: pdf_bbox(p, page_height(doc, *page)),
                charspan: None,
                page_w: None,
                page_h: None,
            }
        }
        DocFormat::Pptx => {
            let slide = prov.map_or(1, |(p, _)| *p).max(1);
            let notes = layer == Layer::Notes;
            let n = slide_para.entry((slide, notes)).or_insert(0);
            let idx = *n;
            *n += 1;
            Anchor::Slide {
                slide,
                shape_id: None,
                para_start: Some(idx),
                para_end: Some(idx),
                notes: notes.then_some(true),
            }
        }
        _ => {
            let before = docx_state.table_before(table_no, w.ooxml.as_ref());
            Anchor::Docx {
                para_start: before,
                para_end: before,
                table_index: Some(table_no),
                cell_range: Some(format!("R1C1:R{}C{}", t.rows.max(1), t.cols.max(1))),
            }
        }
    };
    let table_idx = b.blocks.len();
    b.blocks.push(RawBlock {
        kind: BlockKind::Table,
        level: None,
        text,
        hpath: hpath.clone(),
        parent: None,
        anchor: anchor.clone(),
        table: Some(grid(&t)),
        flags: Vec::new(),
        exact: false,
    });
    // Docx: one kv block per data row so a provenance label can name a row.
    if let Anchor::Docx {
        para_start,
        para_end,
        table_index,
        ..
    } = anchor
    {
        let header: Vec<String> = (0..t.cols)
            .map(|c| {
                t.cells
                    .iter()
                    .find(|x| x.r == 0 && x.c == c && t.header_rows > 0)
                    .map(|x| x.text.clone())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| format!("C{}", c + 1))
            })
            .collect();
        for r in t.header_rows..t.rows {
            let row: Vec<&TableCell> = t.cells.iter().filter(|c| c.r == r).collect();
            let lines: Vec<String> = row
                .iter()
                .filter(|c| !c.text.is_empty())
                .map(|c| format!("{}: {}", header[c.c as usize], c.text))
                .collect();
            if lines.is_empty() {
                continue;
            }
            b.blocks.push(RawBlock {
                kind: BlockKind::Kv,
                level: None,
                text: lines.join("\n"),
                hpath: hpath.clone(),
                parent: Some(table_idx),
                anchor: Anchor::Docx {
                    para_start,
                    para_end,
                    table_index,
                    cell_range: Some(format!("R{}C1:R{}C{}", r + 1, r + 1, t.cols)),
                },
                table: None,
                flags: Vec::new(),
                exact: false,
            });
        }
    }
}

/// Matches Docling text items to body paragraphs of a docx, in order.
#[derive(Default)]
struct DocxState {
    next: usize,
    last: u32,
    unmatched: u32,
    /// Normalized paragraph texts, built on first use.
    norm: Option<Vec<String>>,
}

impl DocxState {
    fn paragraphs(&mut self, ooxml: Option<&Value>) -> &[String] {
        self.norm.get_or_insert_with(|| {
            ooxml
                .and_then(|o| o.get("paragraphs"))
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|p| norm(p.get("text").and_then(Value::as_str).unwrap_or("")))
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    fn table_before(&mut self, table_no: u32, ooxml: Option<&Value>) -> u32 {
        ooxml
            .and_then(|o| o.get("tables"))
            .and_then(Value::as_array)
            .and_then(|t| t.get(table_no as usize - 1))
            .and_then(|t| t.get("before"))
            .and_then(Value::as_u64)
            .map_or(self.last, |v| v as u32)
    }

    fn align(&mut self, b: &mut Builder, text: &str, ooxml: Option<&Value>) -> (u32, u32) {
        let target = norm(text);
        let start = self.next;
        let paras = self.paragraphs(ooxml);
        if paras.is_empty() {
            self.unmatched += 1;
            b.warn("DOCS_PARSE_DOCX_NO_OOXML");
            let n = self.last;
            self.last += 1;
            return (n, n);
        }
        let end = (start + DOCX_LOOKAHEAD).min(paras.len());
        let exact = (start..end).find(|i| paras[*i] == target);
        let found = exact.or_else(|| {
            // Docling may split one paragraph into several items.
            (start.saturating_sub(1)..end)
                .find(|i| !paras[*i].is_empty() && target.len() >= 2 && paras[*i].contains(&target))
        });
        match found {
            Some(i) => {
                self.next = if exact.is_some() { i + 1 } else { i };
                self.last = i as u32;
                (i as u32, i as u32)
            }
            None => {
                self.unmatched += 1;
                b.warn("DOCS_PARSE_DOCX_ALIGN");
                (self.last, self.last)
            }
        }
    }
}

/// Merge the windows of one document into an IR fragment.
pub fn assemble(
    format: DocFormat,
    blob_b3: String,
    windows: Vec<WindowResponse>,
) -> Result<IrFragment, DoclingError> {
    let first = windows.first().ok_or(DoclingError::Empty)?;
    if !first.doc.is_object() {
        return Err(DoclingError::Shape("doc is not an object"));
    }
    let parser = ParserInfo {
        id: ParserId::Docling,
        version: clip(
            first
                .parser
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("unknown"),
            64,
        ),
        adapter_version: first
            .parser
            .get("adapter_version")
            .and_then(Value::as_str)
            .map(str::to_owned),
        profile_sha256: profile_sha256(),
        profile: None,
    };
    let mut b = Builder {
        format,
        blocks: Vec::new(),
        warnings: Vec::new(),
        heading_stack: Vec::new(),
        title: None,
    };
    let mut docx_state = DocxState::default();
    let mut slides = std::collections::BTreeSet::new();
    let mut ordered: Vec<&WindowResponse> = windows.iter().collect();
    ordered.sort_by_key(|w| w.pages.map_or(0, |p| p.0));
    for w in &ordered {
        add_window(&mut b, w, &mut docx_state);
        for tag in &w.warnings {
            // Warnings from the sidecar are already codes.
            if tag.starts_with("DOCS_PARSE_") {
                b.warnings.push(IrWarning {
                    code: tag.clone(),
                    block: None,
                });
            }
        }
    }
    if format == DocFormat::Pptx {
        for blk in &b.blocks {
            if let Anchor::Slide { slide, .. } = &blk.anchor {
                slides.insert(*slide);
            }
        }
    }
    if b.blocks.is_empty() {
        b.warnings.push(IrWarning {
            code: "DOCS_PARSE_EMPTY".to_owned(),
            block: None,
        });
    }
    let page_count = windows.iter().map(|w| w.page_count).max().unwrap_or(0);
    let meta = DocMeta {
        title: b.title.clone(),
        page_count: (format == DocFormat::Pdf && page_count > 0).then_some(page_count),
        slide_count: (format == DocFormat::Pptx)
            .then(|| slides.iter().next_back().copied())
            .flatten(),
        ..DocMeta::default()
    };
    if docx_state.unmatched > 0 {
        b.warnings.push(IrWarning {
            code: "DOCS_PARSE_DOCX_ALIGN".to_owned(),
            block: None,
        });
    }
    Ok(IrFragment {
        blob_b3,
        format,
        parser,
        meta,
        blocks: b.blocks,
        warnings: b.warnings,
    })
}
