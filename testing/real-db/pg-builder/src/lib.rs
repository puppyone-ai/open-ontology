//! Public synthetic rows. Counts and formulas match the a317 shop fixture.
//! Indexes use remainder checks so the emitter stays on Rust 1.94.

#![forbid(unsafe_code)]

use std::fs;
use std::io::Write;
use std::path::Path;

pub const PARTY_ROWS: u32 = 2_000;
pub const ACCOUNT_ROWS: u32 = 8_000;
pub const ENTRY_ROWS: u32 = 90_000;
pub const MARKER: &str = "甲注评";
pub const TABLE_COMMENT: &str = "甲注评名录";

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

pub fn schema_sql(reader: &str) -> Result<String, &'static str> {
    if !ident_ok(reader) {
        return Err("reader ident");
    }
    Ok(format!(
        "CREATE SCHEMA shop;\n\
CREATE TABLE shop.party (\n\
  party_key text PRIMARY KEY,\n\
  display_name text,\n\
  active boolean NOT NULL,\n\
  phone text NOT NULL,\n\
  national_id text NOT NULL\n\
);\n\
CREATE TABLE shop.account (\n\
  account_id int8 PRIMARY KEY,\n\
  party_key text NOT NULL,\n\
  opened boolean NOT NULL,\n\
  label text,\n\
  CONSTRAINT account_party_fk FOREIGN KEY (party_key) REFERENCES shop.party (party_key)\n\
);\n\
CREATE TABLE shop.entry (\n\
  entry_id int8 PRIMARY KEY,\n\
  account_id int8 NOT NULL,\n\
  qty int8 NOT NULL,\n\
  amount numeric(18,4),\n\
  memo text,\n\
  CONSTRAINT entry_account_fk FOREIGN KEY (account_id) REFERENCES shop.account (account_id)\n\
);\n\
COMMENT ON TABLE shop.party IS '{TABLE_COMMENT}';\n\
REVOKE ALL ON SCHEMA shop FROM PUBLIC;\n\
REVOKE ALL ON ALL TABLES IN SCHEMA shop FROM PUBLIC;\n\
GRANT USAGE ON SCHEMA shop TO {reader};\n\
GRANT SELECT ON shop.party, shop.account, shop.entry TO {reader};\n"
    ))
}

pub fn write_fixture(dir: &Path, reader: &str) -> Result<(), &'static str> {
    fs::create_dir_all(dir).map_err(|_| "create")?;
    fs::write(dir.join("schema.sql"), schema_sql(reader)?).map_err(|_| "schema")?;
    write_lines(&dir.join("party.copy"), PARTY_ROWS, party_line)?;
    write_lines(&dir.join("account.copy"), ACCOUNT_ROWS, account_line)?;
    write_lines(&dir.join("entry.copy"), ENTRY_ROWS, entry_line)?;
    let counts = format!("party={PARTY_ROWS}\naccount={ACCOUNT_ROWS}\nentry={ENTRY_ROWS}\n");
    fs::write(dir.join("counts.txt"), counts).map_err(|_| "counts")?;
    Ok(())
}

fn write_lines(path: &Path, rows: u32, line: fn(u32) -> String) -> Result<(), &'static str> {
    let mut file = fs::File::create(path).map_err(|_| "create copy")?;
    for index in 0..rows {
        file.write_all(line(index).as_bytes()).map_err(|_| "write copy")?;
    }
    file.sync_all().map_err(|_| "sync copy")?;
    Ok(())
}

fn ident_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=63).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gold_edges_match_the_public_fixture() {
        assert_eq!(party_line(0), "p00000\t\\N\tt\ttel-00000\tnid-00000\n");
        assert_eq!(party_line(1), "p00001\tn00001\tf\ttel-00001\tnid-00001\n");
        assert_eq!(account_line(0), "0\tp00000\tf\t\\N\n");
        assert_eq!(account_line(1), "1\tp00001\tt\ta1\n");
        assert_eq!(
            entry_line(0),
            "0\t0\t-9223372036854775808\t\\N\t\\N\n"
        );
        assert!(entry_line(1).contains("9223372036854775807"));
        assert_eq!(entry_line(3), "3\t3\t-44997\t1234567890.1234\tm3\n");
        assert!(entry_line(4).contains("-1234567890.1234"));
        assert_eq!(PARTY_ROWS + ACCOUNT_ROWS + ENTRY_ROWS, 100_000);
    }

    #[test]
    fn schema_grants_only_the_private_reader() {
        let sql = schema_sql("oo_a328_reader").expect("schema");
        assert!(sql.contains("COMMENT ON TABLE shop.party IS '甲注评名录'"));
        assert!(sql.contains(MARKER));
        assert!(sql.contains("GRANT SELECT ON shop.party, shop.account, shop.entry TO oo_a328_reader;"));
        assert!(sql.contains("account_party_fk"));
        assert!(sql.contains("entry_account_fk"));
        assert!(!sql.contains("oo_a317"));
        assert!(schema_sql("OO_READER").is_err());
        assert!(schema_sql("reader-role").is_err());
    }
}
