//! `oo-pipeline catalog-import-native` and `install-native-sealed-catalog`.
//!
//! The request has no engine field. The engine is the admitted capture binding.
//! Mature MySQL/PG imports retain their existing commands; native runs use this command.
//! This command does not open a source and does not read shadow values.
//! `install-native-sealed-catalog` applies migration `0008` only. It does not
//! send `0006` again.

use std::io::{self, Write};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};

use crate::catalog_import::{push_schema_column, push_schema_table};
use crate::full_import::hook_digest;
use crate::import_cli::{Stop, read_bounded_stdin, read_ca, read_password};
use crate::native_arrow::{
    column_provenance, fixed_binary_width, mapping_ident, numeric_typmod, parse_decimal_spell,
};
use crate::native_binding::{self, BoundField, ResolvedCapture};
use crate::target_installer::{
    self, install_native_sealed_catalog, sha256_hex, verify_native_catalog_readiness, InstallError,
    ReadinessError,
};

/// Request protocol. There is no engine field.
pub const REQUEST_PROTOCOL: &str = "oo.pipeline.native-sealed-catalog/v1";

/// Stdout protocol. This is not `oo.source.pgsql/v1`.
pub const RESULT_PROTOCOL: &str = "oo.pipeline.sealed-import-catalog/v1";

/// Administrator install request for migration `0008`.
pub const INSTALL_PROTOCOL: &str = "oo.pipeline.install-native-sealed-catalog/v1";

const INSTALL_RESULT: &str = "oo.pipeline.install-native-sealed-catalog-result/v1";

const CLIENT_AUTHORITY: &[&str] = &[
    "admitted",
    "admission_bin",
    "registry_path",
    "registry_sha256",
    "approval",
];

/// Read one native sealed import and write the catalog document.
pub async fn catalog_import_native() -> Result<(), u8> {
    match execute().await {
        Ok(document) => write_stdout(&document),
        Err(stop) => {
            stop.emit();
            Err(1)
        }
    }
}

/// Install migration `0008` on an administrator connection.
pub async fn install_native_sealed_catalog_cli() -> Result<(), u8> {
    match install_execute().await {
        Ok(document) => match io::stdout().write_all(document.as_bytes()) {
            Ok(()) => Ok(()),
            Err(_) => {
                eprintln!("oo-pipeline: catalog output");
                Err(1)
            }
        },
        Err(stop) => {
            stop.emit();
            Err(1)
        }
    }
}

/// Classify a request before a connection or a validator process.
fn classify_request(text: &str) -> Result<NativeRequest, &'static str> {
    let value: Value = serde_json::from_str(text).map_err(|_| "invalid input")?;
    let object = value.as_object().ok_or("invalid input")?;
    if object.contains_key("engine") || object.contains_key("source_engine") {
        return Err("source engine");
    }
    if CLIENT_AUTHORITY.iter().any(|key| object.contains_key(*key)) {
        return Err("nonadmitted");
    }
    let request: NativeRequest = serde_json::from_value(value).map_err(|_| "invalid input")?;
    if request.protocol != REQUEST_PROTOCOL {
        return Err("invalid version");
    }
    validate_closed(&request)?;
    Ok(request)
}

async fn execute() -> Result<Vec<u8>, Stop> {
    let text = read_bounded_stdin("catalog input")?;
    let request = classify_request(&text).map_err(Stop::Fixed)?;
    let captured = native_binding::resolve_sealed(
        &request.source_id,
        request.source_generation,
        &request.snapshot.kind,
        &request.snapshot.value,
        &request.config_digest,
        &request.schema_digest,
        &request.type_digest,
        &request.input_digest,
    )
    .map_err(stop_reason)?;
    if captured.source_id != request.source_id || captured.source_engine.is_empty() {
        return Err(Stop::Fixed("nonadmitted"));
    }
    let password = read_password(&request.target.password_file, "target password file")?;
    let ca = read_ca(&request.target.ca_file, "target ca file")?;
    let mut conn = connect(
        &request.target.host,
        request.target.port,
        &request.target.database,
        &request.target.runtime_principal,
        &password,
        &ca,
    )
    .await?;
    expect_principal(&mut conn, &request.target.runtime_principal).await?;
    set_timeout(&mut conn).await?;
    verify_native_catalog_readiness(&mut conn).await.map_err(ready_stop)?;
    let mut tx = conn.begin().await.map_err(|_| Stop::Fixed("target database"))?;
    if sqlx::raw_sql("SET TRANSACTION READ ONLY")
        .execute(&mut *tx)
        .await
        .is_err()
    {
        let _ = tx.rollback().await;
        return Err(Stop::Fixed("target database"));
    }
    let facts = match read_facts(&mut tx, &request).await {
        Ok(facts) => facts,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };
    let partition_vector = if let Some(scope) = captured.partition_scope.as_ref() {
        let stored_digest: Option<String> = sqlx::query_scalar(
            "SELECT checksum FROM _oo.schema_migrations WHERE version = $1"
        ).bind(target_installer::VERSION_NATIVE_PARTITION)
         .fetch_optional(&mut *tx).await.map_err(map_db)?;
        if stored_digest.as_deref() != Some(target_installer::compiled_native_partition_digest().as_str()) {
            return Err(Stop::Fixed("partition vector"));
        }
        let observed = scope["observed_partition_ids"].as_array()
            .ok_or(Stop::Fixed("partition vector"))?
            .iter().map(|id| id.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>().ok_or(Stop::Fixed("partition vector"))?;
        let text: String = sqlx::query_scalar(
            "SELECT _oo.native_partition_vector($1::uuid, $2, $3, $4, $5::text[], $6, $7, $8, $9)::text"
        )
        .bind(&request.run_id).bind(&request.source_id)
        .bind(scope["logical_schema"].as_str().ok_or(Stop::Fixed("partition vector"))?)
        .bind(scope["logical_table"].as_str().ok_or(Stop::Fixed("partition vector"))?)
        .bind(&observed)
        .bind(scope["partition_id"].as_str().ok_or(Stop::Fixed("partition vector"))?)
        .bind(scope["current_signature"].as_str())
        .bind(request.source_generation).bind(request.target_generation)
        .fetch_one(&mut *tx).await.map_err(map_db)?;
        let vector = serde_json::from_str::<Value>(&text).map_err(|_| Stop::Fixed("partition vector"))?;
        let entries = vector.as_array().ok_or(Stop::Fixed("partition vector"))?;
        if entries.len() != observed.len()
            || entries.iter().filter(|entry| entry["partition_id"] == scope["partition_id"]
                && entry["partition_scope"] == *scope
                && entry["row_count"] == scope["row_count"]).count() != 1
        {
            return Err(Stop::Fixed("partition vector"));
        }
        Some(vector)
    } else { None };
    let pg_delta_bound = if matches!(captured.source_engine.as_str(), "postgres" | "postgresql")
        && captured.objects.len() == 1 && facts.tables.len() == 1
    {
        let object = &captured.objects[0];
        let table = &facts.tables[0];
        if u64::try_from(table.applied_rows).ok() == Some(object.row_count) {
            false
        } else {
            let installed: Option<String> = sqlx::query_scalar(
                "SELECT checksum FROM _oo.schema_migrations WHERE version = $1"
            ).bind(target_installer::VERSION_NATIVE_PARTITION)
             .fetch_optional(&mut *tx).await.map_err(map_db)?;
            if installed.as_deref() != Some(target_installer::compiled_native_partition_digest().as_str()) {
                return Err(Stop::Fixed("schema"));
            }
            let bound: Option<bool> = sqlx::query_scalar(
                "SELECT _oo.native_pg_catalog_delta($1::uuid, $2, $3, $4, $5)"
            ).bind(&request.run_id).bind(&request.source_id)
             .bind(&table.schema_ident).bind(&table.table_ident)
             .bind(i64::try_from(object.row_count).map_err(|_| Stop::Fixed("schema"))?)
             .fetch_one(&mut *tx).await.map_err(map_db)?;
            if bound != Some(true) { return Err(Stop::Fixed("schema")); }
            true
        }
    } else { false };
    let document = match bind_native(&captured, &facts, partition_vector, pg_delta_bound) {
        Ok(document) => document,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };
    tx.commit().await.map_err(|_| Stop::Fixed("target database"))?;
    Ok(document)
}

async fn install_execute() -> Result<String, Stop> {
    let text = read_bounded_stdin("catalog input")?;
    let value: Value = serde_json::from_str(&text).map_err(|_| Stop::Fixed("invalid input"))?;
    if value.get("engine").is_some() || value.get("source_engine").is_some() {
        return Err(Stop::Fixed("source engine"));
    }
    let request: InstallRequest = serde_json::from_value(value).map_err(|_| Stop::Fixed("invalid input"))?;
    if request.protocol != INSTALL_PROTOCOL {
        return Err(Stop::Fixed("invalid version"));
    }
    let password = read_password(&request.target.password_file, "target password file")?;
    let ca = read_ca(&request.target.ca_file, "target ca file")?;
    let mut conn = connect(
        &request.target.host,
        request.target.port,
        &request.target.database,
        &request.target.administrator,
        &password,
        &ca,
    )
    .await?;
    expect_principal(&mut conn, &request.target.administrator).await?;
    if !administrator(&mut conn).await? {
        return Err(Stop::Fixed("target administrator"));
    }
    let outcome = install_native_sealed_catalog(&mut conn).await.map_err(install_stop)?;
    let digest = target_installer::compiled_native_catalog_digest();
    Ok(format!(
        "{}\n",
        serde_json::to_string(&json!({
            "digest": digest,
            "outcome": outcome.to_string(),
            "protocol": INSTALL_RESULT
        }))
        .map_err(|_| Stop::Fixed("catalog output"))?
    ))
}

async fn read_facts(conn: &mut sqlx::PgConnection, request: &NativeRequest) -> Result<NativeFacts, Stop> {
    let text: String = sqlx::query_scalar(
        "SELECT _oo.full_import_native_catalog($1::uuid, $2, $3::uuid, $4, $5, $6, $7, $8, $9)::text",
    )
    .bind(&request.run_id)
    .bind(&request.source_id)
    .bind(&request.owner)
    .bind(&request.config_digest)
    .bind(request.source_generation)
    .bind(request.target_generation)
    .bind(&request.schema_digest)
    .bind(&request.type_digest)
    .bind(&request.input_digest)
    .fetch_one(&mut *conn)
    .await
    .map_err(map_db)?;
    let facts: NativeFacts = serde_json::from_str(&text).map_err(|_| Stop::Fixed("catalog output"))?;
    if facts.state != "awaiting_publish"
        || facts.run_id != request.run_id
        || facts.source_id != request.source_id
        || facts.config_digest != request.config_digest
        || facts.schema_digest != request.schema_digest
        || facts.type_digest != request.type_digest
        || facts.input_digest != request.input_digest
        || facts.source_generation != request.source_generation
        || facts.target_generation != request.target_generation
        || facts.owner != request.owner
    {
        return Err(Stop::Fixed("sealed import digest"));
    }
    Ok(facts)
}

fn bind_native(captured: &ResolvedCapture, facts: &NativeFacts, partition_vector: Option<Value>, pg_delta_bound: bool) -> Result<Vec<u8>, Stop> {
    native_binding::sealed_product(&captured.source_engine).map_err(stop_reason)?;
    if facts.tables.len() != 1 || captured.objects.len() != 1 || captured.fields.is_empty() {
        return Err(Stop::Fixed("schema"));
    }
    let stored_hook = hook_digest();
    if facts.tool_hook_digest != stored_hook {
        return Err(Stop::Fixed("sealed import digest"));
    }
    let table = &facts.tables[0];
    let object = &captured.objects[0];
    if table.table_index < 0
        || !name_agrees(&object.logical_schema, &table.schema_ident)
        || !name_agrees(&object.logical_table, &table.table_ident)
        || (partition_vector.is_none() && !pg_delta_bound && u64::try_from(table.applied_rows).ok() != Some(object.row_count))
        || table.applied_rows < 0
    {
        return Err(Stop::Fixed("schema"));
    }
    if !shadow_name(&table.stored_namespace, "pn_")
        || !shadow_name(&table.physical_name, "pt_")
        || !closed_digest(&table.canonical_digest)
    {
        return Err(Stop::Fixed("schema"));
    }
    if table.columns.len() != captured.fields.len() || table.columns.is_empty() || table.columns.len() > 64 {
        return Err(Stop::Fixed("physical mapping is absent"));
    }
    let mut columns = Vec::with_capacity(table.columns.len());
    let mut preimage = String::new();
    let mut previous_attnum = 0_i32;
    push_schema_table(&mut preimage, &table.schema_ident, &table.table_ident);
    for (field, column) in captured.fields.iter().zip(&table.columns) {
        if column.attnum <= previous_attnum {
            return Err(Stop::Fixed("physical mapping is absent"));
        }
        previous_attnum = column.attnum;
        if !shadow_name(&column.physical_name, "pc_") || !name_agrees(&field.name, &column.logical_column) {
            return Err(Stop::Fixed("schema"));
        }
        let storage = column_storage(field, column.atttypid, column.atttypmod).map_err(Stop::Fixed)?;
        if field.key && precision_loss(storage.oid, storage.typmod) {
            return Err(Stop::Fixed("precision"));
        }
        push_schema_column(
            &mut preimage,
            &field.name,
            storage.oid,
            storage.typmod,
            storage.caveat,
        );
        columns.push(json!({
            "caveat": storage.caveat,
            "key": field.key,
            "logical_name": field.name,
            "oid": storage.oid,
            "source_type": storage.source_type,
            "stored_physical": column.physical_name,
            "typmod": storage.typmod
        }));
    }
    let quality = one_batch(facts, &table.physical_name, table.applied_rows)?;
    let schema_digest = sha256_hex(preimage.as_bytes());
    let mut body = json!({
        "capture": "sealed_import",
        "config_digest": facts.config_digest,
        "engine": captured.source_engine,
        "hook_checked": false,
        "input_digest": facts.input_digest,
        "protocol": RESULT_PROTOCOL,
        "quality": quality,
        "run_id": facts.run_id,
        "schema_digest": schema_digest,
        "shadows_queried": false,
        "source_generation": facts.source_generation,
        "source_id": facts.source_id,
        "source_introspection": "not_used",
        "state": "awaiting_publish",
        "tables": [{
            "accepted_rows": table.applied_rows,
            "canonical_digest": table.canonical_digest,
            "columns": columns,
            "logical_schema": table.schema_ident,
            "logical_table": table.table_ident,
            "rejects": [],
            "stored_namespace": table.stored_namespace,
            "stored_physical": table.physical_name
        }],
        "target_generation": facts.target_generation,
        "tool_hook_digest": facts.tool_hook_digest,
        "type_digest": facts.type_digest
    });
    if let Some(vector) = partition_vector {
        body["partition_vector"] = vector;
    }
    let bytes = serde_json::to_vec(&body).map_err(|_| Stop::Fixed("catalog output"))?;
    if bytes.len() > oo_core::limits::DOCUMENT_MAX_BYTES {
        return Err(Stop::Fixed("catalog output"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| Stop::Fixed("catalog output"))?;
    if !text.contains(RESULT_PROTOCOL) || text.contains("oo.source.pgsql/v1") || text.contains("secret://") {
        return Err(Stop::Fixed("catalog output"));
    }
    Ok(bytes)
}

fn one_batch(facts: &NativeFacts, physical: &str, applied_rows: i64) -> Result<Value, Stop> {
    let batches: Vec<&QualityFact> = facts
        .quality
        .iter()
        .filter(|row| row.kind == "batch" && row.physical_name == physical)
        .collect();
    if batches.len() != 1 || facts.quality.len() != 1 {
        return Err(Stop::Fixed("quality identity"));
    }
    let row = batches[0];
    if row.batch_id.is_empty()
        || row.accepted_rows != Some(applied_rows)
        || row.rejected_rows != Some(0)
        || row.reason.is_some()
        || row.attnum.is_some()
        || row.value_len.is_some()
        || row.row_index.is_some()
        || row.key_digest.is_some()
    {
        return Err(Stop::Fixed("quality identity"));
    }
    Ok(json!({
        "loss_concealed": false,
        "manifest_reject_count": 0,
        "payload_includes_reject_lines": false,
        "stored_omits_manifest_rejects": false,
        "stored_rejected_rows": 0,
        "stored_rows": [{
            "accepted_rows": applied_rows,
            "attnum": null,
            "batch_id": row.batch_id,
            "key_digest": null,
            "kind": "batch",
            "physical_name": physical,
            "reason": null,
            "rejected_rows": 0,
            "row_index": null,
            "value_len": null
        }]
    }))
}

/// Plugin token for one binding field, plus the storage the sealed attribute must have.
pub(crate) fn column_storage(field: &BoundField, oid: i64, typmod: i32) -> Result<ColumnStorage, &'static str> {
    if field.char_length.is_some_and(|width| !(1..=63).contains(&width)) {
        return Err("schema");
    }
    if column_provenance(&field.arrow_type) == "closed" {
        return Err("schema");
    }
    let source_type = source_type_token(field)?;
    if !plugin_token(&source_type) || named_oid(&source_type).is_some_and(|named| named != oid) {
        return Err("schema");
    }
    let expected = expected_storage(field)?;
    let (oid, typmod) = if expected.utf8_flex {
        let varchar = oid == 1043 && typmod == -1;
        let char_n = oid == 1042 && typmod >= 5;
        if !varchar && !char_n {
            return Err("schema");
        }
        (oid, typmod)
    } else if oid == expected.oid && typmod == expected.typmod {
        (oid, typmod)
    } else {
        return Err("schema");
    };
    Ok(ColumnStorage {
        oid,
        typmod,
        source_type,
        caveat: expected.caveat,
    })
}

/// Closed source token. Arrow `int8` is not PostgreSQL `int8`.
/// A document field is `jsonb`, never `utf8` and never json oid 114.
pub(crate) fn source_type_token(field: &BoundField) -> Result<String, &'static str> {
    if let Some(bson) = field.bson_type.as_ref() {
        match (bson.variant.as_str(), bson.subtype.as_deref(), field.arrow_type.as_str()) {
            ("07", None, "fixedsizebinary(12)") => return Ok("objectid".to_string()),
            ("05", Some("04"), "fixedsizebinary(16)") => return Ok("uuid".to_string()),
            ("05", Some("00"), "binary" | "fixedsizebinary(12)" | "fixedsizebinary(16)") => {},
            ("07" | "05", _, _) => return Err("schema"),
            _ => {},
        }
    }
    if let Some(oid) = original_pg_storage(field)? {
        return Ok(if oid == 2950 { "uuid" } else { "text" }.to_string());
    }
    if field.document {
        return document_token(field).map(|()| "jsonb".to_string());
    }
    if field.arrow_type == "int8" {
        return Ok("arrow_int8".to_string());
    }
    if let Some((family, precision, scale)) = parse_decimal_spell(&field.arrow_type) {
        if field.precision != Some(precision) || field.scale != Some(scale) {
            return Err("precision");
        }
        return Ok(format!("{family}_{precision}_{scale}"));
    }
    if field.arrow_type == "decimal128" || field.arrow_type == "decimal256" {
        let precision = field.precision.ok_or("precision")?;
        let scale = field.scale.ok_or("precision")?;
        return Ok(format!("{}_{precision}_{scale}", field.arrow_type));
    }
    if let Some(width) = fixed_binary_width(&field.arrow_type) {
        return Ok(format!("fixedsizebinary_{width}"));
    }
    if field.arrow_type.chars().any(|ch| matches!(ch, '(' | ')' | ',' | ':' | ' ')) {
        let mut out = String::new();
        for ch in field.arrow_type.chars() {
            match ch {
                '(' | ',' | ':' | ' ' => out.push('_'),
                ')' => {}
                other => out.push(other),
            }
        }
        while out.contains("__") {
            out = out.replace("__", "_");
        }
        return Ok(out.trim_matches('_').to_string());
    }
    Ok(field.arrow_type.clone())
}

fn original_pg_storage(field: &BoundField) -> Result<Option<i64>, &'static str> {
    match field.source_oid {
        Some(2950) if fixed_binary_width(&field.arrow_type) == Some(16) && field.source_typmod == Some(-1) && field.source_format.as_ref().is_some_and(|value| !value.is_empty()) => Ok(Some(2950)),
        Some(25) if field.arrow_type == "utf8" && field.source_typmod == Some(-1) && field.source_format.as_ref().is_some_and(|value| !value.is_empty()) => Ok(Some(25)),
        Some(2950 | 25) => Err("schema"),
        _ => Ok(None),
    }
}

struct ExpectedStorage {
    oid: i64,
    typmod: i32,
    caveat: Option<&'static str>,
    utf8_flex: bool,
}

fn expected_storage(field: &BoundField) -> Result<ExpectedStorage, &'static str> {
    if let Some(bson) = field.bson_type.as_ref() {
        match (bson.variant.as_str(), bson.subtype.as_deref(), field.arrow_type.as_str()) {
            ("07", None, "fixedsizebinary(12)") => return Ok(exact(25, -1, None)),
            ("05", Some("04"), "fixedsizebinary(16)") => return Ok(exact(2950, -1, None)),
            ("05", Some("00"), "binary" | "fixedsizebinary(12)" | "fixedsizebinary(16)") => {},
            ("07" | "05", _, _) => return Err("schema"),
            _ => {},
        }
    }
    if let Some(oid) = original_pg_storage(field)? { return Ok(exact(oid, -1, None)); }
    if field.document {
        document_token(field)?;
        return Ok(exact(3802, -1, None));
    }
    if let Some((family, precision, scale)) = decimal_parts(field)? {
        let limit = if family == "decimal128" { 38 } else { 76 };
        if !(1..=limit).contains(&precision) || !(0..=precision).contains(&scale) {
            return Err("precision");
        }
        return Ok(exact(1700, numeric_typmod(precision, scale), None));
    }
    if let Some(width) = signed_width(&field.arrow_type) {
        require_width(field, width, true)?;
        let oid = match width {
            64 => 20,
            32 => 23,
            8 | 16 => 21,
            _ => return Err("schema"),
        };
        return Ok(exact(oid, -1, None));
    }
    match field.arrow_type.as_str() {
        "uint64" => {
            require_width(field, 64, false)?;
            Ok(exact(1700, numeric_typmod(20, 0), Some("bigint_unsigned_numeric")))
        }
        "uint8" => {
            require_width(field, 8, true)?;
            Ok(exact(21, -1, None))
        }
        "timestamp_nanos_utc" => {
            require_zone(field, Some("UTC"))?;
            require_no_numeric(field)?;
            Ok(exact(1700, numeric_typmod(20, 0), None))
        }
        "timestamp_second_naive" | "timestamp_millis_naive" | "timestamp_micros" | "date64" => {
            require_zone(field, None)?;
            require_no_numeric(field)?;
            Ok(exact(1114, -1, None))
        }
        "timestamp_second_utc" => {
            require_zone(field, Some("UTC"))?;
            require_no_numeric(field)?;
            Ok(exact(1184, -1, None))
        }
        "timestamp_micros_utc" => {
            if field.timezone.is_none() {
                return Err("shape");
            }
            require_no_numeric(field)?;
            Ok(exact(1184, -1, None))
        }
        "timestamp(ms,utc)" => {
            require_zone(field, None)?;
            require_no_numeric(field)?;
            Ok(exact(1184, -1, None))
        }
        "date32" => {
            require_zone(field, None)?;
            require_no_numeric(field)?;
            Ok(exact(1082, -1, None))
        }
        "utf8" => {
            require_no_numeric(field)?;
            if let Some(width) = field.char_length {
                Ok(exact(1042, width.saturating_add(4), None))
            } else {
                Ok(ExpectedStorage {
                    oid: 0,
                    typmod: 0,
                    caveat: None,
                    utf8_flex: true,
                })
            }
        }
        "time64_micros" => { require_no_numeric(field)?; Ok(exact(1083, -1, None)) }
        "duration_micros" => { require_no_numeric(field)?; Ok(exact(1186, -1, None)) }
        "time64_nanos" => { require_no_numeric(field)?; Ok(exact(20, -1, None)) }
        "binary" | "dictionary_binary" | "dictionary_int32_binary" => {
            require_no_numeric(field)?;
            Ok(exact(17, -1, None))
        }
        "bool" => {
            require_no_numeric(field)?;
            Ok(exact(16, -1, None))
        }
        "float64" => {
            require_no_numeric(field)?;
            Ok(exact(701, -1, None))
        }
        "list_int64" | "list_binary" | "list(utf8)" => {
            require_no_numeric(field)?;
            Ok(exact(114, -1, None))
        }
        other if fixed_binary_width(other).is_some() => {
            let width = fixed_binary_width(other).ok_or("schema")?;
            if !matches!(width, 12 | 16) {
                return Err("schema");
            }
            require_no_numeric(field)?;
            Ok(exact(17, -1, None))
        }
        other if (other.starts_with("struct(") || other.starts_with("union(dense,")) && other.ends_with(')') => {
            require_no_numeric(field)?;
            Ok(exact(114, -1, None))
        }
        _ => Err("schema"),
    }
}

fn document_token(field: &BoundField) -> Result<(), &'static str> {
    if field.arrow_type == "utf8"
        && field.precision.is_none()
        && field.scale.is_none()
        && field.char_length.is_none()
        && field.timezone.is_none()
    {
        Ok(())
    } else {
        Err("schema")
    }
}

fn exact(oid: i64, typmod: i32, caveat: Option<&'static str>) -> ExpectedStorage {
    ExpectedStorage {
        oid,
        typmod,
        caveat,
        utf8_flex: false,
    }
}

fn decimal_parts(field: &BoundField) -> Result<Option<(String, i32, i32)>, &'static str> {
    if let Some((family, precision, scale)) = parse_decimal_spell(&field.arrow_type) {
        if field.precision != Some(precision) || field.scale != Some(scale) {
            return Err("precision");
        }
        return Ok(Some((family.to_string(), precision, scale)));
    }
    if field.arrow_type == "decimal128" || field.arrow_type == "decimal256" {
        let precision = field.precision.ok_or("precision")?;
        let scale = field.scale.ok_or("precision")?;
        return Ok(Some((field.arrow_type.clone(), precision, scale)));
    }
    Ok(None)
}

fn signed_width(token: &str) -> Option<i32> {
    match token {
        "int8" => Some(8),
        "int16" => Some(16),
        "int32" => Some(32),
        "int64" => Some(64),
        _ => None,
    }
}

fn require_width(field: &BoundField, width: i32, allow_omit: bool) -> Result<(), &'static str> {
    if field.scale.is_some() {
        return Err("precision");
    }
    match field.precision {
        None if allow_omit => Ok(()),
        Some(value) if value == width => Ok(()),
        _ => Err("precision"),
    }
}

fn require_no_numeric(field: &BoundField) -> Result<(), &'static str> {
    if field.precision.is_some() || field.scale.is_some() {
        Err("precision")
    } else {
        Ok(())
    }
}

fn require_zone(field: &BoundField, expect: Option<&str>) -> Result<(), &'static str> {
    if field.timezone.as_deref() == expect {
        Ok(())
    } else {
        Err("shape")
    }
}

fn precision_loss(oid: i64, typmod: i32) -> bool {
    if matches!(oid, 700 | 701) {
        return true;
    }
    if oid != 1700 {
        return false;
    }
    match numeric_parts(typmod) {
        None => true,
        Some((precision, _)) if precision > 38 => true,
        Some(_) => false,
    }
}

fn numeric_parts(typmod: i32) -> Option<(i32, i32)> {
    if typmod < 0 {
        return None;
    }
    let adjusted = typmod - 4;
    let precision = (adjusted >> 16) & 0xFFFF;
    let scale = adjusted & 0xFFFF;
    Some((precision, scale))
}

fn named_oid(name: &str) -> Option<i64> {
    Some(match name {
        "bool" => 16,
        "bytea" => 17,
        "int8" => 20,
        "int2" => 21,
        "int4" => 23,
        "text" => 25,
        "json" => 114,
        "float4" => 700,
        "float8" => 701,
        "bpchar" => 1042,
        "varchar" => 1043,
        "date" => 1082,
        "time" => 1083,
        "timestamp" => 1114,
        "timestamptz" => 1184,
        "numeric" => 1700,
        "uuid" => 2950,
        "jsonb" => 3802,
        _ => return None,
    })
}

fn plugin_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
        })
}

fn name_agrees(original: &str, stored: &str) -> bool {
    if mapping_ident(original).ok().as_deref() == Some(stored) {
        return true;
    }
    let lower = original.to_ascii_lowercase();
    lower == stored && mapping_ident(&lower).ok().as_deref() == Some(stored)
}

fn shadow_name(value: &str, prefix: &str) -> bool {
    let Some(rest) = value.strip_prefix(prefix) else {
        return false;
    };
    rest.len() == 60 && rest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// Column storage recorded on the catalog. `caveat` is a closed compiler token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ColumnStorage {
    /// PostgreSQL oid of the sealed attribute.
    pub oid: i64,
    /// PostgreSQL typmod of the sealed attribute.
    pub typmod: i32,
    /// Plugin token. Not a PostgreSQL type name.
    pub source_type: String,
    /// Closed caveat, when the normalized type needs one.
    pub caveat: Option<&'static str>,
}

fn validate_closed(request: &NativeRequest) -> Result<(), &'static str> {
    if !closed_source(&request.source_id)
        || !closed_uuid(&request.run_id)
        || !closed_uuid(&request.owner)
        || !closed_digest(&request.config_digest)
        || !closed_digest(&request.schema_digest)
        || !closed_digest(&request.type_digest)
        || !closed_digest(&request.input_digest)
        || request.source_generation < 1
        || request.target_generation < 1
        || !closed_token(&request.snapshot.kind, 64)
        || !closed_token(&request.snapshot.value, 128)
        || !closed_token(&request.target.host, 253)
        || !closed_token(&request.target.database, 63)
        || !closed_token(&request.target.runtime_principal, 63)
        || request.target.port == 0
        || !request.target.password_file.starts_with('/')
        || !request.target.ca_file.starts_with('/')
        || request.target.password_file.contains('\0')
        || request.target.ca_file.contains('\0')
    {
        return Err("invalid input");
    }
    Ok(())
}

fn closed_token(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && !value.contains("://")
        && !value.to_ascii_lowercase().contains("password")
        && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

fn closed_source(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn closed_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn closed_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            let hyphen = matches!(index, 8 | 13 | 18 | 23);
            (hyphen && *byte == b'-') || (!hyphen && byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
}

async fn connect(
    host: &str,
    port: u16,
    database: &str,
    username: &str,
    password: &str,
    ca: &[u8],
) -> Result<sqlx::postgres::PgConnection, Stop> {
    let options = PgConnectOptions::from_explicit()
        .host(host)
        .port(port)
        .username(username)
        .password(password)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    if options.get_options().is_some() {
        return Err(Stop::Fixed("target connect"));
    }
    let options = options.disable_statement_logging();
    match tokio::time::timeout(Duration::from_secs(30), options.connect()).await {
        Ok(Ok(conn)) => Ok(conn),
        _ => Err(Stop::Fixed("target connect")),
    }
}

async fn expect_principal(conn: &mut sqlx::postgres::PgConnection, principal: &str) -> Result<(), Stop> {
    let row = sqlx::query("SELECT CURRENT_USER::text, SESSION_USER::text")
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| Stop::Fixed("target principal"))?;
    let current: String = row.try_get(0).map_err(|_| Stop::Fixed("target principal"))?;
    let session: String = row.try_get(1).map_err(|_| Stop::Fixed("target principal"))?;
    if current == principal && session == principal {
        Ok(())
    } else {
        Err(Stop::Fixed("target principal"))
    }
}

async fn set_timeout(conn: &mut sqlx::postgres::PgConnection) -> Result<(), Stop> {
    sqlx::raw_sql("SET statement_timeout = '30s'")
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(|_| Stop::Fixed("target database"))
}

async fn administrator(conn: &mut sqlx::postgres::PgConnection) -> Result<bool, Stop> {
    let row = sqlx::query("SELECT rolsuper FROM pg_catalog.pg_roles WHERE rolname = current_user")
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| Stop::Fixed("target database"))?;
    let superuser: bool = row.try_get(0).map_err(|_| Stop::Fixed("target database"))?;
    if superuser {
        return Ok(true);
    }
    let member: bool = sqlx::query_scalar(
        "SELECT pg_catalog.pg_has_role(current_user, 'oo_target_owner', 'MEMBER')",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(|_| Stop::Fixed("target administrator"))?;
    Ok(member)
}

fn map_db(err: sqlx::Error) -> Stop {
    let text = err.to_string();
    if text.contains("sealed import is absent") {
        Stop::Fixed("sealed import is absent")
    } else if text.contains("sealed import owner") {
        Stop::Fixed("sealed import owner")
    } else if text.contains("sealed import source") {
        Stop::Fixed("sealed import source")
    } else if text.contains("sealed import digest") || text.contains("sealed import identity is closed") {
        Stop::Fixed("sealed import digest")
    } else if text.contains("sealed import is partial") {
        Stop::Fixed("sealed import is partial")
    } else if text.contains("physical mapping is absent") {
        Stop::Fixed("physical mapping is absent")
    } else if text.contains("full_import_native_catalog") && text.contains("does not exist") {
        Stop::Fixed("native sealed catalog is not installed")
    } else {
        Stop::Fixed("target database")
    }
}

fn ready_stop(err: ReadinessError) -> Stop {
    match err {
        ReadinessError::NotInstalled | ReadinessError::Incomplete => {
            Stop::Fixed("native sealed catalog is not installed")
        }
        ReadinessError::ChecksumMismatch { .. } => Stop::Fixed("migration checksum"),
        ReadinessError::Database { .. } => Stop::Fixed("target database"),
    }
}

fn install_stop(err: InstallError) -> Stop {
    match err {
        InstallError::NotAdministrator => Stop::Fixed("target administrator"),
        InstallError::ChecksumMismatch { .. } => Stop::Fixed("migration checksum"),
        InstallError::MissingRelation { .. } | InstallError::Refused { .. } => {
            Stop::Fixed("target catalog refused")
        }
        InstallError::Database { sqlstate, .. } if sqlstate == "OO001" => Stop::Fixed("migration checksum"),
        InstallError::Database { .. } => Stop::Fixed("target database"),
    }
}

fn stop_reason(reason: &'static str) -> Stop {
    match reason {
        "source_engine_unknown" => Stop::Fixed("source engine"),
        other => Stop::Fixed(other),
    }
}

fn write_stdout(document: &[u8]) -> Result<(), u8> {
    match io::stdout().write_all(document) {
        Ok(()) => Ok(()),
        Err(_) => {
            eprintln!("oo-pipeline: catalog output");
            Err(1)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeRequest {
    protocol: String,
    run_id: String,
    source_id: String,
    owner: String,
    source_generation: i64,
    target_generation: i64,
    config_digest: String,
    schema_digest: String,
    type_digest: String,
    input_digest: String,
    snapshot: SnapshotIn,
    target: TargetIn,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotIn {
    kind: String,
    value: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetIn {
    host: String,
    port: u16,
    database: String,
    runtime_principal: String,
    password_file: String,
    ca_file: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallRequest {
    protocol: String,
    target: InstallTarget,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallTarget {
    host: String,
    port: u16,
    database: String,
    administrator: String,
    password_file: String,
    ca_file: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeFacts {
    state: String,
    run_id: String,
    source_id: String,
    source_generation: i64,
    target_generation: i64,
    config_digest: String,
    type_digest: String,
    input_digest: String,
    schema_digest: String,
    tool_hook_digest: String,
    owner: String,
    tables: Vec<TableFact>,
    quality: Vec<QualityFact>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TableFact {
    table_index: i32,
    schema_ident: String,
    table_ident: String,
    applied_rows: i64,
    canonical_digest: String,
    physical_name: String,
    stored_namespace: String,
    columns: Vec<ColumnFact>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnFact {
    attnum: i32,
    atttypid: i64,
    atttypmod: i32,
    logical_column: String,
    physical_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QualityFact {
    physical_name: String,
    batch_id: String,
    kind: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    value_len: Option<i64>,
    #[serde(default)]
    attnum: Option<i32>,
    #[serde(default)]
    row_index: Option<i64>,
    #[serde(default)]
    key_digest: Option<String>,
    #[serde(default)]
    accepted_rows: Option<i64>,
    #[serde(default)]
    rejected_rows: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(arrow_type: &str, precision: Option<i32>, scale: Option<i32>) -> BoundField {
        BoundField {
            name: "col".to_string(),
            arrow_type: arrow_type.to_string(),
            nullable: true,
            precision,
            scale,
            timezone: None,
            char_length: None,
            key: false,
            document: false,
            source_oid: None,
            source_typmod: None,
            source_format: None,
            bson_type: None,
        }
    }

    #[test]
    fn observed_pg_types_are_distinct_from_untyped_arrow() {
        assert!(name_agrees("OO04_FIX", &mapping_ident("OO04_FIX").unwrap()));
        assert!(name_agrees("T_BATCH", &mapping_ident("T_BATCH").unwrap()));
        assert!(!name_agrees("T_BATCH", &mapping_ident("T_WIDE").unwrap()));
        let mut uuid = field("fixedsizebinary(16)", None, None);
        assert_eq!(column_storage(&uuid, 17, -1).unwrap().oid, 17);
        uuid.source_oid = Some(2950);
        uuid.source_typmod = Some(-1);
        uuid.source_format = Some("uuid".to_string());
        assert_eq!(column_storage(&uuid, 2950, -1).unwrap().source_type, "uuid");
        assert!(column_storage(&uuid, 17, -1).is_err());
        uuid.arrow_type = "utf8".to_string();
        uuid.source_oid = Some(25);
        uuid.source_format = Some("text".to_string());
        assert_eq!(column_storage(&uuid, 25, -1).unwrap().source_type, "text");
    }

    #[test]
    fn admitted_bson_source_types_match_stored_oids() {
        use crate::native_binding::BsonType;
        let mut field = field("fixedsizebinary(12)", None, None);
        field.bson_type = Some(BsonType { variant: "07".to_string(), subtype: None });
        assert_eq!(column_storage(&field, 25, -1).unwrap().source_type, "objectid");
        assert!(column_storage(&field, 17, -1).is_err());
        field.arrow_type = "fixedsizebinary(16)".to_string();
        field.bson_type = Some(BsonType { variant: "05".to_string(), subtype: Some("04".to_string()) });
        assert_eq!(column_storage(&field, 2950, -1).unwrap().source_type, "uuid");
        assert!(column_storage(&field, 17, -1).is_err());
        field.bson_type = Some(BsonType { variant: "05".to_string(), subtype: Some("00".to_string()) });
        assert_eq!(column_storage(&field, 17, -1).unwrap().source_type, "fixedsizebinary_16");
    }

    #[test]
    fn engine_key_is_refused_before_a_connection() {
        let body = r#"{"protocol":"oo.pipeline.native-sealed-catalog/v1","engine":"mysql","source_id":"sr_w45"}"#;
        assert_eq!(classify_request(body).unwrap_err(), "source engine");
        let tagged = body.replace("\"engine\"", "\"source_engine\"");
        assert_eq!(classify_request(&tagged).unwrap_err(), "source engine");
        let authority = r#"{"protocol":"oo.pipeline.native-sealed-catalog/v1","admitted":true}"#;
        assert_eq!(classify_request(authority).unwrap_err(), "nonadmitted");
    }

    #[test]
    fn original_source_products_require_real_native_admission() {
        assert_eq!(native_binding::sealed_product("mysql"), Ok(()));
        assert_eq!(native_binding::sealed_product("postgresql"), Ok(()));
        assert_eq!(native_binding::sealed_product("postgres"), Ok(()));
        assert_eq!(native_binding::sealed_product("sqlserver"), Ok(()));
        assert_eq!(native_binding::sealed_product("snowflake"), Err("source_engine_unknown"));
        assert_eq!(native_binding::sealed_product("starrocks"), Ok(()));
        assert_eq!(native_binding::sealed_product("doris"), Ok(()));
        assert_eq!(native_binding::sealed_product("clickhouse"), Ok(()));
        assert_eq!(native_binding::sealed_product("oracle"), Ok(()));
        assert_eq!(native_binding::sealed_product("mongodb"), Ok(()));
        assert_eq!(native_binding::sealed_product("mariadb"), Ok(()));
    }

    #[test]
    fn source_tokens_keep_precision_and_do_not_use_pg_names() {
        let decimal = field("decimal128(38,9)", Some(38), Some(9));
        assert_eq!(source_type_token(&decimal).unwrap(), "decimal128_38_9");
        let wide = field("decimal256(76,10)", Some(76), Some(10));
        assert_eq!(source_type_token(&wide).unwrap(), "decimal256_76_10");
        let fixed = field("fixedsizebinary(12)", None, None);
        assert_eq!(source_type_token(&fixed).unwrap(), "fixedsizebinary_12");
        let millis = field("timestamp(ms,utc)", None, None);
        assert_eq!(source_type_token(&millis).unwrap(), "timestamp_ms_utc");
        let unioned = field("union(dense,0:utf8,1:int64)", None, None);
        assert_eq!(source_type_token(&unioned).unwrap(), "union_dense_0_utf8_1_int64");
        let structured = field("struct(kind:utf8,n:int32)", None, None);
        assert_eq!(source_type_token(&structured).unwrap(), "struct_kind_utf8_n_int32");
        let list = field("list(utf8)", None, None);
        assert_eq!(source_type_token(&list).unwrap(), "list_utf8");
        let narrow = field("int8", None, None);
        assert_eq!(source_type_token(&narrow).unwrap(), "arrow_int8");
        assert_ne!(source_type_token(&narrow).unwrap(), "int8");
        assert_ne!(source_type_token(&narrow).unwrap(), "int2");
    }

    #[test]
    fn sealed_attribute_types_keep_char_decimal_and_seconds() {
        let int64 = field("int64", None, None);
        let stored = column_storage(&int64, 20, -1).unwrap();
        assert_eq!(stored.source_type, "int64");
        assert_eq!(stored.oid, 20);
        assert!(column_storage(&field("int64", Some(32), None), 20, -1).is_err());
        let seconds = field("timestamp_second_naive", None, None);
        let stored = column_storage(&seconds, 1114, -1).unwrap();
        assert_eq!(stored.oid, 1114);
        assert_eq!(stored.typmod, -1);
        assert_eq!(column_provenance("timestamp_second_naive"), "timestamp seconds since unix epoch stored as timestamp; microseconds are seconds * 1000000");
        let day = field("date32", None, None);
        assert_eq!(column_storage(&day, 1082, -1).unwrap().oid, 1082);
        assert_ne!(column_provenance("date32"), "closed");
        let mut char_field = field("utf8", None, None);
        char_field.char_length = Some(6);
        let stored = column_storage(&char_field, 1042, 10).unwrap();
        assert_eq!(stored.typmod, 10);
        assert_eq!(stored.source_type, "utf8");
        assert!(column_storage(&char_field, 1043, -1).is_err());
        let mut text = field("utf8", None, None);
        assert_eq!(column_storage(&text, 1043, -1).unwrap().typmod, -1);
        assert_eq!(column_storage(&text, 1042, 10).unwrap().typmod, 10);
        text.char_length = None;
        assert!(column_storage(&text, 25, -1).is_err());
        let wide = field("decimal256(76,10)", Some(76), Some(10));
        let stored = column_storage(&wide, 1700, numeric_typmod(76, 10)).unwrap();
        assert_eq!(stored.typmod, 4_980_750);
        assert_eq!(stored.source_type, "decimal256_76_10");
        assert!(stored.caveat.is_none());
        let mut nanos = field("timestamp_nanos_utc", None, None);
        nanos.timezone = Some("UTC".to_string());
        let stored = column_storage(&nanos, 1700, numeric_typmod(20, 0)).unwrap();
        assert!(stored.caveat.is_none());
        let unsigned = field("uint64", Some(64), None);
        let stored = column_storage(&unsigned, 1700, numeric_typmod(20, 0)).unwrap();
        assert_eq!(stored.caveat, Some("bigint_unsigned_numeric"));
        let bytes = field("binary", None, None);
        assert_eq!(column_storage(&bytes, 17, -1).unwrap().source_type, "binary");
        assert_ne!(column_provenance("binary"), "closed");
        assert_eq!(numeric_typmod(38, 9), 2_490_381);
        let mut document = field("utf8", None, None);
        document.document = true;
        let stored = column_storage(&document, 3802, -1).unwrap();
        assert_eq!(stored.source_type, "jsonb");
        assert_eq!((stored.oid, stored.typmod), (3802, -1));
        assert!(column_storage(&document, 114, -1).is_err());
        assert!(column_storage(&document, 3802, 0).is_err());
        assert!(column_storage(&document, 1043, -1).is_err());
        let mut widened = document.clone();
        widened.char_length = Some(6);
        assert_eq!(source_type_token(&widened), Err("schema"));
        let mut binary = document.clone();
        binary.arrow_type = "binary".to_string();
        assert_eq!(source_type_token(&binary), Err("schema"));
        let plain = field("utf8", None, None);
        assert_eq!(source_type_token(&plain).unwrap(), "utf8");
        assert!(column_storage(&plain, 3802, -1).is_err());
        assert_eq!(column_storage(&plain, 114, -1).unwrap_err(), "schema");
        let listed = field("list(utf8)", None, None);
        assert_eq!(column_storage(&listed, 114, -1).unwrap().oid, 114);
        assert_ne!(column_storage(&listed, 114, -1).unwrap().source_type, "jsonb");
    }

    #[test]
    fn native_catalog_script_reads_attributes_and_does_not_copy() {
        let script = include_str!("../migrations/0008_native_sealed_catalog.sql");
        assert!(script.contains("full_import_native_catalog"));
        assert!(script.contains("pg_attribute"));
        assert!(script.contains("sealed import is absent"));
        assert!(script.contains("sealed import owner"));
        assert!(script.contains("sealed import source"));
        assert!(script.contains("sealed import digest"));
        assert!(script.contains("sealed import is partial"));
        assert!(script.contains("physical mapping is absent"));
        assert!(!script.contains("COPY"));
        assert!(!script.contains("set_config"));
        assert!(!script.to_ascii_lowercase().contains("select *"));
    }
}
