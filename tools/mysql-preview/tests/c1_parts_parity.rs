//! Parity of the shared C1 canonical form between the MySQL source decoder and the
//! PostgreSQL text readback (`oo_pipeline::c1_parts`).
//!
//! For each column in `pg17-readback-v2.json` (values recorded from a real
//! postgres:17 server), the MySQL value is decoded by `oo_connectors::mysql::values`
//! and canonicalized under the target type. The result must equal the golden
//! expectation in `golden-v2.json`, which was computed independently by
//! `gen_golden.py`. The PostgreSQL side is checked against the same golden value in
//! `oo-pipeline`'s unit tests. Both sides therefore agree with each other.

use oo_connectors::mysql::map::StoreKind;
use oo_connectors::mysql::values::{RawCell, decode_cell};
use oo_pipeline::c1_parts::{canonical_cell, logical_type_of_pg, pg_text_cell, source_cell};
use serde_json::Value;

const GOLDEN: &str =
    include_str!("../../../crates/oo-pipeline/tests/fixtures/c1_parts/golden-v2.json");
const PG_READBACK: &str =
    include_str!("../../../crates/oo-pipeline/tests/fixtures/c1_parts/pg17-readback-v2.json");

fn date(year: u16, month: u8, day: u8) -> RawCell {
    RawCell::Date {
        year,
        month,
        day,
        hour: 0,
        minute: 0,
        second: 0,
        micros: 0,
    }
}

/// The MySQL value and storage kind that produce each recorded PostgreSQL column.
fn mysql_sample(column: &str) -> (StoreKind, RawCell, bool) {
    let text = |bytes: &[u8]| RawCell::Bytes(bytes.to_vec());
    match column {
        "c_bool" => (StoreKind::Bool, RawCell::Int(1), false),
        "c_i2" => (StoreKind::Int { width: 2 }, RawCell::Int(-32768), false),
        "c_i4" => (StoreKind::Int { width: 4 }, RawCell::Int(2147483647), false),
        "c_i8" => (StoreKind::Int { width: 8 }, RawCell::Int(i64::MIN), false),
        "c_u64" => (
            StoreKind::Numeric {
                precision: 20,
                scale: 0,
            },
            RawCell::Uint(u64::MAX),
            false,
        ),
        "c_dec" => (
            StoreKind::Numeric {
                precision: 10,
                scale: 2,
            },
            text(b"-12.3"),
            false,
        ),
        "c_dec0" => (
            StoreKind::Numeric {
                precision: 30,
                scale: 4,
            },
            text(b"0.0001"),
            false,
        ),
        "c_dec1" => (
            StoreKind::Numeric {
                precision: 38,
                scale: 10,
            },
            text(b"1234.5"),
            false,
        ),
        "c_dec_zero" => (
            StoreKind::Numeric {
                precision: 10,
                scale: 4,
            },
            text(b"0.0000"),
            false,
        ),
        // Precision above 38 uses the Decimal256 path in the MySQL decoder.
        "c_dec_wide" => (
            StoreKind::Numeric {
                precision: 65,
                scale: 30,
            },
            text(b"-12345678901234567890123456789012345.123456789012345678901234567890"),
            false,
        ),
        "c_f4" => (StoreKind::Float4, RawCell::Float(0.1f32), false),
        "c_f8neg0" => (StoreKind::Float8, RawCell::Double(-0.0), false),
        "c_f8big" => (StoreKind::Float8, RawCell::Double(1.5e300), false),
        "c_f8small" => (StoreKind::Float8, RawCell::Double(1e-7), false),
        "c_varchar" => (
            StoreKind::Text { limit: Some(255) },
            text("héllo 世界 😀".as_bytes()),
            false,
        ),
        "c_varchar_sp" => (StoreKind::Text { limit: Some(255) }, text(b"a  "), false),
        // MySQL CHAR returns its value without trailing spaces.
        "c_bpchar" => (StoreKind::Text { limit: Some(3) }, text(b"a"), false),
        "c_json" => (
            StoreKind::Json,
            text(br#"{"b": 1.0, "a": [true, null]}"#),
            false,
        ),
        "c_bytea" => (StoreKind::Bytes, RawCell::Bytes(vec![0, 255, 16]), false),
        "c_date" => (StoreKind::Date, date(2024, 2, 29), false),
        "c_date_min" => (StoreKind::Date, date(1, 1, 1), false),
        "c_dt" => (
            StoreKind::TimestampNaive,
            RawCell::Date {
                year: 2024,
                month: 1,
                day: 2,
                hour: 3,
                minute: 4,
                second: 5,
                micros: 1,
            },
            false,
        ),
        "c_dt_pre" => (
            StoreKind::TimestampNaive,
            RawCell::Date {
                year: 1969,
                month: 12,
                day: 31,
                hour: 23,
                minute: 59,
                second: 59,
                micros: 500_000,
            },
            false,
        ),
        "c_tstz" => (
            StoreKind::TimestampTz,
            RawCell::Date {
                year: 2024,
                month: 1,
                day: 2,
                hour: 3,
                minute: 4,
                second: 5,
                micros: 123_456,
            },
            false,
        ),
        "c_time_neg" => (
            StoreKind::Time,
            RawCell::Time {
                negative: true,
                // 838:59:59 is 34 days and 22 hours. The decoder accepts hours only up to 23.
                days: 34,
                hours: 22,
                minutes: 59,
                seconds: 59,
                micros: 0,
            },
            false,
        ),
        "c_time_pos" => (
            StoreKind::Time,
            RawCell::Time {
                negative: false,
                days: 0,
                hours: 12,
                minutes: 34,
                seconds: 56,
                micros: 7,
            },
            false,
        ),
        "c_time_zero" => (
            StoreKind::Time,
            RawCell::Time {
                negative: false,
                days: 0,
                hours: 0,
                minutes: 0,
                seconds: 0,
                micros: 0,
            },
            false,
        ),
        "c_null" => (StoreKind::Int { width: 4 }, RawCell::Null, true),
        other => panic!("no MySQL sample for {other}"),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn mysql_decoder_and_postgres_readback_agree_with_the_golden_bytes_for_every_type() {
    let golden: Value = serde_json::from_str(GOLDEN).expect("golden json");
    let readback: Value = serde_json::from_str(PG_READBACK).expect("readback json");
    let parity = golden["parity"].as_array().expect("parity");
    let columns = readback["columns"].as_array().expect("columns");
    assert_eq!(columns.len(), 28);
    for column in columns {
        let name = column["name"].as_str().expect("name");
        let oid = column["oid"].as_i64().expect("oid");
        let typmod = column["typmod"].as_i64().expect("typmod") as i32;
        let expect = parity
            .iter()
            .find(|entry| entry["column"] == name)
            .expect("golden entry")["expect_hex"]
            .as_str()
            .expect("hex");
        let ty = logical_type_of_pg(oid, typmod).expect("target type");

        let (store, raw, nullable) = mysql_sample(name);
        let (decoded, reject) = decode_cell(store, &raw, nullable).expect("decode");
        assert!(
            reject.is_none(),
            "column {name} was rejected by the decoder"
        );
        let from_mysql = canonical_cell(&ty, &source_cell(&decoded).expect("source cell"))
            .expect("mysql canonical");
        assert_eq!(
            hex(&from_mysql),
            expect,
            "MySQL decoder path, column {name}"
        );

        let text = column["text"].as_str();
        let from_pg =
            canonical_cell(&ty, &pg_text_cell(oid, text).expect("pg cell")).expect("pg canonical");
        assert_eq!(
            hex(&from_pg),
            expect,
            "PostgreSQL readback path, column {name}"
        );
    }
}

#[test]
fn a_one_digit_difference_in_the_value_changes_the_canonical_bytes() {
    let ty = logical_type_of_pg(1700, 655366).expect("numeric(10,2)");
    let (store, raw, nullable) = mysql_sample("c_dec");
    let (decoded, _) = decode_cell(store, &raw, nullable).expect("decode");
    let from_mysql = canonical_cell(&ty, &source_cell(&decoded).expect("cell")).expect("canon");
    let other = pg_text_cell(1700, Some("-12.31")).expect("pg cell");
    let from_pg = canonical_cell(&ty, &other).expect("canon");
    assert_ne!(from_mysql, from_pg);
}
