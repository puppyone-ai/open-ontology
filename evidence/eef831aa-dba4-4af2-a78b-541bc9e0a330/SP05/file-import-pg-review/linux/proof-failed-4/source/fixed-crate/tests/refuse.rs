//! Admission and projection checks. No database is opened.

#![forbid(unsafe_code)]

use std::sync::Arc;

use arrow_array::{ArrayRef, Decimal128Array, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use file_import_pg::{
    config_digest, error_document, exit_code, import_replace, parse_request, physical_plan,
    project_verified, run_paths, ColumnSpec, COPY_BUDGETS, ERROR_PROTOCOL,
};
use oo_connectors::{VerifiedBatch, VerifiedField, VerifiedRow};
use oo_core::limits::DOCUMENT_MAX_BYTES;
use oo_pipeline::copy_encoder::{ArrowScalar, CellPresence, LogicalValue};
use serde_json::{json, Value};

fn base() -> Value {
    json!({
        "protocol": "oo.pipeline.file-import-pg/v1",
        "strategy": "replace",
        "parsing_mode": "trusted_public_fixture",
        "locale": "C",
        "type_policy": "preserve_row",
        "object": {"sha256": "a".repeat(64), "path": "/tmp/object.csv"},
        "worker": {"sha256": "b".repeat(64), "path": "/tmp/worker"},
        "profile": {
            "format": "csv",
            "id": "rows",
            "bom": "refuse",
            "header": "exact",
            "null": {"policy": "empty"},
            "columns": [{
                "name": "id",
                "parse": "utf8",
                "pg": "text",
                "nullable": false,
                "primary_key": true,
                "precision": null,
                "scale": null
            }]
        },
        "budget": {
            "batch_rows": 32,
            "batch_bytes": 1_048_576,
            "max_record_bytes": 65_536,
            "max_field_bytes": 16_384,
            "max_total_bytes": 4_194_304,
            "max_rows": 1000,
            "max_columns": 16,
            "max_fields": 64,
            "worker_deadline_ms": 30_000,
            "parent_deadline_ms": 60_000
        },
        "target": {
            "host": "oo-a308-pg",
            "port": 5432,
            "database": "oo_a308_file",
            "user": "oo_a308_runtime",
            "server_name": "oo-a308-pg",
            "ca_path": "/tmp/ca.pem"
        },
        "fence": {"shard": "filecsv"},
        "mapping": {"namespace": "fileimp", "table": "rows_csv"},
        "source": "src_csv"
    })
}

fn parse(value: &Value) -> Result<file_import_pg::ImportRequest, file_import_pg::ImportError> {
    parse_request(&value.to_string())
}

fn refuse(value: &Value) -> file_import_pg::ImportError {
    match parse(value) {
        Ok(_) => panic!("admitted"),
        Err(error) => error,
    }
}

#[test]
fn closed_requests() {
    let mut bad = base();
    bad["protocol"] = json!("oo.other/v1");
    let error = refuse(&bad);
    assert_eq!((error.code, error.detail, error.child_started), ("closed_input", "protocol", false));
    assert_eq!(exit_code(&error), 2);

    let mut extra = base();
    extra["extra"] = json!(1);
    assert_eq!(refuse(&extra).detail, "unknown_field");

    let mut secret = base();
    secret["budget"]["password"] = json!("nope");
    assert_eq!(refuse(&secret).code, "credential");

    let mut token = base();
    token["target"]["token"] = json!("nope");
    assert_eq!(refuse(&token).code, "credential");

    for format in ["parquet", "excel", "xlsx", "sqlite", "sql_dump", "dump"] {
        let mut value = base();
        value["profile"]["format"] = json!(format);
        let error = refuse(&value);
        assert_eq!(error.code, "unsupported_format", "{format}");
        assert_eq!(exit_code(&error), 2);
    }
    for strategy in ["append", "upsert"] {
        let mut value = base();
        value["strategy"] = json!(strategy);
        assert_eq!(refuse(&value).code, "unsupported_strategy");
    }
    for mode in ["production_untrusted", "runsc"] {
        let mut value = base();
        value["parsing_mode"] = json!(mode);
        assert_eq!(refuse(&value).code, "unsupported_mode");
    }
    let mut locale = base();
    locale["locale"] = json!("en_US.UTF-8");
    assert_eq!(refuse(&locale).detail, "locale");
    let mut policy = base();
    policy["type_policy"] = json!("coerce");
    assert_eq!(refuse(&policy).detail, "type_policy");
    let mut port = base();
    port["target"]["port"] = json!(1.5);
    assert_eq!(refuse(&port).detail, "number");
    port["target"]["port"] = json!(0);
    assert_eq!(refuse(&port).detail, "port");
    let mut missing = base();
    missing.as_object_mut().expect("object").remove("source");
    assert_eq!(refuse(&missing).detail, "missing_key");
}

#[test]
fn token_null_and_config_digest() {
    let mut token = base();
    token["profile"]["null"] = json!({"policy": "token", "token": "\\N"});
    let admitted = parse(&token).expect("token policy");
    assert_eq!(admitted.null_line(), "token:\\N");

    let mut utf8 = base();
    utf8["locale"] = json!("C.UTF-8");
    assert!(parse(&utf8).is_ok());

    let mut left = base();
    let mut right = base();
    let left_sha = "a".repeat(64);
    let right_sha = "b".repeat(64);
    assert_ne!(left_sha, right_sha);
    left["object"]["sha256"] = json!(left_sha);
    right["object"]["sha256"] = json!(right_sha);
    let left_request = parse(&left).expect("left");
    let right_request = parse(&right).expect("right");
    assert_eq!(config_digest(&left_request), config_digest(&right_request));
    let left_names = physical_plan(&left_request).expect("left names");
    let right_names = physical_plan(&right_request).expect("right names");
    assert_eq!(left_names, right_names);
    assert!(left_names.namespace.starts_with("pn_") && left_names.namespace.len() == 63);
    assert!(left_names.table.starts_with("pt_") && left_names.table.len() == 63);
    assert_eq!(left_names.columns.len(), 1);
    assert!(left_names.columns[0].starts_with("pc_"));

    let mut moved = base();
    moved["locale"] = json!("C.UTF-8");
    let moved_request = parse(&moved).expect("locale");
    assert_ne!(config_digest(&left_request), config_digest(&moved_request));
}

#[test]
fn decimal_typmod_and_projection() {
    let column = ColumnSpec::try_new("amount", "decimal", "numeric", true, false, Some(10), Some(2)).expect("decimal");
    assert_eq!(column.oid(), 1700);
    assert_eq!(column.typmod(), ((10_i32 << 16) | 2) + 4);

    let columns = vec![
        ColumnSpec::try_new("id", "utf8", "text", false, true, None, None).expect("id"),
        column,
    ];
    let id = Arc::new(StringArray::from(vec![Some("a")])) as ArrayRef;
    let amount = Arc::new(
        Decimal128Array::from(vec![Some(150_i128)])
            .with_precision_and_scale(10_u8, 2_i8)
            .expect("decimal array"),
    ) as ArrayRef;
    let batch = verified(
        vec![("id", DataType::Utf8, false), ("amount", DataType::Decimal128(10_u8, 2_i8), true)],
        vec![id, amount],
        &[vec!["value", "value"]],
    );
    let typed = project_verified(&columns, &batch, "src_csv", "fileimp", "rows_csv", [9; 32]).expect("project");
    let cell = &typed.rows[0].cells[1];
    assert_eq!(cell.actual_precision, Some(3));
    assert_eq!(cell.actual_scale, Some(2));
    match &cell.value {
        Some(LogicalValue::Decimal(exact)) => {
            assert!(!exact.is_negative());
            assert_eq!(exact.coefficient(), "150");
            assert_eq!(exact.scale(), 2);
        }
        other => panic!("decimal {other:?}"),
    }
    assert_eq!(cell.arrow, Some(ArrowScalar::Decimal128(150)));
    let first = file_import_pg::hash_typed_batch(&typed).expect("hash");
    let second = file_import_pg::hash_typed_batch(&typed).expect("hash again");
    assert_eq!(first, second);
    assert_eq!(first.len(), 64);
    assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn int8_minmax_hash_and_presence() {
    let columns = vec![ColumnSpec::try_new("n", "int64", "int8", false, true, None, None).expect("n")];
    let values = Arc::new(Int64Array::from(vec![Some(i64::MIN), Some(i64::MAX)])) as ArrayRef;
    let batch = verified(
        vec![("n", DataType::Int64, false)],
        vec![values],
        &[vec!["value"], vec!["value"]],
    );
    let typed = project_verified(&columns, &batch, "src_csv", "fileimp", "rows_int", [3; 32]).expect("int");
    match &typed.rows[0].cells[0].value {
        Some(LogicalValue::Int(exact)) => assert_eq!(exact.to_string(), "-9223372036854775808"),
        other => panic!("min {other:?}"),
    }
    match &typed.rows[1].cells[0].value {
        Some(LogicalValue::Int(exact)) => assert_eq!(exact.to_string(), "9223372036854775807"),
        other => panic!("max {other:?}"),
    }
    let both = file_import_pg::hash_typed_batch(&typed).expect("both");
    let again = file_import_pg::hash_typed_batch(&typed).expect("both again");
    assert_eq!(both, again);
    let only_min = verified(
        vec![("n", DataType::Int64, false)],
        vec![Arc::new(Int64Array::from(vec![Some(i64::MIN)])) as ArrayRef],
        &[vec!["value"]],
    );
    let min_batch = project_verified(&columns, &only_min, "src_csv", "fileimp", "rows_int", [3; 32]).expect("min");
    let min_hash = file_import_pg::hash_typed_batch(&min_batch).expect("min hash");
    assert_ne!(both, min_hash);

    let text = vec![ColumnSpec::try_new("note", "utf8", "text", true, false, None, None).expect("note")];
    let empty = verified(
        vec![("note", DataType::Utf8, true)],
        vec![Arc::new(StringArray::from(vec![Some("")])) as ArrayRef],
        &[vec!["empty"]],
    );
    let empty_batch = project_verified(&text, &empty, "src_csv", "fileimp", "rows_note", [1; 32]).expect("empty");
    assert_eq!(empty_batch.rows[0].cells[0].presence, CellPresence::Value);
    assert_eq!(empty_batch.rows[0].cells[0].arrow, Some(ArrowScalar::Utf8(String::new())));

    let nulls = verified(
        vec![("note", DataType::Utf8, true)],
        vec![Arc::new(StringArray::from(vec![None::<&str>])) as ArrayRef],
        &[vec!["null"]],
    );
    let null_batch = project_verified(&text, &nulls, "src_csv", "fileimp", "rows_note", [1; 32]).expect("null");
    assert_eq!(null_batch.rows[0].cells[0].presence, CellPresence::Null);

    let columns = vec![
        ColumnSpec::try_new("id", "utf8", "text", false, true, None, None).expect("id"),
        ColumnSpec::try_new("payload", "utf8", "json", true, false, None, None).expect("payload"),
    ];
    let missing = verified(
        vec![("id", DataType::Utf8, false), ("payload", DataType::Utf8, true)],
        vec![
            Arc::new(StringArray::from(vec![Some("m1")])) as ArrayRef,
            Arc::new(StringArray::from(vec![None::<&str>])) as ArrayRef,
        ],
        &[vec!["value", "missing"]],
    );
    let error = project_verified(&columns, &missing, "src_csv", "fileimp", "rows_missing", [4; 32]).expect_err("missing");
    assert_eq!((error.code, error.detail, error.child_started), ("missing_field", "missing_field", false));
    assert_eq!(exit_code(&error), 2);
    let document = error_document(&error);
    assert_eq!(
        document,
        format!(
            "{{\"protocol\":\"{ERROR_PROTOCOL}\",\"code\":\"missing_field\",\"detail\":\"missing_field\",\"child_started\":false,\"published\":false}}"
        )
    );
}

#[tokio::test]
async fn path_admission_does_not_open_a_database() {
    let dir = std::env::temp_dir().join("oo-a308-refuse");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let password = dir.join("password");
    std::fs::write(&password, b"\n").expect("empty");
    let request = dir.join("request.json");
    std::fs::write(&request, base().to_string()).expect("request");
    let error = run_paths(&request, &password).await.expect_err("empty password");
    assert_eq!((error.code, error.detail), ("closed_input", "password"));

    std::fs::write(&password, b"abcd\r\n").expect("password");
    let error = run_paths(&request, &password).await.expect_err("missing object");
    assert_eq!((error.code, error.detail, error.child_started), ("input_io", "object", false));

    let link = dir.join("link.json");
    std::os::unix::fs::symlink(&request, &link).expect("symlink");
    let error = run_paths(&link, &password).await.expect_err("symlink");
    assert_eq!(error.detail, "not_regular");

    std::fs::write(dir.join("bad.json"), [0xff]).expect("bad");
    let error = run_paths(&dir.join("bad.json"), &password).await.expect_err("encoding");
    assert_eq!(error.detail, "encoding");

    let huge = dir.join("huge.json");
    std::fs::write(&huge, vec![b'x'; DOCUMENT_MAX_BYTES + 1]).expect("huge");
    let error = run_paths(&huge, &password).await.expect_err("huge");
    assert_eq!(error.detail, "json");

    let admitted = parse(&base()).expect("request");
    let error = import_replace(&admitted, "").await.expect_err("blank");
    assert_eq!((error.code, error.detail, error.child_started), ("closed_input", "password", false));
    let _ = std::fs::remove_dir_all(&dir);
}

fn verified(fields: Vec<(&str, DataType, bool)>, columns: Vec<ArrayRef>, presence: &[Vec<&str>]) -> VerifiedBatch {
    let schema_fields: Vec<Field> = fields
        .iter()
        .map(|(name, ty, nullable)| Field::new(*name, ty.clone(), *nullable))
        .collect();
    let record = RecordBatch::try_new(Arc::new(Schema::new(schema_fields)), columns).expect("batch");
    let rows = presence
        .iter()
        .enumerate()
        .map(|(index, row)| VerifiedRow {
            row_index: index as u64,
            line: index as u64 + 1,
            byte_offset: 0,
            fields: row
                .iter()
                .enumerate()
                .map(|(slot, token)| VerifiedField {
                    name: fields[slot].0.to_string(),
                    line: index as u64 + 1,
                    byte_offset: 0,
                    presence: match *token {
                        "value" => "value",
                        "null" => "null",
                        "missing" => "missing",
                        "empty" => "empty",
                        _ => panic!("presence"),
                    },
                })
                .collect(),
        })
        .collect();
    VerifiedBatch {
        batch_index: 0,
        record,
        rows,
        raw_bytes: 1,
        arrow_bytes: 1,
        schema_identity: [0; 32],
        profile_digest: [0; 32],
        config_identity: [0; 32],
    }
}

#[test]
fn copy_budgets_are_fixed() {
    assert_eq!(COPY_BUDGETS.max_value_bytes, 1_048_576);
    assert_eq!(COPY_BUDGETS.max_row_bytes, 4_194_304);
    assert_eq!(COPY_BUDGETS.max_chunk_bytes, 8_388_608);
    assert_eq!(COPY_BUDGETS.max_rows_per_chunk, 1024);
}
