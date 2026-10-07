//! Public shop fixture. Row formulas match `testing/real-db/pg-e2e/CONTRACT.md`.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use crate::sha256::{self, hex};
use crate::{fail, Fail};

pub const PARTY_ROWS: i64 = 2_000;
pub const ACCOUNT_ROWS: i64 = 8_000;
pub const ENTRY_ROWS: i64 = 90_000;
pub const PARTY_COMMENT: &str = "甲注评名录";

pub fn party_key(index: i64) -> String {
    format!("p{index:05}")
}

pub fn format_amount(k: i64) -> Option<String> {
    if k == 3 {
        return Some("1234567890.1234".to_string());
    }
    if k == 4 {
        return Some("-1234567890.1234".to_string());
    }
    if k == 5 {
        return Some("0.0001".to_string());
    }
    if k == 6 {
        return Some("-0.0001".to_string());
    }
    if k % 29 == 0 {
        return None;
    }
    Some(format_minor((k % 10_000) - 5_000))
}

pub fn format_minor(minor: i64) -> String {
    let negative = minor < 0;
    let abs = minor.unsigned_abs();
    let whole = abs / 10_000;
    let frac = abs % 10_000;
    if negative {
        format!("-{whole}.{frac:04}")
    } else {
        format!("{whole}.{frac:04}")
    }
}

pub fn entry_qty(k: i64) -> i64 {
    if k == 0 {
        i64::MIN
    } else if k == 1 {
        i64::MAX
    } else if k == 2 {
        0
    } else {
        k - 45_000
    }
}

fn push_field(out: &mut String, value: Option<&str>) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\t');
    }
    match value {
        None => out.push_str("\\N"),
        Some(text) => out.push_str(text),
    }
}

fn finish(out: &mut String) {
    out.push('\n');
}

pub fn party_line(index: i64) -> String {
    let key = party_key(index);
    let display = if index % 17 == 0 {
        None
    } else {
        Some(format!("n{index:05}"))
    };
    let active = if index % 2 == 0 { "t" } else { "f" };
    let phone = format!("tel-{index:05}");
    let national_id = format!("nid-{index:05}");
    let mut out = String::new();
    push_field(&mut out, Some(&key));
    push_field(&mut out, display.as_deref());
    push_field(&mut out, Some(active));
    push_field(&mut out, Some(&phone));
    push_field(&mut out, Some(&national_id));
    finish(&mut out);
    out
}

pub fn account_line(index: i64) -> String {
    let id = index.to_string();
    let party = party_key(index % PARTY_ROWS);
    let opened = if index % 3 != 0 { "t" } else { "f" };
    let label = if index % 19 == 0 {
        None
    } else {
        Some(format!("a{index}"))
    };
    let mut out = String::new();
    push_field(&mut out, Some(&id));
    push_field(&mut out, Some(&party));
    push_field(&mut out, Some(opened));
    push_field(&mut out, label.as_deref());
    finish(&mut out);
    out
}

pub fn entry_line(index: i64) -> String {
    let id = index.to_string();
    let account = (index % ACCOUNT_ROWS).to_string();
    let qty = entry_qty(index).to_string();
    let amount = format_amount(index);
    let memo = if index % 13 == 0 {
        None
    } else {
        Some(format!("m{index}"))
    };
    let mut out = String::new();
    push_field(&mut out, Some(&id));
    push_field(&mut out, Some(&account));
    push_field(&mut out, Some(&qty));
    push_field(&mut out, amount.as_deref());
    push_field(&mut out, memo.as_deref());
    finish(&mut out);
    out
}

pub fn write_table(path: &Path, rows: i64, line: fn(i64) -> String) -> Result<String, Fail> {
    let mut file = File::create(path).map_err(|err| fail(1, format!("create {}: {err}", path.display())))?;
    let mut body = Vec::new();
    for index in 0..rows {
        let text = line(index);
        file.write_all(text.as_bytes())
            .map_err(|err| fail(1, format!("write {}: {err}", path.display())))?;
        body.extend_from_slice(text.as_bytes());
    }
    Ok(hex(&sha256::hash(&body)))
}

pub fn shop_bundle_sha256() -> String {
    let mut body = Vec::new();
    body.extend_from_slice(b"party\n");
    for index in 0..PARTY_ROWS {
        body.extend_from_slice(party_line(index).as_bytes());
    }
    body.extend_from_slice(b"account\n");
    for index in 0..ACCOUNT_ROWS {
        body.extend_from_slice(account_line(index).as_bytes());
    }
    body.extend_from_slice(b"entry\n");
    for index in 0..ENTRY_ROWS {
        body.extend_from_slice(entry_line(index).as_bytes());
    }
    hex(&sha256::hash(&body))
}

pub fn null_amount_count() -> i64 {
    (0..ENTRY_ROWS).filter(|index| format_amount(*index).is_none()).count() as i64
}

pub fn entry_probe(index: i64) -> String {
    let amount = format_amount(index).unwrap_or_else(|| "<NULL>".to_string());
    format!("{index}|{amount}|{}", entry_qty(index))
}
