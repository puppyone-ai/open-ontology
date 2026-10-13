use crate::fail::Fail;

/// Exact base-10 decimal. Extra fractional digits are accepted only when they are zeros.
pub fn parse_decimal(text: &str, precision: u8, scale: u8) -> Result<i128, Fail> {
    if precision == 0 || precision > 38 || u16::from(scale) > u16::from(precision) {
        return Err(Fail::new(
            "unsupported_precision",
            format!("precision={precision} scale={scale}"),
        ));
    }
    let text = text.trim();
    if text.is_empty() || text.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return Err(Fail::new("decimal_text", "blank"));
    }
    let (negative, rest) = match text.as_bytes()[0] {
        b'+' => return Err(Fail::new("decimal_text", "sign")),
        b'-' => (true, &text[1..]),
        _ => (false, text),
    };
    if rest.is_empty() || rest.bytes().filter(|byte| *byte == b'.').count() > 1 {
        return Err(Fail::new("decimal_text", "shape"));
    }
    let (whole, frac) = match rest.split_once('.') {
        Some((whole, frac)) => {
            if frac.is_empty() {
                return Err(Fail::new("decimal_text", "trailing-dot"));
            }
            (whole, frac)
        }
        None => (rest, ""),
    };
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !frac.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(Fail::new("decimal_text", "digits"));
    }
    if frac.len() > usize::from(scale) {
        let extra = &frac.as_bytes()[usize::from(scale)..];
        if extra.iter().any(|byte| *byte != b'0') {
            return Err(Fail::new(
                "decimal_scale",
                format!("frac={} scale={scale}", frac.len()),
            ));
        }
    }
    let frac = if frac.len() > usize::from(scale) {
        &frac[..usize::from(scale)]
    } else {
        frac
    };
    let mut digits = String::new();
    digits.push_str(whole);
    digits.push_str(frac);
    for _ in frac.len()..usize::from(scale) {
        digits.push('0');
    }
    let width = digits.trim_start_matches('0').len();
    let width = if width == 0 { 1 } else { width };
    if width > usize::from(precision) {
        return Err(Fail::new(
            "decimal_precision",
            format!("digits={width} precision={precision}"),
        ));
    }
    let mut value: i128 = 0;
    for digit in digits.bytes() {
        value = value
            .checked_mul(10)
            .and_then(|next| next.checked_add(i128::from(digit - b'0')))
            .ok_or_else(|| Fail::new("decimal_precision", "i128"))?;
    }
    if negative && value != 0 {
        value = value
            .checked_neg()
            .ok_or_else(|| Fail::new("decimal_precision", "neg"))?;
    }
    Ok(value)
}

pub fn format_unscaled(value: i128, scale: u8) -> Result<String, Fail> {
    let negative = value < 0;
    let abs = value
        .checked_abs()
        .ok_or_else(|| Fail::new("decimal_text", "abs"))?;
    let mut digits = abs.to_string();
    let text = if scale == 0 {
        digits
    } else {
        let scale = usize::from(scale);
        if digits.len() <= scale {
            let mut padded = String::new();
            for _ in digits.len()..=scale {
                padded.push('0');
            }
            padded.push_str(&digits);
            digits = padded;
        }
        let split = digits.len() - scale;
        format!("{}.{}", &digits[..split], &digits[split..])
    };
    if negative {
        Ok(format!("-{text}"))
    } else {
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::{format_unscaled, parse_decimal};

    #[test]
    fn exact_scale_and_sum_parts() {
        assert_eq!(parse_decimal("0.3000", 18, 4).unwrap(), 3000);
        assert_eq!(parse_decimal("0.4000", 18, 4).unwrap(), 4000);
        assert_eq!(parse_decimal("0.7000", 18, 4).unwrap(), 7000);
        assert_eq!(parse_decimal("-1.2500", 18, 4).unwrap(), -12500);
        assert_eq!(format_unscaled(7000, 4).unwrap(), "0.7000");
        assert_eq!(format_unscaled(-12500, 4).unwrap(), "-1.2500");
        assert_eq!(parse_decimal("9.75", 10, 2).unwrap(), 975);
        assert_eq!(parse_decimal("0.700000", 18, 4).unwrap(), 7000);
    }

    #[test]
    fn decimal38_fits_and_narrow_cast_does_not() {
        let wide = "12345678901234567890.123456789012345678";
        let parsed = parse_decimal(wide, 38, 18).unwrap();
        assert_eq!(format_unscaled(parsed, 18).unwrap(), wide);
        assert!(parse_decimal("123456789.01", 10, 2).is_err());
        assert_eq!(parse_decimal("99999999.99", 10, 2).unwrap(), 9_999_999_999);
    }

    #[test]
    fn rejects_float_text() {
        assert!(parse_decimal("1.5e1", 10, 2).is_err());
        assert!(parse_decimal("1.25", 10, 1).is_err());
    }
}
