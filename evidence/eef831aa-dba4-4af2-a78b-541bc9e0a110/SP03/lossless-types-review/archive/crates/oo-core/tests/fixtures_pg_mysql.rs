//! Synthetic PostgreSQL and MySQL cells. No database driver is loaded.
//! Expected decimals and the Shanghai instant come from the fixture and from
//! `date`, not from the converter.

use oo_core::TYPE_POLICY_V1;
use oo_core::arrow_meta::ArrowTypeName;
use oo_core::batch::{
    BatchColumn, BatchMeta, BatchPath, BatchRow, BatchVersion, CellSlot, TypePolicyVersion,
    validate_batch, validate_batch_row,
};
use oo_core::canonical::{FieldValue, LogicalField, LogicalType, LogicalValue, encode_value};
use oo_core::config::model::{FileMode, Tinyint1Policy, TypePolicy, ZeroDatePolicy};
use oo_core::config::version::ConfigVersion;
use oo_core::convert::{
    CharsetClaim, MysqlIntKind, RawCell, RejectReason, convert_cell, mysql_integer_pg_type,
};
use oo_core::decimal::{DecimalStorage, ExactDecimal};
use oo_core::text::{HexDigest, Label, SourceName, TableName};

fn policy() -> TypePolicy {
    TypePolicy {
        zero_date: ZeroDatePolicy::Null,
        tinyint1: Tinyint1Policy::Boolean,
        ..TypePolicy::default()
    }
}

fn decimal(raw: &RawCell) -> ExactDecimal {
    match convert_cell(raw, &policy(), "UTC", false).value.unwrap() {
        LogicalValue::Decimal(value) => value,
        other => panic!("expected decimal, got {other:?}"),
    }
}

#[test]
fn fixture_file_says_no_driver_was_used() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/fixtures/pg-mysql/cases.json"
    ))
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["driver"], "none");
    assert_eq!(value["shanghai_2026_10_01_091203_unix_seconds"], 1790817123);
}

#[test]
fn unsigned_numeric_and_scale_keep_exact_digits() {
    let max = decimal(&RawCell::MysqlInt {
        text: "18446744073709551615".into(),
        unsigned: true,
        kind: MysqlIntKind::Big,
    });
    assert_eq!(max.to_plain_string(), "18446744073709551615");
    assert_eq!(max.scale(), 0);
    assert_eq!(max.storage_class(), DecimalStorage::Decimal128);
    assert_eq!(
        mysql_integer_pg_type(MysqlIntKind::Big, true, &policy()),
        "numeric(20,0)"
    );

    let scaled = decimal(&RawCell::Decimal("123.4500".into()));
    assert_eq!(scaled.to_plain_string(), "123.4500");
    assert_eq!(scaled.scale(), 4);

    let wide = "9".repeat(40);
    let forty = decimal(&RawCell::Decimal(wide.clone()));
    assert_eq!(forty.coefficient(), wide);
    assert_eq!(forty.storage_class(), DecimalStorage::Decimal256);
    assert!(!forty.fits_decimal128());

    let huge = "9".repeat(80);
    let eighty = decimal(&RawCell::Decimal(huge.clone()));
    assert_eq!(eighty.coefficient(), huge);
    assert_eq!(eighty.storage_class(), DecimalStorage::DecimalString);

    let negative = convert_cell(
        &RawCell::MysqlInt {
            text: "-1".into(),
            unsigned: true,
            kind: MysqlIntKind::Int,
        },
        &policy(),
        "UTC",
        false,
    );
    assert!(negative.value.is_none());
    assert_eq!(negative.reject.unwrap().reason, RejectReason::OutOfRange);
}

#[test]
fn dates_timezones_and_encodings_reject_without_dropping_the_reason() {
    let rules = policy();
    let zero = convert_cell(&RawCell::Date("0000-00-00".into()), &rules, "UTC", false);
    assert!(zero.value.is_none());
    assert_eq!(zero.reject.unwrap().reason, RejectReason::ZeroDate);

    let invalid = convert_cell(&RawCell::Date("2026-02-31".into()), &rules, "UTC", false);
    assert_eq!(invalid.reject.unwrap().reason, RejectReason::InvalidDate);

    let gap = convert_cell(
        &RawCell::LocalTimestamp("2026-03-08 02:30:00".into()),
        &rules,
        "America/New_York",
        false,
    );
    assert_eq!(gap.reject.unwrap().reason, RejectReason::DstGap);
    let overlap = convert_cell(
        &RawCell::LocalTimestamp("2026-11-01 01:30:00".into()),
        &rules,
        "America/New_York",
        false,
    );
    assert_eq!(overlap.reject.unwrap().reason, RejectReason::DstOverlap);

    let shanghai = convert_cell(
        &RawCell::LocalTimestamp("2026-10-01 09:12:03".into()),
        &rules,
        "Asia/Shanghai",
        false,
    );
    assert!(shanghai.reject.is_none());
    assert_eq!(
        shanghai.value,
        Some(LogicalValue::TimestampTzMicros(1_790_817_123_000_000))
    );
    let other = convert_cell(
        &RawCell::LocalTimestamp("2026-10-01 09:12:03".into()),
        &rules,
        "America/Chicago",
        false,
    );
    assert!(other.reject.is_none(), "{:?}", other.reject);
    assert_eq!(
        other.value,
        Some(LogicalValue::TimestampTzMicros(1_790_863_923_000_000))
    );

    let nul = convert_cell(&RawCell::Text("a\0b".into()), &rules, "UTC", true);
    assert_eq!(nul.value, Some(LogicalValue::Text("ab".into())));
    let rejected = nul.reject.unwrap();
    assert_eq!(rejected.reason, RejectReason::NulRejected);
    assert_eq!(rejected.masked, "masked:3chars");

    let bytes = [0xff, 0xfe];
    let gbk = convert_cell(
        &RawCell::Bytes {
            data: bytes.to_vec(),
            charset: CharsetClaim::Gbk,
        },
        &rules,
        "UTC",
        false,
    );
    assert!(gbk.value.is_none());
    let gbk = gbk.reject.unwrap();
    assert_eq!(gbk.reason, RejectReason::EncodingNotInSp00Decoder);
    assert_eq!(gbk.raw_len, 2);
    assert_eq!(gbk.masked, "fffe");
    let gb18030 = convert_cell(
        &RawCell::Bytes {
            data: bytes.to_vec(),
            charset: CharsetClaim::Gb18030,
        },
        &rules,
        "UTC",
        true,
    );
    assert_eq!(gb18030.reject.unwrap().masked, "masked:2bytes");
    let bad_utf8 = convert_cell(
        &RawCell::Bytes {
            data: bytes.to_vec(),
            charset: CharsetClaim::Utf8,
        },
        &rules,
        "UTC",
        false,
    );
    assert_eq!(
        bad_utf8.reject.unwrap().reason,
        RejectReason::EncodingNotInSp00Decoder
    );
}

#[test]
fn floats_json_and_batch_rules() {
    let rules = policy();
    let nan = convert_cell(
        &RawCell::FloatBits(0x7ff8_0000_0000_0001),
        &rules,
        "UTC",
        false,
    );
    let nan2 = convert_cell(
        &RawCell::FloatBits(0xfff8_0000_0000_0001),
        &rules,
        "UTC",
        false,
    );
    assert_eq!(nan.value, nan2.value);
    let neg_zero = convert_cell(
        &RawCell::FloatBits(0x8000_0000_0000_0000),
        &rules,
        "UTC",
        false,
    );
    let pos_zero = convert_cell(&RawCell::FloatBits(0), &rules, "UTC", false);
    assert_eq!(neg_zero.value, pos_zero.value);

    let left = convert_cell(
        &RawCell::Json(r#"{"b":1,"a":true}"#.into()),
        &rules,
        "UTC",
        false,
    );
    let right = convert_cell(
        &RawCell::Json(r#"{"a":true,"b":1e0}"#.into()),
        &rules,
        "UTC",
        false,
    );
    let fields = [LogicalField {
        name: "doc".into(),
        logical_type: LogicalType::Json,
        nullable: true,
    }];
    let a = encode_value(
        TYPE_POLICY_V1,
        &fields,
        &[FieldValue::value(left.value.unwrap())],
    )
    .unwrap();
    let b = encode_value(
        TYPE_POLICY_V1,
        &fields,
        &[FieldValue::value(right.value.unwrap())],
    )
    .unwrap();
    assert_eq!(a, b);

    let meta = sample_batch(
        vec![TableName::new("amount").unwrap()],
        vec!["amount".into()],
    );
    assert!(
        validate_batch(&meta).is_empty(),
        "{:?}",
        validate_batch(&meta)
    );
    let collision = sample_batch(vec![TableName::new("_oo_op").unwrap()], vec![]);
    assert!(validate_batch(&collision).iter().any(|item| {
        let message = item.message.to_ascii_lowercase();
        message.contains("must")
            && message.contains("rename")
            && !item.message.contains("is renamed")
    }));
    let row = BatchRow {
        cells: vec![CellSlot::Null],
        op: 9,
        position: ExactDecimal::parse_sql(&format!("-{}", "9".repeat(39))).unwrap(),
        source_ts_us: Some(1),
        source_ts_unavailable: true,
        unavailable: vec![0],
    };
    let errors = validate_batch_row(&meta, &row);
    assert!(
        errors
            .iter()
            .any(|item| item.message.contains("op must be"))
    );
    assert!(
        errors
            .iter()
            .any(|item| item.message.contains("decimal128(38,0)"))
    );
    assert!(
        errors
            .iter()
            .any(|item| item.message.contains("unavailable is separate"))
    );
    assert!(
        errors
            .iter()
            .any(|item| item.message.contains("primary-key"))
    );
}

fn sample_batch(columns: Vec<TableName>, primary_key: Vec<String>) -> BatchMeta {
    BatchMeta {
        protocol_version: BatchVersion,
        source: SourceName::new("erp_mysql").unwrap(),
        table: TableName::new("kh").unwrap(),
        schema_fingerprint: HexDigest::new(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap(),
        batch_id: Label::new("batch-1").unwrap(),
        path: BatchPath::Cdc,
        file_mode: None,
        config_version: ConfigVersion,
        type_policy_version: TypePolicyVersion,
        hook_digest: HexDigest::new(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )
        .unwrap(),
        txn_id: Label::new("txn-1").unwrap(),
        txn_sequence: 1,
        epoch: 3,
        primary_key: primary_key
            .into_iter()
            .map(|item| TableName::new(item).unwrap())
            .collect(),
        columns: columns
            .into_iter()
            .map(|name| BatchColumn {
                name,
                arrow: ArrowTypeName::Utf8,
                nullable: false,
            })
            .collect(),
    }
}

#[test]
fn snapshot_full_and_upsert_follow_the_batch_rules() {
    let mut meta = sample_batch(vec![TableName::new("id").unwrap()], vec!["id".into()]);
    meta.path = BatchPath::Snapshot;
    meta.txn_sequence = 4;
    assert!(
        validate_batch(&meta)
            .iter()
            .any(|item| item.path == "/txn_sequence")
    );
    meta.txn_sequence = 0;
    assert!(validate_batch(&meta).is_empty());
    meta.path = BatchPath::Full;
    meta.primary_key.clear();
    assert!(validate_batch(&meta).is_empty(), "full may be a multiset");
    meta.path = BatchPath::File;
    meta.file_mode = Some(FileMode::Upsert);
    assert!(
        validate_batch(&meta)
            .iter()
            .any(|item| item.path == "/primary_key")
    );
    let early = ExactDecimal::parse_sql("-1").unwrap();
    assert!(!early.fits_position());
    let epoch = convert_cell(&RawCell::EpochMicros(-1_000_000), &policy(), "UTC", false);
    assert_eq!(
        epoch.value,
        Some(LogicalValue::TimestampTzMicros(-1_000_000))
    );
}
