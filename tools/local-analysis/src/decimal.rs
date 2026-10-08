use crate::fault::Fault;
use crate::limits::{MAX_COEFFICIENT_DIGITS, MAX_SUM_DIGITS};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decimal {
    pub negative: bool,
    pub coefficient: String,
    pub scale: i32,
    pub text: String,
}

pub fn plain_decimal(negative: bool, coefficient: &str, scale: i32) -> String {
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if scale == 0 {
        out.push_str(coefficient);
        return out;
    }
    if scale < 0 {
        out.push_str(coefficient);
        out.extend(std::iter::repeat_n('0', scale.unsigned_abs() as usize));
        return out;
    }
    let scale_us = scale as usize;
    if coefficient.len() > scale_us {
        let split = coefficient.len() - scale_us;
        out.push_str(&coefficient[..split]);
        out.push('.');
        out.push_str(&coefficient[split..]);
    } else {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', scale_us - coefficient.len()));
        out.push_str(coefficient);
    }
    out
}

pub fn canonical_decimal(negative: bool, coefficient: &str, scale: i32) -> bool {
    if coefficient.is_empty()
        || coefficient.len() > MAX_COEFFICIENT_DIGITS
        || !coefficient.bytes().all(|byte| byte.is_ascii_digit())
    {
        return false;
    }
    if coefficient.len() > 1 && coefficient.as_bytes()[0] == b'0' {
        return false;
    }
    if scale.unsigned_abs() > MAX_COEFFICIENT_DIGITS as u32 {
        return false;
    }
    !(coefficient == "0" && negative)
}

pub fn canonical_i64(text: &str) -> bool {
    match text.parse::<i64>() {
        Ok(value) => value.to_string() == text,
        Err(_) => false,
    }
}

pub fn int_decimal(text: &str) -> Result<Decimal, Fault> {
    if !canonical_i64(text) {
        return Err(Fault::source());
    }
    let negative = text.starts_with('-');
    let coefficient = if negative { &text[1..] } else { text };
    let owned = coefficient.to_string();
    let rendered = plain_decimal(negative, &owned, 0);
    Ok(Decimal {
        negative,
        coefficient: owned,
        scale: 0,
        text: rendered,
    })
}

pub fn add_decimals(left: &Decimal, right: &Decimal) -> Result<Decimal, Fault> {
    let scale = left.scale.max(right.scale);
    let a = align(left, scale)?;
    let b = align(right, scale)?;
    let (negative, coefficient) = add_signed(&a, left.negative, &b, right.negative)?;
    let text = plain_decimal(negative, &coefficient, scale);
    Ok(Decimal {
        negative,
        coefficient,
        scale,
        text,
    })
}

fn align(value: &Decimal, scale: i32) -> Result<String, Fault> {
    let delta = scale - value.scale;
    if delta < 0 {
        return Err(Fault::budget());
    }
    let extra = usize::try_from(delta).map_err(|_| Fault::budget())?;
    if value.coefficient.len().saturating_add(extra) > MAX_SUM_DIGITS {
        return Err(Fault::budget());
    }
    let mut out = value.coefficient.clone();
    out.extend(std::iter::repeat_n('0', extra));
    Ok(out)
}

fn add_signed(
    left: &str,
    left_neg: bool,
    right: &str,
    right_neg: bool,
) -> Result<(bool, String), Fault> {
    let (negative, digits) = if left_neg == right_neg {
        (left_neg, add_mag(left, right)?)
    } else if cmp_mag(left, right) != std::cmp::Ordering::Less {
        (left_neg, sub_mag(left, right)?)
    } else {
        (right_neg, sub_mag(right, left)?)
    };
    let coefficient = strip(&digits);
    if coefficient == "0" {
        Ok((false, coefficient))
    } else {
        Ok((negative, coefficient))
    }
}

fn cmp_mag(left: &str, right: &str) -> std::cmp::Ordering {
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

fn add_mag(left: &str, right: &str) -> Result<String, Fault> {
    let a = left.as_bytes();
    let b = right.as_bytes();
    let width = a.len().max(b.len());
    if width.saturating_add(1) > MAX_SUM_DIGITS {
        return Err(Fault::budget());
    }
    let mut out = vec![b'0'; width + 1];
    let mut carry = 0_u16;
    for index in 0..width {
        let av = digit_at(a, index);
        let bv = digit_at(b, index);
        let sum = u16::from(av) + u16::from(bv) + carry;
        out[width - index] = b'0' + (sum % 10) as u8;
        carry = sum / 10;
    }
    out[0] = b'0' + carry as u8;
    Ok(bytes_to_string(&out))
}

fn sub_mag(left: &str, right: &str) -> Result<String, Fault> {
    let a = left.as_bytes();
    let b = right.as_bytes();
    let mut out = vec![b'0'; a.len()];
    let mut borrow = 0_i16;
    for index in 0..a.len() {
        let av = i16::from(digit_at(a, index));
        let bv = i16::from(digit_at(b, index));
        let mut diff = av - bv - borrow;
        if diff < 0 {
            diff += 10;
            borrow = 1;
        } else {
            borrow = 0;
        }
        out[a.len() - 1 - index] = b'0' + diff as u8;
    }
    if borrow != 0 {
        return Err(Fault::source());
    }
    Ok(bytes_to_string(&out))
}

fn digit_at(bytes: &[u8], from_end: usize) -> u8 {
    if from_end >= bytes.len() {
        0
    } else {
        bytes[bytes.len() - 1 - from_end] - b'0'
    }
}

fn strip(digits: &str) -> String {
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn bytes_to_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

#[cfg(test)]
mod tests {
    use super::{Decimal, add_decimals, plain_decimal};

    fn dec(negative: bool, coefficient: &str, scale: i32) -> Decimal {
        Decimal {
            negative,
            coefficient: coefficient.to_string(),
            scale,
            text: plain_decimal(negative, coefficient, scale),
        }
    }

    #[test]
    fn aligns_scales_and_keeps_zeros() {
        let sum = add_decimals(&dec(false, "150", 2), &dec(false, "25", 1)).unwrap();
        assert_eq!(sum.text, "4.00");
        assert_eq!(sum.coefficient, "400");
        assert_eq!(sum.scale, 2);
    }

    #[test]
    fn rejects_a_sum_past_the_digit_budget() {
        let coefficient = "9".repeat(crate::limits::MAX_SUM_DIGITS);
        let err = add_decimals(&dec(false, &coefficient, 0), &dec(false, "1", 0)).unwrap_err();
        assert_eq!(err, crate::fault::Fault::budget());
    }

    #[test]
    fn negative_result_and_exact_zero() {
        let mixed = add_decimals(&dec(true, "25", 1), &dec(false, "150", 2)).unwrap();
        assert_eq!(mixed.text, "-1.00");
        let zero = add_decimals(&dec(true, "1", 4), &dec(false, "1", 4)).unwrap();
        assert!(!zero.negative);
        assert_eq!(zero.text, "0.0000");
    }
}
