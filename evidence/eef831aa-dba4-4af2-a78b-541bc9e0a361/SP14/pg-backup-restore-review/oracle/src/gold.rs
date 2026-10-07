//! Shop COPY text from testing/real-db/pg-e2e/CONTRACT.md.
//! Row text is tab-separated, null is `\N`, boolean is `t` or `f`.

use std::fs::File;
use std::io::Write;
use std::path::Path;

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
    if !out.is_empty() {
        out.push('\t');
    }
    match value {
        None => out.push_str("\\N"),
        Some(text) => out.push_str(text),
    }
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
    out.push('\n');
    out
}

pub fn account_line(index: i64) -> String {
    let id = index.to_string();
    let party = party_key(index % PARTY_ROWS);
    let opened = if index % 3 == 0 { "f" } else { "t" };
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
    out.push('\n');
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
    out.push('\n');
    out
}

pub fn null_amount_count() -> i64 {
    (0..ENTRY_ROWS).filter(|index| format_amount(*index).is_none()).count() as i64
}

pub fn amount_probe() -> String {
    let mut lines = Vec::new();
    for index in 0..=6 {
        let amount = format_amount(index).unwrap_or_else(|| "<NULL>".to_string());
        lines.push(format!("{index}|{amount}|{}", entry_qty(index)));
    }
    lines.join("\n")
}

pub fn compare_generated(path: &std::path::Path, rows: i64, line: fn(i64) -> String) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    let mut offset = 0usize;
    for index in 0..rows {
        let expect = line(index);
        let bytes = expect.as_bytes();
        let end = offset + bytes.len();
        if data.len() < end || &data[offset..end] != bytes {
            return Err(format!("{} mismatch at row {index}", path.display()));
        }
        offset = end;
    }
    if offset != data.len() {
        return Err(format!("{} has {} trailing bytes", path.display(), data.len() - offset));
    }
    Ok(())
}

pub fn write_rows(path: &Path, rows: i64, line: fn(i64) -> String) -> Result<(), String> {
    let mut file = File::create(path).map_err(|err| format!("create {}: {err}", path.display()))?;
    for index in 0..rows {
        file.write_all(line(index).as_bytes())
            .map_err(|err| format!("write {}: {err}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        account_line, amount_probe, entry_line, entry_qty, format_amount, format_minor, null_amount_count, party_line,
        PARTY_COMMENT,
    };

    #[test]
    fn hand_literals_match_the_public_contract() {
        assert_eq!(PARTY_COMMENT, "甲注评名录");
        assert!(PARTY_COMMENT.contains("甲注评"));
        assert_eq!(party_line(0), "p00000\t\\N\tt\ttel-00000\tnid-00000\n");
        assert_eq!(party_line(1), "p00001\tn00001\tf\ttel-00001\tnid-00001\n");
        assert_eq!(party_line(17), "p00017\t\\N\tf\ttel-00017\tnid-00017\n");
        assert_eq!(account_line(0), "0\tp00000\tf\t\\N\n");
        assert_eq!(account_line(1), "1\tp00001\tt\ta1\n");
        assert_eq!(account_line(19), "19\tp00019\tt\t\\N\n");
        assert_eq!(entry_line(0), "0\t0\t-9223372036854775808\t\\N\t\\N\n");
        assert_eq!(entry_line(1), "1\t1\t9223372036854775807\t-0.4999\tm1\n");
        assert_eq!(entry_line(2), "2\t2\t0\t-0.4998\tm2\n");
        assert_eq!(entry_line(3), "3\t3\t-44997\t1234567890.1234\tm3\n");
        assert_eq!(entry_line(4), "4\t4\t-44996\t-1234567890.1234\tm4\n");
        assert_eq!(entry_line(5), "5\t5\t-44995\t0.0001\tm5\n");
        assert_eq!(entry_line(6), "6\t6\t-44994\t-0.0001\tm6\n");
        assert_eq!(entry_line(13), "13\t13\t-44987\t-0.4987\t\\N\n");
        assert_eq!(entry_line(29), "29\t29\t-44971\t\\N\tm29\n");
        assert_eq!(entry_line(5000), "5000\t5000\t-40000\t0.0000\tm5000\n");
        assert_eq!(entry_qty(0), i64::MIN);
        assert_eq!(entry_qty(1), i64::MAX);
        assert_eq!(format_amount(29), None);
        assert_eq!(format_minor(-5000), "-0.5000");
        assert_eq!(format_minor(0), "0.0000");
        let mut nulls = 0i64;
        for index in 0..90_000 {
            if index % 29 == 0 {
                nulls += 1;
            }
        }
        assert_eq!(nulls, 3104);
        assert_eq!(null_amount_count(), 3104);
        assert!(amount_probe().starts_with("0|<NULL>|-9223372036854775808\n"));
        assert!(amount_probe().contains("\n3|1234567890.1234|-44997\n"));
    }
}
