//! Exact decimal comparison. Digit strings only.

use oo_core::ExactDecimal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Passed { difference: ExactDecimal },
    Failed { difference: ExactDecimal },
}

/// Both limits are optional. Absent means exact equality. Both present means both must hold.
pub fn compare(
    metric: &ExactDecimal,
    control: &ExactDecimal,
    abs: Option<&ExactDecimal>,
    rel: Option<&ExactDecimal>,
) -> Result<Verdict, ()> {
    if abs.is_some_and(|limit| limit.is_negative()) || rel.is_some_and(|limit| limit.is_negative()) {
        return Err(());
    }
    let difference = abs_diff(metric, control)?;
    let abs_ok = match abs {
        Some(limit) => less_or_equal(&difference, limit)?,
        None => true,
    };
    let rel_ok = match rel {
        Some(limit) => relative_ok(&difference, control, limit)?,
        None => true,
    };
    let exact_ok = abs.is_some() || rel.is_some() || is_zero(&difference);
    let passed = abs_ok && rel_ok && exact_ok;
    if passed {
        Ok(Verdict::Passed { difference })
    } else {
        Ok(Verdict::Failed { difference })
    }
}

pub fn numeric_eq(left: &ExactDecimal, right: &ExactDecimal) -> bool {
    abs_diff(left, right)
        .ok()
        .is_some_and(|difference| is_zero(&difference))
}

fn is_zero(value: &ExactDecimal) -> bool {
    value.coefficient() == "0"
}

fn abs_diff(left: &ExactDecimal, right: &ExactDecimal) -> Result<ExactDecimal, ()> {
    let scale = left.scale().max(right.scale());
    let left_digits = left.rescale_exact(scale).map_err(|_| ())?;
    let right_digits = right.rescale_exact(scale).map_err(|_| ())?;
    let digits = if left.is_negative() == right.is_negative() {
        match cmp_digits(left_digits.coefficient(), right_digits.coefficient()) {
            std::cmp::Ordering::Equal => "0".to_owned(),
            std::cmp::Ordering::Greater => {
                sub_digits(left_digits.coefficient(), right_digits.coefficient())
            }
            std::cmp::Ordering::Less => {
                sub_digits(right_digits.coefficient(), left_digits.coefficient())
            }
        }
    } else {
        add_digits(left_digits.coefficient(), right_digits.coefficient())
    };
    ExactDecimal::new(false, digits, scale).map_err(|_| ())
}

fn less_or_equal(left: &ExactDecimal, right: &ExactDecimal) -> Result<bool, ()> {
    if left.is_negative() && !right.is_negative() {
        return Ok(true);
    }
    if !left.is_negative() && right.is_negative() {
        return Ok(false);
    }
    let scale = left.scale().max(right.scale());
    let left_digits = left.rescale_exact(scale).map_err(|_| ())?;
    let right_digits = right.rescale_exact(scale).map_err(|_| ())?;
    let order = cmp_digits(left_digits.coefficient(), right_digits.coefficient());
    if left.is_negative() {
        Ok(order != std::cmp::Ordering::Less)
    } else {
        Ok(order != std::cmp::Ordering::Greater)
    }
}

fn relative_ok(difference: &ExactDecimal, control: &ExactDecimal, rel: &ExactDecimal) -> Result<bool, ()> {
    if is_zero(control) {
        return Ok(is_zero(difference));
    }
    let control_abs = ExactDecimal::new(false, control.coefficient(), control.scale()).map_err(|_| ())?;
    // |diff| <= rel * |control|
    // D * 10^(-sd) <= R * C * 10^(-(sr+sc))
    let product = mul_digits(rel.coefficient(), control_abs.coefficient());
    let left_exp = i64::from(rel.scale()) + i64::from(control_abs.scale());
    let right_exp = i64::from(difference.scale());
    let gap = left_exp - right_exp;
    let order = if gap >= 0 {
        let shifted = shift10(difference.coefficient(), gap)?;
        cmp_digits(&shifted, &product)
    } else {
        let shifted = shift10(&product, -gap)?;
        cmp_digits(difference.coefficient(), &shifted)
    };
    Ok(order != std::cmp::Ordering::Greater)
}

fn shift10(digits: &str, zeros: i64) -> Result<String, ()> {
    if !(0..=16_384).contains(&zeros) {
        return Err(());
    }
    if digits.len() + zeros as usize > 16_384 {
        return Err(());
    }
    let mut out = digits.to_owned();
    out.extend(std::iter::repeat_n('0', zeros as usize));
    Ok(out)
}

fn cmp_digits(left: &str, right: &str) -> std::cmp::Ordering {
    let left = trim_zeros(left);
    let right = trim_zeros(right);
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

fn trim_zeros(digits: &str) -> &str {
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() { "0" } else { trimmed }
}

fn add_digits(left: &str, right: &str) -> String {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut out = Vec::new();
    let mut carry = 0_u16;
    let mut i = left.len();
    let mut j = right.len();
    while i > 0 || j > 0 || carry > 0 {
        let a = if i > 0 {
            i -= 1;
            u16::from(left[i] - b'0')
        } else {
            0
        };
        let b = if j > 0 {
            j -= 1;
            u16::from(right[j] - b'0')
        } else {
            0
        };
        let sum = a + b + carry;
        out.push(char::from(b'0' + (sum % 10) as u8));
        carry = sum / 10;
    }
    if out.is_empty() {
        return "0".to_owned();
    }
    out.iter().rev().collect()
}

fn sub_digits(left: &str, right: &str) -> String {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut out = Vec::new();
    let mut borrow = 0_i16;
    let mut i = left.len();
    let mut j = right.len();
    while i > 0 {
        i -= 1;
        let mut a = i16::from(left[i] - b'0') - borrow;
        let b = if j > 0 {
            j -= 1;
            i16::from(right[j] - b'0')
        } else {
            0
        };
        if a < b {
            a += 10;
            borrow = 1;
        } else {
            borrow = 0;
        }
        out.push(char::from(b'0' + (a - b) as u8));
    }
    while out.last() == Some(&'0') && out.len() > 1 {
        out.pop();
    }
    if out.is_empty() {
        "0".to_owned()
    } else {
        out.iter().rev().collect()
    }
}

fn mul_digits(left: &str, right: &str) -> String {
    let left = trim_zeros(left);
    let right = trim_zeros(right);
    if left == "0" || right == "0" {
        return "0".to_owned();
    }
    let mut acc = vec![0_u32; left.len() + right.len()];
    let lb = left.as_bytes();
    let rb = right.as_bytes();
    for i in 0..lb.len() {
        for j in 0..rb.len() {
            acc[i + j + 1] += u32::from(lb[i] - b'0') * u32::from(rb[j] - b'0');
        }
    }
    for index in (1..acc.len()).rev() {
        let carry = acc[index] / 10;
        acc[index] %= 10;
        acc[index - 1] += carry;
    }
    let mut started = false;
    let mut out = String::new();
    for digit in acc {
        if digit == 0 && !started {
            continue;
        }
        started = true;
        out.push(char::from(b'0' + digit as u8));
    }
    if out.is_empty() {
        "0".to_owned()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(text: &str) -> ExactDecimal {
        ExactDecimal::parse_sql(text).expect(text)
    }

    #[test]
    fn scale_and_hand_literal() {
        let hand = dec("1000000000000000000.01");
        assert_eq!(hand.to_plain_string(), "1000000000000000000.01");
        assert!(numeric_eq(&hand, &dec("1000000000000000000.010")));
        assert!(numeric_eq(&dec("1.0"), &dec("1.00")));
        assert!(numeric_eq(&dec("0.010"), &dec("0.01")));
        assert!(numeric_eq(&dec("-1.0"), &dec("-1.00")));
        assert!(!numeric_eq(&dec("-1.00"), &dec("1.00")));
    }

    #[test]
    fn tolerance_and_is_stricter() {
        let metric = dec("101");
        let control = dec("100");
        assert!(matches!(
            compare(&metric, &control, Some(&dec("1")), Some(&dec("0.01"))),
            Ok(Verdict::Passed { .. })
        ));
        assert!(matches!(
            compare(&metric, &control, Some(&dec("0.5")), Some(&dec("0.01"))),
            Ok(Verdict::Failed { .. })
        ));
        assert!(matches!(
            compare(&metric, &control, Some(&dec("2")), Some(&dec("0.001"))),
            Ok(Verdict::Failed { .. })
        ));
        assert!(matches!(
            compare(&dec("1.010"), &dec("1.000"), Some(&dec("0.01")), None),
            Ok(Verdict::Passed { .. })
        ));
        assert!(matches!(
            compare(&dec("1.02"), &dec("1.00"), Some(&dec("0.00")), None),
            Ok(Verdict::Failed { .. })
        ));
    }

    #[test]
    fn zero_control_relative() {
        assert!(matches!(
            compare(&dec("0.00"), &dec("0"), None, Some(&dec("0.01"))),
            Ok(Verdict::Passed { .. })
        ));
        assert!(matches!(
            compare(&dec("0.01"), &dec("0.00"), None, Some(&dec("0.01"))),
            Ok(Verdict::Failed { .. })
        ));
        assert!(compare(&dec("1"), &dec("1"), Some(&dec("-0.01")), None).is_err());
        assert_eq!(mul_digits("25", "25"), "625");
        assert_eq!(mul_digits("999", "999"), "998001");
    }
}
