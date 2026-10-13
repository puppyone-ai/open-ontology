use crate::fail::{fail, Gate};

/// Render an unscaled decimal integer at `scale` without binary floating point.
/// Trailing fractional zeros stay. A leading minus is kept for non-zero values.
pub fn format_scaled(unscaled: &str, scale: i8) -> Gate<String> {
    if scale < 0 {
        return Err(fail("negative decimal scale"));
    }
    let scale = usize::try_from(scale).map_err(|_| fail("decimal scale does not fit"))?;
    let negative = unscaled.starts_with('-');
    let raw = unscaled.trim_start_matches(['-', '+']);
    if raw.is_empty() || !raw.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(fail(format!("decimal digits are not exact: {unscaled}")));
    }
    let stripped = raw.trim_start_matches('0');
    let digits = if stripped.is_empty() { "0" } else { stripped };
    let body = if scale == 0 {
        digits.to_string()
    } else if digits.len() > scale {
        let split = digits.len() - scale;
        let head = &digits[..split];
        let tail = &digits[split..];
        format!("{head}.{tail}")
    } else {
        let pad = "0".repeat(scale - digits.len());
        format!("0.{pad}{digits}")
    };
    if negative && digits != "0" {
        Ok(format!("-{body}"))
    } else {
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use arrow_array::array::PrimitiveArray;
    use arrow_array::types::Decimal256Type;
    use arrow_buffer::i256;

    use super::format_scaled;

    #[test]
    fn scale_keeps_trailing_zeros_and_the_sign() {
        assert_eq!(format_scaled("0", 10).unwrap(), "0.0000000000");
        assert_eq!(format_scaled("10000000000", 10).unwrap(), "1.0000000000");
        assert_eq!(format_scaled("-425000000000", 10).unwrap(), "-42.5000000000");
        let amount = "12345678901234567890123456789012345678901234567890";
        assert_eq!(
            format_scaled(amount, 10).unwrap(),
            "1234567890123456789012345678901234567890.1234567890"
        );
        let rendered = format_scaled(amount, 10).unwrap();
        assert!(!rendered.contains('e') && !rendered.contains('E'));
    }

    #[test]
    fn arrow_i256_display_is_the_exact_unscaled_integer() {
        let unscaled = "12345678901234567890123456789012345678901234567890";
        let value = i256::from_str(unscaled).unwrap();
        assert_eq!(value.to_string(), unscaled);
        let array = PrimitiveArray::<Decimal256Type>::from(vec![value]);
        let array = array.with_precision_and_scale(76, 10).unwrap();
        assert_eq!(array.value(0).to_string(), unscaled);
        assert_eq!(
            format_scaled(&array.value(0).to_string(), 10).unwrap(),
            "1234567890123456789012345678901234567890.1234567890"
        );
    }
}
