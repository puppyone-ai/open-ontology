//! Exact ratio comparison. Digit strings only. No binary float.

use std::cmp::Ordering;

use oo_core::ExactDecimal;

pub fn threshold_fraction(decimal: &ExactDecimal) -> Result<(String, String), &'static str> {
    if decimal.is_negative() {
        return Err("negative");
    }
    let coefficient = decimal.coefficient();
    if decimal.scale() >= 0 {
        let scale = usize::try_from(decimal.scale()).map_err(|_| "scale")?;
        return Ok((coefficient.to_owned(), pow10(scale)));
    }
    let zeros = usize::try_from(decimal.scale().unsigned_abs()).map_err(|_| "scale")?;
    let mut numerator = String::with_capacity(coefficient.len().saturating_add(zeros));
    numerator.push_str(coefficient);
    numerator.extend(std::iter::repeat_n('0', zeros));
    Ok((trim_zeros(&numerator), "1".to_owned()))
}

pub fn compare_coverage(
    numerator: u64,
    denominator: u64,
    threshold_numerator: &str,
    threshold_denominator: &str,
) -> Ordering {
    let left = mul_u64(threshold_denominator, numerator);
    let right = mul_u64(threshold_numerator, denominator);
    cmp_digits(&left, &right)
}

fn pow10(scale: usize) -> String {
    let mut out = String::with_capacity(scale.saturating_add(1));
    out.push('1');
    out.extend(std::iter::repeat_n('0', scale));
    out
}

fn trim_zeros(text: &str) -> String {
    let trimmed = text.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn mul_u64(digits: &str, factor: u64) -> String {
    if digits == "0" || factor == 0 {
        return "0".to_owned();
    }
    let mut carry = 0u128;
    let mut out = Vec::with_capacity(digits.len().saturating_add(20));
    for byte in digits.as_bytes().iter().rev() {
        let digit = u128::from(byte.wrapping_sub(b'0'));
        let product = digit * u128::from(factor) + carry;
        let next = product % 10;
        out.push(b'0' + u8::try_from(next).unwrap_or(0));
        carry = product / 10;
    }
    while carry > 0 {
        let next = carry % 10;
        out.push(b'0' + u8::try_from(next).unwrap_or(0));
        carry /= 10;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_else(|_| "0".to_owned())
}

fn cmp_digits(left: &str, right: &str) -> Ordering {
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decimal(text: &str) -> ExactDecimal {
        ExactDecimal::parse_sql(text).expect(text)
    }

    #[test]
    fn nineteen_twentieths_equals_lexical_0_95() {
        let (numer, denom) = threshold_fraction(&decimal("0.95")).expect("fraction");
        assert_eq!(numer, "95");
        assert_eq!(denom, "100");
        assert_eq!(compare_coverage(19, 20, &numer, &denom), Ordering::Equal);
        assert_eq!(compare_coverage(95, 100, &numer, &denom), Ordering::Equal);
        assert_eq!(compare_coverage(94, 100, &numer, &denom), Ordering::Less);
        assert_eq!(compare_coverage(96, 100, &numer, &denom), Ordering::Greater);
    }

    #[test]
    fn trailing_zero_keeps_its_denominator() {
        let (numer, denom) = threshold_fraction(&decimal("0.950")).expect("fraction");
        assert_eq!((numer.as_str(), denom.as_str()), ("950", "1000"));
        assert_eq!(compare_coverage(95, 100, &numer, &denom), Ordering::Equal);
    }

    #[test]
    fn one_hundred_over_one_hundred_five_is_above_0_95() {
        let (numer, denom) = threshold_fraction(&decimal("0.95")).expect("fraction");
        assert_eq!(
            compare_coverage(100, 105, &numer, &denom),
            Ordering::Greater
        );
    }
}
