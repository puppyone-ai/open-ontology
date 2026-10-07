//! PostgreSQL numeric binary to [`ExactDecimal`].
//!
//! Base-10000 digits, big-endian. NaN, infinity, a short payload, and a
//! fractional digit past `dscale` are refused. Nothing here uses `f64`.

use std::error::Error;
use std::fmt;

use oo_core::ExactDecimal;
use postgres_types::{FromSql, Type};

const SIGN_POS: u16 = 0x0000;
const SIGN_NEG: u16 = 0x4000;
const MAX_DIGITS: usize = 8_192;

#[derive(Debug)]
struct NumericReject;

impl fmt::Display for NumericReject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("numeric")
    }
}

impl Error for NumericReject {}

/// One numeric value decoded from the binary payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgNumeric {
    decimal: ExactDecimal,
}

impl PgNumeric {
    pub fn decimal(&self) -> &ExactDecimal {
        &self.decimal
    }

    pub fn decode(raw: &[u8]) -> Result<ExactDecimal, ()> {
        if raw.len() < 8 {
            return Err(());
        }
        let ndigits = read_i16(raw, 0)?;
        let weight = read_i16(raw, 2)?;
        let sign = read_i16(raw, 4)? as u16;
        let dscale = read_i16(raw, 6)?;
        if dscale < 0 || ndigits < 0 {
            return Err(());
        }
        let count = ndigits as usize;
        if count > MAX_DIGITS || raw.len() != 8 + count * 2 {
            return Err(());
        }
        if sign != SIGN_POS && sign != SIGN_NEG {
            return Err(());
        }
        let mut groups = Vec::with_capacity(count);
        for index in 0..count {
            let digit = read_i16(raw, 8 + index * 2)?;
            if !(0..=9999).contains(&digit) {
                return Err(());
            }
            groups.push(digit as u16);
        }
        let (int_part, frac_part) = layout(weight, dscale as usize, &groups)?;
        let mut coefficient = String::new();
        coefficient.push_str(&int_part);
        coefficient.push_str(&frac_part);
        if coefficient.len() > MAX_DIGITS {
            return Err(());
        }
        let negative = sign == SIGN_NEG;
        ExactDecimal::new(negative, coefficient, i32::from(dscale)).map_err(|_| ())
    }
}

impl<'a> FromSql<'a> for PgNumeric {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        if ty.oid() != 1700 {
            return Err(Box::new(NumericReject));
        }
        let decimal = Self::decode(raw).map_err(|()| Box::new(NumericReject) as Box<_>)?;
        Ok(Self { decimal })
    }

    fn accepts(ty: &Type) -> bool {
        ty.oid() == 1700
    }
}

fn read_i16(raw: &[u8], at: usize) -> Result<i16, ()> {
    let bytes = raw.get(at..at + 2).ok_or(())?;
    Ok(i16::from_be_bytes([bytes[0], bytes[1]]))
}

fn layout(weight: i16, dscale: usize, groups: &[u16]) -> Result<(String, String), ()> {
    if dscale > MAX_DIGITS {
        return Err(());
    }
    let mut int_part = String::new();
    let mut frac_part = String::new();
    if groups.is_empty() {
        int_part.push('0');
    } else if weight < 0 {
        let lead_groups = i32::from(weight).checked_neg().ok_or(())?.checked_sub(1).ok_or(())?;
        if lead_groups < 0 || lead_groups as usize > MAX_DIGITS / 4 {
            return Err(());
        }
        frac_part.push_str(&"0".repeat(lead_groups as usize * 4));
        for digit in groups {
            push_group(&mut frac_part, *digit)?;
        }
        int_part.push('0');
    } else {
        for (index, digit) in groups.iter().enumerate() {
            let group_weight = i32::from(weight) - index as i32;
            if group_weight >= 0 {
                push_group(&mut int_part, *digit)?;
            } else {
                push_group(&mut frac_part, *digit)?;
            }
        }
        let last = i32::from(weight) - groups.len() as i32 + 1;
        if last > 0 {
            if last as usize > MAX_DIGITS / 4 {
                return Err(());
            }
            int_part.push_str(&"0".repeat(last as usize * 4));
        }
        if int_part.is_empty() {
            int_part.push('0');
        }
    }
    if int_part.len() > MAX_DIGITS || frac_part.len() > MAX_DIGITS {
        return Err(());
    }
    if frac_part.len() < dscale {
        frac_part.push_str(&"0".repeat(dscale - frac_part.len()));
    } else if frac_part.len() > dscale {
        if frac_part.as_bytes()[dscale..].iter().any(|byte| *byte != b'0') {
            return Err(());
        }
        frac_part.truncate(dscale);
    }
    Ok((int_part, frac_part))
}

fn push_group(out: &mut String, digit: u16) -> Result<(), ()> {
    if out.len() + 4 > MAX_DIGITS {
        return Err(());
    }
    out.push_str(&format!("{digit:04}"));
    Ok(())
}

/// Integer result OIDs become scale-0 decimals. `i64::MIN` keeps its magnitude.
pub fn int_to_decimal(value: i64) -> ExactDecimal {
    if value == i64::MIN {
        return ExactDecimal::new(true, "9223372036854775808", 0).expect("i64 min digits");
    }
    if value < 0 {
        ExactDecimal::new(true, (-value).to_string(), 0).expect("negative int digits")
    } else {
        ExactDecimal::new(false, value.to_string(), 0).expect("int digits")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(weight: i16, sign: u16, dscale: i16, digits: &[u16]) -> Vec<u8> {
        let mut raw = Vec::new();
        raw.extend_from_slice(&(digits.len() as i16).to_be_bytes());
        raw.extend_from_slice(&weight.to_be_bytes());
        raw.extend_from_slice(&sign.to_be_bytes());
        raw.extend_from_slice(&dscale.to_be_bytes());
        for digit in digits {
            raw.extend_from_slice(&digit.to_be_bytes());
        }
        raw
    }

    #[test]
    fn numeric_oid_is_1700() {
        assert_eq!(Type::NUMERIC.oid(), 1700);
    }

    #[test]
    fn hand_literal_decodes() {
        let raw = encode(4, SIGN_POS, 2, &[100, 0, 0, 0, 0, 100]);
        let decimal = PgNumeric::decode(&raw).expect("hand");
        assert_eq!(decimal.to_plain_string(), "1000000000000000000.01");
        assert_eq!(decimal.scale(), 2);
        assert!(!decimal.is_negative());
    }

    #[test]
    fn small_layouts() {
        let one = PgNumeric::decode(&encode(0, SIGN_POS, 2, &[1, 100])).expect("1.01");
        assert_eq!(one.to_plain_string(), "1.01");
        let cent = PgNumeric::decode(&encode(-1, SIGN_POS, 2, &[100])).expect("0.01");
        assert_eq!(cent.to_plain_string(), "0.01");
        let zero = PgNumeric::decode(&encode(0, SIGN_POS, 2, &[])).expect("0");
        assert_eq!(zero.to_plain_string(), "0.00");
        let neg = PgNumeric::decode(&encode(0, SIGN_NEG, 2, &[1, 100])).expect("-1.01");
        assert_eq!(neg.to_plain_string(), "-1.01");
    }

    #[test]
    fn weight_and_dscale_edges() {
        let tiny = PgNumeric::decode(&encode(-2, SIGN_POS, 6, &[100])).expect("tiny");
        assert_eq!(tiny.to_plain_string(), "0.000001");
        let padded = PgNumeric::decode(&encode(0, SIGN_POS, 4, &[1])).expect("pad");
        assert_eq!(padded.to_plain_string(), "1.0000");
        assert!(PgNumeric::decode(&encode(0, SIGN_POS, 1, &[1, 100])).is_err());
        assert!(PgNumeric::decode(&encode(0, 0xC000, 0, &[])).is_err());
        assert!(PgNumeric::decode(&encode(0, SIGN_POS, 0, &[10_000])).is_err());
        assert!(PgNumeric::decode(&[0, 1]).is_err());
    }

    #[test]
    fn int_min_is_exact() {
        assert_eq!(
            int_to_decimal(i64::MIN).to_plain_string(),
            "-9223372036854775808"
        );
        assert_eq!(int_to_decimal(0).to_plain_string(), "0");
    }
}
