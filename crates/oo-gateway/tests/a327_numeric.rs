//! Public numeric cell. Wire decode and admission live in the library tests.

use oo_core::ExactDecimal;
use oo_gateway::query_executor::Cell;

#[test]
fn hand_literals_are_exact_decimals() {
    let positive = ExactDecimal::new(false, "123456789012345678", 4).expect("positive");
    assert!(!positive.is_negative());
    assert_eq!(positive.coefficient(), "123456789012345678");
    assert_eq!(positive.scale(), 4);
    assert_eq!(positive.to_string(), "12345678901234.5678");
    assert!(matches!(Cell::Decimal(positive), Cell::Decimal(_)));

    let negative = ExactDecimal::new(true, "123456789012345678", 4).expect("negative");
    assert!(negative.is_negative());
    assert_eq!(negative.to_string(), "-12345678901234.5678");

    let small = ExactDecimal::new(false, "1", 4).expect("small");
    assert_eq!(small.to_string(), "0.0001");
    let small_neg = ExactDecimal::new(true, "1", 4).expect("small neg");
    assert_eq!(small_neg.to_string(), "-0.0001");
    assert!(small_neg.is_negative());

    let zero = ExactDecimal::new(true, "0", 4).expect("zero");
    assert!(!zero.is_negative());
    assert_eq!(zero.coefficient(), "0");
    assert_eq!(zero.scale(), 4);
    assert_eq!(zero.to_string(), "0.0000");
    assert!(matches!(Cell::Null, Cell::Null));
}
