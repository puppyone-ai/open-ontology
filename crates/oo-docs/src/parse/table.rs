//! Tabular blocks shared by the csv and xlsx parsers (10 §3.5, §4.4.3).
//!
//! A table region becomes one `sheet_table` block (the whole region, with a
//! cell grid capped at [`GRID_CELL_CAP`] cells) followed by one `kv` block per
//! non-empty data row (header -> value lines). Rows are 1-based and count
//! blank rows; the anchor of a row is the A1 range over the region's columns.

use std::collections::BTreeMap;

use oo_core::docir::{
    Anchor, BlockFlag, BlockKind, MAX_BLOCKS, TableCell, TableGrid, a1_from_cell,
};

use crate::normalize::{IrWarning, RawBlock};

/// Cells per file (10 §4.4.3), shared by csv and xlsx.
pub const MAX_CELLS: usize = 5_000_000;
/// Columns per row (`XFD`).
pub const MAX_COLS: usize = 16_384;
/// Longest cell text kept, in characters (`table.cells[].text` limit).
pub const MAX_CELL_CHARS: usize = 32_768;
/// Cells written into a `sheet_table` grid; the rest only counts in
/// `rows`/`cols` (warning `DOCS_PARSE_TABLE_TRUNCATED`).
pub const GRID_CELL_CAP: usize = 20_000;
/// Characters of the `sheet_table` text rendering.
pub const TABLE_TEXT_CHARS: usize = 16_000;

/// One cell after merged-region expansion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    /// Original merged-region height and width (1 when not merged).
    pub rs: u32,
    pub cs: u32,
    /// Typed as a number, date or boolean in the source.
    pub numeric: bool,
    pub formula: Formula,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Formula {
    None,
    /// The formula has a cached value, which is what `text` holds.
    Cached,
    /// The formula has no cached value; `text` is empty.
    NoCache,
}

impl Cell {
    pub fn text(text: String, numeric: bool) -> Cell {
        let (text, truncated) = clip(text);
        Cell {
            text,
            rs: 1,
            cs: 1,
            numeric,
            formula: Formula::None,
            truncated,
        }
    }
}

fn clip(text: String) -> (String, bool) {
    if text.chars().count() <= MAX_CELL_CHARS {
        (text, false)
    } else {
        (text.chars().take(MAX_CELL_CHARS).collect(), true)
    }
}

/// Sparse cell map, keys are (row, col), both 1-based.
pub type Cells = BTreeMap<(u32, u32), Cell>;

/// A rectangular region, 1-based inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub r0: u32,
    pub r1: u32,
    pub c0: u32,
    pub c1: u32,
}

fn filled(cell: &Cell) -> bool {
    !cell.text.trim().is_empty() || cell.formula == Formula::NoCache
}

/// Split the used cells into regions separated by at least one empty row.
/// The column extent of a region is the bounding extent of its filled cells.
pub fn detect_regions(cells: &Cells) -> Vec<Region> {
    let mut regions: Vec<Region> = Vec::new();
    let mut open: Option<Region> = None;
    let mut last_row = 0u32;
    for (&(r, c), cell) in cells {
        if !filled(cell) {
            continue;
        }
        match open.as_mut() {
            Some(reg) if r <= last_row + 1 => {
                reg.r1 = reg.r1.max(r);
                reg.c0 = reg.c0.min(c);
                reg.c1 = reg.c1.max(c);
            }
            _ => {
                if let Some(done) = open.take() {
                    regions.push(done);
                }
                open = Some(Region {
                    r0: r,
                    r1: r,
                    c0: c,
                    c1: c,
                });
            }
        }
        last_row = r;
    }
    if let Some(done) = open {
        regions.push(done);
    }
    regions
}

/// First and last filled row, for single-region sources (csv).
pub fn detect_regions_all(cells: &Cells) -> Option<(u32, u32)> {
    let mut it = cells.iter().filter(|(_, c)| filled(c)).map(|(k, _)| k.0);
    let first = it.next()?;
    let last = cells
        .iter()
        .rev()
        .find(|(_, c)| filled(c))
        .map_or(first, |(k, _)| k.0);
    Some((first, last))
}

fn looks_numeric(text: &str) -> bool {
    let t = text.trim().replace(',', "");
    !t.is_empty() && t.trim_end_matches('%').parse::<f64>().is_ok()
}

/// The first row is a header when the region has at least two rows and every
/// filled cell of the row is non-numeric text with no repeated value.
fn header_row(cells: &Cells, reg: &Region) -> bool {
    if reg.r1 == reg.r0 {
        return false;
    }
    let mut seen = Vec::new();
    for c in reg.c0..=reg.c1 {
        let Some(cell) = cells.get(&(reg.r0, c)) else {
            continue;
        };
        let t = cell.text.trim();
        if t.is_empty() {
            continue;
        }
        if cell.numeric || looks_numeric(t) {
            return false;
        }
        let key = t.to_lowercase();
        if seen.contains(&key) {
            return false;
        }
        seen.push(key);
    }
    !seen.is_empty()
}

fn col_name(c: u32) -> String {
    a1_from_cell(c, 1).map_or_else(String::new, |s| s.trim_end_matches('1').to_owned())
}

fn range(reg: &Region, r0: u32, r1: u32) -> String {
    let a = a1_from_cell(reg.c0, r0).unwrap_or_else(|| "A1".to_owned());
    let b = a1_from_cell(reg.c1, r1).unwrap_or_else(|| a.clone());
    if a == b { a } else { format!("{a}:{b}") }
}

/// Where the blocks of a table go and how many `kv` blocks may still be made.
pub struct Sink<'a> {
    pub blocks: &'a mut Vec<RawBlock>,
    pub warnings: &'a mut Vec<IrWarning>,
    /// `kv` blocks still allowed (the block limit minus tables to come).
    pub kv_budget: usize,
}

impl Sink<'_> {
    pub fn warn(&mut self, code: &str, block: Option<usize>) {
        self.warnings.push(IrWarning {
            code: code.to_owned(),
            block,
        });
    }
}

/// Blocks reserved so every table can still get its `sheet_table` block.
pub fn kv_budget(tables: usize) -> usize {
    MAX_BLOCKS.saturating_sub(tables + 1_000)
}

/// Emit one table region. `sheet` is `None` for csv.
pub fn emit_table(
    sink: &mut Sink<'_>,
    sheet: Option<&str>,
    hpath: &[String],
    cells: &Cells,
    reg: &Region,
) {
    let has_header = header_row(cells, reg);
    let first_data = if has_header { reg.r0 + 1 } else { reg.r0 };
    let names: Vec<String> = (reg.c0..=reg.c1)
        .map(|c| {
            let named = has_header
                .then(|| cells.get(&(reg.r0, c)))
                .flatten()
                .map(|x| x.text.trim().to_owned())
                .filter(|t| !t.is_empty());
            named.unwrap_or_else(|| col_name(c))
        })
        .collect();

    let mut flags = Vec::new();
    let add_flag = |f: BlockFlag, flags: &mut Vec<BlockFlag>| {
        if !flags.contains(&f) {
            flags.push(f);
        }
    };
    if !has_header {
        add_flag(BlockFlag::HeaderInferred, &mut flags);
    }
    for c in cells.range((reg.r0, 0)..=(reg.r1, u32::MAX)) {
        let ((_, col), cell) = c;
        if *col < reg.c0 || *col > reg.c1 {
            continue;
        }
        if cell.rs > 1 || cell.cs > 1 {
            add_flag(BlockFlag::MergedCellsExpanded, &mut flags);
        }
        match cell.formula {
            Formula::Cached => add_flag(BlockFlag::FormulaCached, &mut flags),
            Formula::NoCache => add_flag(BlockFlag::FormulaNoCache, &mut flags),
            Formula::None => {}
        }
        if cell.truncated {
            add_flag(BlockFlag::TruncatedCell, &mut flags);
        }
    }

    // Grid and text rendering.
    let rows = reg.r1 - reg.r0 + 1;
    let cols = reg.c1 - reg.c0 + 1;
    let mut grid: Vec<TableCell> = Vec::new();
    let mut text = String::new();
    let mut text_full = false;
    let mut grid_full = false;
    let mut line_row = 0u32;
    let mut line = String::new();
    for (&(r, c), cell) in cells.range((reg.r0, 0)..=(reg.r1, u32::MAX)) {
        if c < reg.c0 || c > reg.c1 {
            continue;
        }
        if grid.len() < GRID_CELL_CAP {
            grid.push(TableCell {
                r: r - reg.r0,
                c: c - reg.c0,
                rs: (cell.rs > 1).then_some(cell.rs),
                cs: (cell.cs > 1).then_some(cell.cs),
                text: cell.text.clone(),
                header: (has_header && r == reg.r0).then_some(true),
            });
        } else {
            grid_full = true;
        }
        if !text_full {
            if r != line_row {
                if !line.is_empty() {
                    text.push_str(&line);
                    text.push('\n');
                    line.clear();
                }
                line_row = r;
            }
            if !line.is_empty() {
                line.push_str(" | ");
            }
            line.push_str(&cell.text.replace('\n', " "));
            if text.chars().count() + line.chars().count() > TABLE_TEXT_CHARS {
                text_full = true;
            }
        }
    }
    if !text_full && !line.is_empty() {
        text.push_str(&line);
    }
    let text = if text_full {
        text.chars().take(TABLE_TEXT_CHARS).collect()
    } else {
        text.trim_end().to_owned()
    };
    let table_idx = sink.blocks.len();
    sink.blocks.push(RawBlock {
        kind: BlockKind::SheetTable,
        level: None,
        text,
        hpath: hpath.to_vec(),
        parent: None,
        anchor: Anchor::Cells {
            sheet: sheet.map(str::to_owned),
            range: range(reg, reg.r0, reg.r1),
        },
        table: Some(TableGrid {
            rows,
            cols,
            header_rows: u32::from(has_header),
            cells: grid,
        }),
        flags,
        exact: false,
    });
    if grid_full || text_full {
        sink.warn("DOCS_PARSE_TABLE_TRUNCATED", Some(table_idx));
    }

    // One kv block per non-empty data row.
    let mut over_budget = false;
    for r in first_data..=reg.r1 {
        let mut lines = Vec::new();
        let mut row_flags = Vec::new();
        for c in reg.c0..=reg.c1 {
            let Some(cell) = cells.get(&(r, c)) else {
                continue;
            };
            if !cell.text.trim().is_empty() {
                lines.push(format!("{}: {}", names[(c - reg.c0) as usize], cell.text));
            }
            if cell.rs > 1 || cell.cs > 1 {
                add_flag(BlockFlag::MergedCellsExpanded, &mut row_flags);
            }
            match cell.formula {
                Formula::Cached => add_flag(BlockFlag::FormulaCached, &mut row_flags),
                Formula::NoCache => add_flag(BlockFlag::FormulaNoCache, &mut row_flags),
                Formula::None => {}
            }
            if cell.truncated {
                add_flag(BlockFlag::TruncatedCell, &mut row_flags);
            }
        }
        if lines.is_empty() {
            continue;
        }
        if sink.kv_budget == 0 {
            over_budget = true;
            break;
        }
        sink.kv_budget -= 1;
        sink.blocks.push(RawBlock {
            kind: BlockKind::Kv,
            level: None,
            text: lines.join("\n"),
            hpath: hpath.to_vec(),
            parent: Some(table_idx),
            anchor: Anchor::Cells {
                sheet: sheet.map(str::to_owned),
                range: range(reg, r, r),
            },
            table: None,
            flags: row_flags,
            exact: false,
        });
    }
    if over_budget {
        sink.warn("DOCS_PARSE_ROWS_TRUNCATED", Some(table_idx));
    }
}
