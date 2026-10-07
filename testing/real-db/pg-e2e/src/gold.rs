//! Hand formulas for the public fixture. Indexes match `CONTRACT.md`.

#![forbid(unsafe_code)]

use std::io::Write;

pub const PARTY_ROWS: u32 = 2_000;
pub const ACCOUNT_ROWS: u32 = 8_000;
pub const ENTRY_ROWS: u32 = 90_000;
pub const SOURCE_GENERATION: i64 = 317;
pub const MARKER: &str = "甲注评";

pub const SOURCE_ID: &str = "pubfix";
pub const HOST: &str = "oo-a317-pg";
pub const PORT: u16 = 5432;
pub const SOURCE_DB: &str = "oo_a317_src";
pub const TARGET_DB: &str = "oo_a317_dst";
pub const READER: &str = "oo_a317_reader";
pub const RUNTIME: &str = "oo_a317_runtime";
pub const LOGIN: &str = "oo_a317_login";
pub const ADMIN: &str = "oo_fixture_admin";
pub const SECRET_REF: &str = "secret://pg/a317/reader";
pub const SHARD: &str = "a317main";
pub const OWNER: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a317";

pub fn party_key(index: u32) -> String {
    format!("p{index:05}")
}

pub fn party_display(index: u32) -> Option<String> {
    if index.is_multiple_of(17) {
        None
    } else {
        Some(format!("n{index:05}"))
    }
}

pub fn party_active(index: u32) -> bool {
    index.is_multiple_of(2)
}

pub fn party_phone(index: u32) -> String {
    format!("tel-{index:05}")
}

pub fn party_national_id(index: u32) -> String {
    format!("nid-{index:05}")
}

pub fn account_party(index: u32) -> String {
    party_key(index % PARTY_ROWS)
}

pub fn account_opened(index: u32) -> bool {
    !index.is_multiple_of(3)
}

pub fn account_label(index: u32) -> Option<String> {
    if index.is_multiple_of(19) {
        None
    } else {
        Some(format!("a{index}"))
    }
}

pub fn entry_account(index: u32) -> i64 {
    i64::from(index % ACCOUNT_ROWS)
}

pub fn entry_qty(index: u32) -> i64 {
    match index {
        0 => i64::MIN,
        1 => i64::MAX,
        2 => 0,
        _ => i64::from(index) - 45_000,
    }
}

/// Minor units of `0.0001`. `None` is SQL NULL.
pub fn entry_amount_minor(index: u32) -> Option<i64> {
    match index {
        3 => Some(12_345_678_901_234),
        4 => Some(-12_345_678_901_234),
        5 => Some(1),
        6 => Some(-1),
        _ if index.is_multiple_of(29) => None,
        _ => Some(i64::from(index % 10_000) - 5_000),
    }
}

pub fn format_minor(minor: i64) -> String {
    let negative = minor < 0;
    let absolute = minor.unsigned_abs();
    let whole = absolute / 10_000;
    let frac = absolute % 10_000;
    if negative {
        format!("-{whole}.{frac:04}")
    } else {
        format!("{whole}.{frac:04}")
    }
}

pub fn entry_memo(index: u32) -> Option<String> {
    if index.is_multiple_of(13) {
        None
    } else {
        Some(format!("m{index}"))
    }
}

pub fn config_preimage() -> String {
    "oo-pg-config-identity/v1\n\
source_id=pubfix\n\
host=oo-a317-pg\n\
port=5432\n\
database=oo_a317_src\n\
username=oo_a317_reader\n\
secret_ref=secret://pg/a317/reader\n\
connect_ms=30000\n\
statement_ms=60000\n\
lock_ms=30000\n\
max_rows=10000\n\
max_bytes=8388608\n\
table\tshop\tparty\n\
table\tshop\taccount\n\
table\tshop\tentry\n"
        .to_owned()
}

pub fn config_identity() -> String {
    crate::sha::sha256_hex(config_preimage().as_bytes())
}

fn copy_field(out: &mut String, value: Option<&str>) {
    match value {
        None => out.push_str("\\N"),
        Some(text) => out.push_str(text),
    }
}

fn push_bool(out: &mut String, value: bool) {
    out.push(if value { 't' } else { 'f' });
}

pub fn party_line(index: u32) -> String {
    let key = party_key(index);
    let display = party_display(index);
    let phone = party_phone(index);
    let national = party_national_id(index);
    let mut line = String::new();
    copy_field(&mut line, Some(&key));
    line.push('\t');
    copy_field(&mut line, display.as_deref());
    line.push('\t');
    push_bool(&mut line, party_active(index));
    line.push('\t');
    copy_field(&mut line, Some(&phone));
    line.push('\t');
    copy_field(&mut line, Some(&national));
    line.push('\n');
    line
}

pub fn account_line(index: u32) -> String {
    let key = account_party(index);
    let label = account_label(index);
    let mut line = String::new();
    line.push_str(&index.to_string());
    line.push('\t');
    copy_field(&mut line, Some(&key));
    line.push('\t');
    push_bool(&mut line, account_opened(index));
    line.push('\t');
    copy_field(&mut line, label.as_deref());
    line.push('\n');
    line
}

pub fn entry_line(index: u32) -> String {
    let memo = entry_memo(index);
    let amount = entry_amount_minor(index).map(format_minor);
    let mut line = String::new();
    line.push_str(&index.to_string());
    line.push('\t');
    line.push_str(&entry_account(index).to_string());
    line.push('\t');
    line.push_str(&entry_qty(index).to_string());
    line.push('\t');
    copy_field(&mut line, amount.as_deref());
    line.push('\t');
    copy_field(&mut line, memo.as_deref());
    line.push('\n');
    line
}

pub fn write_party<W: Write>(mut out: W) -> Result<(), crate::Fail> {
    for index in 0..PARTY_ROWS {
        out.write_all(party_line(index).as_bytes())
            .map_err(|_| crate::fail("write party"))?;
    }
    Ok(())
}

pub fn write_account<W: Write>(mut out: W) -> Result<(), crate::Fail> {
    for index in 0..ACCOUNT_ROWS {
        out.write_all(account_line(index).as_bytes())
            .map_err(|_| crate::fail("write account"))?;
    }
    Ok(())
}

pub fn write_entry<W: Write>(mut out: W) -> Result<(), crate::Fail> {
    for index in 0..ENTRY_ROWS {
        out.write_all(entry_line(index).as_bytes())
            .map_err(|_| crate::fail("write entry"))?;
    }
    Ok(())
}

pub fn line_at(table: &str, index: u32) -> Result<String, crate::Fail> {
    match table {
        "party" if index < PARTY_ROWS => Ok(party_line(index)),
        "account" if index < ACCOUNT_ROWS => Ok(account_line(index)),
        "entry" if index < ENTRY_ROWS => Ok(entry_line(index)),
        _ => Err(crate::fail("copy table")),
    }
}

pub fn row_count(table: &str) -> Result<u32, crate::Fail> {
    match table {
        "party" => Ok(PARTY_ROWS),
        "account" => Ok(ACCOUNT_ROWS),
        "entry" => Ok(ENTRY_ROWS),
        _ => Err(crate::fail("copy table")),
    }
}
