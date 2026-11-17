//! XLSX parser (`oo-xlsx`) on calamine's streaming cell reader (10 §3.5, §4.4.3).
//!
//! Cells are streamed into a sparse map (calamine's dense `Range` would
//! allocate the whole bounding box of a hostile sheet) and capped at
//! 5,000,000 per workbook. Merged regions are expanded: every covered cell
//! carries the top-left value and the region's `rs`/`cs`. Each sheet is split
//! into table regions at fully empty rows; a lone one-cell region is a
//! `paragraph` (a title), anything else a `sheet_table` plus `kv` row blocks.
//!
//! Known gap: hidden rows and columns are not skipped, because calamine does
//! not expose them and reading the sheet XML here would need a deflate
//! decoder (a new dependency). Hidden sheets are skipped with a warning.

use std::io::Cursor;

use calamine::{DataRef, Reader, SheetType, SheetVisible, Xlsx, XlsxError};
use oo_core::docir::{Anchor, BlockKind, DocFormat, DocMeta, ParserId, a1_from_cell};

use super::table::{
    Cell, Cells, Formula, MAX_CELLS, MAX_COLS, Sink, detect_regions, emit_table, kv_budget,
};
use super::{ParseError, blob_b3, parser_info};
use crate::ingest::zipguard::{self, Package, ZipError};
use crate::normalize::{IrFragment, IrWarning, RawBlock};

const PROFILE: &str = "oo-xlsx/1;calamine=0.38;merged=expand;regions=blank-row;header=first-row-text;kv=header-value;hidden-sheets=skip;cells<=5000000";
const MAX_ROWS: u32 = 1_048_576;
/// Merged regions larger than this are not expanded.
const MERGE_EXPAND_CAP: u64 = 10_000;
const OLE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

impl From<ZipError> for ParseError {
    fn from(e: ZipError) -> Self {
        match e {
            ZipError::Bomb(_) => ParseError::ZipBomb,
            ZipError::Corrupt(_) => ParseError::Corrupt,
            ZipError::Encrypted => ParseError::Encrypted,
        }
    }
}

fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn float_text(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

fn datetime_text(dt: &calamine::ExcelDateTime) -> String {
    let (y, mo, d, h, mi, s, _) = dt.to_ymd_hms_milli();
    if dt.is_duration() {
        let total = (dt.as_f64() * 86_400.0).round() as i64;
        return format!(
            "{}:{:02}:{:02}",
            total / 3600,
            (total / 60) % 60,
            total % 60
        );
    }
    if dt.as_f64().abs() < 1.0 {
        return format!("{h:02}:{mi:02}:{s:02}");
    }
    if h == 0 && mi == 0 && s == 0 {
        format!("{y:04}-{mo:02}-{d:02}")
    } else {
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}")
    }
}

fn value_text(v: &DataRef<'_>) -> (String, bool) {
    match v {
        DataRef::Int(i) => (i.to_string(), true),
        DataRef::Float(f) => (float_text(*f), true),
        DataRef::String(s) => (s.clone(), false),
        DataRef::SharedString(s) => ((*s).to_owned(), false),
        DataRef::Bool(b) => ((if *b { "TRUE" } else { "FALSE" }).to_owned(), true),
        DataRef::DateTime(dt) => (datetime_text(dt), true),
        DataRef::DateTimeIso(s) | DataRef::DurationIso(s) => (s.clone(), true),
        DataRef::Error(e) => (format!("{e}"), false),
        DataRef::Empty => (String::new(), false),
    }
}

fn open_error(e: XlsxError) -> ParseError {
    match e {
        XlsxError::Password => ParseError::Encrypted,
        _ => ParseError::Corrupt,
    }
}

pub fn parse_xlsx_fragment(bytes: &[u8]) -> Result<IrFragment, ParseError> {
    parse_xlsx_fragment_capped(bytes, MAX_CELLS)
}

/// [`parse_xlsx_fragment`] with an explicit workbook cell cap (tests).
pub fn parse_xlsx_fragment_capped(
    bytes: &[u8],
    max_cells: usize,
) -> Result<IrFragment, ParseError> {
    if bytes.starts_with(&OLE_MAGIC) {
        // Encrypted OOXML is an OLE2 wrapper; any other OLE2 is a legacy file.
        let enc = bytes
            .windows(32)
            .any(|w| w == utf16le("EncryptedPackage").as_slice());
        return Err(if enc {
            ParseError::Encrypted
        } else {
            ParseError::Corrupt
        });
    }
    let report = zipguard::check_bytes(bytes)?;
    if report.package != Package::Xlsx {
        return Err(ParseError::Corrupt);
    }
    let mut wb: Xlsx<_> =
        calamine::open_workbook_from_rs(Cursor::new(bytes)).map_err(open_error)?;

    let sheets: Vec<_> = wb.sheets_metadata().to_vec();
    let mut blocks: Vec<RawBlock> = Vec::new();
    let mut warnings: Vec<IrWarning> = Vec::new();
    let mut meta_sheets: Vec<String> = Vec::new();
    let mut kv_left = kv_budget(0);
    let mut total_cells = 0usize;

    for sheet in sheets {
        if sheet.typ != SheetType::WorkSheet {
            continue;
        }
        if sheet.visible != SheetVisible::Visible {
            warnings.push(IrWarning {
                code: "DOCS_PARSE_HIDDEN_SKIPPED".to_owned(),
                block: None,
            });
            continue;
        }
        let name = sheet.name;
        meta_sheets.push(name.clone());
        let merges = wb
            .merge_cells_by_sheet_name(&name)
            .map_err(|_| ParseError::Corrupt)?;
        let mut cells = Cells::new();
        {
            let mut reader = wb
                .worksheet_cells_reader(&name)
                .map_err(|_| ParseError::Corrupt)?;
            while let Some(rec) = reader.next_cell_with_formula().map_err(open_error)? {
                let (row0, col0) = rec.pos;
                if row0 >= MAX_ROWS || col0 as usize >= MAX_COLS {
                    continue;
                }
                let (text, numeric) = value_text(&rec.value);
                let mut cell = Cell::text(text, numeric);
                if rec.formula.is_some() {
                    cell.formula = if cell.text.trim().is_empty() {
                        Formula::NoCache
                    } else {
                        Formula::Cached
                    };
                }
                if cell.text.trim().is_empty() && cell.formula == Formula::None {
                    continue;
                }
                total_cells += 1;
                if total_cells > max_cells {
                    return Err(ParseError::LimitCells);
                }
                cells.insert((row0 + 1, col0 + 1), cell);
            }
        }
        expand_merges(
            &mut cells,
            &merges,
            &mut warnings,
            &mut total_cells,
            max_cells,
        )?;

        let hpath = vec![name.clone()];
        for reg in detect_regions(&cells) {
            let only = (reg.r0 == reg.r1 && reg.c0 == reg.c1)
                .then(|| cells.get(&(reg.r0, reg.c0)))
                .flatten();
            if let Some(cell) = only {
                let a1 = a1_from_cell(reg.c0, reg.r0).unwrap_or_else(|| "A1".to_owned());
                blocks.push(RawBlock {
                    kind: BlockKind::Paragraph,
                    level: None,
                    text: cell.text.clone(),
                    hpath: hpath.clone(),
                    parent: None,
                    anchor: Anchor::Cells {
                        sheet: Some(name.clone()),
                        range: a1,
                    },
                    table: None,
                    flags: Vec::new(),
                    exact: false,
                });
                continue;
            }
            let mut sink = Sink {
                blocks: &mut blocks,
                warnings: &mut warnings,
                kv_budget: kv_left,
            };
            emit_table(&mut sink, Some(&name), &hpath, &cells, &reg);
            kv_left = sink.kv_budget;
        }
    }

    if blocks.is_empty() {
        warnings.push(IrWarning {
            code: "DOCS_PARSE_EMPTY".to_owned(),
            block: None,
        });
    }
    meta_sheets.truncate(256);
    Ok(IrFragment {
        blob_b3: blob_b3(bytes),
        format: DocFormat::Xlsx,
        parser: parser_info(ParserId::OoXlsx, PROFILE),
        meta: DocMeta {
            sheets: meta_sheets,
            has_macros: report.has_macros.then_some(true),
            ..DocMeta::default()
        },
        blocks,
        warnings,
    })
}

/// Copy each merged region's top-left cell into every covered cell and mark
/// the region's size on all of them.
fn expand_merges(
    cells: &mut Cells,
    merges: &[calamine::Dimensions],
    warnings: &mut Vec<IrWarning>,
    total_cells: &mut usize,
    max_cells: usize,
) -> Result<(), ParseError> {
    for m in merges {
        let (r0, c0) = (m.start.0 + 1, m.start.1 + 1);
        let (r1, c1) = (m.end.0 + 1, m.end.1 + 1);
        if r1 < r0 || c1 < c0 || r1 > MAX_ROWS || c1 as usize > MAX_COLS {
            continue;
        }
        let (h, w) = (r1 - r0 + 1, c1 - c0 + 1);
        if h == 1 && w == 1 {
            continue;
        }
        let Some(origin) = cells.get(&(r0, c0)).cloned() else {
            continue;
        };
        if u64::from(h) * u64::from(w) > MERGE_EXPAND_CAP {
            if let Some(c) = cells.get_mut(&(r0, c0)) {
                c.rs = h;
                c.cs = w;
            }
            warnings.push(IrWarning {
                code: "DOCS_PARSE_MERGE_TOO_LARGE".to_owned(),
                block: None,
            });
            continue;
        }
        for r in r0..=r1 {
            for c in c0..=c1 {
                let mut copy = origin.clone();
                copy.rs = h;
                copy.cs = w;
                if cells.insert((r, c), copy).is_none() {
                    *total_cells += 1;
                    if *total_cells > max_cells {
                        return Err(ParseError::LimitCells);
                    }
                }
            }
        }
    }
    Ok(())
}

/// Parse a `.xlsx` file to canonical IR.
pub fn parse_xlsx(bytes: &[u8]) -> Result<crate::normalize::Normalized, ParseError> {
    Ok(crate::normalize::finish(parse_xlsx_fragment(bytes)?)?)
}
