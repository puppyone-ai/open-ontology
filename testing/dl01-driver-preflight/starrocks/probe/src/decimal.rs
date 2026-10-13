use crate::fail::Fail;

pub fn parse_unscaled(text: &str, scale: u8) -> Result<i128, Fail> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.contains(['e', 'E', '+', ' ']) {
        return Err(Fail::new("decimal", "not a base-10 literal"));
    }
    let negative = trimmed.starts_with('-');
    let body = if negative { &trimmed[1..] } else { trimmed };
    if body.is_empty() || body.starts_with('-') {
        return Err(Fail::new("decimal", "sign"));
    }
    let (whole, frac) = match body.split_once('.') {
        Some((whole, frac)) => (whole, frac),
        None => (body, ""),
    };
    if whole.is_empty()
        || !whole.chars().all(|item| item.is_ascii_digit())
        || !frac.chars().all(|item| item.is_ascii_digit())
    {
        return Err(Fail::new("decimal", "digits"));
    }
    if frac.len() > usize::from(scale) {
        return Err(Fail::new("precision_loss", format!("scale {scale} text {trimmed}")));
    }
    let mut digits = whole.to_string();
    digits.push_str(frac);
    for _ in frac.len()..usize::from(scale) {
        digits.push('0');
    }
    let significant = digits.trim_start_matches('0');
    if significant.len() > 38 {
        return Err(Fail::new("decimal_precision", significant.len().to_string()));
    }
    let magnitude: i128 = if significant.is_empty() {
        0
    } else {
        significant
            .parse()
            .map_err(|_| Fail::new("decimal_precision", "i128"))?
    };
    if negative {
        magnitude
            .checked_neg()
            .ok_or_else(|| Fail::new("decimal_precision", "neg"))
    } else {
        Ok(magnitude)
    }
}

#[cfg(test)]
mod tests {
    use super::parse_unscaled;

    #[test]
    fn pads_and_rejects_float_or_extra_scale() {
        assert_eq!(parse_unscaled("4.5000", 4).unwrap(), 45_000);
        assert_eq!(parse_unscaled("-0.0003", 4).unwrap(), -3);
        assert_eq!(parse_unscaled("0.0000", 4).unwrap(), 0);
        assert_eq!(parse_unscaled("1.2", 4).unwrap(), 12_000);
        assert!(parse_unscaled("1.23456", 4).is_err());
        assert!(parse_unscaled("1e2", 4).is_err());
        assert!(parse_unscaled("1.25", 4).unwrap() != 0);
    }

    #[test]
    fn decimal38_stays_integer_scaled() {
        let text = "22345678901234567890123456789.123456789";
        let value = parse_unscaled(text, 9).unwrap();
        assert_eq!(value.to_string(), "22345678901234567890123456789123456789");
    }
}
