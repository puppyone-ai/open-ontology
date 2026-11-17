//! Delimited text parser (`oo-csv`, 10 §3.5, §4.4.3).
//!
//! The dialect is sniffed from the first 64 KiB: the delimiter among `,` tab
//! `;` with the most records sharing one stable column count. Rows are
//! records (a quoted newline stays inside its field); row numbers start at 1
//! and include the header and blank lines, so `#R12` is the 12th record.

use oo_core::docir::{DocFormat, DocMeta, ParserId};

use super::decode::decode_text;
use super::table::{
    Cell, Cells, MAX_CELLS, MAX_COLS, Region, Sink, detect_regions_all, emit_table, kv_budget,
};
use super::{ParseError, blob_b3, parser_info};
use crate::normalize::{IrFragment, IrWarning};

const PROFILE: &str = "oo-csv/1;delims=comma,tab,semicolon;sample=65536;rows=records;header=first-row-text;kv=header-value;cells<=5000000";

const SAMPLE_BYTES: usize = 64 * 1024;
const SAMPLE_RECORDS: usize = 100;

struct Records {
    rows: Vec<Vec<String>>,
    unclosed_quote: bool,
}

fn read_records(text: &str, delim: char, limit_records: Option<usize>) -> Records {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut quoted_field = false;
    let mut chars = text.chars().peekable();
    let mut unclosed_quote = false;
    let mut any = false;
    while let Some(ch) = chars.next() {
        any = true;
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        match ch {
            '"' if field.is_empty() && !quoted_field => {
                in_quotes = true;
                quoted_field = true;
            }
            c if c == delim => {
                row.push(std::mem::take(&mut field));
                quoted_field = false;
            }
            '\r' | '\n' => {
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                quoted_field = false;
                rows.push(std::mem::take(&mut row));
                any = false;
                if limit_records.is_some_and(|n| rows.len() >= n) {
                    return Records {
                        rows,
                        unclosed_quote,
                    };
                }
            }
            c => field.push(c),
        }
    }
    if in_quotes {
        unclosed_quote = true;
    }
    if any || !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Records {
        rows,
        unclosed_quote,
    }
}

/// Pick the delimiter from a sample of the text.
pub fn sniff_delimiter(text: &str) -> char {
    let mut end = text.len().min(SAMPLE_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let sample = &text[..end];
    // A sample cut mid-record would skew the last row: drop it.
    let sample = if end < text.len() {
        sample.rsplit_once('\n').map_or(sample, |(head, _)| head)
    } else {
        sample
    };
    let mut best = (0usize, 0usize, ',');
    for delim in [',', '\t', ';'] {
        let recs = read_records(sample, delim, Some(SAMPLE_RECORDS));
        let counts: Vec<usize> = recs
            .rows
            .iter()
            .filter(|r| !(r.len() == 1 && r[0].is_empty()))
            .map(Vec::len)
            .collect();
        let Some(mode) = counts
            .iter()
            .copied()
            .max_by_key(|m| (counts.iter().filter(|c| *c == m).count(), *m))
        else {
            continue;
        };
        if mode < 2 {
            continue;
        }
        let stable = counts.iter().filter(|c| **c == mode).count();
        if (stable, mode) > (best.0, best.1) {
            best = (stable, mode, delim);
        }
    }
    best.2
}

pub fn parse_csv_fragment(bytes: &[u8]) -> Result<IrFragment, ParseError> {
    parse_csv_fragment_capped(bytes, MAX_CELLS)
}

/// [`parse_csv_fragment`] with an explicit cell cap (tests).
pub fn parse_csv_fragment_capped(bytes: &[u8], max_cells: usize) -> Result<IrFragment, ParseError> {
    let decoded = decode_text(bytes)?;
    let delim = sniff_delimiter(&decoded.text);
    let recs = read_records(&decoded.text, delim, None);

    let mut cells = Cells::new();
    let mut total = 0usize;
    let mut max_cols = 0usize;
    for (i, row) in recs.rows.iter().enumerate() {
        if row.len() > MAX_COLS {
            return Err(ParseError::LimitCells);
        }
        max_cols = max_cols.max(row.len());
        for (j, text) in row.iter().enumerate() {
            if text.trim().is_empty() {
                continue;
            }
            total += 1;
            if total > max_cells {
                return Err(ParseError::LimitCells);
            }
            let text = text.replace("\r\n", "\n");
            cells.insert((i as u32 + 1, j as u32 + 1), Cell::text(text, false));
        }
    }

    let mut blocks = Vec::new();
    let mut warnings = Vec::new();
    if recs.unclosed_quote {
        warnings.push(IrWarning {
            code: "DOCS_PARSE_CSV_UNCLOSED_QUOTE".to_owned(),
            block: None,
        });
    }
    // One table: first to last filled row, all columns.
    let rows = detect_regions_all(&cells);
    if let Some((r0, r1)) = rows {
        let reg = Region {
            r0,
            r1,
            c0: 1,
            c1: max_cols.max(1) as u32,
        };
        let mut sink = Sink {
            blocks: &mut blocks,
            warnings: &mut warnings,
            kv_budget: kv_budget(1),
        };
        emit_table(&mut sink, None, &[], &cells, &reg);
    } else {
        warnings.push(IrWarning {
            code: "DOCS_PARSE_EMPTY".to_owned(),
            block: None,
        });
    }
    Ok(IrFragment {
        blob_b3: blob_b3(bytes),
        format: DocFormat::Csv,
        parser: parser_info(ParserId::OoCsv, PROFILE),
        meta: DocMeta {
            encoding: Some(decoded.encoding),
            ..DocMeta::default()
        },
        blocks,
        warnings,
    })
}

/// Parse a `.csv` file to canonical IR.
pub fn parse_csv(bytes: &[u8]) -> Result<crate::normalize::Normalized, ParseError> {
    Ok(crate::normalize::finish(parse_csv_fragment(bytes)?)?)
}
