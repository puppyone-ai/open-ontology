//! Canonical typed preimage for one table.
//!
//! The source side is the sealed spool. The target side is a typed catalog
//! read. Row count is recorded and is not sufficient.

use std::collections::BTreeMap;

use crate::case::{Cell, Row, Table};
use crate::sha::sha256_hex;

pub fn cell_ok(cell: &Cell) -> Result<(), &'static str> {
    match cell {
        Cell::Int8(_) => Ok(()),
        Cell::Decimal { scale, text } => {
            if decimal_ok(*scale, text) {
                Ok(())
            } else {
                Err("decimal")
            }
        }
        Cell::Null => Ok(()),
        Cell::Time(text) => {
            if time_ok(text) {
                Ok(())
            } else {
                Err("time")
            }
        }
        Cell::Binary(text) => {
            if text.len() % 2 == 0
                && text
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            {
                Ok(())
            } else {
                Err("binary")
            }
        }
        Cell::Text(_) => Ok(()),
        Cell::Bool(_) => Ok(()),
    }
}

pub fn canonical_cell(cell: &Cell) -> String {
    match cell {
        Cell::Int8(value) => format!("int8:{value}"),
        Cell::Decimal { scale, text } => format!("decimal:{scale}:{text}"),
        Cell::Null => "null:".to_string(),
        Cell::Time(text) => format!("time:{text}"),
        Cell::Binary(text) => format!("binary:{text}"),
        Cell::Text(text) => format!("text:{}", escape_text(text)),
        Cell::Bool(value) => {
            if *value {
                "bool:true".to_string()
            } else {
                "bool:false".to_string()
            }
        }
    }
}

fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

pub fn canonical_preimage(
    schema: &str,
    name: &str,
    keys: &[String],
    rows: &[Row],
) -> Result<String, &'static str> {
    let mut keyed = Vec::with_capacity(rows.len());
    for row in rows {
        keyed.push((row_key(row, keys)?, row));
    }
    keyed.sort_by(|left, right| left.0.cmp(&right.0));
    let mut out = String::new();
    out.push_str("local-copy-acceptance/v1 table\n");
    out.push_str("schema=");
    out.push_str(schema);
    out.push('\n');
    out.push_str("name=");
    out.push_str(name);
    out.push('\n');
    out.push_str("keys=");
    out.push_str(&keys.join(","));
    out.push('\n');
    out.push_str("count=");
    out.push_str(&keyed.len().to_string());
    out.push('\n');
    out.push_str("---\n");
    for (_, row) in keyed {
        let mut names: Vec<&str> = row.cells.keys().map(String::as_str).collect();
        names.sort_unstable();
        for (index, column) in names.iter().enumerate() {
            if index > 0 {
                out.push('\t');
            }
            out.push_str(column);
            out.push('=');
            out.push_str(&canonical_cell(&row.cells[*column]));
        }
        out.push('\n');
    }
    Ok(out)
}

fn row_key(row: &Row, keys: &[String]) -> Result<String, &'static str> {
    let mut parts = Vec::with_capacity(keys.len());
    for key in keys {
        let cell = row.cells.get(key).ok_or("key_coverage")?;
        if matches!(cell, Cell::Null) {
            return Err("null_key");
        }
        parts.push(canonical_cell(cell));
    }
    Ok(parts.join("\u{0000}"))
}

#[derive(Clone, Debug)]
pub struct TableReport {
    pub schema: String,
    pub name: String,
    pub source_count: u64,
    pub target_count: u64,
    pub source_hash: String,
    pub target_hash: String,
    pub hash_equal: bool,
    pub key_coverage: bool,
    pub reasons: Vec<&'static str>,
}

pub fn reconcile_table(table: &Table) -> TableReport {
    let mut reasons = Vec::new();
    if !name_ok(&table.schema)
        || !name_ok(&table.name)
        || table.key.is_empty()
        || table.key.iter().any(|key| !name_ok(key))
    {
        reasons.push("typed_name");
    }
    push_side(&mut reasons, &table.source_rows);
    push_side(&mut reasons, &table.target_rows);
    if table.source_rows.len() as u64 != table.count
        || table.target_rows.len() as u64 != table.count
    {
        reasons.push("count_mismatch");
    }
    let source = side_hash(&table.schema, &table.name, &table.key, &table.source_rows);
    let target = side_hash(&table.schema, &table.name, &table.key, &table.target_rows);
    match (&source, &target) {
        (Err(reason), _) | (_, Err(reason)) => {
            if !reasons.contains(reason) {
                reasons.push(*reason);
            }
        }
        (Ok(source_text), Ok(target_text)) => {
            if source_text != target_text {
                reasons.push("typed_mismatch");
            }
            if !keys_cover(&table.key, &table.source_rows, &table.target_rows) {
                reasons.push("key_coverage");
            }
        }
    }
    reasons.sort_unstable();
    reasons.dedup();
    let (source_hash, target_hash, hash_equal) = match (source, target) {
        (Ok(source_text), Ok(target_text)) => {
            let source_hash = sha256_hex(source_text.as_bytes());
            let target_hash = sha256_hex(target_text.as_bytes());
            let hash_equal = source_hash == target_hash;
            (source_hash, target_hash, hash_equal)
        }
        _ => (String::new(), String::new(), false),
    };
    TableReport {
        schema: table.schema.clone(),
        name: table.name.clone(),
        source_count: table.source_rows.len() as u64,
        target_count: table.target_rows.len() as u64,
        source_hash,
        target_hash,
        hash_equal,
        key_coverage: reasons.iter().all(|reason| {
            *reason != "key_coverage" && *reason != "duplicate_key" && *reason != "null_key"
        }),
        reasons,
    }
}

fn side_hash(
    schema: &str,
    name: &str,
    keys: &[String],
    rows: &[Row],
) -> Result<String, &'static str> {
    if let Some(reason) = side_reason(rows) {
        return Err(reason);
    }
    if duplicate_keys(keys, rows) {
        return Err("duplicate_key");
    }
    canonical_preimage(schema, name, keys, rows)
}

fn push_side(reasons: &mut Vec<&'static str>, rows: &[Row]) {
    if let Some(reason) = side_reason(rows) {
        if !reasons.contains(&reason) {
            reasons.push(reason);
        }
    }
    if rows
        .iter()
        .any(|row| row.cells.values().any(|cell| cell_ok(cell).is_err()))
        && !reasons.contains(&"typed_cell")
    {
        reasons.push("typed_cell");
    }
}

fn side_reason(rows: &[Row]) -> Option<&'static str> {
    let mut columns: Option<Vec<&str>> = None;
    for row in rows {
        if row.cells.is_empty() {
            return Some("typed_name");
        }
        for name in row.cells.keys() {
            if !name_ok(name) {
                return Some("typed_name");
            }
        }
        let mut names: Vec<&str> = row.cells.keys().map(String::as_str).collect();
        names.sort_unstable();
        match &columns {
            None => columns = Some(names),
            Some(expected) if expected == &names => {}
            Some(_) => return Some("typed_mismatch"),
        }
    }
    None
}

fn duplicate_keys(keys: &[String], rows: &[Row]) -> bool {
    let mut seen = BTreeMap::new();
    for row in rows {
        let Ok(key) = row_key(row, keys) else {
            return false;
        };
        if seen.insert(key, ()).is_some() {
            return true;
        }
    }
    false
}

fn keys_cover(keys: &[String], source: &[Row], target: &[Row]) -> bool {
    let source_keys = key_set(keys, source);
    let target_keys = key_set(keys, target);
    match (source_keys, target_keys) {
        (Some(source_keys), Some(target_keys)) => {
            source_keys == target_keys && source_keys.len() == source.len()
        }
        _ => false,
    }
}

fn key_set(keys: &[String], rows: &[Row]) -> Option<BTreeMap<String, ()>> {
    let mut seen = BTreeMap::new();
    for row in rows {
        let key = row_key(row, keys).ok()?;
        if seen.insert(key, ()).is_some() {
            return None;
        }
    }
    Some(seen)
}

pub fn name_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    (1..=128).contains(&bytes.len())
        && !bytes
            .iter()
            .any(|byte| matches!(byte, b'\\' | b'\t' | b'\n' | b'\r' | b'=' | b',' | 0))
        && !text.chars().any(|ch| (ch as u32) < 0x20)
}

fn decimal_ok(scale: u32, text: &str) -> bool {
    if scale > 38 || text.is_empty() {
        return false;
    }
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if rest.is_empty() {
        return false;
    }
    let (integer, fraction) = match rest.split_once('.') {
        Some((integer, fraction)) => {
            if scale == 0 {
                return false;
            }
            (integer, fraction)
        }
        None => {
            if scale != 0 {
                return false;
            }
            (rest, "")
        }
    };
    if fraction.len() != scale as usize || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    if !integer_ok(integer) {
        return false;
    }
    if negative && integer == "0" && (scale == 0 || fraction.bytes().all(|byte| byte == b'0')) {
        return false;
    }
    true
}

fn integer_ok(text: &str) -> bool {
    match text.as_bytes() {
        [b'0'] => true,
        [b'1'..=b'9', rest @ ..] => rest.iter().all(|byte| byte.is_ascii_digit()),
        _ => false,
    }
}

fn time_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    if clock_ok(bytes) {
        return true;
    }
    if bytes.len() < 19 {
        return false;
    }
    if !digits(&bytes[0..4])
        || bytes[4] != b'-'
        || !digits(&bytes[5..7])
        || bytes[7] != b'-'
        || !digits(&bytes[8..10])
    {
        return false;
    }
    if bytes[10] != b' ' && bytes[10] != b'T' {
        return false;
    }
    if !digits(&bytes[11..13])
        || bytes[13] != b':'
        || !digits(&bytes[14..16])
        || bytes[16] != b':'
        || !digits(&bytes[17..19])
    {
        return false;
    }
    let month = parse_2(&bytes[5..7]);
    let day = parse_2(&bytes[8..10]);
    let hour = parse_2(&bytes[11..13]);
    let minute = parse_2(&bytes[14..16]);
    let second = parse_2(&bytes[17..19]);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return false;
    }
    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while bytes.get(index).is_some_and(|byte| byte.is_ascii_digit()) {
            index += 1;
        }
        let width = index - start;
        if !(1..=6).contains(&width) {
            return false;
        }
    }
    match bytes.get(index) {
        None => true,
        Some(b'Z') => index + 1 == bytes.len(),
        Some(b'+') | Some(b'-') => {
            let sign = index;
            if bytes.len() != sign + 6 {
                return false;
            }
            digits(&bytes[sign + 1..sign + 3])
                && bytes[sign + 3] == b':'
                && digits(&bytes[sign + 4..sign + 6])
                && parse_2(&bytes[sign + 1..sign + 3]) <= 23
                && parse_2(&bytes[sign + 4..sign + 6]) <= 59
        }
        _ => false,
    }
}

fn clock_ok(bytes: &[u8]) -> bool {
    if bytes.len() < 8
        || !digits(&bytes[0..2])
        || bytes[2] != b':'
        || !digits(&bytes[3..5])
        || bytes[5] != b':'
        || !digits(&bytes[6..8])
    {
        return false;
    }
    if parse_2(&bytes[0..2]) > 23 || parse_2(&bytes[3..5]) > 59 || parse_2(&bytes[6..8]) > 60 {
        return false;
    }
    if bytes.len() == 8 {
        return true;
    }
    if bytes[8] != b'.' {
        return false;
    }
    let fraction = &bytes[9..];
    (1..=6).contains(&fraction.len()) && digits(fraction)
}

fn digits(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| byte.is_ascii_digit())
}

fn parse_2(bytes: &[u8]) -> u32 {
    u32::from(bytes[0] - b'0') * 10 + u32::from(bytes[1] - b'0')
}
