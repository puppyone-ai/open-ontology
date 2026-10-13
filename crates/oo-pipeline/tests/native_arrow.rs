//! Native Arrow physical import against the frozen Oracle P2 stream.
//! Admission files stay outside this crate. Set `OO_NATIVE_RECEIPT` and
//! `OO_NATIVE_DECISION` when the evidence tree is not beside the crate.

use std::path::{Path, PathBuf};

use oo_pipeline::native_arrow::{
    arrow_sha256, copy_digest, decode_arrow_stream, plan_with_operator, ArrowFault, CopyColumnDecl,
    NativeCell, OperatorEnv, ADMISSION_BIN_SHA256, ADMISSION_BIN_SHA256_3C55,
    ADMISSION_BIN_SHA256_E465, IMPORT_PROTOCOL,
    MYSQL_MANIFEST_PROTOCOL, approved_admission_bin,
    ORACLE_P2_BATCH_SHA256, RESULT_PROTOCOL, SEMANTIC_CATALOG_PROTOCOL,
};

fn plan_closed(document: &Value) -> oo_pipeline::native_arrow::Plan {
    plan_with_operator(document, OperatorEnv::Absent)
}

fn plan_fixed(
    document: &Value,
    bin: Option<&str>,
    registry: Option<&str>,
    registry_sha256: Option<&str>,
) -> oo_pipeline::native_arrow::Plan {
    plan_with_operator(
        document,
        OperatorEnv::Fixed {
            bin: bin.map(str::to_string),
            registry: registry.map(str::to_string),
            registry_sha256: registry_sha256.map(str::to_string),
        },
    )
}
use serde_json::{json, Value};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native/oracle-p2-scn2175138.arrow")
}

fn receipt_path() -> PathBuf {
    std::env::var_os("OO_NATIVE_RECEIPT").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "../../evidence/2ecaf08e-eb23-4b7a-a720-e07459e6f2a6/DL01/budget-report/native-readonly-completion/receipt.json",
        )
    })
}

fn decision_path() -> PathBuf {
    std::env::var_os("OO_NATIVE_DECISION").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "../../evidence/2ecaf08e-eb23-4b7a-a720-e07459e6f2a6/DL01/budget-report/native-readonly-completion/linux/replay/clickhouse-attempt06.stdout",
        )
    })
}

fn column(
    name: &str,
    logical: &str,
    arrow_type: &str,
    nullable: bool,
    precision: Option<i32>,
    scale: Option<i32>,
    char_length: Option<i32>,
) -> CopyColumnDecl {
    CopyColumnDecl {
        name: name.to_string(),
        logical_name: logical.to_string(),
        arrow_type: arrow_type.to_string(),
        nullable,
        precision,
        scale,
        char_length,
        document: false,
    }
}

fn oracle_columns() -> Vec<CopyColumnDecl> {
    vec![
        column("ID", "id", "int16", false, None, None, None),
        column("N16", "n16", "decimal128", true, Some(16), Some(6), None),
        column("N1", "n1", "int8", true, None, None, None),
        column("D", "d", "timestamp_micros", true, None, None, None),
        column("C6", "c6", "utf8", true, None, None, Some(6)),
        column("V2", "v2", "utf8", true, None, None, None),
        column("R", "r", "binary", true, None, None, None),
    ]
}

fn require_file(path: &Path) {
    assert!(path.is_file(), "missing {}", path.display());
}

fn hex_digest() -> String {
    "ab".repeat(32)
}

fn base_manifest(engine: &str, source_id: &str) -> Value {
    json!({
        "protocol": IMPORT_PROTOCOL,
        "source_engine": engine,
        "target_storage": "postgresql",
        "source_id": source_id,
        "source_generation": 1,
        "snapshot": {"kind": "scn", "value": "2175138"},
        "catalog_identity": {
            "config_digest": hex_digest(),
            "schema_digest": hex_digest(),
            "type_digest": hex_digest(),
            "input_digest": hex_digest()
        },
        "admission": {
            "receipt_path": receipt_path(),
            "decision_path": decision_path()
        }
    })
}

fn assert_closed(result: &Value, reason: &str) {
    assert_eq!(result["protocol"], RESULT_PROTOCOL);
    assert_eq!(result["load_state"], "not_sealed");
    assert_eq!(result["reason"], reason);
    assert_eq!(result["loader_invoked"], false);
    assert!(result["run_id"].is_null());
    let text = result.to_string();
    assert!(!text.contains("policy_approved"));
    assert!(!text.contains("secret://"));
}

fn utf8(cell: &NativeCell) -> &str {
    match cell {
        NativeCell::Utf8(text) => text,
        other => panic!("expected text, got {other:?}"),
    }
}

fn int_cell(cell: &NativeCell) -> (u8, i64) {
    match cell {
        NativeCell::Int { bit_width, value } => (*bit_width, *value),
        other => panic!("expected int, got {other:?}"),
    }
}

#[test]
fn oracle_stream_decodes_five_rows() {
    let bytes = std::fs::read(fixture()).expect("arrow");
    assert_eq!(bytes.len(), 2120);
    let stream = decode_arrow_stream(&bytes).expect("decode");
    assert_eq!(stream.rows, 5);
    assert_eq!(stream.fields.len(), 7);
    assert_eq!(stream.fields[0].name, "ID");
    assert!(!stream.fields[0].nullable);
    assert_eq!(stream.fields[0].arrow_type, "int16");
    assert_eq!(stream.fields[1].arrow_type, "decimal128");
    assert_eq!(stream.fields[1].precision, Some(16));
    assert_eq!(stream.fields[1].scale, Some(6));
    assert!(stream.fields[1].nullable);
    assert_eq!(stream.fields[2].arrow_type, "int8");
    assert_eq!(stream.fields[3].arrow_type, "timestamp_micros");
    assert!(stream.fields[3].timezone.is_none());
    assert_eq!(stream.fields[4].arrow_type, "utf8");
    assert_eq!(stream.fields[5].arrow_type, "utf8");
    assert_eq!(stream.fields[6].arrow_type, "binary");

    let ids: Vec<i64> = stream.columns[0].iter().map(|cell| int_cell(cell).1).collect();
    assert_eq!(ids, vec![41, 42, 43, 44, 45]);
    assert!(stream.columns[0].iter().all(|cell| int_cell(cell).0 == 16));

    match &stream.columns[1][0] {
        NativeCell::Decimal { precision, scale, coefficient } => {
            assert_eq!(*precision, 16);
            assert_eq!(*scale, 6);
            assert_eq!(*coefficient, 3_141_592_653_589_793);
        }
        other => panic!("{other:?}"),
    }
    match &stream.columns[1][1] {
        NativeCell::Decimal { scale, coefficient, .. } => {
            assert_eq!(*scale, 6);
            assert_eq!(*coefficient, -1);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(stream.columns[1][2], NativeCell::Null);
    match &stream.columns[1][3] {
        NativeCell::Decimal { coefficient, .. } => assert_eq!(*coefficient, 9_999_999_999_999_999),
        other => panic!("{other:?}"),
    }
    match &stream.columns[1][4] {
        NativeCell::Decimal { coefficient, scale, .. } => {
            assert_eq!(*coefficient, 0);
            assert_eq!(*scale, 6);
        }
        other => panic!("{other:?}"),
    }

    assert_eq!(int_cell(&stream.columns[2][0]), (8, 0));
    assert_eq!(int_cell(&stream.columns[2][1]), (8, 1));
    assert_eq!(stream.columns[2][2], NativeCell::Null);
    assert_eq!(int_cell(&stream.columns[2][3]), (8, 0));
    assert_eq!(int_cell(&stream.columns[2][4]), (8, 1));
    assert!(!matches!(stream.columns[2][0], NativeCell::Bool(_)));

    let stamps = [
        1_791_616_302_000_000i64,
        1_791_590_400_000_000,
        1_791_676_799_000_000,
        1_791_633_601_000_000,
        1_835_462_707_000_000,
    ];
    for (cell, expected) in stream.columns[3].iter().zip(stamps) {
        assert_eq!(cell, &NativeCell::TimestampMicros(expected));
    }

    assert_eq!(utf8(&stream.columns[4][0]), "ZQ    ");
    assert_eq!(utf8(&stream.columns[4][1]), "      ");
    assert_eq!(utf8(&stream.columns[4][2]), "Z     ");
    assert_eq!(utf8(&stream.columns[4][3]), "AB C  ");
    assert_eq!(utf8(&stream.columns[4][4]), "n     ");
    assert_eq!(utf8(&stream.columns[5][0]), "alpha");
    assert_eq!(stream.columns[5][1], NativeCell::Null);
    assert_eq!(stream.columns[5][2], NativeCell::Null);
    assert_eq!(utf8(&stream.columns[5][3]), " ");
    assert_eq!(utf8(&stream.columns[5][4]), "x");
    assert_eq!(stream.columns[6][0], NativeCell::Binary(vec![0x00, 0x01, 0xff, 0x00]));
    assert_eq!(stream.columns[6][1], NativeCell::Null);
    assert_eq!(stream.columns[6][2], NativeCell::Binary(vec![0x00]));
    assert_eq!(stream.columns[6][3], NativeCell::Binary(vec![0xde, 0xad, 0x00, 0xbe]));
    assert_eq!(stream.columns[6][4], NativeCell::Binary(vec![0x00, 0x00, 0x00, 0x00]));
}

#[test]
fn copy_digest_is_deterministic() {
    let bytes = std::fs::read(fixture()).expect("arrow");
    let first = copy_digest(&bytes, "oracle_p2", "oo04_fix", "t_batch", 1, &oracle_columns()).expect("copy");
    let second = copy_digest(&bytes, "oracle_p2", "oo04_fix", "t_batch", 1, &oracle_columns()).expect("copy");
    assert_eq!(first, second);
    assert_eq!(
        first,
        "ab6e9bbec7a41944e96295d2027eff885d816e7131b11af170328a46de432124"
    );
    assert_eq!(first.len(), 64);
    assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
}

#[test]
fn wrong_scale_is_precision() {
    let bytes = std::fs::read(fixture()).expect("arrow");
    let mut columns = oracle_columns();
    columns[1].scale = Some(5);
    let err = copy_digest(&bytes, "oracle_p2", "oo04_fix", "t_batch", 1, &columns).expect_err("scale");
    assert_eq!(err, ArrowFault::Precision);
}

#[test]
fn truncated_and_empty_streams_fail_closed() {
    assert_eq!(decode_arrow_stream(&[]).expect_err("empty"), ArrowFault::Empty);
    let bytes = std::fs::read(fixture()).expect("arrow");
    assert_eq!(
        decode_arrow_stream(&bytes[..bytes.len() - 1]).expect_err("cut"),
        ArrowFault::Truncated
    );
    let mut extra = bytes.clone();
    extra.push(0);
    assert_eq!(decode_arrow_stream(&extra).expect_err("tail"), ArrowFault::Truncated);
}

#[test]
fn semantic_catalog_is_not_a_physical_import() {
    let document = json!({
        "protocol": SEMANTIC_CATALOG_PROTOCOL,
        "source_engine": "clickhouse",
        "tables": [{"logical_table": "events"}]
    });
    let plan = plan_closed(&document);
    assert!(!plan.loader_ready());
    assert_closed(&plan.result, "semantic_catalog");
}

#[test]
fn mysql_manifest_stays_on_its_entrypoint() {
    let document = json!({
        "protocol": MYSQL_MANIFEST_PROTOCOL,
        "source_engine": "mysql"
    });
    let plan = plan_closed(&document);
    assert!(!plan.loader_ready());
    assert_closed(&plan.result, "mysql_entrypoint");
    assert!(!plan.result.to_string().contains("the manifest protocol is not the sealed import manifest"));
}

#[test]
fn postgresql_and_unknown_engines_fail_closed() {
    let mut postgres = base_manifest("postgresql", "oracle_p2");
    postgres["admission"] = Value::Null;
    let plan = plan_closed(&postgres);
    assert_closed(&plan.result, "postgresql_retag");
    assert!(!plan.loader_ready());

    let hive = base_manifest("hive", "oracle_p2");
    let plan = plan_closed(&hive);
    assert_closed(&plan.result, "source_engine_unknown");
    assert!(!plan.loader_ready());
}

#[test]
fn oracle_batch_is_not_admitted() {
    require_file(&receipt_path());
    require_file(&decision_path());
    let document = base_manifest("oracle", "oracle_p2");
    let plan = plan_closed(&document);
    assert!(!plan.loader_ready());
    assert_closed(&plan.result, "nonadmitted");
    let bytes = std::fs::read(fixture()).expect("arrow");
    let stream = decode_arrow_stream(&bytes).expect("decode");
    assert_eq!(stream.rows, 5);
    assert_eq!(arrow_sha256(&bytes), ORACLE_P2_BATCH_SHA256);
}

#[test]
fn clickhouse_without_arrow_is_missing() {
    require_file(&receipt_path());
    require_file(&decision_path());
    let mut document = base_manifest("clickhouse", "clickhouse_268");
    document["tables"] = json!([{
        "logical_schema": "oo04_fix",
        "logical_table": "t_batch",
        "row_count": 5,
        "batch_sha256": ORACLE_P2_BATCH_SHA256,
        "arrow_path": fixture().with_extension("missing"),
        "columns": []
    }]);
    let plan = plan_closed(&document);
    assert!(!plan.loader_ready());
    assert_closed(&plan.result, "arrow_missing");
    assert_eq!(plan.result["admission"]["batch_digests_bound"], false);
}

#[test]
fn clickhouse_admission_does_not_cover_the_oracle_batch() {
    require_file(&receipt_path());
    require_file(&decision_path());
    let mut document = base_manifest("clickhouse", "clickhouse_268");
    document["tables"] = json!([{
        "logical_schema": "oo04_fix",
        "logical_table": "t_batch",
        "row_count": 5,
        "batch_sha256": ORACLE_P2_BATCH_SHA256,
        "arrow_path": fixture(),
        "columns": []
    }]);
    let plan = plan_closed(&document);
    assert!(!plan.loader_ready());
    assert_closed(&plan.result, "digest_mismatch");
    assert_eq!(plan.result["admission"]["admitted_source_id"], "clickhouse_268");
    assert_eq!(plan.result["admission"]["batch_digests_bound"], false);
}

#[test]
fn synthetic_decision_is_not_admitted() {
    require_file(&receipt_path());
    let dir = std::env::temp_dir().join(format!("oo-native-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tmpdir");
    let decision = dir.join("decision.json");
    std::fs::write(
        &decision,
        br#"{"protocol":"oo.source-admission.decision/v1","admitted":true,"identity":{"source_id":"clickhouse_268"}}"#,
    )
    .expect("write");
    let mut document = base_manifest("clickhouse", "clickhouse_268");
    document["admission"]["decision_path"] = json!(decision);
    let plan = plan_closed(&document);
    assert!(!plan.loader_ready());
    assert_closed(&plan.result, "nonadmitted");
    let _ = std::fs::remove_dir_all(&dir);
}

fn clickhouse_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native/clickhouse-attempt06.arrow")
}

fn clickhouse_columns() -> Vec<CopyColumnDecl> {
    vec![
        column("reading_id", "reading_id", "uint64", false, Some(64), None, None),
        column("version", "version", "uint64", false, Some(64), None, None),
        column("amount", "amount", "decimal256", false, Some(76), Some(10), None),
        column("note", "note", "binary", true, None, None, None),
        column("code", "code", "dictionary_binary", false, None, None, None),
        column("code_or_null", "code_or_null", "dictionary_binary", true, None, None, None),
        column("ts", "ts", "timestamp_nanos_utc", false, None, None, None),
        column("tags", "tags", "list_int64", false, None, None, None),
        column("labels", "labels", "list_binary", false, None, None, None),
        column("qty", "qty", "int64", false, Some(64), None, None),
        column("label", "label", "binary", false, None, None, None),
        column("wide_u64", "wide_u64", "uint64", false, Some(64), None, None),
    ]
}

fn plain_decimal256(cell: &NativeCell) -> String {
    let NativeCell::Decimal256 { scale, coefficient, .. } = cell else {
        panic!("expected decimal256");
    };
    let negative = coefficient[31] & 0x80 != 0;
    let mut bytes = *coefficient;
    if negative {
        let mut carry = 1u16;
        for byte in bytes.iter_mut() {
            let wide = u16::from(!*byte) + carry;
            *byte = wide as u8;
            carry = wide >> 8;
        }
    }
    let mut digits = Vec::new();
    if bytes.iter().all(|byte| *byte == 0) {
        digits.push(b'0');
    } else {
        while bytes.iter().any(|byte| *byte != 0) {
            let mut remainder = 0u32;
            for byte in bytes.iter_mut().rev() {
                let current = (remainder << 8) | u32::from(*byte);
                *byte = (current / 10) as u8;
                remainder = current % 10;
            }
            digits.push(b'0' + remainder as u8);
        }
        digits.reverse();
    }
    let text = String::from_utf8(digits).expect("digits");
    let scale = usize::try_from(*scale).unwrap_or(0);
    let body = if scale == 0 {
        text.clone()
    } else if text.len() > scale {
        let split = text.len() - scale;
        format!("{}.{}", &text[..split], &text[split..])
    } else {
        format!("0.{:0>width$}", text, width = scale)
    };
    if negative && text != "0" { format!("-{body}") } else { body }
}

fn json_cell(cell: &NativeCell) -> &str {
    match cell {
        NativeCell::Json(text) => text,
        other => panic!("expected json, got {other:?}"),
    }
}

fn binary_cell(cell: &NativeCell) -> &[u8] {
    match cell {
        NativeCell::Binary(bytes) => bytes,
        other => panic!("expected binary, got {other:?}"),
    }
}

#[test]
fn clickhouse_stream_decodes_three_rows() {
    let bytes = std::fs::read(clickhouse_fixture()).expect("arrow");
    assert_eq!(bytes.len(), 4624);
    assert_eq!(
        arrow_sha256(&bytes),
        "3e32e4f95456b2285bdaf9aa2988578635d97cabe5e83d07319c574f272fcf8b"
    );
    let stream = decode_arrow_stream(&bytes).expect("decode");
    assert_eq!(stream.rows, 3);
    assert_eq!(stream.fields.len(), 12);
    assert_eq!(stream.fields[0].arrow_type, "uint64");
    assert_eq!(stream.fields[2].arrow_type, "decimal256");
    assert_eq!(stream.fields[2].precision, Some(76));
    assert_eq!(stream.fields[2].scale, Some(10));
    assert_eq!(stream.fields[4].arrow_type, "dictionary_binary");
    assert_eq!(stream.fields[6].arrow_type, "timestamp_nanos_utc");
    assert_eq!(stream.fields[6].timezone.as_deref(), Some("UTC"));
    assert_eq!(stream.fields[7].arrow_type, "list_int64");
    assert_eq!(stream.fields[8].arrow_type, "list_binary");
    let ids: Vec<u64> = stream.columns[0].iter().map(|cell| match cell {
        NativeCell::UInt64(value) => *value,
        other => panic!("{other:?}"),
    }).collect();
    assert_eq!(ids, vec![101, 102, 103]);
    assert!(stream.columns[0].iter().all(|cell| !matches!(cell, NativeCell::Int { .. })));
    match &stream.columns[1][0] {
        NativeCell::UInt64(value) => assert_eq!(*value, 2),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        plain_decimal256(&stream.columns[2][0]),
        "1234567890123456789012345678901234567890.1234567890"
    );
    assert_eq!(plain_decimal256(&stream.columns[2][1]), "0.0000000000");
    assert_eq!(plain_decimal256(&stream.columns[2][2]), "-42.5000000000");
    assert_eq!(stream.columns[3][0], NativeCell::Null);
    assert_eq!(binary_cell(&stream.columns[3][1]), b"");
    assert_eq!(binary_cell(&stream.columns[3][2]), b"present");
    assert_eq!(binary_cell(&stream.columns[4][0]), b"alpha");
    assert_eq!(binary_cell(&stream.columns[4][1]), b"alpha");
    assert_eq!(binary_cell(&stream.columns[4][2]), b"gamma");
    assert_eq!(stream.columns[5][0], NativeCell::Null);
    assert_eq!(binary_cell(&stream.columns[5][1]), b"beta");
    assert_eq!(binary_cell(&stream.columns[5][2]), b"");
    let stamps = [1_710_506_096_123_456_789i64, 1_704_153_600_000_000_000, 1_706_745_599_999_999_999];
    for (cell, expected) in stream.columns[6].iter().zip(stamps) {
        match cell {
            NativeCell::TimestampNanos { value, timezone } => {
                assert_eq!(*value, expected);
                assert_eq!(timezone, "UTC");
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(json_cell(&stream.columns[7][0]), "[1,-2,0]");
    assert_eq!(json_cell(&stream.columns[7][1]), "[]");
    assert_eq!(json_cell(&stream.columns[7][2]), "[9223372036854775807,-9223372036854775808]");
    assert_eq!(json_cell(&stream.columns[8][0]), "[\"alpha\",null,\"\"]");
    assert_eq!(json_cell(&stream.columns[8][1]), "[null]");
    assert_eq!(json_cell(&stream.columns[8][2]), "[]");
    assert_eq!(int_cell(&stream.columns[9][0]), (64, 7));
    assert_eq!(int_cell(&stream.columns[9][1]), (64, 0));
    assert_eq!(binary_cell(&stream.columns[10][0]), b"kept");
    assert_eq!(binary_cell(&stream.columns[10][1]), b"");
    match &stream.columns[11][0] {
        NativeCell::UInt64(value) => assert_eq!(*value, u64::MAX),
        other => panic!("{other:?}"),
    }
    match &stream.columns[11][2] {
        NativeCell::UInt64(value) => assert_eq!(*value, 42),
        other => panic!("{other:?}"),
    }
}

#[test]
fn clickhouse_copy_digest_encodes_without_dropping_columns() {
    let bytes = std::fs::read(clickhouse_fixture()).expect("arrow");
    let first = copy_digest(&bytes, "clickhouse_268", "readings", "final", 1, &clickhouse_columns()).expect("copy");
    let second = copy_digest(&bytes, "clickhouse_268", "readings", "final", 1, &clickhouse_columns()).expect("copy");
    assert_eq!(first, second);
    assert_eq!(
        first,
        "0fb023c3168e9ae55a0a4ab6f6d87ddbf8533253f4f6349afe48a21b77a69f8c"
    );
}

#[test]
fn registry_digest_mismatch_is_nonadmitted_before_a_missing_arrow() {
    let dir = std::env::temp_dir().join(format!("oo-native-reg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let registry = dir.join("registry.json");
    std::fs::write(&registry, b"{}\n").expect("write");
    let mut document = base_manifest("clickhouse", "clickhouse_268");
    document["tables"] = json!([{
        "logical_schema": "readings",
        "logical_table": "final",
        "row_count": 3,
        "batch_sha256": "3e32e4f95456b2285bdaf9aa2988578635d97cabe5e83d07319c574f272fcf8b",
        "arrow_path": dir.join("missing.arrow"),
        "columns": []
    }]);
    let sha = "ab".repeat(32);
    let plan = plan_fixed(
        &document,
        Some("/tmp/oo-native-missing-bin"),
        registry.to_str(),
        Some(sha.as_str()),
    );
    assert_closed(&plan.result, "nonadmitted");
    assert_ne!(plan.result["reason"], "arrow_missing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn manifest_cannot_select_a_registry_or_executable() {
    let dir = std::env::temp_dir().join(format!("oo-native-select-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let marker = dir.join("ran");
    let binary = dir.join("chosen.sh");
    std::fs::write(&binary, format!("#!/bin/sh\ntouch {}\n", marker.display())).expect("script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&binary).expect("meta").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&binary, permissions).expect("chmod");
    }
    let mut document = base_manifest("clickhouse", "clickhouse_268");
    document["registry_path"] = json!("/tmp/client-picked-registry.json");
    document["admission_bin"] = json!(binary);
    let sha = "ab".repeat(32);
    let plan = plan_fixed(&document, Some(binary.to_str().unwrap()), Some(binary.to_str().unwrap()), Some(sha.as_str()));
    assert_closed(&plan.result, "nonadmitted");
    assert!(!marker.exists(), "client-selected executable ran");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn partial_or_untrusted_operator_env_is_nonadmitted() {
    let mut document = base_manifest("clickhouse", "clickhouse_268");
    document["tables"] = json!([{
        "logical_schema": "readings",
        "logical_table": "final",
        "row_count": 3,
        "batch_sha256": "3e32e4f95456b2285bdaf9aa2988578635d97cabe5e83d07319c574f272fcf8b",
        "arrow_path": "/tmp/oo-native-absent.arrow",
        "columns": []
    }]);
    let partial = plan_fixed(&document, None, Some("/tmp/registry.json"), None);
    assert_closed(&partial.result, "nonadmitted");
    let untrusted = plan_fixed(&document, Some("UNTRUSTED"), Some("UNTRUSTED"), Some("UNTRUSTED"));
    assert_closed(&untrusted.result, "nonadmitted");
}

#[test]
fn writable_settings_and_wrong_binary_do_not_run() {
    let dir = std::env::temp_dir().join(format!("oo-native-write-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let marker = dir.join("ran");
    let binary = dir.join("bin.sh");
    std::fs::write(&binary, format!("#!/bin/sh\ntouch {}\n", marker.display())).expect("script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&binary).expect("meta").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&binary, permissions).expect("chmod");
    }
    let report = dir.join("report.json");
    let config = dir.join("config.json");
    let catalog = dir.join("catalog.json");
    let capture = dir.join("capture.json");
    std::fs::write(
        &report,
        br#"{"protocol":"oo.source-admission.report/v1","source_id":"clickhouse_268","driver":{"kind":"native-http","name":"ch-reader","version":"0.1.0","digest":"8b88485c0940e298a8edad500731f0a7688c61a135a943b217f9597e191c7f93"}}"#,
    ).expect("report");
    std::fs::write(
        &config,
        br#"{"source_id":"clickhouse_268","budget":{"unit":"Rows","hard_limit":100000,"confidence":"unknown"},"write_options":{"signal_table":true,"heartbeat_write":false,"create_stream":false,"create_trigger":false},"debezium":{"snapshot_mode":"never","create_stream":false,"create_trigger":false},"requested_write_class":"R0","r1w":null,"native":{"clickhouse":{"readonly":"1","quota":{}}}}"#,
    ).expect("config");
    let digest = "ab".repeat(32);
    std::fs::write(
        &catalog,
        format!(
            r#"{{"protocol":"oo.pipeline.sealed-import-catalog/v1","engine":"clickhouse","source_id":"clickhouse_268","source_generation":1,"schema_digest":"{digest}","type_digest":"{digest}","config_digest":"{digest}","input_digest":"{digest}"}}"#
        ),
    ).expect("catalog");
    std::fs::write(&capture, b"capture\n").expect("capture");
    let decision = "cd".repeat(32);
    let registry = registry_for(&dir, &report, &config, &catalog, &capture, &digest, &decision);
    let registry_sha = arrow_sha256(&std::fs::read(&registry).unwrap());
    let binary_text = binary.to_str().unwrap().to_string();
    let registry_text = registry.to_str().unwrap().to_string();
    let mut document = base_manifest("clickhouse", "clickhouse_268");
    document["snapshot"] = json!({"kind": "token", "value": "oo-dl01w1-40-arrow-final"});
    document["tables"] = json!([{
        "logical_schema": "readings",
        "logical_table": "final",
        "row_count": 3,
        "batch_sha256": "3e32e4f95456b2285bdaf9aa2988578635d97cabe5e83d07319c574f272fcf8b",
        "arrow_path": "/tmp/oo-native-absent.arrow",
        "columns": []
    }]);
    let plan = plan_fixed(
        &document,
        Some(binary_text.as_str()),
        Some(registry_text.as_str()),
        Some(registry_sha.as_str()),
    );
    assert_closed(&plan.result, "nonadmitted");
    assert!(!marker.exists(), "validator stand-in ran");
    let _ = std::fs::remove_dir_all(&dir);
}

fn registry_for(
    dir: &Path,
    report: &Path,
    config: &Path,
    catalog: &Path,
    capture: &Path,
    digest: &str,
    decision: &str,
) -> PathBuf {
    let registry = dir.join("registry.json");
    let receipt = receipt_path();
    let body = json!({
        "protocol": "oo.source-admission.binding-registry/v1",
        "bindings": [{
            "protocol": "oo.source-admission.capture-binding/v1",
            "source_id": "clickhouse_268",
            "source_engine": "clickhouse",
            "source_generation": 1,
            "snapshot": {"kind": "token", "value": "oo-dl01w1-40-arrow-final"},
            "report_sha256": arrow_sha256(&std::fs::read(report).unwrap()),
            "config_sha256": arrow_sha256(&std::fs::read(config).unwrap()),
            "decision_sha256": decision,
            "report_path": report,
            "config_path": config,
            "driver_digest": "8b88485c0940e298a8edad500731f0a7688c61a135a943b217f9597e191c7f93",
            "driver_kind": "native-http",
            "driver_name": "ch-reader",
            "driver_version": "0.1.0",
            "capture_receipt_sha256": arrow_sha256(&std::fs::read(capture).unwrap()),
            "capture_receipt_path": capture,
            "catalog_sha256": arrow_sha256(&std::fs::read(catalog).unwrap()),
            "catalog_path": catalog,
            "admission_receipt_path": receipt,
            "schema_digest": digest,
            "type_digest": digest,
            "config_digest": digest,
            "input_digest": digest,
            "objects": [{
                "logical_schema": "readings",
                "logical_table": "final",
                "row_count": 3,
                "arrow_sha256": "3e32e4f95456b2285bdaf9aa2988578635d97cabe5e83d07319c574f272fcf8b"
            }],
            "fields": [{"name": "reading_id", "arrow_type": "uint64", "nullable": false, "precision": 64}]
        }]
    });
    std::fs::write(&registry, serde_json::to_vec(&body).unwrap()).expect("registry");
    registry
}

const CLICKHOUSE_DECISION_SHA256: &str =
    "aedc6b434c4c5f2c9f2a4c4dd96258e4fd6dbdaeb2587da046dedc85add1ece7";
const ORACLE_DECISION_SHA256: &str =
    "ddafa84c68f8d662e5718d89c68e7d3374a7e83ef2245bcc102bf08b45a4cc35";
const CLICKHOUSE_SCHEMA_DIGEST: &str =
    "c1a230daee6da35d527245ff8c73a6c90fc4f5eb8baec0cd1f2abfe68fe11894";
const CLICKHOUSE_TYPE_DIGEST: &str =
    "b935b6a9eb5f3384060ddaae7c588b6718933b644c0b8814d7deb467fe39bbb1";
const CLICKHOUSE_CONFIG_DIGEST: &str =
    "435ac4a5d24920afab3343cb9de50a7e71d53ad107b35205cd686b596ea3b64a";
const CLICKHOUSE_INPUT_DIGEST: &str =
    "007be53b723c87abb6594871ae082fe14d67c5b01aa192ff26c2c2c6371865cd";
const CLICKHOUSE_DRIVER_DIGEST: &str =
    "8b88485c0940e298a8edad500731f0a7688c61a135a943b217f9597e191c7f93";
const CLICKHOUSE_ARROW_SHA256: &str =
    "3e32e4f95456b2285bdaf9aa2988578635d97cabe5e83d07319c574f272fcf8b";

fn proof_required() -> bool {
    std::env::var_os("OO_NATIVE_BINDING_PROOF_TOUCH").is_some()
}

fn repo_file(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn frozen_admission_bin() -> Option<PathBuf> {
    let path = std::env::var_os("OO_NATIVE_ADMISSION_BIN_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
                "../../evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/admission-1/compile-5df/bin/oo-source-admission",
            )
        });
    if !path.is_file() {
        assert!(!proof_required(), "frozen admission binary is absent");
        return None;
    }
    let bytes = std::fs::read(&path).expect("admission binary");
    assert!(
        approved_admission_bin(&arrow_sha256(&bytes)),
        "frozen admission binary is not an approved version"
    );
    Some(path)
}

#[test]
fn approved_validators_are_the_two_frozen_versions() {
    assert_eq!(
        ADMISSION_BIN_SHA256,
        "a205eb5797f69d2c95e4e1cc533fce5642be5424df7669fb8a49fef4026eeaf2"
    );
    assert_eq!(
        ADMISSION_BIN_SHA256_3C55,
        "ae7dc727181ae101bab84e328d39c38c3fdd6613255f79062404cbc9c8069577"
    );
    assert_eq!(
        ADMISSION_BIN_SHA256_E465,
        "e465241fc1292b6eb5a13cf12efbffe360a9fbf1cbb03c02affb0241a4e58e1f"
    );
    assert!(approved_admission_bin(ADMISSION_BIN_SHA256));
    assert!(approved_admission_bin(ADMISSION_BIN_SHA256_3C55));
    assert!(approved_admission_bin(ADMISSION_BIN_SHA256_E465));
    assert!(!approved_admission_bin(
        "0000000000000000000000000000000000000000000000000000000000000000"
    ));
    assert!(!approved_admission_bin(
        "ae645bda4a5b48a620e036975edccbd865f2c82558812bb8288302813cf7ed0d"
    ));
}

fn validate_frozen(bin: &Path, report: &Path, config: &Path) -> Option<std::process::Output> {
    match std::process::Command::new(bin)
        .arg("validate")
        .arg("--report")
        .arg(report)
        .arg("--config")
        .arg(config)
        .env_clear()
        .output()
    {
        Ok(output) => {
            // macOS reports an ELF as status 126. A signal has no code. Neither is a decision.
            if matches!(output.status.code(), Some(126) | Some(127) | None) {
                assert!(!proof_required(), "frozen admission binary did not start");
                return None;
            }
            Some(output)
        }
        Err(_) => {
            assert!(!proof_required(), "frozen admission binary did not start");
            None
        }
    }
}

fn binding_fields(columns: &[CopyColumnDecl]) -> Value {
    Value::Array(
        columns
            .iter()
            .map(|column| {
                let mut field = serde_json::Map::new();
                field.insert("name".to_string(), json!(column.name));
                field.insert("arrow_type".to_string(), json!(column.arrow_type));
                field.insert("nullable".to_string(), json!(column.nullable));
                if let Some(precision) = column.precision {
                    field.insert("precision".to_string(), json!(precision));
                }
                if let Some(scale) = column.scale {
                    field.insert("scale".to_string(), json!(scale));
                }
                if column.arrow_type == "timestamp_nanos_utc" {
                    field.insert("timezone".to_string(), json!("UTC"));
                }
                Value::Object(field)
            })
            .collect(),
    )
}

fn manifest_columns(columns: &[CopyColumnDecl]) -> Value {
    Value::Array(
        columns
            .iter()
            .map(|column| {
                let mut field = serde_json::Map::new();
                field.insert("name".to_string(), json!(column.name));
                field.insert("logical_name".to_string(), json!(column.logical_name));
                field.insert("arrow_type".to_string(), json!(column.arrow_type));
                field.insert("nullable".to_string(), json!(column.nullable));
                if let Some(precision) = column.precision {
                    field.insert("precision".to_string(), json!(precision));
                }
                if let Some(scale) = column.scale {
                    field.insert("scale".to_string(), json!(scale));
                }
                if let Some(char_length) = column.char_length {
                    field.insert("char_length".to_string(), json!(char_length));
                }
                Value::Object(field)
            })
            .collect(),
    )
}

fn file_sha(path: &Path) -> String {
    arrow_sha256(&std::fs::read(path).expect("hash input"))
}

#[test]
fn clickhouse_snapshot_mismatch_is_nonadmitted_before_arrow() {
    let dir = std::env::temp_dir().join(format!("oo-native-snap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let digest = "ab".repeat(32);
    let registry = dir.join("registry.json");
    let body = json!({
        "protocol": "oo.source-admission.binding-registry/v1",
        "bindings": [{
            "protocol": "oo.source-admission.capture-binding/v1",
            "source_id": "clickhouse_268",
            "source_engine": "clickhouse",
            "source_generation": 1,
            "snapshot": {"kind": "token", "value": "other-snapshot"},
            "schema_digest": digest,
            "type_digest": digest,
            "config_digest": digest,
            "input_digest": digest
        }]
    });
    std::fs::write(&registry, serde_json::to_vec(&body).unwrap()).expect("registry");
    let registry_sha = file_sha(&registry);
    let mut document = base_manifest("clickhouse", "clickhouse_268");
    document["snapshot"] = json!({"kind": "token", "value": "oo-dl01w1-40-arrow-final"});
    document["tables"] = json!([{
        "logical_schema": "readings",
        "logical_table": "final",
        "row_count": 3,
        "batch_sha256": CLICKHOUSE_ARROW_SHA256,
        "arrow_path": dir.join("missing.arrow"),
        "columns": []
    }]);
    let plan = plan_fixed(
        &document,
        Some("/tmp/oo-native-missing-bin"),
        registry.to_str(),
        Some(registry_sha.as_str()),
    );
    assert_closed(&plan.result, "nonadmitted");
    assert!(!plan.loader_ready());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn clickhouse_binding_plans_when_the_frozen_cli_admits() {
    let Some(bin) = frozen_admission_bin() else {
        return;
    };
    let report = repo_file("../../tools/source-admission/fixtures/native/clickhouse-attempt06-report.json");
    let config = repo_file("../../tools/source-admission/fixtures/native/clickhouse-attempt06-config.json");
    let catalog = repo_file("../../evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-05/catalog-faithful.json");
    let capture = repo_file("../../evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-driver-preflight/raw/attempt-06/arrow-final-decode.json");
    require_file(&report);
    require_file(&config);
    require_file(&catalog);
    require_file(&capture);
    let Some(output) = validate_frozen(&bin, &report, &config) else {
        return;
    };
    assert_eq!(output.status.code(), Some(0));
    let decision_sha = arrow_sha256(&output.stdout);
    assert_eq!(decision_sha, CLICKHOUSE_DECISION_SHA256);
    assert_eq!(decision_sha, file_sha(&decision_path()));
    let decision: Value = serde_json::from_slice(&output.stdout).expect("decision json");
    assert_eq!(decision["protocol"], "oo.source-admission.decision/v1");
    assert_eq!(decision["admitted"], true);
    assert_eq!(decision.pointer("/identity/source_id").and_then(|value| value.as_str()), Some("clickhouse_268"));
    let catalog_json: Value = serde_json::from_slice(&std::fs::read(&catalog).unwrap()).unwrap();
    assert_eq!(catalog_json["schema_digest"], CLICKHOUSE_SCHEMA_DIGEST);
    assert_eq!(catalog_json["type_digest"], CLICKHOUSE_TYPE_DIGEST);
    assert_eq!(catalog_json["config_digest"], CLICKHOUSE_CONFIG_DIGEST);
    assert_eq!(catalog_json["input_digest"], CLICKHOUSE_INPUT_DIGEST);
    let dir = std::env::temp_dir().join(format!("oo-native-ch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let registry = dir.join("registry.json");
    let fields = binding_fields(&clickhouse_columns());
    let body = json!({
        "protocol": "oo.source-admission.binding-registry/v1",
        "bindings": [{
            "protocol": "oo.source-admission.capture-binding/v1",
            "source_id": "clickhouse_268",
            "source_engine": "clickhouse",
            "source_generation": 1,
            "snapshot": {"kind": "token", "value": "oo-dl01w1-40-arrow-final"},
            "report_sha256": file_sha(&report),
            "config_sha256": file_sha(&config),
            "decision_sha256": decision_sha,
            "report_path": report,
            "config_path": config,
            "driver_digest": CLICKHOUSE_DRIVER_DIGEST,
            "driver_kind": "native-http",
            "driver_name": "ch-reader",
            "driver_version": "0.1.0",
            "capture_receipt_sha256": file_sha(&capture),
            "capture_receipt_path": capture,
            "catalog_sha256": file_sha(&catalog),
            "catalog_path": catalog,
            "admission_receipt_path": receipt_path(),
            "schema_digest": CLICKHOUSE_SCHEMA_DIGEST,
            "type_digest": CLICKHOUSE_TYPE_DIGEST,
            "config_digest": CLICKHOUSE_CONFIG_DIGEST,
            "input_digest": CLICKHOUSE_INPUT_DIGEST,
            "objects": [{
                "logical_schema": "readings",
                "logical_table": "final",
                "row_count": 3,
                "arrow_sha256": CLICKHOUSE_ARROW_SHA256
            }],
            "fields": fields
        }]
    });
    std::fs::write(&registry, serde_json::to_vec(&body).unwrap()).expect("registry");
    let registry_sha = file_sha(&registry);
    let bin_text = bin.to_str().unwrap().to_string();
    let registry_text = registry.to_str().unwrap().to_string();
    let mut document = base_manifest("clickhouse", "clickhouse_268");
    document["snapshot"] = json!({"kind": "token", "value": "oo-dl01w1-40-arrow-final"});
    document["catalog_identity"] = json!({
        "config_digest": CLICKHOUSE_CONFIG_DIGEST,
        "schema_digest": CLICKHOUSE_SCHEMA_DIGEST,
        "type_digest": CLICKHOUSE_TYPE_DIGEST,
        "input_digest": CLICKHOUSE_INPUT_DIGEST
    });
    document["tables"] = json!([{
        "logical_schema": "readings",
        "logical_table": "final",
        "row_count": 3,
        "batch_sha256": CLICKHOUSE_ARROW_SHA256,
        "arrow_path": clickhouse_fixture(),
        "columns": manifest_columns(&clickhouse_columns())
    }]);
    let plan = plan_fixed(&document, Some(&bin_text), Some(&registry_text), Some(&registry_sha));
    assert!(plan.loader_ready());
    assert_eq!(plan.result["reason"], "target_absent");
    assert_eq!(plan.result["load_state"], "not_sealed");
    assert_eq!(plan.result["loader_invoked"], false);
    assert_eq!(plan.result["admission"]["scope"], "recomputed_capture_binding");
    assert_eq!(plan.result["admission"]["batch_digests_bound"], true);
    assert_eq!(plan.result["admission"]["decision_sha256"], decision_sha);
    assert_eq!(plan.result["admission"]["registry_sha256"], registry_sha);
    assert_eq!(plan.result["admission"]["driver_digest"], CLICKHOUSE_DRIVER_DIGEST);
    assert_eq!(plan.result["tables"][0]["row_count"], 3);
    assert_eq!(plan.result["tables"][0]["columns"].as_array().map(|items| items.len()), Some(12));
    assert_eq!(
        plan.result["tables"][0]["copy_digest"],
        "0fb023c3168e9ae55a0a4ab6f6d87ddbf8533253f4f6349afe48a21b77a69f8c"
    );
    let text = plan.result.to_string();
    assert!(!text.contains("secret://"));
    assert!(!text.contains("policy_approved"));
    assert!(plan.result.get("admitted").is_none());
    assert!(plan.result["admission"].get("admitted").is_none());

    let mut crossed = document.clone();
    crossed["tables"][0]["logical_schema"] = json!("other_schema");
    let crossed_plan = plan_fixed(&crossed, Some(&bin_text), Some(&registry_text), Some(&registry_sha));
    assert_closed(&crossed_plan.result, "schema");
    assert!(!crossed_plan.loader_ready());

    let mut tampered_body = body;
    tampered_body["bindings"][0]["decision_sha256"] = json!(format!("{}0", &decision_sha[..63]));
    let tampered = dir.join("tampered.json");
    std::fs::write(&tampered, serde_json::to_vec(&tampered_body).unwrap()).unwrap();
    let tampered_sha = file_sha(&tampered);
    let tampered_text = tampered.to_str().unwrap().to_string();
    let tampered_plan = plan_fixed(&document, Some(&bin_text), Some(&tampered_text), Some(&tampered_sha));
    assert_closed(&tampered_plan.result, "nonadmitted");
    assert!(!tampered_plan.loader_ready());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn oracle_historical_receipt_is_cross_source() {
    let Some(bin) = frozen_admission_bin() else {
        return;
    };
    let report = repo_file("../../evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/admission-1/report.json");
    let config = repo_file("../../evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/admission-1/config.json");
    let historical = repo_file("../../evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/admission-1/decision.json");
    require_file(&report);
    require_file(&config);
    require_file(&historical);
    let Some(output) = validate_frozen(&bin, &report, &config) else {
        return;
    };
    assert_eq!(output.status.code(), Some(2));
    let decision_sha = arrow_sha256(&output.stdout);
    assert_eq!(decision_sha, ORACLE_DECISION_SHA256);
    assert_eq!(decision_sha, file_sha(&historical));
    let decision: Value = serde_json::from_slice(&output.stdout).expect("decision json");
    assert_eq!(decision["admitted"], false);
    assert_eq!(decision.pointer("/identity/source_id").and_then(|value| value.as_str()), Some("oo04_t_batch"));
    let dir = std::env::temp_dir().join(format!("oo-native-ora-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let digest = "cd".repeat(32);
    let catalog = dir.join("catalog.json");
    std::fs::write(
        &catalog,
        format!(
            r#"{{"protocol":"oo.pipeline.sealed-import-catalog/v1","engine":"oracle","source_id":"oo04_t_batch","source_generation":1,"schema_digest":"{digest}","type_digest":"{digest}","config_digest":"{digest}","input_digest":"{digest}"}}"#
        ),
    ).unwrap();
    let capture = dir.join("capture.json");
    std::fs::write(&capture, b"oracle-capture\n").unwrap();
    let registry = dir.join("registry.json");
    let body = json!({
        "protocol": "oo.source-admission.binding-registry/v1",
        "bindings": [{
            "protocol": "oo.source-admission.capture-binding/v1",
            "source_id": "oo04_t_batch",
            "source_engine": "oracle",
            "source_generation": 1,
            "snapshot": {"kind": "scn", "value": "2175138"},
            "report_sha256": file_sha(&report),
            "config_sha256": file_sha(&config),
            "decision_sha256": decision_sha,
            "report_path": report,
            "config_path": config,
            "driver_digest": "d729181747bd69ba53cde2956ea9ddc06cf09d0badaf4a9e89944b7c0b7b4397",
            "driver_kind": "native-rust",
            "driver_name": "oracledb",
            "driver_version": "26.0.0-beta.4",
            "capture_receipt_sha256": file_sha(&capture),
            "capture_receipt_path": capture,
            "catalog_sha256": file_sha(&catalog),
            "catalog_path": catalog,
            "admission_receipt_path": receipt_path(),
            "schema_digest": digest,
            "type_digest": digest,
            "config_digest": digest,
            "input_digest": digest,
            "objects": [{
                "logical_schema": "oo04_fix",
                "logical_table": "t_batch",
                "row_count": 5,
                "arrow_sha256": ORACLE_P2_BATCH_SHA256
            }],
            "fields": [{"name": "ID", "arrow_type": "int16", "nullable": false}]
        }]
    });
    std::fs::write(&registry, serde_json::to_vec(&body).unwrap()).unwrap();
    let registry_sha = file_sha(&registry);
    let mut document = base_manifest("oracle", "oo04_t_batch");
    document["snapshot"] = json!({"kind": "scn", "value": "2175138"});
    document["catalog_identity"] = json!({
        "config_digest": digest,
        "schema_digest": digest,
        "type_digest": digest,
        "input_digest": digest
    });
    document["tables"] = json!([{
        "logical_schema": "oo04_fix",
        "logical_table": "t_batch",
        "row_count": 5,
        "batch_sha256": ORACLE_P2_BATCH_SHA256,
        "arrow_path": fixture(),
        "columns": manifest_columns(&oracle_columns())
    }]);
    let plan = plan_fixed(
        &document,
        bin.to_str(),
        registry.to_str(),
        Some(registry_sha.as_str()),
    );
    assert_closed(&plan.result, "cross_source");
    assert_eq!(plan.result["admission"]["receipt_sha256"], "65b7f22fe26686f8a1e61d705a78d5a1a69f2a123de0b9dfce827cce2ffb59ef");
    assert!(!plan.loader_ready());
    let _ = std::fs::remove_dir_all(&dir);
}

fn frozen_arrow_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/batch.arrow",
    )
}

fn cell_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[test]
fn arrow_file_keeps_fixed_binary_uint8_millis_list_struct_and_union() {
    let path = frozen_arrow_file();
    if !path.is_file() {
        assert!(!proof_required(), "frozen arrow file is absent");
        return;
    }
    let bytes = std::fs::read(&path).expect("arrow file");
    let stream = decode_arrow_stream(&bytes).expect("file");
    assert_eq!(stream.rows, 16);
    let tokens: Vec<(&str, &str, bool)> = stream
        .fields
        .iter()
        .map(|field| (field.name.as_str(), field.arrow_type.as_str(), field.nullable))
        .collect();
    assert_eq!(
        tokens,
        vec![
            ("_id", "fixedsizebinary(12)", false),
            ("code", "utf8", false),
            ("flag", "int32", false),
            ("units", "int64", false),
            ("price", "decimal128", false),
            ("created", "timestamp(ms,utc)", false),
            ("uid", "fixedsizebinary(16)", false),
            ("blob", "binary", false),
            ("blob_presence", "uint8", false),
            ("weight", "float64", false),
            ("warehouse", "utf8", false),
            ("whCode", "utf8", false),
            ("lane", "union(dense,0:utf8,1:int64)", false),
            ("memo", "utf8", true),
            ("memo_presence", "uint8", false),
            ("marks", "list(utf8)", true),
            ("marks_presence", "uint8", false),
            ("pack", "struct(kind:utf8,n:int32)", true),
            ("pack_presence", "uint8", false),
        ]
    );
    assert_eq!(stream.fields[4].precision, Some(34));
    assert_eq!(stream.fields[4].scale, Some(7));
    assert!(stream.fields[5].timezone.is_none());
    match &stream.columns[0][0] {
        NativeCell::Binary(bytes) => assert_eq!(cell_hex(bytes), "c141d04141eef04100000000"),
        other => panic!("_id {other:?}"),
    }
    match &stream.columns[5][0] {
        NativeCell::TimestampMicrosUtc { value, timezone } => {
            assert_eq!(*value, 1_725_000_000_000_000);
            assert_eq!(timezone, "UTC");
        }
        other => panic!("created {other:?}"),
    }
    match &stream.columns[6][0] {
        NativeCell::Binary(bytes) => assert_eq!(bytes.len(), 16),
        other => panic!("uid {other:?}"),
    }
    assert_eq!(stream.columns[8][0], NativeCell::UInt8(3));
    assert!(matches!(stream.columns[13][0], NativeCell::Null));
    assert_eq!(stream.columns[14][0], NativeCell::UInt8(0));
    assert_eq!(
        stream.columns[12][0],
        NativeCell::Json(r#"{"type_id":0,"type":"utf8","value":"lane-0"}"#.to_string())
    );
    assert_eq!(
        stream.columns[12][4],
        NativeCell::Json(r#"{"type_id":1,"type":"int64","value":4104}"#.to_string())
    );
    assert_eq!(stream.columns[15][0], NativeCell::Json(r#"["m","0"]"#.to_string()));
    assert!(matches!(stream.columns[15][3], NativeCell::Null));
    assert_eq!(stream.columns[15][4], NativeCell::Json("[]".to_string()));
    assert_eq!(
        stream.columns[17][0],
        NativeCell::Json(r#"{"kind":"crate","n":0}"#.to_string())
    );
    assert!(matches!(stream.columns[17][5], NativeCell::Null));
    let mut broken = bytes.clone();
    broken[0] = b'X';
    assert!(decode_arrow_stream(&broken).is_err());
}

fn starrocks_arrow(name: &str) -> PathBuf {
    repo_file(&format!(
        "../../evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-driver-preflight/native/read/{name}.arrow"
    ))
}

fn sr_columns(table: &str) -> Vec<CopyColumnDecl> {
    match table {
        "pk_accounts" => vec![
            column("account_id", "account_id", "int64", false, None, None, None),
            column("balance", "balance", "decimal128(38,9)", true, Some(38), Some(9), None),
            column("nickname", "nickname", "utf8", true, None, None, None),
            column("opened", "opened", "timestamp_second_naive", true, None, None, None),
        ],
        "agg_metrics" => vec![
            column("metric_day", "metric_day", "date32", false, None, None, None),
            column("metric_name", "metric_name", "utf8", false, None, None, None),
            column("qty", "qty", "int64", true, None, None, None),
            column("revenue", "revenue", "decimal128(38,4)", true, Some(38), Some(4), None),
        ],
        "dup_events" => vec![
            column("event_id", "event_id", "int64", false, None, None, None),
            column("bucket", "bucket", "date32", false, None, None, None),
            column("seen", "seen", "timestamp_second_naive", true, None, None, None),
            column("amount", "amount", "decimal128(18,4)", true, Some(18), Some(4), None),
            column("note", "note", "utf8", true, None, None, None),
        ],
        "uniq_dim" => vec![
            column("dim_id", "dim_id", "int64", false, None, None, None),
            column("label", "label", "utf8", true, None, None, None),
            column("score", "score", "decimal128(10,2)", true, Some(10), Some(2), None),
        ],
        "dup_partition_sums" => vec![
            column("part", "part", "utf8", false, None, None, None),
            column("row_count", "row_count", "int64", false, None, None, None),
            column("amount_sum", "amount_sum", "decimal128(38,4)", true, Some(38), Some(4), None),
        ],
        _ => panic!("unknown table"),
    }
}

#[test]
fn starrocks_standard_types_decode_without_rewriting_tokens() {
    let cases = [
        ("pk_accounts", 3, "4463f9c889619fcd4e84428cb818b4d36c0534b5435b8df42d4ea82bc493ca6a"),
        ("agg_metrics", 2, "c44b8d588d996c1c6b6986df2a00160549c6a4a2af80cc9aae24605c3c91d693"),
        ("dup_events", 6, "e644ea2adaf7a9020c5b9662dfa923188d3509098e181c4a70a004bf0f47e8c0"),
        ("uniq_dim", 2, "9d08843ba68f57c9dbf8077f3e4572e839e71887ba981e0aa3db92b9b8a5ae06"),
        ("dup_partition_sums", 2, "68f2ee8ef7fc3b419a900835eb0ca4fb64f451335328ee2bba534ed98dd01d4d"),
    ];
    for (name, rows, digest) in cases {
        let bytes = std::fs::read(starrocks_arrow(name)).expect(name);
        assert_eq!(arrow_sha256(&bytes), digest, "{name}");
        let stream = decode_arrow_stream(&bytes).unwrap_or_else(|err| panic!("{name} {err:?}"));
        assert_eq!(stream.rows, rows, "{name}");
        for field in &stream.fields {
            if field.arrow_type == "int64" {
                assert_eq!(field.precision, Some(64), "{name}.{}", field.name);
                assert!(field.scale.is_none());
            }
        }
        let encoded = copy_digest(&bytes, "sr_w45", "oo_w45_sr", name, 1, &sr_columns(name))
            .unwrap_or_else(|err| panic!("{name} copy {err:?}"));
        assert_eq!(encoded.len(), 64, "{name}");
    }
    let accounts = decode_arrow_stream(&std::fs::read(starrocks_arrow("pk_accounts")).unwrap()).unwrap();
    assert_eq!(accounts.fields[0].arrow_type, "int64");
    assert_eq!(accounts.fields[1].arrow_type, "decimal128");
    assert_eq!(accounts.fields[1].precision, Some(38));
    assert_eq!(accounts.fields[1].scale, Some(9));
    assert_eq!(accounts.fields[3].arrow_type, "timestamp_second_naive");
    assert!(accounts.fields[3].timezone.is_none());
    assert_eq!(int_cell(&accounts.columns[0][0]), (64, 300));
    match &accounts.columns[3][0] {
        NativeCell::TimestampMicros(value) => assert_eq!(*value, 1_777_691_045_000_000),
        other => panic!("opened {other:?}"),
    }
    assert!(matches!(accounts.columns[3][1], NativeCell::Null));
    let metrics = decode_arrow_stream(&std::fs::read(starrocks_arrow("agg_metrics")).unwrap()).unwrap();
    assert_eq!(metrics.fields[0].arrow_type, "date32");
    match &metrics.columns[0][0] {
        NativeCell::Date32(days) => assert_eq!(*days, 20_574),
        other => panic!("metric_day {other:?}"),
    }
    let mut narrowed = sr_columns("uniq_dim");
    narrowed[0].precision = Some(32);
    assert_eq!(
        copy_digest(
            &std::fs::read(starrocks_arrow("uniq_dim")).unwrap(),
            "sr_w45",
            "oo_w45_sr",
            "uniq_dim",
            1,
            &narrowed,
        ),
        Err(ArrowFault::Precision)
    );
    let mut retagged = sr_columns("pk_accounts");
    retagged[3].arrow_type = "timestamp_micros".to_string();
    assert_eq!(
        copy_digest(
            &std::fs::read(starrocks_arrow("pk_accounts")).unwrap(),
            "sr_w45",
            "oo_w45_sr",
            "pk_accounts",
            1,
            &retagged,
        ),
        Err(ArrowFault::Shape)
    );
}

#[test]
fn clickhouse_binary_does_not_become_text() {
    let bytes = std::fs::read(clickhouse_fixture()).expect("arrow");
    let stream = decode_arrow_stream(&bytes).expect("decode");
    assert_eq!(stream.fields[3].arrow_type, "binary");
    assert_eq!(stream.fields[4].arrow_type, "dictionary_binary");
    assert_eq!(stream.fields[5].arrow_type, "dictionary_binary");
    assert_eq!(stream.fields[10].arrow_type, "binary");
    let digest = copy_digest(&bytes, "clickhouse_268", "readings", "final", 1, &clickhouse_columns())
        .expect("bytea copy");
    assert_eq!(digest.len(), 64);
    let mut pretended = clickhouse_columns();
    pretended[3].arrow_type = "utf8".to_string();
    assert_eq!(
        copy_digest(&bytes, "clickhouse_268", "readings", "final", 1, &pretended),
        Err(ArrowFault::Shape)
    );
}
