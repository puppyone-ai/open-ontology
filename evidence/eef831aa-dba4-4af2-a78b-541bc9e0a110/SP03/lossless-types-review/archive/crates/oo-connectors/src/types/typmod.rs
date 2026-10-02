//! Signed 11-bit PostgreSQL `numeric` typmod.
//!
//! PostgreSQL stores `((precision << 16) | scale_bits) + 4`, where `scale_bits`
//! is an 11-bit two's-complement value. The frozen catalog helper
//! [`crate::numeric_parts`] keeps the low 16 bits unsigned. This module does
//! not change that helper.

/// Declared precision and signed scale. `None` from [`numeric_typmod_v1`] means
/// unconstrained `numeric` (`typmod < 0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumericTypmodV1 {
    pub precision: i32,
    pub scale: i32,
}

/// Decode a column `atttypmod` with the PostgreSQL 11-bit scale rule.
pub fn numeric_typmod_v1(typmod: i32) -> Option<NumericTypmodV1> {
    if typmod < 0 {
        return None;
    }
    let adjusted = typmod.wrapping_sub(4);
    let precision = (adjusted >> 16) & 0xFFFF;
    let raw = adjusted & 0x7FF;
    // `((raw ^ 0x400) - 0x400)` sign-extends 11 bits. Same as numeric.c.
    let scale = (raw ^ 0x400) - 0x400;
    Some(NumericTypmodV1 { precision, scale })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::numeric_parts;

    fn typmod(precision: i32, scale: i32) -> i32 {
        let bits = scale & 0x7FF;
        ((precision << 16) | bits) + 4
    }

    #[test]
    fn negative_scale_disagrees_with_catalog_v1() {
        let value = typmod(10, -2);
        let signed = numeric_typmod_v1(value).unwrap();
        assert_eq!(signed.precision, 10);
        assert_eq!(signed.scale, -2);
        assert_eq!(numeric_parts(value), Some((10, 2046)));
    }

    #[test]
    fn positive_scale_above_precision_matches_catalog_v1() {
        let value = typmod(4, 6);
        assert_eq!(
            numeric_typmod_v1(value),
            Some(NumericTypmodV1 {
                precision: 4,
                scale: 6
            })
        );
        assert_eq!(numeric_parts(value), Some((4, 6)));
    }

    #[test]
    fn unconstrained_is_absent() {
        assert_eq!(numeric_typmod_v1(-1), None);
        assert_eq!(numeric_parts(-1), None);
    }
}
