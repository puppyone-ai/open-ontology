use oo_core::ExactDecimal;
use oo_core::canonical::{parse_json, validate_date};

use crate::error::{Result, err};

const MONTH_LEN: [u32; 13] = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

pub fn ymd(year: i32, month: u8, day: u8) -> Result<String> {
    validate_date(year, month, day)
        .map_err(|_| err(format!("invalid civil date {year}-{month}-{day}")))?;
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

pub fn add_days(year: i32, month: u8, day: u8, extra: u32) -> Result<String> {
    if year != 2026 {
        return Err(err("add_days only walks 2026"));
    }
    let mut cursor_month = u32::from(month);
    let mut cursor_day = u32::from(day) + extra;
    while cursor_month <= 12 && cursor_day > MONTH_LEN[cursor_month as usize] {
        cursor_day -= MONTH_LEN[cursor_month as usize];
        cursor_month += 1;
    }
    if cursor_month > 12 {
        return Err(err("date walked out of 2026"));
    }
    ymd(
        year,
        u8::try_from(cursor_month).unwrap_or(0),
        u8::try_from(cursor_day).unwrap_or(0),
    )
}

pub fn date_2026(index: u64) -> Result<String> {
    add_days(2026, 1, 1, u32::try_from(index % 240).unwrap_or(0))
}

pub fn fen(cents: i64) -> String {
    cents.to_string()
}

/// Integer fen when `cents` is on. Exact scale-2 yuan when it is off.
/// The coefficient at scale 2 is the same minor-unit amount.
pub fn money(cents: i64, integer_fen: bool) -> Result<String> {
    if integer_fen {
        return Ok(fen(cents));
    }
    let negative = cents < 0;
    let digits = cents.unsigned_abs().to_string();
    let lexical = if negative {
        format!("-{digits}")
    } else {
        digits
    };
    scale_num(&lexical, 2)
}

pub fn parse_money(text: &str, integer_fen: bool) -> Result<i64> {
    if integer_fen {
        if text.is_empty() || text.bytes().any(|byte| matches!(byte, b'.' | b'e' | b'E')) {
            return Err(err("amount is not integer fen"));
        }
        return text
            .parse::<i64>()
            .map_err(|_| err("amount is not integer fen"));
    }
    let decimal = ExactDecimal::parse_sql(text).map_err(|_| {
        err("amount is not exact fixed-point yuan while cents is off")
    })?;
    if decimal.scale() != 2 || decimal.to_plain_string() != text {
        return Err(err(
            "amount is not exact fixed-point yuan while cents is off",
        ));
    }
    let mut digits = decimal.coefficient().to_string();
    if decimal.is_negative() {
        digits.insert(0, '-');
    }
    digits.parse::<i64>().map_err(|_| {
        err("amount is not exact fixed-point yuan while cents is off")
    })
}

pub fn scale_num(digits: &str, scale: i32) -> Result<String> {
    let negative = digits.starts_with('-');
    let body = digits.trim_start_matches('-');
    Ok(ExactDecimal::new(negative, body, scale)
        .map_err(|error| err(error.to_string()))?
        .to_plain_string())
}

pub fn high_prec(lexical: &str) -> Result<String> {
    Ok(ExactDecimal::parse_sql(lexical)
        .map_err(|error| err(error.to_string()))?
        .to_plain_string())
}

pub fn checked_json(text: &str) -> Result<String> {
    parse_json(text).map_err(|_| err(format!("ext_json rejected by canonical parser: {text}")))?;
    if text.contains('\r') {
        return Err(err("json text contains CR"));
    }
    Ok(text.to_string())
}

pub fn obj_json(reference: &str, qty: &str) -> Result<String> {
    let mut ref_text = String::new();
    for ch in reference.chars() {
        match ch {
            '"' | '\\' => return Err(err("reference is not a plain token")),
            _ => ref_text.push(ch),
        }
    }
    checked_json(&format!("{{\"qty\":{qty},\"ref\":\"{ref_text}\"}}"))
}

#[cfg(test)]
mod tests {
    use super::{high_prec, money, obj_json, parse_money, scale_num, ymd};

    #[test]
    fn lexical_scale_survives_and_json_parses() {
        assert_eq!(scale_num("17550", 2).unwrap(), "175.50");
        assert_eq!(money(17_550, true).unwrap(), "17550");
        assert_eq!(money(17_550, false).unwrap(), "175.50");
        assert_eq!(money(50, false).unwrap(), "0.50");
        assert_eq!(parse_money("17550", true).unwrap(), 17_550);
        assert_eq!(parse_money("175.50", false).unwrap(), 17_550);
        assert!(parse_money("175.50", true).is_err());
        assert!(parse_money("17550", false).is_err());
        assert!(parse_money("175.5", false).is_err());
        assert_eq!(
            high_prec("1234567890123456789.123456789012").unwrap(),
            "1234567890123456789.123456789012"
        );
        let json = obj_json("REF-HX-001", "10.50").unwrap();
        assert!(json.contains("10.50"));
        assert!(ymd(2026, 2, 30).is_err());
        assert!(ymd(0, 1, 1).is_err());
    }
}
