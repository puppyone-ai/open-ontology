//! Regressions for the SP00-W02 / SP00-W03 freeze review.
//!
//! Expectations come from the spec and `contracts/canonical-value-v1.md`.
//! A review sample that called every negative decimal scale invalid is not
//! the rule this file encodes.

use oo_core::TYPE_POLICY_V1;
use oo_core::api::{
    ErrorEnvelope, Freshness, QueryCell, QueryColumn, SourceWatermark, parse_api_json,
};
use oo_core::batch::{
    BatchColumn, BatchMeta, BatchPath, BatchVersion, TypePolicyVersion, validate_batch,
};
use oo_core::canonical::{
    FieldValue, FloatClass, JsonNode, LogicalField, LogicalType, LogicalValue, encode_value,
    parse_json,
};
use oo_core::config::model::{FileMode, Tinyint1Policy, TypePolicy, ZeroDatePolicy};
use oo_core::config::parse_agent_yaml;
use oo_core::config::version::ConfigVersion;
use oo_core::convert::{RawCell, RejectReason, TimeDecision, convert_cell, decide_local_time};
use oo_core::decimal::FlexDecimal;
use oo_core::oos::{CivilDate, FactPin, merge_effective};
use oo_core::text::{HexDigest, HttpsUrl, Label, QuerySql, SourceName, TableName};
use serde::Deserialize;
use serde_json::json;

fn cjk(count: usize) -> String {
    "测".repeat(count)
}

fn no_extra<T>(_: &T) -> Vec<oo_core::Diagnostic> {
    Vec::new()
}

#[test]
fn sql_budget_is_utf8_bytes_not_scalars() {
    let ascii_ok = "a".repeat(65_536);
    let ascii_over = "a".repeat(65_537);
    assert!(serde_json::from_str::<QuerySql>(&serde_json::to_string(&ascii_ok).unwrap()).is_ok());
    assert!(
        serde_json::from_str::<QuerySql>(&serde_json::to_string(&ascii_over).unwrap()).is_err()
    );
    // 21,846 U+6D4B scalars are 65,538 UTF-8 bytes.
    let overflow = cjk(21_846);
    assert_eq!(overflow.len(), 65_538);
    assert!(
        serde_json::from_str::<QuerySql>(&serde_json::to_string(&overflow).unwrap()).is_err(),
        "SQL over 64KiB UTF-8 must be rejected"
    );
    let under = cjk(21_845);
    assert_eq!(under.len(), 65_535);
    assert!(serde_json::from_str::<QuerySql>(&serde_json::to_string(&under).unwrap()).is_ok());
}

#[test]
fn query_cell_budget_is_500_scalars() {
    let ok = "测".repeat(500);
    let over = "测".repeat(501);
    assert!(matches!(
        serde_json::from_str::<QueryCell>(&serde_json::to_string(&ok).unwrap()).unwrap(),
        QueryCell::Text(_)
    ));
    assert!(serde_json::from_str::<QueryCell>(&serde_json::to_string(&over).unwrap()).is_err());
}

#[test]
fn instructions_budget_is_utf8_bytes() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/config/agent.yaml"
    );
    let base = std::fs::read_to_string(path).unwrap();
    let marker = "本公司财年从 4 月开始。金额默认单位为元。\n  watch!";
    assert!(base.contains(marker), "agent example instructions changed");
    // A literal block keeps a trailing newline, so the scalar is `count + 1` chars.
    // 6,000 CJK scalars are under the 16,384 scalar cap and over 16KiB of UTF-8.
    let yaml = base.replace(marker, &cjk(6_000));
    assert_eq!(cjk(6_000).len(), 18_000);
    assert!(
        parse_agent_yaml(&yaml).is_err(),
        "6,000 CJK scalars are 18,000 bytes and exceed 16KiB"
    );
    let ascii = base.replace(marker, &"a".repeat(16_383));
    assert!(parse_agent_yaml(&ascii).is_ok());
}

#[test]
fn civil_date_rejects_impossible_gregorian_days() {
    assert!(CivilDate::new("2026-02-31").is_err());
    assert!(CivilDate::new("1900-02-29").is_err());
    assert!(CivilDate::new("0000-00-00").is_err());
    assert!(CivilDate::new("9999-99-99").is_err());
    assert!(CivilDate::new("2024-02-29").is_ok());
    assert!(CivilDate::new("2026-10-01").is_ok());
}

fn civil(
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    micros: u32,
) -> oo_core::tz::CivilLocal {
    oo_core::tz::CivilLocal {
        year,
        month,
        day,
        hour,
        minute,
        second,
        micros,
    }
}

#[test]
fn zone_conversion_matches_independent_instants() {
    // Instants below are the IANA civil-time offsets for these dates, also
    // reproduced by Python zoneinfo on 2026-10-01 (microseconds = unix seconds
    // × 1_000_000). They are not the output of this crate's encoder.
    assert_eq!(
        decide_local_time("Asia/Shanghai", civil(2026, 10, 1, 9, 12, 3, 0)),
        TimeDecision::Unambiguous(1_790_817_123_000_000)
    );
    assert_eq!(
        decide_local_time("Asia/Shanghai", civil(1991, 7, 1, 12, 0, 0, 0)),
        TimeDecision::Unambiguous(678_337_200_000_000)
    );
    assert_eq!(
        decide_local_time("UTC", civil(2026, 10, 1, 9, 12, 3, 0)),
        TimeDecision::Unambiguous(1_790_845_923_000_000)
    );
    assert_eq!(
        decide_local_time("America/New_York", civil(2026, 7, 1, 12, 0, 0, 0)),
        TimeDecision::Unambiguous(1_782_921_600_000_000)
    );
    assert_eq!(
        decide_local_time("America/Chicago", civil(2026, 7, 1, 12, 0, 0, 0)),
        TimeDecision::Unambiguous(1_782_925_200_000_000)
    );
    // 2026-10-01 is still CDT (UTC-5). Same wall clock as the UTC row above,
    // five hours later as a Unix instant.
    assert_eq!(
        decide_local_time("America/Chicago", civil(2026, 10, 1, 9, 12, 3, 0)),
        TimeDecision::Unambiguous(1_790_863_923_000_000)
    );
    assert_eq!(
        decide_local_time("America/New_York", civil(2026, 3, 8, 2, 0, 0, 0)),
        TimeDecision::Gap
    );
    assert_eq!(
        decide_local_time("America/New_York", civil(2026, 3, 8, 2, 59, 59, 0)),
        TimeDecision::Gap
    );
    // fold 0 is the earlier instant (EDT). fold 1 is the later instant (EST).
    // Python zoneinfo on 2026-10-01: 01:30 fold 0 unix 1793511000, fold 1 unix 1793514600.
    assert_eq!(
        decide_local_time("America/New_York", civil(2026, 11, 1, 1, 30, 0, 0)),
        TimeDecision::Overlap {
            fold0_micros: 1_793_511_000_000_000,
            fold1_micros: 1_793_514_600_000_000,
        }
    );
    assert_eq!(
        decide_local_time("America/New_York", civil(2026, 11, 1, 1, 30, 1, 0)),
        TimeDecision::Overlap {
            fold0_micros: 1_793_511_001_000_000,
            fold1_micros: 1_793_514_601_000_000,
        }
    );
    assert_eq!(
        decide_local_time("America/New_York", civil(2026, 11, 1, 1, 0, 0, 0)),
        TimeDecision::Overlap {
            fold0_micros: 1_793_509_200_000_000,
            fold1_micros: 1_793_512_800_000_000,
        }
    );
    assert_eq!(
        decide_local_time("America/New_York", civil(2026, 11, 1, 1, 59, 59, 0)),
        TimeDecision::Overlap {
            fold0_micros: 1_793_512_799_000_000,
            fold1_micros: 1_793_516_399_000_000,
        }
    );
    assert_eq!(
        decide_local_time("America/New_York", civil(2026, 11, 1, 2, 0, 0, 0)),
        TimeDecision::Unambiguous(1_793_516_400_000_000)
    );
    assert_eq!(
        decide_local_time("America/New_York", civil(2026, 3, 8, 3, 0, 0, 0)),
        TimeDecision::Unambiguous(1_772_953_200_000_000)
    );
    assert_eq!(
        decide_local_time("Asia/Shanghai", civil(2026, 10, 1, 9, 12, 3, 500_000)),
        TimeDecision::Unambiguous(1_790_817_123_500_000)
    );
    assert_eq!(
        decide_local_time("Not/AZone", civil(2026, 10, 1, 9, 12, 3, 0)),
        TimeDecision::UnknownZone
    );
    assert_eq!(
        decide_local_time("Asia/Shanghai", civil(2026, 2, 31, 0, 0, 0, 0)),
        TimeDecision::InvalidCivil
    );
}

#[test]
fn raw_zero_date_policy_does_not_accept_invalid_local_time() {
    let rules = TypePolicy {
        zero_date: ZeroDatePolicy::Raw,
        tinyint1: Tinyint1Policy::Boolean,
        ..TypePolicy::default()
    };
    let outcome = convert_cell(
        &RawCell::LocalTimestamp("2026-10-01 24:00:00".into()),
        &rules,
        "Asia/Shanghai",
        false,
    );
    assert!(outcome.value.is_none());
    assert_eq!(outcome.reject.unwrap().reason, RejectReason::InvalidDate);

    let fraction = convert_cell(
        &RawCell::LocalTimestamp("2026-10-01 09:12:03.5".into()),
        &rules,
        "Asia/Shanghai",
        false,
    );
    assert_eq!(
        fraction.value,
        Some(oo_core::canonical::LogicalValue::TimestampTzMicros(
            1_790_817_123_500_000
        ))
    );
    let padded = convert_cell(
        &RawCell::LocalTimestamp("2026-10-01 09:12:03.500000".into()),
        &rules,
        "Asia/Shanghai",
        false,
    );
    assert_eq!(padded.value, fraction.value);
    let too_fine = convert_cell(
        &RawCell::LocalTimestamp("2026-10-01 09:12:03.5000000".into()),
        &rules,
        "Asia/Shanghai",
        false,
    );
    assert!(too_fine.value.is_none());
    assert_eq!(too_fine.reject.unwrap().reason, RejectReason::Precision);

    let gap = convert_cell(
        &RawCell::LocalTimestamp("2026-03-08 02:30:00".into()),
        &rules,
        "America/New_York",
        false,
    );
    assert!(gap.value.is_none());
    assert_eq!(gap.reject.unwrap().reason, RejectReason::DstGap);
    let unknown = convert_cell(
        &RawCell::LocalTimestamp("2026-10-01 09:12:03".into()),
        &rules,
        "Not/AZone",
        false,
    );
    assert!(unknown.value.is_none());
    assert_eq!(
        unknown.reject.unwrap().reason,
        RejectReason::TimezoneUnknown
    );
}

#[test]
fn https_endpoint_rejects_userinfo_query_and_fragment() {
    assert!(HttpsUrl::new("https://s3.internal").is_ok());
    assert!(HttpsUrl::new("https://llm.internal/v1").is_ok());
    assert!(HttpsUrl::new("https://user:s3cret@h/x").is_err());
    assert!(HttpsUrl::new("https://example.com/x?token=s3cret").is_err());
    assert!(HttpsUrl::new("https://example.com/x#frag").is_err());
    let err = HttpsUrl::new("https://user:s3cret@h/x").unwrap_err();
    assert!(
        !err.contains("s3cret"),
        "diagnostics must not echo the secret"
    );
}

#[test]
fn retryable_must_match_the_error_code() {
    let bad = r#"{"error":{"code":"RATE_LIMITED","message":"slow","retryable":false}}"#;
    assert!(serde_json::from_str::<ErrorEnvelope>(bad).is_err());
    assert!(parse_api_json::<ErrorEnvelope>(bad, no_extra).is_err());
    let good = r#"{"error":{"code":"RATE_LIMITED","message":"slow","retryable":true}}"#;
    let envelope = serde_json::from_str::<ErrorEnvelope>(good).unwrap();
    assert!(envelope.consistent());
    assert!(parse_api_json::<ErrorEnvelope>(good, no_extra).is_ok());
}

#[test]
fn freshness_and_position_and_decimal_parameters_are_checked() {
    assert!(
        serde_json::from_str::<Freshness>(
            r#"{"table":"logistics.receipt","as_of":"not-a-date","lag_ms":0,"state":"live"}"#
        )
        .is_err()
    );
    assert!(serde_json::from_str::<Freshness>(
        r#"{"table":"logistics.receipt","as_of":"2026-10-01T01:12:03Z","lag_ms":0,"state":"live"}"#
    )
    .is_ok());
    assert!(serde_json::from_str::<SourceWatermark>(
        r#"{"source":"tms_pg","consistency":"transactional","cross_table_consistent":true,"applied_pos":"1.50"}"#
    )
    .is_err());
    assert!(serde_json::from_str::<SourceWatermark>(
        r#"{"source":"tms_pg","consistency":"transactional","cross_table_consistent":true,"applied_pos":"18446744073709551615"}"#
    )
    .is_ok());
    assert!(
        serde_json::from_str::<QueryColumn>(
            r#"{"name":"amount","type":"decimal128","precision":255,"scale":-1}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<QueryColumn>(
            r#"{"name":"amount","type":"decimal128","precision":10,"scale":-2}"#
        )
        .is_ok()
    );
    assert!(
        serde_json::from_str::<QueryColumn>(
            r#"{"name":"amount","type":"numeric","precision":3,"scale":5}"#
        )
        .is_ok()
    );
}

#[test]
fn flex_decimal_json_number_keeps_digits() {
    let number = FlexDecimal::deserialize(&mut serde_json::Deserializer::from_str(
        "18446744073709551617",
    ))
    .expect("exact integer");
    let text = FlexDecimal::deserialize(&mut serde_json::Deserializer::from_str(
        "\"18446744073709551617\"",
    ))
    .unwrap();
    assert_eq!(number.0.coefficient(), "18446744073709551617");
    assert_eq!(number.0, text.0);
    let small = FlexDecimal::deserialize(&mut serde_json::Deserializer::from_str("0.001")).unwrap();
    assert_eq!(small.0.coefficient(), "1");
    assert_eq!(small.0.scale(), 3);
}

#[test]
fn stale_pins_cover_only_real_overrides() {
    let generated = json!({"columns":[{"name":"amount","note":"up"}],"elsewhere":1});
    let curated = json!({"columns":[{"name":"amount","note":"curated"}]});
    let pin = |path: &str, generated_fp: &str| FactPin {
        path: path.to_owned(),
        generated_fingerprint: HexDigest::new(generated_fp).unwrap(),
        recorded_fingerprint: HexDigest::new("b".repeat(64)).unwrap(),
    };
    let unrelated = merge_effective(
        &generated,
        &curated,
        &[pin(
            "/elsewhere",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )],
    )
    .unwrap();
    assert!(unrelated.stale.is_empty());
    assert_eq!(unrelated.effective["columns"][0]["note"], "curated");
    let changed = merge_effective(
        &generated,
        &curated,
        &[pin(
            "/columns/amount/note",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )],
    )
    .unwrap();
    assert_eq!(changed.stale.len(), 1);
    assert_eq!(changed.effective["columns"][0]["note"], "curated");
    assert!(
        unrelated
            .missing_pins
            .iter()
            .any(|path| path == "/columns/amount/note"),
        "a real override without a pin is review-required: {:?}",
        unrelated.missing_pins
    );
    assert!(changed.missing_pins.is_empty());

    let boundary = merge_effective(
        &json!({"columns":[{"name":"amount","note":"up"},{"name":"amount_extra","note":"up"}]}),
        &json!({"columns":[{"name":"amount_extra","note":"curated"}]}),
        &[pin(
            "/columns/amount",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )],
    )
    .unwrap();
    assert!(boundary.stale.is_empty());
    assert!(
        boundary
            .missing_pins
            .iter()
            .any(|path| path == "/columns/amount_extra/note")
    );

    let escaped = merge_effective(
        &json!({"labels":{"a/b":"up","m~n":"up"}}),
        &json!({"labels":{"a/b":"curated"}}),
        &[pin(
            "/labels/a~1b",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )],
    )
    .unwrap();
    assert_eq!(escaped.stale.len(), 1);
    assert_eq!(escaped.stale[0].path, "/labels/a~1b");
    assert_eq!(escaped.effective["labels"]["a/b"], "curated");
    assert_eq!(escaped.effective["labels"]["m~n"], "up");
}

#[test]
fn system_column_collision_does_not_claim_a_rename() {
    let meta = BatchMeta {
        protocol_version: BatchVersion,
        source: SourceName::new("erp_mysql").unwrap(),
        table: TableName::new("kh").unwrap(),
        schema_fingerprint: HexDigest::new("a".repeat(64)).unwrap(),
        batch_id: Label::new("batch-1").unwrap(),
        path: BatchPath::Full,
        file_mode: None,
        config_version: ConfigVersion,
        type_policy_version: TypePolicyVersion,
        hook_digest: HexDigest::new("b".repeat(64)).unwrap(),
        txn_id: Label::new("txn-1").unwrap(),
        txn_sequence: 0,
        epoch: 1,
        primary_key: Vec::new(),
        columns: vec![BatchColumn {
            name: TableName::new("_oo_op").unwrap(),
            arrow: oo_core::arrow_meta::ArrowTypeName::Int8,
            nullable: false,
        }],
    };
    let errors = validate_batch(&meta);
    assert_eq!(meta.columns[0].name.as_str(), "_oo_op");
    let message = errors
        .iter()
        .map(|item| item.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(message.to_ascii_lowercase().contains("must"));
    assert!(message.to_ascii_lowercase().contains("rename"));
    assert!(
        !message.contains("is renamed"),
        "the validator must not claim it already renamed the column: {message}"
    );
    let _ = FileMode::Upsert;
}

#[test]
fn canonical_json_input_is_capped_before_parse() {
    let mut raw = " ".repeat(1_048_577);
    raw.push('0');
    assert_eq!(raw.len(), 1_048_578);
    assert!(parse_json(&raw).is_err());
}

#[test]
fn utf16_surrogate_pair_is_one_scalar() {
    let escaped = parse_json(r#""\uD83D\uDE00""#).unwrap();
    let raw = parse_json("\"😀\"").unwrap();
    assert_eq!(escaped, JsonNode::String("😀".into()));
    assert_eq!(escaped, raw);
    assert!(parse_json(r#""\uD800""#).is_err());
    assert!(parse_json(r#""\uDE00\uD83D""#).is_err());
}

#[test]
fn hand_built_float_and_json_use_the_same_normalization() {
    let fields = [LogicalField {
        name: "v".into(),
        logical_type: LogicalType::Float64,
        nullable: true,
    }];
    let finite_neg_zero = encode_value(
        TYPE_POLICY_V1,
        &fields,
        &[FieldValue::value(LogicalValue::Float(FloatClass::Finite(
            (-0.0f64).to_bits(),
        )))],
    )
    .unwrap();
    let zero = encode_value(
        TYPE_POLICY_V1,
        &fields,
        &[FieldValue::value(LogicalValue::Float(FloatClass::Zero))],
    )
    .unwrap();
    assert_eq!(finite_neg_zero, zero);
    let nan_bits = encode_value(
        TYPE_POLICY_V1,
        &fields,
        &[FieldValue::value(LogicalValue::Float(FloatClass::Finite(
            f64::NAN.to_bits(),
        )))],
    )
    .unwrap();
    let nan = encode_value(
        TYPE_POLICY_V1,
        &fields,
        &[FieldValue::value(LogicalValue::Float(FloatClass::Nan))],
    )
    .unwrap();
    assert_eq!(nan_bits, nan);

    let json_fields = [LogicalField {
        name: "doc".into(),
        logical_type: LogicalType::Json,
        nullable: true,
    }];
    let unsorted = JsonNode::Object(vec![
        (
            "b".into(),
            JsonNode::Number(oo_core::ExactDecimal::parse_sql("1").unwrap()),
        ),
        ("a".into(), JsonNode::Bool(true)),
    ]);
    let sorted = JsonNode::Object(vec![
        ("a".into(), JsonNode::Bool(true)),
        (
            "b".into(),
            JsonNode::Number(oo_core::ExactDecimal::parse_sql("1").unwrap()),
        ),
    ]);
    let left = encode_value(
        TYPE_POLICY_V1,
        &json_fields,
        &[FieldValue::value(LogicalValue::Json(unsorted))],
    )
    .unwrap();
    let right = encode_value(
        TYPE_POLICY_V1,
        &json_fields,
        &[FieldValue::value(LogicalValue::Json(sorted))],
    )
    .unwrap();
    assert_eq!(left, right);
    let dup = JsonNode::Object(vec![
        ("a".into(), JsonNode::Bool(true)),
        ("a".into(), JsonNode::Bool(false)),
    ]);
    assert!(
        encode_value(
            TYPE_POLICY_V1,
            &json_fields,
            &[FieldValue::value(LogicalValue::Json(dup))]
        )
        .is_err()
    );
    let mut deep = JsonNode::Null;
    for _ in 0..40 {
        deep = JsonNode::Array(vec![deep]);
    }
    assert!(
        encode_value(
            TYPE_POLICY_V1,
            &json_fields,
            &[FieldValue::value(LogicalValue::Json(deep))]
        )
        .is_err()
    );
}

#[test]
fn schema_max_length_is_not_the_utf8_byte_budget() {
    let cjk = cjk(21_846);
    assert_eq!(cjk.chars().count(), 21_846);
    assert_eq!(cjk.len(), 65_538);
    let ceiling = json!({"type": "string", "maxLength": 65536});
    let ceiling_only = jsonschema::draft202012::options()
        .offline()
        .build(&ceiling)
        .unwrap();
    assert!(
        ceiling_only.is_valid(&json!(cjk)),
        "maxLength counts scalars and accepts 21,846 CJK scalars"
    );
    let both = json!({
        "type": "string",
        "maxLength": 65536,
        "x-utf8-max-bytes": 65536
    });
    let checked = oo_core::schema_util::compile_schema(&both).unwrap();
    assert!(!checked.is_valid(&json!(cjk)));
    assert!(serde_json::from_str::<QuerySql>(&serde_json::to_string(&cjk).unwrap()).is_err());
    let ascii_ok = "a".repeat(65_536);
    let ascii_over = "a".repeat(65_537);
    assert!(checked.is_valid(&json!(ascii_ok)));
    assert!(ceiling_only.is_valid(&json!(ascii_ok)));
    assert!(!checked.is_valid(&json!(ascii_over)));
    assert!(!ceiling_only.is_valid(&json!(ascii_over)));

    let date = json!({"type": "string", "format": "date"});
    let formats = jsonschema::draft202012::options()
        .offline()
        .should_validate_formats(true)
        .build(&date)
        .unwrap();
    assert!(
        formats.is_valid(&json!("0000-02-29")),
        "jsonschema format date treats year 0 as a leap year"
    );
    assert!(CivilDate::new("0000-02-29").is_err());
    assert!(formats.is_valid(&json!("2026-10-01")));
    assert!(CivilDate::new("2026-02-31").is_err());
}

#[test]
fn yaml_and_json_numbers_stay_lexical() {
    let yaml = oo_core::parse_yaml_value(
        "n: 18446744073709551617\nr: .001\nq: \"0.001\"\n",
        "oo.config/v1",
    )
    .unwrap();
    let number = match &yaml["n"] {
        serde_json::Value::Number(number) => number.as_str(),
        other => panic!("expected a JSON number, got {other}"),
    };
    assert_eq!(number, "18446744073709551617");
    let ratio = match &yaml["r"] {
        serde_json::Value::Number(number) => number.as_str(),
        other => panic!("expected a JSON number, got {other}"),
    };
    assert_eq!(ratio, ".001");
    assert_eq!(yaml["q"], json!("0.001"));
    assert!(oo_core::parse_yaml_value("n: .inf\n", "oo.config/v1").is_err());
    assert!(oo_core::parse_yaml_value("n: 01\n", "oo.config/v1").is_err());
    assert!(oo_core::ExactDecimal::parse_json_number("1e2147483647").is_err());
    assert!(parse_json("1e2147483647").is_err());
    let pair = oo_core::parse_json_value(r#""\uD83D\uDE00""#, "oo.api/v1").unwrap();
    assert_eq!(pair, json!("😀"));
    assert!(oo_core::parse_json_value(r#""\uD800""#, "oo.api/v1").is_err());

    let wide = oo_core::ExactDecimal::new(false, "9".repeat(38), -38).unwrap();
    assert!(wide.fits_decimal128());
    assert!(
        !oo_core::ExactDecimal::new(false, "9".repeat(38), -39)
            .unwrap()
            .fits_decimal128()
    );
    assert!(
        !oo_core::ExactDecimal::new(false, "9".repeat(39), 0)
            .unwrap()
            .fits_decimal128()
    );
    assert!(
        oo_core::ExactDecimal::new(false, "9".repeat(76), -76)
            .unwrap()
            .fits_decimal256()
    );
    assert!(
        !oo_core::ExactDecimal::new(false, "9".repeat(76), -77)
            .unwrap()
            .fits_decimal256()
    );
    let spelled = oo_core::arrow_meta::ArrowTypeName::decimal128(10, -2)
        .unwrap()
        .series60()
        .unwrap();
    assert_eq!(spelled, "decimal128(10,-2)");
    assert!(oo_core::arrow_meta::ArrowTypeName::decimal128(39, -1).is_err());
}

#[test]
fn https_path_credentials_are_rejected_without_echoing_them() {
    let err = HttpsUrl::new("https://s3.internal/password=s3cret").unwrap_err();
    assert!(!err.contains("s3cret"));
    assert!(HttpsUrl::new("https://s3.internal?x=1").is_err());
    assert!(HttpsUrl::new("https://[2001:db8::1]/v1").is_ok());
}

#[test]
fn canonical_node_budget_stops_before_a_huge_object_is_kept() {
    let mut raw = String::from("{");
    for index in 0..60_000 {
        if index > 0 {
            raw.push(',');
        }
        raw.push_str(&format!("\"k{index}\":0"));
    }
    raw.push('}');
    assert!(parse_json(&raw).is_err());
    let handmade: Vec<_> = (0..70_000)
        .map(|index| (format!("k{index}"), JsonNode::Null))
        .collect();
    let fields = [LogicalField {
        name: "doc".into(),
        logical_type: LogicalType::Json,
        nullable: true,
    }];
    assert!(
        encode_value(
            TYPE_POLICY_V1,
            &fields,
            &[FieldValue::value(LogicalValue::Json(JsonNode::Object(
                handmade
            )))]
        )
        .is_err()
    );
}
