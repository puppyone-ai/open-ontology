//! Lossless finite-f64 text for the C5 digest, including the sign of zero.
//!
//! `digits e exponent` denotes an integer coefficient times a power of ten.
//! FLOAT4 callers must widen the original f32 exactly, not format it as f32.

const MAX_TEXT_BYTES: usize = 64;

pub fn canonical(value: f64) -> Result<String, &'static str> {
    if !value.is_finite() {
        return Err("c5_float_nonfinite");
    }
    let text = serde_json::to_string(&value).map_err(|_| "c5_float_serialize")?;
    let result = normalize_json_number(&text)?;
    // Equality of the output must imply equality of the original IEEE bits.
    if result
        .parse::<f64>()
        .map_err(|_| "c5_float_roundtrip")?
        .to_bits()
        != value.to_bits()
    {
        return Err("c5_float_roundtrip");
    }
    Ok(result)
}

fn normalize_json_number(text: &str) -> Result<String, &'static str> {
    if text.is_empty() || text.len() > MAX_TEXT_BYTES || !text.is_ascii() {
        return Err("c5_float_lexical");
    }
    let bytes = text.as_bytes();
    let negative = bytes[0] == b'-';
    let mut at = usize::from(negative);
    let integer_start = at;
    match bytes.get(at) {
        Some(b'0') => at += 1,
        Some(b'1'..=b'9') => {
            while bytes.get(at).is_some_and(u8::is_ascii_digit) {
                at += 1;
            }
        }
        _ => return Err("c5_float_lexical"),
    }
    let integer_end = at;
    let mut fraction_start = at;
    let mut fraction_end = at;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        fraction_start = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        fraction_end = at;
        if fraction_start == fraction_end {
            return Err("c5_float_lexical");
        }
    }
    let mut exponent = 0_i32;
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        at += 1;
        let exponent_negative = bytes.get(at) == Some(&b'-');
        if matches!(bytes.get(at), Some(b'-' | b'+')) {
            at += 1;
        }
        let exponent_start = at;
        while let Some(digit) = bytes.get(at).filter(|digit| digit.is_ascii_digit()) {
            exponent = exponent
                .checked_mul(10)
                .and_then(|n| n.checked_add(i32::from(*digit - b'0')))
                .filter(|n| *n <= 400)
                .ok_or("c5_float_exponent")?;
            at += 1;
        }
        if at == exponent_start {
            return Err("c5_float_lexical");
        }
        if exponent_negative {
            exponent = -exponent;
        }
    }
    if at != bytes.len() {
        return Err("c5_float_lexical");
    }
    let value = text.parse::<f64>().map_err(|_| "c5_float_lexical")?;
    if !value.is_finite() {
        return Err("c5_float_nonfinite");
    }
    let digits = format!(
        "{}{}",
        &text[integer_start..integer_end],
        &text[fraction_start..fraction_end]
    );
    let sign = if negative { "-" } else { "" };
    let Some(first) = digits.bytes().position(|byte| byte != b'0') else {
        return Ok(format!("{sign}0e0"));
    };
    if value == 0.0 {
        return Err("c5_float_underflow");
    }
    let end = digits.trim_end_matches('0').len();
    let significant = &digits[first..end];
    if significant.len() > 17 {
        return Err("c5_float_precision");
    }
    exponent -= i32::try_from(fraction_end - fraction_start).map_err(|_| "c5_float_lexical")?;
    exponent += i32::try_from(digits.len() - end).map_err(|_| "c5_float_lexical")?;
    Ok(format!("{sign}{significant}e{exponent}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_float_successor_preserves_every_archive_scope_guard() {
        let before = include_str!("../migrations/0065_native_c5_multipart_initialize.sql");
        let start = before.find("CREATE FUNCTION _oo.c5_seed_multipart_digest(").unwrap();
        let end = before[start..].find("\nCREATE FUNCTION _oo.c5_digest_receipt_current").unwrap() + start;
        let expected = before[start..end]
            .replacen("CREATE FUNCTION", "CREATE OR REPLACE FUNCTION", 1)
            .replace(
                "NOT IN (16,17,20,21,23,25,1042,1043,1700,1082,1114,1184)",
                "NOT IN (16,17,20,21,23,25,701,1042,1043,1700,1082,1114,1184)",
            );
        let after = include_str!("../migrations/0072_native_c5_float_archive.sql");
        assert_eq!(
            after[after.find("CREATE OR REPLACE FUNCTION").unwrap()..].trim(),
            expected.trim()
        );
    }

    #[test]
    fn lexical_normalization_is_exact_not_decimal_rounding() {
        for (input, expected) in [
            ("0", "0e0"),
            ("-0.0", "-0e0"),
            ("1.2300", "123e-2"),
            ("0.00001230", "123e-7"),
            ("-1200.00E+03", "-12e5"),
            ("100000000000000000000.0", "1e20"),
            ("1e+20", "1e20"),
            ("5e-324", "5e-324"),
            ("2.2250738585072014e-308", "22250738585072014e-324"),
        ] {
            assert_eq!(normalize_json_number(input), Ok(expected.into()));
        }
    }

    #[test]
    fn refuses_invalid_nonfinite_underflow_and_oversize_text() {
        for input in [
            "",
            "-",
            "+1",
            "01",
            "-01",
            ".1",
            "1.",
            "1e",
            "1e+",
            "1e-",
            " 1",
            "1 ",
            "1\n",
            "NaN",
            "Infinity",
            "-Infinity",
            "null",
            "0x1",
            "1_0",
            "1,0",
            "1e309",
            "1e-400",
            "1e999999999",
            "1.234567890123456789",
        ] {
            assert!(normalize_json_number(input).is_err(), "{input:?}");
        }
        assert!(normalize_json_number(&"0".repeat(65)).is_err());
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(canonical(value), Err("c5_float_nonfinite"));
        }
    }
}
