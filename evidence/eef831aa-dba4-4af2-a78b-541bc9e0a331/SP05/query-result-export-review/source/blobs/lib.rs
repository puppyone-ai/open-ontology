#![forbid(unsafe_code)]

mod csv;
mod fault;
mod json;
mod limits;
mod parse;
mod path;
mod sha256;
mod sidecar;

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use csv::{escape_text, push_record_end, write_field};
use limits::{
    CSV_NAME, MAX_COLUMN_NAME_BYTES, MAX_COLUMNS, MAX_CSV_BYTES, MAX_REQUEST_BYTES,
    MAX_SIDECAR_BYTES, MAX_SOURCE_BYTES, SIDECAR_NAME, SOURCE_NAME,
};
use parse::{Cell, Parsed, parse_request_doc};
use sidecar::{ColumnStat, ObservedKind, SidecarParts};

pub use fault::Fault;
pub use sha256::sha256_hex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub csv: Vec<u8>,
    pub sidecar: Vec<u8>,
    pub complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completeness {
    Complete,
    Partial,
}

pub fn render(source: &[u8], columns: &[&str]) -> Result<Export, Fault> {
    check_columns(columns)?;
    if source.len() > MAX_SOURCE_BYTES {
        return Err(Fault::budget());
    }
    let parsed = parse::parse_result(source)?;
    if !parsed.counts_match() {
        return Err(Fault::source());
    }
    if let Some(width) = parsed.rows.first().map(Vec::len) {
        if width != columns.len() {
            return Err(Fault::request());
        }
        if parsed.rows.iter().any(|row| row.len() != width) {
            return Err(Fault::source());
        }
    }
    let csv = build_csv(columns, &parsed)?;
    let scanned = scan(columns, &parsed)?;
    let complete = parsed.clean_success();
    let sidecar = sidecar::sidecar_bytes(&SidecarParts {
        complete,
        source,
        csv: &csv,
        parsed: &parsed,
        columns: &scanned.stats,
        formula_cells: &scanned.formula_cells,
        null_cells: &scanned.null_cells,
        empty_text_cells: &scanned.empty_text_cells,
        int_cells: &scanned.int_cells,
    })
    .map_err(|()| Fault::source())?;
    if sidecar.len() > MAX_SIDECAR_BYTES {
        return Err(Fault::budget());
    }
    Ok(Export {
        csv,
        sidecar,
        complete,
    })
}

pub fn export_to(source: &[u8], columns: &[&str], output_dir: &Path) -> Result<Completeness, Fault> {
    let export = render(source, columns)?;
    let dir = path::require_dir(output_dir)?;
    let source_path = dir.join(SOURCE_NAME);
    let csv_path = dir.join(CSV_NAME);
    let side_path = dir.join(SIDECAR_NAME);
    path::require_absent(&source_path)?;
    path::require_absent(&csv_path)?;
    path::require_absent(&side_path)?;
    let mut created = Vec::new();
    if let Err(err) = write_created(&source_path, source, &mut created)
        .and_then(|()| write_created(&csv_path, &export.csv, &mut created))
        .and_then(|()| write_created(&side_path, &export.sidecar, &mut created))
    {
        for path in created.iter().rev() {
            let _ = fs::remove_file(path);
        }
        return Err(err);
    }
    if export.complete {
        Ok(Completeness::Complete)
    } else {
        Ok(Completeness::Partial)
    }
}

pub fn read_request_file(path: &Path) -> Result<Vec<u8>, Fault> {
    path::read_regular(path, MAX_REQUEST_BYTES, Fault::request())
}

pub fn execute_request(bytes: &[u8]) -> Result<Completeness, Fault> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(Fault::budget());
    }
    let doc = parse_request_doc(bytes)?;
    let source = path::read_regular(
        Path::new(&doc.source_file),
        MAX_SOURCE_BYTES,
        Fault::source(),
    )?;
    let columns: Vec<&str> = doc.columns.iter().map(String::as_str).collect();
    export_to(&source, &columns, Path::new(&doc.output_dir))
}

fn check_columns(columns: &[&str]) -> Result<(), Fault> {
    if columns.is_empty() {
        return Err(Fault::request());
    }
    if columns.len() > MAX_COLUMNS {
        return Err(Fault::budget());
    }
    for (index, name) in columns.iter().enumerate() {
        if name.is_empty() || name.len() > MAX_COLUMN_NAME_BYTES || name.bytes().any(|byte| byte == 0)
        {
            return Err(Fault::request());
        }
        if columns[..index].iter().any(|other| other == name) {
            return Err(Fault::request());
        }
    }
    Ok(())
}

fn build_csv(columns: &[&str], parsed: &Parsed) -> Result<Vec<u8>, Fault> {
    let mut csv = Vec::new();
    for (index, name) in columns.iter().enumerate() {
        if index > 0 {
            csv.push(b',');
        }
        let escaped = escape_text(name);
        write_field(&mut csv, &escaped);
    }
    push_record_end(&mut csv);
    for row in &parsed.rows {
        for (index, cell) in row.iter().enumerate() {
            if index > 0 {
                csv.push(b',');
            }
            match cell {
                Cell::Null => {}
                Cell::Bool(true) => write_field(&mut csv, "true"),
                Cell::Bool(false) => write_field(&mut csv, "false"),
                Cell::Int(text) => write_field(&mut csv, text),
                Cell::Text(text) => {
                    let escaped = escape_text(text);
                    write_field(&mut csv, &escaped);
                }
            }
        }
        push_record_end(&mut csv);
        if csv.len() > MAX_CSV_BYTES {
            return Err(Fault::budget());
        }
    }
    if csv.len() > MAX_CSV_BYTES {
        return Err(Fault::budget());
    }
    Ok(csv)
}

struct Scan {
    stats: Vec<ColumnStat>,
    formula_cells: Vec<[u32; 2]>,
    null_cells: Vec<[u32; 2]>,
    empty_text_cells: Vec<[u32; 2]>,
    int_cells: Vec<[u32; 2]>,
}

fn scan(columns: &[&str], parsed: &Parsed) -> Result<Scan, Fault> {
    let mut stats: Vec<ColumnStat> = columns
        .iter()
        .map(|name| ColumnStat {
            name: (*name).to_string(),
            kind: ObservedKind::None,
            nulls: 0,
            empty_text: 0,
        })
        .collect();
    let mut formula_cells = Vec::new();
    let mut null_cells = Vec::new();
    let mut empty_text_cells = Vec::new();
    let mut int_cells = Vec::new();
    for (row_index, row) in parsed.rows.iter().enumerate() {
        let row_u = u32::try_from(row_index).map_err(|_| Fault::budget())?;
        for (column_index, cell) in row.iter().enumerate() {
            let column_u = u32::try_from(column_index).map_err(|_| Fault::budget())?;
            let stat = &mut stats[column_index];
            match cell {
                Cell::Null => {
                    stat.nulls = stat.nulls.saturating_add(1);
                    if stat.kind == ObservedKind::None {
                        stat.kind = ObservedKind::Null;
                    }
                    null_cells.push([row_u, column_u]);
                }
                Cell::Bool(_) => set_kind(&mut stat.kind, ObservedKind::Bool)?,
                Cell::Int(_) => {
                    set_kind(&mut stat.kind, ObservedKind::Int)?;
                    int_cells.push([row_u, column_u]);
                }
                Cell::Text(text) => {
                    set_kind(&mut stat.kind, ObservedKind::Text)?;
                    if text.is_empty() {
                        stat.empty_text = stat.empty_text.saturating_add(1);
                        empty_text_cells.push([row_u, column_u]);
                    }
                    if csv::formula_prone(text) {
                        formula_cells.push([row_u, column_u]);
                    }
                }
            }
        }
    }
    Ok(Scan {
        stats,
        formula_cells,
        null_cells,
        empty_text_cells,
        int_cells,
    })
}

fn set_kind(slot: &mut ObservedKind, next: ObservedKind) -> Result<(), Fault> {
    match *slot {
        ObservedKind::None | ObservedKind::Null => *slot = next,
        current if current == next => {}
        _ => return Err(Fault::source()),
    }
    Ok(())
}

fn write_created(path: &Path, bytes: &[u8], created: &mut Vec<PathBuf>) -> Result<(), Fault> {
    write_new(path, bytes)?;
    created.push(path.to_path_buf());
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Fault> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| Fault::path())?;
    if let Err(err) = write_synced(&mut file, bytes) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(err);
    }
    Ok(())
}

fn write_synced(file: &mut File, bytes: &[u8]) -> Result<(), Fault> {
    file.write_all(bytes).map_err(|_| Fault::io())?;
    file.sync_all().map_err(|_| Fault::io())?;
    Ok(())
}
