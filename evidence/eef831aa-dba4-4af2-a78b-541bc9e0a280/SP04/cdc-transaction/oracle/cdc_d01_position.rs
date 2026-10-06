//! Proof-only overlay. Not a product file and not a workspace member edit.
//! Parses the frozen CDC position strings with `oo_core::Position`.

use oo_core::rfc3339::Rfc3339Timestamp;
use oo_core::Position;

#[test]
fn frozen_positions_fit_oo_core_position() {
    for text in [
        "0",
        "1",
        "94490029632",
        "18446744073709551616",
        "18446744073709551617",
        "18446744168199581248",
    ] {
        let position: Position = serde_json::from_str(&format!("\"{text}\"")).expect(text);
        let decimal = position.as_decimal();
        assert!(decimal.fits_position(), "{text}");
        assert_eq!(decimal.scale(), 0, "{text}");
        assert!(!decimal.is_negative(), "{text}");
        assert_eq!(decimal.to_plain_string(), text, "{text}");
        let back = serde_json::to_string(&position).expect("json");
        assert_eq!(back, format!("\"{text}\""));
    }
}

#[test]
fn max_epoch_and_lsn_fit_and_wider_values_do_not() {
    let max = (u128::from(u32::MAX) << 64) | u128::from(u64::MAX);
    let text = max.to_string();
    assert!(text.len() <= 38, "{}", text.len());
    let position: Position = serde_json::from_str(&format!("\"{text}\"")).expect("max");
    assert!(position.as_decimal().fits_position());
    assert_eq!(position.as_decimal().to_plain_string(), text);

    let wide = "9".repeat(39);
    assert!(serde_json::from_str::<Position>(&format!("\"{wide}\"")).is_err());
    assert!(serde_json::from_str::<Position>("1").is_err());
}

#[test]
fn sample_timestamps_keep_the_stored_spelling() {
    let fraction = Rfc3339Timestamp::new("2026-04-15T08:30:00.123456Z").expect("fraction");
    assert_eq!(fraction.as_str(), "2026-04-15T08:30:00.123456Z");
    let whole = Rfc3339Timestamp::new("2026-04-15T08:30:01Z").expect("whole");
    assert_eq!(whole.as_str(), "2026-04-15T08:30:01Z");
}
