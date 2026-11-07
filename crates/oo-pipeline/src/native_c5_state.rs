use std::io::{Read, Write};

use serde_json::{Value, json};
use sqlx::Connection;

const MULTIPART_INITIALIZE_VERSION: &str = "native-c5-multipart-initialize/v1";
const MULTIPART_INITIALIZE_SQL: &str =
    include_str!("../migrations/0065_native_c5_multipart_initialize.sql");
const PUBLISHED_INITIALIZE_VERSION: &str = "native-c5-published-baseline/v1";
const PUBLISHED_INITIALIZE_SQL: &str =
    include_str!("../migrations/0069_native_c5_published_baseline.sql");
const EMPTY_INITIALIZE_VERSION: &str = "native-c5-empty-baseline/v1";
const EMPTY_INITIALIZE_SQL: &str = include_str!("../migrations/0071_native_c5_empty_baseline.sql");
const ORACLE_RANGE_SQL: &str = include_str!("../migrations/0090_native_c5_oracle_range.sql");

fn validate_oracle_state(
    document: &Value,
) -> Result<crate::native_c5_range::OracleSnapshot, &'static str> {
    let initialize = document["operation"] == "initialize_oracle_ranges";
    if document["operation"] != "observe_oracle_unchanged" && !initialize {
        return Err("c5_oracle_operation");
    }
    let allowed = if initialize {
        &[
            "protocol",
            "operation",
            "source_id",
            "schema",
            "table",
            "target",
            "expected_source_generation",
            "oracle_snapshot",
            "run_id",
            "schema_fingerprint",
            "key_column",
            "chunks",
        ][..]
    } else {
        &[
            "protocol",
            "operation",
            "source_id",
            "schema",
            "table",
            "target",
            "expected_source_generation",
            "oracle_snapshot",
            "chunk_id",
            "rows",
        ][..]
    };
    if document
        .as_object()
        .is_none_or(|o| o.keys().any(|k| !allowed.contains(&k.as_str())))
        || document["expected_source_generation"]
            .as_i64()
            .is_none_or(|n| n < 1)
    {
        return Err("c5_oracle_scope");
    }
    let oracle: crate::native_c5_range::OracleSnapshot =
        serde_json::from_value(document["oracle_snapshot"].clone())
            .map_err(|_| "c5_oracle_snapshot")?;
    if !oracle.valid()
        || oracle.source_schema != text(document, "schema")?
        || oracle.source_table != text(document, "table")?
    {
        return Err("c5_oracle_snapshot");
    }
    if initialize {
        text(document, "run_id")?;
        text(document, "schema_fingerprint")?;
        text(document, "key_column")?;
        if document["chunks"]
            .as_array()
            .is_none_or(|v| v.is_empty() || v.len() > 4096)
        {
            return Err("c5_oracle_topology");
        }
    } else {
        text(document, "chunk_id")?;
        if document["rows"]
            .as_array()
            .is_none_or(|v| v.len() > 100_000)
        {
            return Err("c5_oracle_rows");
        }
    }
    Ok(oracle)
}

async fn execute_oracle_state(document: &Value) -> Result<Value, &'static str> {
    validate_oracle_state(document)?;
    let mut conn = crate::native_update::connect_target(&document["target"]).await?;
    let installed: Option<String> = sqlx::query_scalar(
        "SELECT checksum FROM _oo.schema_migrations WHERE version='native-c5-oracle-range/v1'",
    )
    .fetch_optional(&mut conn)
    .await
    .map_err(|_| "c5_oracle_schema")?;
    if installed.as_deref()
        != Some(crate::native_c5_multipart::digest(ORACLE_RANGE_SQL.as_bytes()).as_str())
    {
        return Err("c5_oracle_schema");
    }
    let mut tx = conn.begin().await.map_err(|_| "c5_oracle_state")?;
    let source = text(document, "source_id")?;
    let expected = document["expected_source_generation"]
        .as_i64()
        .ok_or("c5_oracle_scope")?;
    let context = document["oracle_snapshot"].to_string();
    let raw: String = if document["operation"] == "initialize_oracle_ranges" {
        sqlx::query_scalar(
            "SELECT _oo.c5_oracle_initialize($1,$2::uuid,$3::jsonb,$4,$5,$6::jsonb)::text",
        )
        .bind(source)
        .bind(text(document, "run_id")?)
        .bind(&context)
        .bind(text(document, "schema_fingerprint")?)
        .bind(text(document, "key_column")?)
        .bind(document["chunks"].to_string())
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| "c5_oracle_initialize")?
    } else {
        sqlx::query(
            "SELECT source_generation FROM _oo.source_watermark WHERE source=$1 FOR UPDATE",
        )
        .bind(source)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| "c5_oracle_scope")?;
        sqlx::query_scalar("SELECT _oo.c5_oracle_check_unchanged($1,$2::jsonb,$3,$4::jsonb)::text")
            .bind(source)
            .bind(&context)
            .bind(text(document, "chunk_id")?)
            .bind(document["rows"].to_string())
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| "c5_oracle_observation")?
    };
    let actual: i64 =
        sqlx::query_scalar("SELECT source_generation FROM _oo.source_watermark WHERE source=$1")
            .bind(source)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| "c5_oracle_scope")?;
    if actual != expected {
        return Err("c5_oracle_stale_generation");
    }
    let result: Value = serde_json::from_str(&raw).map_err(|_| "c5_oracle_state")?;
    tx.commit().await.map_err(|_| "c5_oracle_state")?;
    Ok(
        json!({"protocol":"oo.pipeline.native-c5-oracle-state/v1","source_id":source,
        "source_generation":actual,"data_gen":false,"result":result}),
    )
}

pub async fn execute(document: &Value) -> Result<Value, &'static str> {
    if document["protocol"] != "oo.pipeline.native-c5-state/v1" {
        return Err("invalid_version");
    }
    let operation = text(document, "operation")?;
    if matches!(
        operation,
        "initialize_oracle_ranges" | "observe_oracle_unchanged"
    ) {
        return execute_oracle_state(document).await;
    }
    let source = text(document, "source_id")?;
    let schema = text(document, "schema")?;
    let table = text(document, "table")?;
    if matches!(
        operation,
        "initialize_multipart_if_absent"
            | "initialize_published_if_absent"
            | "initialize_empty_if_absent"
    ) {
        validate_multipart_initialize(document)?;
    }
    if operation == "observe"
        && document["nochange"]["c5_observation"]["protocol"] == "oo.c5.empty-observation/v1"
    {
        validate_empty_observation(document)?;
    }
    if operation == "observe_partial" {
        validate_partial_observation(document)?;
    }
    if operation == "hydrate_multipart" {
        validate_hydration_request(document)?;
    }
    if matches!(operation, "initialize_full" | "initialize_full_if_absent") {
        if ["lo", "hi", "chunks", "expected_chunks", "boundaries"]
            .iter()
            .any(|key| document.get(*key).is_some())
        {
            return Err("c5_bounded_baseline_unsupported");
        }
        if [schema, table]
            .iter()
            .any(|name| match crate::native_arrow::mapping_ident(name) {
                Ok(mapped) => mapped != *name,
                Err(()) => true,
            })
        {
            return Err("c5_mapped_names_unsupported");
        }
    }
    let mut conn = crate::native_update::connect_target(&document["target"]).await?;
    if operation == "export" {
        let value: Option<String> =
            sqlx::query_scalar("SELECT _oo.c5_authority_export($1,$2,$3)::text")
                .bind(source)
                .bind(schema)
                .bind(table)
                .fetch_one(&mut conn)
                .await
                .map_err(|_| "c5_state")?;
        let value: Value =
            serde_json::from_str(&value.ok_or("c5_state_missing")?).map_err(|_| "c5_state")?;
        if let Some(path) = document["authority_file"].as_str() {
            let path = std::path::Path::new(path);
            if !path.is_absolute() || !path.parent().is_some_and(|parent| parent.is_dir()) {
                return Err("c5_authority_file");
            }
            let temp = path.with_extension(format!("{}.pending", std::process::id()));
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp).map_err(|_| "c5_authority_file")?;
            file.write_all(&serde_json::to_vec(&value).map_err(|_| "c5_state")?)
                .map_err(|_| "c5_authority_file")?;
            file.sync_all().map_err(|_| "c5_authority_file")?;
            std::fs::rename(&temp, path).map_err(|_| "c5_authority_file")?;
            std::fs::File::open(path.parent().ok_or("c5_authority_file")?)
                .and_then(|dir| dir.sync_all())
                .map_err(|_| "c5_authority_file")?;
        }
        return Ok(value);
    }
    // Run the transaction, then always release the session-level shard fence
    // that adopt_target_generation took and close the connection gracefully.
    // Leaving both to the connection drop let the next call race the old
    // backend's exit and fail pg_try_advisory_lock as "fence".
    let outcome: Result<Value, &'static str> = async {
        let mut tx = conn.begin().await.map_err(|_| "c5_state")?;
        crate::native_update::lease_hold(
            &mut tx,
            text(document, "lease_id")?,
            Some(
                document["lease_generation"]
                    .as_i64()
                    .filter(|generation| *generation > 0)
                    .ok_or("lease_required")?,
            ),
        )
        .await?;
        let shard = text(&document["target"], "shard")?;
        let owner = text(&document["target"], "owner")?;
        let fence: i64 = sqlx::query_scalar("SELECT _oo.adopt_target_generation($1,$2::uuid)")
            .bind(shard)
            .bind(owner)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| "fence")?;
        sqlx::query("SELECT _oo.full_import_bind_session($1,$2::uuid,$3)")
            .bind(shard)
            .bind(owner)
            .bind(fence)
            .execute(&mut *tx)
            .await
            .map_err(|_| "fence")?;
        let result = match operation {
            "initialize_empty_if_absent" => {
                require_empty_schema(&mut tx).await?;
                let raw: String = sqlx::query_scalar(
                    "SELECT _oo.c5_initialize_empty_if_absent($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9::uuid,$10)::text",
                )
                .bind(shard)
                .bind(owner)
                .bind(fence)
                .bind(source)
                .bind(schema)
                .bind(table)
                .bind(text(document, "schema_fingerprint")?)
                .bind(document["expected_source_generation"].as_i64().ok_or("c5_state_scope")?)
                .bind(text(document, "lease_id")?)
                .bind(document["lease_generation"].as_i64().ok_or("lease_required")?)
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| "c5_empty_initialize")?;
                serde_json::from_str(&raw).map_err(|_| "c5_state")?
            }
            "initialize_multipart_if_absent" | "initialize_published_if_absent" => {
                let type_preimages = widening_type_preimages(document)?;
                let adaptation = if operation == "initialize_published_if_absent" {
                    let installed: Option<String> = sqlx::query_scalar(
                        "SELECT checksum FROM _oo.schema_migrations WHERE version=$1",
                    )
                    .bind(crate::target_installer::VERSION_NATIVE_C5_WIDENING_REBASE)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|_| "c5_widening_schema")?;
                    if let Some(installed) = installed {
                        if installed
                            != crate::target_installer::compiled_native_c5_widening_rebase_digest()
                        {
                            return Err("c5_widening_schema");
                        }
                        let type_receipt: Option<String> = sqlx::query_scalar(
                            "SELECT checksum FROM _oo.schema_migrations WHERE version=$1",
                        )
                        .bind(crate::target_installer::VERSION_NATIVE_C5_WIDENING_TYPE_RECEIPT)
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(|_| "c5_widening_schema")?;
                        if type_receipt.as_deref() != Some(
                            crate::target_installer::compiled_native_c5_widening_type_receipt_digest().as_str()) {
                            return Err("c5_widening_schema");
                        }
                        let statement = if type_preimages.is_some() {
                            let installed: Option<String> = sqlx::query_scalar(
                                "SELECT checksum FROM _oo.schema_migrations WHERE version=$1",
                            )
                            .bind(crate::target_installer::VERSION_NATIVE_C5_WIDENING_SOURCE_TYPES)
                            .fetch_optional(&mut *tx)
                            .await
                            .map_err(|_| "c5_widening_schema")?;
                            if installed.as_deref() != Some(
                                crate::target_installer::compiled_native_c5_widening_source_types_digest().as_str()) {
                                return Err("c5_widening_schema");
                            }
                            "SELECT _oo.c5_rebase_widening_if_needed($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9::text[],$10::uuid,$11,$12,$13)::text"
                        } else {
                            "SELECT _oo.c5_rebase_widening_if_needed($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9::text[],$10::uuid,$11)::text"
                        };
                        let mut query = sqlx::query_scalar::<_, String>(statement)
                            .bind(shard)
                            .bind(owner)
                            .bind(fence)
                            .bind(source)
                            .bind(schema)
                            .bind(table)
                            .bind(text(document, "schema_fingerprint")?)
                            .bind(
                                document["expected_source_generation"]
                                    .as_i64()
                                    .ok_or("c5_state_scope")?,
                            )
                            .bind(
                                document["key_columns"]
                                    .as_array()
                                    .ok_or("c5_keys")?
                                    .iter()
                                    .map(|key| key.as_str().ok_or("c5_keys"))
                                    .collect::<Result<Vec<_>, _>>()?,
                            )
                            .bind(text(document, "lease_id")?)
                            .bind(
                                document["lease_generation"]
                                    .as_i64()
                                    .ok_or("lease_required")?,
                            );
                        if let Some((old, new)) = &type_preimages {
                            query = query.bind(old).bind(new);
                        }
                        let value = query
                            .fetch_one(&mut *tx)
                            .await
                            .map_err(|_| "c5_widening_rebase")?;
                        let value: Value =
                            serde_json::from_str(&value).map_err(|_| "c5_widening_rebase")?;
                        if !value["rebased"].is_boolean()
                            || (value["rebased"] == true
                                && (value["published"] != false || value["acknowledged"] != false))
                        {
                            return Err("c5_widening_rebase");
                        }
                        Some(value)
                    } else {
                        if type_preimages.is_some() {
                            return Err("c5_widening_schema");
                        }
                        None
                    }
                } else {
                    None
                };
                let statement = if operation == "initialize_published_if_absent" {
                    require_published_schema(&mut tx).await?;
                    "SELECT _oo.c5_initialize_published_if_absent($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9::text[],$10)::text"
                } else {
                    require_multipart_schema(&mut tx).await?;
                    "SELECT _oo.c5_initialize_multipart_if_absent($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9::text[],$10)::text"
                };
                let result: String = sqlx::query_scalar(statement)
                    .bind(shard)
                    .bind(owner)
                    .bind(fence)
                    .bind(source)
                    .bind(schema)
                    .bind(table)
                    .bind(text(document, "schema_fingerprint")?)
                    .bind(
                        document["chunk_topology_revision"]
                            .as_i64()
                            .ok_or("c5_topology")?,
                    )
                    .bind(
                        document["key_columns"]
                            .as_array()
                            .ok_or("c5_keys")?
                            .iter()
                            .map(|v| v.as_str().ok_or("c5_keys"))
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .bind(
                        document["expected_source_generation"]
                            .as_i64()
                            .ok_or("c5_state_scope")?,
                    )
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|error| {
                        if error
                            .as_database_error()
                            .is_some_and(|e| e.code().as_deref() == Some("OO010"))
                        {
                            "c5_state_scope"
                        } else if operation == "initialize_published_if_absent" {
                            "c5_published_initialize"
                        } else {
                            "c5_multipart_initialize"
                        }
                    })?;
                let mut result: Value = serde_json::from_str(&result).map_err(|_| "c5_state")?;
                if let Some(adaptation) = adaptation.filter(|value| value["rebased"] == true) {
                    result["schema_adaptation"] = adaptation;
                }
                result
            }
            "hydrate_multipart" => {
                require_multipart_schema(&mut tx).await?;
                let chunk = text(document, "chunk_id")?;
                let generation = document["expected_source_generation"]
                    .as_i64()
                    .ok_or("c5_state_scope")?;
                let raw: String = sqlx::query_scalar(
                    "SELECT _oo.c5_multipart_hydration_manifest($1,$2::uuid,$3,$4,$5,$6,$7,$8)::text",
                )
                .bind(shard)
                .bind(owner)
                .bind(fence)
                .bind(source)
                .bind(schema)
                .bind(table)
                .bind(chunk)
                .bind(generation)
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| "c5_hydration_baseline")?;
                let authority: Value =
                    serde_json::from_str(&raw).map_err(|_| "c5_hydration_baseline")?;
                if authority["baseline_protocol"] == "oo.c5.published-baseline/v1" {
                    require_published_schema(&mut tx).await?;
                }
                let bytes = read_private_part(text(document, "arrow_path")?)?;
                let members = hydration_members(&authority, &bytes)?;
                let seed = archive_digest(&authority, &bytes)?;
                let result: String = sqlx::query_scalar(
                    "SELECT _oo.c5_hydrate_multipart($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9,$10::jsonb)::text",
                )
                .bind(shard)
                .bind(owner)
                .bind(fence)
                .bind(source)
                .bind(schema)
                .bind(table)
                .bind(chunk)
                .bind(generation)
                .bind(text(&authority, "arrow_sha256")?)
                .bind(serde_json::to_string(&members).map_err(|_| "c5_hydration_keys")?)
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| "c5_hydration_membership")?;
                if let Some(digest) = &seed {
                    sqlx::query("SELECT _oo.c5_seed_multipart_digest($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9,$10,$11::jsonb)")
                        .bind(shard).bind(owner).bind(fence).bind(source).bind(schema).bind(table)
                        .bind(chunk).bind(generation).bind(text(&authority, "arrow_sha256")?)
                        .bind(text(&authority["baseline"], "projection_sha256")?).bind(digest.to_string())
                        .execute(&mut *tx).await.map_err(|_| "c5_archive_digest")?;
                }
                let mut result: Value = serde_json::from_str(&result).map_err(|_| "c5_state")?;
                result["archive_digest_seeded"] = json!(seed.is_some());
                result
            }
            "observe_partial" => {
                require_multipart_schema(&mut tx).await?;
                if document.get("loader_receipt").is_some() {
                    let scope = partial_range_scope(&document["range_scope"])?;
                    let guard = if scope["protocol"] == crate::native_c5_parts::RANGE_PROTOCOL {
                        "SELECT _oo.c5_parts_observation_receipt_guard($1,$2::uuid,$3,$4::jsonb,$5::jsonb)"
                    } else {
                        "SELECT _oo.c5_observation_receipt_guard($1,$2::uuid,$3,$4::jsonb,$5::jsonb)"
                    };
                    sqlx::query(guard)
                        .bind(shard)
                        .bind(owner)
                        .bind(fence)
                        .bind(scope.to_string())
                        .bind(document["loader_receipt"].to_string())
                        .execute(&mut *tx)
                        .await
                        .map_err(|_| "c5_observation_receipt")?;
                }
                let observation = &document["observation"];
                let chunks = observation["chunks"]
                    .as_array()
                    .cloned()
                    .unwrap_or_else(|| vec![observation["chunk"].clone()]);
                let mut result = Value::Null;
                for chunk in chunks {
                    let mut part = observation.clone();
                    part["chunk"] = chunk;
                    let raw: String = sqlx::query_scalar(
                        "SELECT _oo.c5_observe_partial($1,$2::uuid,$3,$4,$5,$6,$7,$8::jsonb)::text",
                    )
                    .bind(shard)
                    .bind(owner)
                    .bind(fence)
                    .bind(source)
                    .bind(schema)
                    .bind(table)
                    .bind(
                        document["expected_source_generation"]
                            .as_i64()
                            .ok_or("c5_state_scope")?,
                    )
                    .bind(part.to_string())
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|_| "c5_partial_observation")?;
                    result = serde_json::from_str(&raw).map_err(|_| "c5_state")?;
                }
                if result.is_null() {
                    return Err("c5_partial_observation");
                }
                result
            }
            "record_rejects" => {
                let masked =
                    crate::masked_rejects::project(source, schema, table, &document["rejects"])?;
                let observation = text(document, "observation_sha256")?;
                let stored: Option<String> =
                    sqlx::query_scalar("SELECT checksum FROM _oo.schema_migrations WHERE version=$1")
                        .bind(crate::target_installer::VERSION_MASKED_REJECTS)
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(|_| "reject_schema")?;
                if stored.as_deref()
                    != Some(crate::target_installer::compiled_masked_rejects_digest().as_str())
                {
                    return Err("reject_schema");
                }
                let written: i64 = sqlx::query_scalar(
                    "SELECT _oo.record_masked_rejects($1,$2::uuid,$3,$4,$5,$6,$7,$8::jsonb)",
                )
                .bind(shard)
                .bind(owner)
                .bind(fence)
                .bind(source)
                .bind(schema)
                .bind(table)
                .bind(observation)
                .bind(serde_json::to_string(&masked).map_err(|_| "reject_records")?)
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| "reject_write")?;
                json!({"rejects_written":written,"data_gen":false,"apply":false,"ack":false,"receipt":null})
            }
            "initialize_full" | "initialize_full_if_absent" => {
                let existing: Option<String> =
                    sqlx::query_scalar("SELECT _oo.c5_authority_export($1,$2,$3)::text")
                        .bind(source)
                        .bind(schema)
                        .bind(table)
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(|_| "c5_state")?;
                let existing: Option<Value> = existing
                    .map(|v| serde_json::from_str(&v))
                    .transpose()
                    .map_err(|_| "c5_state")?;
                let initialized = operation == "initialize_full_if_absent" && existing.is_some();
                if initialized {
                    let state = existing.as_ref().ok_or("c5_state")?;
                    let chunks = state["chunks"].as_array().ok_or("c5_state")?;
                    if chunks.len() != 1 || !chunks[0]["lo"].is_null() || !chunks[0]["hi"].is_null() {
                        return Err("c5_bounded_baseline_unsupported");
                    }
                    if state["schema_fingerprint"] != document["schema_fingerprint"]
                        || state["chunk_topology_revision"] != document["chunk_topology_revision"]
                        || state["source_generation"] != document["expected_source_generation"]
                    {
                        return Err("c5_state_scope");
                    }
                } else {
                    sqlx::query(
                        "SELECT _oo.c5_initialize_full($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9,$10::text[])",
                    )
                    .bind(shard)
                    .bind(owner)
                    .bind(fence)
                    .bind(source)
                    .bind(schema)
                    .bind(table)
                    .bind(text(document, "schema_fingerprint")?)
                    .bind(
                        document["chunk_topology_revision"]
                            .as_i64()
                            .ok_or("c5_topology")?,
                    )
                    .bind(text(document, "chunk_id")?)
                    .bind(
                        document["key_columns"]
                            .as_array()
                            .ok_or("c5_keys")?
                            .iter()
                            .map(|v| v.as_str().ok_or("c5_keys"))
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| {
                        if error.as_database_error().is_some_and(|e| {
                            e.message().starts_with("C5 multipart baseline unsupported")
                        }) {
                            "c5_multipart_baseline_unsupported"
                        } else {
                            "c5_initialize"
                        }
                    })?;
                }
                json!({"initialized":true,"receipt":null})
            }
            "begin_scan" => {
                let next: i64 = sqlx::query_scalar(
                    "SELECT _oo.c5_scan_begin($1,$2::uuid,$3,$4,$5,$6,$7,$8::jsonb)",
                )
                .bind(shard)
                .bind(owner)
                .bind(fence)
                .bind(source)
                .bind(schema)
                .bind(table)
                .bind(text(document, "scan_id")?)
                .bind(document["expected_chunks"].to_string())
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| "c5_scan")?;
                if document.get("scan_layers").is_some()
                    || document.get("scan_observed_at_ms").is_some()
                {
                    require_multipart_schema(&mut tx).await?;
                    sqlx::query(
                        "SELECT _oo.c5_scan_context($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9::jsonb,$10)",
                    )
                    .bind(shard)
                    .bind(owner)
                    .bind(fence)
                    .bind(source)
                    .bind(schema)
                    .bind(table)
                    .bind(text(document, "scan_id")?)
                    .bind(
                        document["expected_source_generation"]
                            .as_i64()
                            .ok_or("c5_scan")?,
                    )
                    .bind(document["scan_layers"].to_string())
                    .bind(document["scan_observed_at_ms"].as_i64().ok_or("c5_scan")?)
                    .execute(&mut *tx)
                    .await
                    .map_err(|_| "c5_scan_context")?;
                }
                json!({"next_chunk_index":next})
            }
            "observe" => {
                let nochange = &document["nochange"];
                if nochange["protocol"] != "oo.jobs.native-c5-nochange/v1"
                    || nochange["data_gen"] != false
                    || nochange["apply"] != false
                    || nochange["ack"] != false
                    || nochange["source_id"] != source
                    || nochange["source_generation"] != nochange["acknowledged_source_generation"]
                {
                    return Err("c5_nochange");
                }
                if nochange["c5_observation"]["protocol"] == "oo.c5.empty-observation/v1" {
                    require_empty_schema(&mut tx).await?;
                    let raw: String = sqlx::query_scalar(
                        "SELECT _oo.c5_observe_empty($1,$2::uuid,$3,$4,$5,$6,$7,$8::uuid,$9,$10::jsonb)::text",
                    )
                    .bind(shard)
                    .bind(owner)
                    .bind(fence)
                    .bind(source)
                    .bind(schema)
                    .bind(table)
                    .bind(nochange["source_generation"].as_i64().ok_or("c5_nochange")?)
                    .bind(text(document, "lease_id")?)
                    .bind(document["lease_generation"].as_i64().ok_or("lease_required")?)
                    .bind(nochange.to_string())
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|_| "c5_empty_observation")?;
                    let result = serde_json::from_str(&raw).map_err(|_| "c5_state")?;
                    tx.commit().await.map_err(|_| "c5_state")?;
                    return Ok(result);
                }
                if nochange["c5_observation"]["work"]["payload_mode"] == "metadata_only" {
                    let raw: String =
                        sqlx::query_scalar("SELECT _oo.c5_authority_export($1,$2,$3)::text")
                            .bind(source)
                            .bind(schema)
                            .bind(table)
                            .fetch_one(&mut *tx)
                            .await
                            .map_err(|_| "c5_state")?;
                    let authority: Value = serde_json::from_str(&raw).map_err(|_| "c5_state")?;
                    validate_metadata_observation(document, &authority)?;
                    // A hints-only poll must not rewrite chunk scan times or complete L3/L4.
                    tx.commit().await.map_err(|_| "c5_state")?;
                    return Ok(
                        json!({"metadata_only":true,"full_digest_scan_completed":false,
                        "data_gen":false,"apply":false,"ack":false,"receipt":null}),
                    );
                }
                sqlx::query("SELECT _oo.c5_observe($1,$2::uuid,$3,$4,$5,$6,$7,$8::jsonb)")
                    .bind(shard)
                    .bind(owner)
                    .bind(fence)
                    .bind(source)
                    .bind(schema)
                    .bind(table)
                    .bind(
                        nochange["source_generation"]
                            .as_i64()
                            .ok_or("c5_nochange")?,
                    )
                    .bind(nochange["c5_observation"].to_string())
                    .execute(&mut *tx)
                    .await
                    .map_err(|_| "c5_observation")?;
                json!({"data_gen":false,"apply":false,"ack":false,"receipt":null})
            }
            _ => return Err("c5_operation"),
        };
        tx.commit().await.map_err(|_| "c5_state")?;
        Ok(result)
    }
    .await;
    if let Ok(shard) = text(&document["target"], "shard") {
        let _ = sqlx::query("SELECT _oo.native_update_release_fence($1)")
            .bind(shard)
            .execute(&mut conn)
            .await;
    }
    let _ = conn.close().await;
    outcome
}

#[cfg(test)]
mod widening_migration_tests {
    use super::*;

    #[test]
    fn privileged_observer_preserves_every_ordinary_reader_fence() {
        let old = include_str!("../migrations/0068_native_c5_overlay_visibility.sql");
        let start = old.find("CREATE FUNCTION _oo.c5_overlay_visible(").unwrap();
        let end = old.find("ALTER FUNCTION _oo.c5_overlay_finish").unwrap();
        let expected = old[start..end]
            .replacen("CREATE FUNCTION _oo.c5_overlay_visible(", "CREATE OR REPLACE FUNCTION _oo.c5_overlay_visible(", 1)
            .replacen("IF p_reader IN ('oo_target_owner','oo_target_admin') THEN RETURN true; END IF;",
                "IF p_reader IN ('oo_target_owner','oo_target_admin')\n    OR EXISTS(SELECT FROM pg_catalog.pg_roles WHERE rolname=p_reader AND rolsuper)\n  THEN RETURN true; END IF;", 1);
        let new = include_str!("../migrations/0108_native_c5_privileged_observer.sql");
        assert_eq!(
            new[new.find("CREATE OR REPLACE FUNCTION").unwrap()..].trim_end(),
            expected.trim_end()
        );
    }

    #[test]
    fn widening_preimages_bind_exact_bytes_and_reject_drift_or_other_operations() {
        let path = std::env::temp_dir().join(format!(
            "oo-widening-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bytes = b"[]\n";
        std::fs::write(&path, bytes).unwrap();
        let pin = json!({"path":path,"sha256":crate::target_installer::sha256_hex(bytes)});
        let mut request = json!({"operation":"initialize_published_if_absent",
            "widening_type_preimages":{"old":pin,"new":pin}});
        assert_eq!(
            widening_type_preimages(&request).unwrap(),
            Some(("[]\n".into(), "[]\n".into()))
        );
        request["operation"] = json!("initialize_multipart_if_absent");
        assert_eq!(
            widening_type_preimages(&request).unwrap_err(),
            "c5_widening_scope"
        );
        request["operation"] = json!("initialize_published_if_absent");
        std::fs::write(&path, b"[]").unwrap();
        assert_eq!(
            widening_type_preimages(&request).unwrap_err(),
            "c5_widening_preimage_digest"
        );
        std::fs::remove_file(path).unwrap();
        assert!(widening_type_preimages(&json!({})).unwrap().is_none());
    }

    #[test]
    fn unbounded_target_widening_requires_sealed_source_types_and_complete_projection() {
        let sql = include_str!("../migrations/0107_native_c5_widening_source_types.sql");
        for guard in [
            "sha256(convert_to(p_old_types,'UTF8'))",
            "sha256(convert_to(p_new_types,'UTF8'))",
            "new_run.schema_digest IS DISTINCT FROM old_run.schema_digest",
            "old_type-'source_type'-'target_typmod'",
            "old_col.logical_column=ANY(p_keys)",
            "new_width<=old_width",
            "n<>jsonb_array_length(old_types)",
            "c5_parts_identity_check",
            "c5_overlay_is_valid",
        ] {
            assert!(sql.contains(guard), "missing {guard}");
        }
    }

    #[test]
    fn widening_accepts_a_new_sealed_type_digest_without_weakening_other_guards() {
        let original = include_str!("../migrations/0105_native_c5_widening_rebase.sql");
        let revised = include_str!("../migrations/0106_native_c5_widening_type_receipt.sql");
        let old = &original[original
            .find("CREATE FUNCTION _oo.c5_rebase_widening_if_needed(")
            .unwrap()..];
        let new = &revised[revised
            .find("CREATE OR REPLACE FUNCTION _oo.c5_rebase_widening_if_needed(")
            .unwrap()..];
        let equality = "  OR new_run.type_digest IS DISTINCT FROM old_run.type_digest\n";
        assert_eq!(old.matches(equality).count(), 1);
        assert_eq!(
            new,
            old.replace(equality, "")
                .replacen("CREATE FUNCTION", "CREATE OR REPLACE FUNCTION", 1)
        );
        assert!(new.contains("new_col.atttypmod<=old_col.atttypmod"));
        assert!(new.contains("old_col.logical_column=ANY(p_keys)"));
        assert!(new.contains("c5_overlay_is_valid"));
    }
}

fn validate_metadata_observation(document: &Value, authority: &Value) -> Result<(), &'static str> {
    let nochange = &document["nochange"];
    let observation = &nochange["c5_observation"];
    let layers: [bool; 4] = serde_json::from_value(nochange["c5_completed_layers"].clone())
        .map_err(|_| "c5_metadata_observation")?;
    if layers[2]
        || layers[3]
        || observation["full_digest_scan_completed"] != false
        || observation["chunks"] != json!([])
        || observation["scan_id"] != Value::Null
        || observation["expected_chunks"] != Value::Null
        || observation["next_chunk_index"] != 0
        || observation["work"]["digest_rows"] != 0
        || observation["work"]["captured_rows"] != 0
        || nochange["source_generation"] != document["expected_source_generation"]
        || nochange["source_generation"] != authority["source_generation"]
        || nochange["parent_digest"] != authority["input_digest"]
        || !authority["scan_id"].is_null()
        || authority["empty_only"] == true
        || ["source_id", "schema", "table", "schema_fingerprint"]
            .iter()
            .any(|key| observation[*key] != authority[*key] || observation[*key] != document[*key])
        || observation["chunk_topology_revision"] != authority["chunk_topology_revision"]
    {
        return Err("c5_metadata_observation");
    }
    Ok(())
}

async fn require_multipart_schema(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), &'static str> {
    let stored: Option<String> =
        sqlx::query_scalar("SELECT checksum FROM _oo.schema_migrations WHERE version=$1")
            .bind(MULTIPART_INITIALIZE_VERSION)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| "c5_multipart_initialize_schema")?;
    if stored.as_deref()
        != Some(crate::target_installer::sha256_hex(MULTIPART_INITIALIZE_SQL.as_bytes()).as_str())
    {
        return Err("c5_multipart_initialize_schema");
    }
    Ok(())
}

async fn require_published_schema(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), &'static str> {
    let stored: Option<String> =
        sqlx::query_scalar("SELECT checksum FROM _oo.schema_migrations WHERE version=$1")
            .bind(PUBLISHED_INITIALIZE_VERSION)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| "c5_published_initialize_schema")?;
    if stored.as_deref()
        != Some(crate::target_installer::sha256_hex(PUBLISHED_INITIALIZE_SQL.as_bytes()).as_str())
    {
        return Err("c5_published_initialize_schema");
    }
    Ok(())
}

async fn require_empty_schema(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), &'static str> {
    let stored: Option<String> =
        sqlx::query_scalar("SELECT checksum FROM _oo.schema_migrations WHERE version=$1")
            .bind(EMPTY_INITIALIZE_VERSION)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| "c5_empty_initialize_schema")?;
    if stored.as_deref()
        != Some(crate::target_installer::sha256_hex(EMPTY_INITIALIZE_SQL.as_bytes()).as_str())
    {
        return Err("c5_empty_initialize_schema");
    }
    Ok(())
}

fn validate_empty_observation(document: &Value) -> Result<(), &'static str> {
    let nochange = &document["nochange"];
    let obs = &nochange["c5_observation"];
    let fields = [
        "protocol",
        "source_id",
        "schema",
        "table",
        "source_generation",
        "table_source_generation",
        "schema_fingerprint",
        "baseline_receipt",
        "observation_id",
        "lease_id",
        "lease_generation",
        "started_at_ms",
        "checked_at_ms",
        "source_query",
        "rows_returned",
        "row_limit",
        "snapshot_complete",
        "read_only",
        "schema_checked_before",
        "schema_checked_after",
    ];
    if obs.as_object().is_none_or(|object| {
        object.len() != fields.len() || fields.iter().any(|key| !object.contains_key(*key))
    }) || [
        "source_id",
        "schema",
        "table",
        "lease_id",
        "lease_generation",
    ]
    .iter()
    .any(|key| obs[*key] != document[*key])
        || nochange["source_generation"] != document["expected_source_generation"]
        || obs["source_generation"] != nochange["source_generation"]
        || nochange["acknowledged_source_generation"] != nochange["source_generation"]
        || obs["rows_returned"].as_u64() != Some(0)
        || obs["row_limit"].as_u64() != Some(1)
        || [
            "snapshot_complete",
            "read_only",
            "schema_checked_before",
            "schema_checked_after",
        ]
        .iter()
        .any(|key| obs[*key] != true)
        || !matches!((obs["started_at_ms"].as_i64(), obs["checked_at_ms"].as_i64()),
            (Some(started), Some(checked)) if started >= 0 && checked >= started)
        || !matches!((obs["table_source_generation"].as_i64(), obs["source_generation"].as_i64()),
            (Some(table), Some(high)) if table > 0 && table <= high)
        || !obs["baseline_receipt"].is_object()
    {
        return Err("c5_empty_observation");
    }
    Ok(())
}

fn validate_hydration_request(document: &Value) -> Result<(), &'static str> {
    if document.as_object().is_none_or(|object| {
        object.keys().any(|key| {
            ![
                "protocol",
                "operation",
                "source_id",
                "schema",
                "table",
                "target",
                "lease_id",
                "lease_generation",
                "expected_source_generation",
                "chunk_id",
                "arrow_path",
            ]
            .contains(&key.as_str())
        })
    }) {
        return Err("c5_hydration_request");
    }
    if document["expected_source_generation"]
        .as_i64()
        .is_none_or(|n| n < 1)
    {
        return Err("c5_state_scope");
    }
    let chunk = text(document, "chunk_id")?;
    let index = chunk.parse::<usize>().map_err(|_| "c5_hydration_chunk")?;
    if index.to_string() != chunk {
        return Err("c5_hydration_chunk");
    }
    if !std::path::Path::new(text(document, "arrow_path")?).is_absolute() {
        return Err("c5_hydration_path");
    }
    Ok(())
}

fn read_private_part(path: &str) -> Result<Vec<u8>, &'static str> {
    const MAX_PART: u64 = 32 * 1024 * 1024;
    let path = std::path::Path::new(path);
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "c5_hydration_path")?;
    if !path.is_absolute() || !metadata.is_file() || metadata.len() > MAX_PART {
        return Err("c5_hydration_path");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("c5_hydration_path");
        }
    }
    let file = std::fs::File::open(path).map_err(|_| "c5_hydration_path")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file.metadata().map_err(|_| "c5_hydration_path")?;
        if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
            return Err("c5_hydration_path");
        }
    }
    let mut bytes = Vec::new();
    file.take(MAX_PART + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "c5_hydration_path")?;
    if bytes.len() as u64 > MAX_PART {
        return Err("c5_hydration_path");
    }
    Ok(bytes)
}

fn hydration_members(
    authority: &Value,
    bytes: &[u8],
) -> Result<Vec<crate::native_c5_range::TypedKey>, &'static str> {
    use crate::native_c5_multipart::Manifest;
    use crate::native_c5_range::{KeyComponent, RangeScope};

    let baseline = &authority["baseline"];
    let kind = baseline["baseline_kind"].as_str().unwrap_or("multipart");
    if authority
        .get("baseline_kind")
        .is_some_and(|value| value != kind)
    {
        return Err("c5_hydration_baseline");
    }
    let row_count = match kind {
        "single" => {
            let proof = &authority["single_receipt"];
            if authority["baseline_protocol"] != "oo.c5.published-baseline/v1"
                || authority.get("multipart_scope") != Some(&Value::Null)
                || authority["chunk_id"] != "0"
                || authority["capture_hi"] != Value::Null
                || proof["run_id"] != baseline["run_id"]
                || proof["source_id"] != baseline["source"]
                || proof["schema"] != baseline["schema_name"]
                || proof["table"] != baseline["table_name"]
                || proof["physical_name"] != baseline["physical_name"]
                || proof["source_generation"] != baseline["source_generation"]
                || proof["schema_digest"] != baseline["schema_digest"]
                || proof["type_digest"] != baseline["type_digest"]
                || proof["config_digest"] != baseline["config_digest"]
                || proof["input_digest"] != authority["arrow_sha256"]
                || proof["row_count"] != authority["row_count"]
            {
                return Err("c5_hydration_baseline");
            }
            if crate::target_installer::sha256_hex(bytes) != text(authority, "arrow_sha256")? {
                return Err("c5_hydration_digest");
            }
            authority["row_count"]
                .as_u64()
                .filter(|n| *n <= 100_000)
                .ok_or("c5_hydration_rows")?
        }
        "multipart" => {
            let manifest = Manifest::parse(&authority["multipart_scope"])?;
            let index = text(authority, "chunk_id")?
                .parse::<usize>()
                .map_err(|_| "c5_hydration_chunk")?;
            manifest.verify_part(index, bytes)?;
            let part = manifest.parts.get(index).ok_or("c5_hydration_chunk")?;
            if authority["arrow_sha256"] != part.arrow_sha256
                || authority["row_count"].as_u64() != Some(part.rows)
                || baseline["source"] != manifest.source_id
                || baseline["schema_name"] != manifest.schema
                || baseline["table_name"] != manifest.table
                || baseline["source_generation"].as_i64() != Some(manifest.source_generation)
                || baseline["schema_digest"] != manifest.schema_sha256
            {
                return Err("c5_hydration_baseline");
            }
            part.rows
        }
        _ => return Err("c5_hydration_baseline"),
    };
    let decoded =
        crate::native_arrow::decode_arrow_stream(bytes).map_err(|_| "c5_hydration_arrow")?;
    let selected = baseline["selected_columns"]
        .as_array()
        .ok_or("c5_hydration_schema")?;
    if decoded.rows as u64 != row_count
        || decoded.fields.len() != selected.len()
        || decoded.fields.iter().zip(selected).any(|(field, name)| {
            name.as_str()
                != crate::native_arrow::mapping_ident(&field.name)
                    .ok()
                    .as_deref()
        })
    {
        return Err("c5_hydration_schema");
    }
    let keys = baseline["key_columns"]
        .as_array()
        .ok_or("c5_hydration_keys")?;
    let kinds = baseline["key_kinds"]
        .as_array()
        .ok_or("c5_hydration_keys")?;
    if keys.len() != kinds.len() {
        return Err("c5_hydration_keys");
    }
    let indexes = keys
        .iter()
        .map(|key| {
            decoded
                .fields
                .iter()
                .position(|field| {
                    key.as_str()
                        == crate::native_arrow::mapping_ident(&field.name)
                            .ok()
                            .as_deref()
                        && !field.nullable
                })
                .ok_or("c5_hydration_keys")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let members = RangeScope::decoded_keys(&decoded, &indexes)?;
    if members.iter().any(|key| {
        key.iter().zip(kinds).any(|(part, kind)| {
            kind != match part {
                KeyComponent::Int(_) => "Int",
                KeyComponent::Text(_) => "Text",
                KeyComponent::Bytes(_) => "Bytes",
            }
        })
    }) || members
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len()
        != members.len()
    {
        return Err("c5_hydration_keys");
    }
    if kind == "multipart" && authority["baseline_protocol"] == "oo.c5.published-baseline/v1" {
        let high: crate::native_c5_range::TypedKey =
            serde_json::from_value(authority["capture_hi"].clone())
                .map_err(|_| "c5_hydration_source_key_order")?;
        if members.last() != Some(&high) {
            return Err("c5_hydration_source_key_order");
        }
    }
    Ok(members)
}

fn archive_digest(authority: &Value, bytes: &[u8]) -> Result<Option<Value>, &'static str> {
    use crate::native_arrow::NativeCell;
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;

    if crate::target_installer::sha256_hex(bytes) != text(authority, "arrow_sha256")? {
        return Err("c5_archive_digest");
    }
    let decoded =
        crate::native_arrow::decode_arrow_stream(bytes).map_err(|_| "c5_hydration_arrow")?;
    let columns = authority["columns"]
        .as_array()
        .ok_or("c5_hydration_schema")?;
    if columns.len() != decoded.fields.len() || columns.len() != decoded.columns.len() {
        return Err("c5_hydration_schema");
    }
    for (column, field) in columns.iter().zip(&decoded.fields) {
        if column["name"].as_str()
            != crate::native_arrow::mapping_ident(&field.name)
                .ok()
                .as_deref()
            || column["nullable"] != field.nullable
        {
            return Err("c5_hydration_schema");
        }
        if !matches!(
            column["target_oid"].as_i64(),
            Some(16 | 17 | 20 | 21 | 23 | 25 | 701 | 1042 | 1043 | 1700 | 1082 | 1114 | 1184)
        ) {
            return Ok(None);
        }
        if column["target_oid"] == 1700 {
            let Some(typmod) = column["target_typmod"].as_i64().filter(|n| *n >= 4) else {
                return Ok(None);
            };
            if !(1..=38).contains(&((typmod - 4) >> 16))
                || !(0..=30).contains(&((typmod - 4) & 65535))
            {
                return Ok(None);
            }
        }
    }
    let mut hi = 0u64;
    let mut lo = 0u64;
    for row in 0..decoded.rows {
        let mut frame = String::new();
        for (index, (values, column)) in decoded.columns.iter().zip(columns).enumerate() {
            let cell = values.get(row).ok_or("c5_hydration_schema")?;
            if index > 0 {
                frame.push('#');
            }
            if matches!(cell, NativeCell::Null) {
                if column["nullable"] != true {
                    return Err("c5_hydration_schema");
                }
                frame.push_str("#1");
                continue;
            }
            let bytes = match (column["target_oid"].as_i64(), cell) {
                (Some(20 | 21 | 23), NativeCell::Int { value, .. }) => {
                    value.to_string().into_bytes()
                }
                (Some(16), NativeCell::Bool(value)) => {
                    if *value {
                        b"1".to_vec()
                    } else {
                        b"0".to_vec()
                    }
                }
                (Some(25 | 1042 | 1043), NativeCell::Utf8(value)) => value.as_bytes().to_vec(),
                (Some(17), NativeCell::Binary(value)) => value.clone(),
                (Some(701), NativeCell::Float64(bits)) => {
                    // MySQL FLOAT is already widened exactly in retained Arrow.
                    crate::native_c5_float::canonical(f64::from_bits(*bits))?.into_bytes()
                }
                (Some(1082), NativeCell::Date32(value)) => value.to_string().into_bytes(),
                (Some(1114), NativeCell::TimestampMicros(value)) => value.to_string().into_bytes(),
                (Some(1184), NativeCell::TimestampMicrosUtc { value, timezone })
                    if timezone == "UTC" =>
                {
                    value.to_string().into_bytes()
                }
                (
                    Some(1700),
                    NativeCell::Decimal {
                        precision,
                        scale,
                        coefficient,
                    },
                ) => {
                    let typmod = column["target_typmod"]
                        .as_i64()
                        .ok_or("c5_hydration_schema")?
                        - 4;
                    if i64::from(*precision) != typmod >> 16 || i64::from(*scale) != typmod & 65535
                    {
                        return Err("c5_hydration_schema");
                    }
                    oo_core::ExactDecimal::new(
                        *coefficient < 0,
                        coefficient.unsigned_abs().to_string(),
                        i32::from(*scale),
                    )
                    .map_err(|_| "c5_hydration_schema")?
                    .to_plain_string()
                    .into_bytes()
                }
                _ => return Ok(None),
            };
            for byte in bytes {
                write!(frame, "{byte:02X}").map_err(|_| "c5_archive_digest")?;
            }
            frame.push_str("#0");
        }
        let hash = Sha256::digest(frame.as_bytes());
        hi ^= u64::from_be_bytes(hash[..8].try_into().map_err(|_| "c5_archive_digest")?);
        lo ^= u64::from_be_bytes(hash[8..16].try_into().map_err(|_| "c5_archive_digest")?);
    }
    Ok(Some(json!([
        decoded.rows,
        format!("{hi:016x}"),
        format!("{lo:016x}")
    ])))
}

fn partial_range_scope(scope: &Value) -> Result<Value, &'static str> {
    if scope["protocol"] == crate::native_c5_parts::RANGE_PROTOCOL {
        crate::native_c5_parts::validate_range_scope(scope)?;
        Ok(scope.clone())
    } else {
        serde_json::to_value(crate::native_c5_range::RangeScope::parse(scope)?)
            .map_err(|_| "c5_observation_receipt")
    }
}

fn validate_partial_observation(document: &Value) -> Result<(), &'static str> {
    if document["expected_source_generation"]
        .as_i64()
        .is_none_or(|n| n < 1)
    {
        return Err("c5_state_scope");
    }
    let observation = &document["observation"];
    if observation["scan_id"].is_null() {
        if observation.get("scan_id").is_none()
            || observation.get("expected_chunks") != Some(&Value::Null)
        {
            return Err("c5_state_request");
        }
    } else {
        text(observation, "scan_id")?;
    }
    text(observation, "schema_fingerprint")?;
    let chunk = &observation["chunk"];
    if observation["chunk_topology_revision"]
        .as_i64()
        .is_none_or(|n| n < 1)
        || observation["checked_at_ms"].as_u64().is_none()
        || !chunk.is_object()
        || chunk["digest"].as_array().is_none_or(|d| d.len() != 3)
        || !matches!(
            chunk["digest_codec"].as_str(),
            Some("mysql-md5-xor/v1" | "oo.c5.arrow-sha256-xor/v1")
        )
        || chunk["row_count"].as_u64().is_none()
        || chunk["digest"][0] != chunk["row_count"]
    {
        return Err("c5_partial_observation");
    }
    for field in ["chunk_id", "arrow_sha256", "projection_sha256"] {
        text(chunk, field)?;
    }
    if let Some(chunks) = observation.get("chunks") {
        let chunks = chunks
            .as_array()
            .filter(|c| !c.is_empty())
            .ok_or("c5_partial_observation")?;
        let expected = observation["expected_chunks"].as_array();
        if chunks.last() != Some(chunk)
            || expected.is_some_and(|expected| {
                chunks.len() > expected.len()
                    || chunks
                        .iter()
                        .zip(expected)
                        .any(|(chunk, id)| !id.is_string() || chunk["chunk_id"] != *id)
            })
            || (expected.is_none()
                && (!observation["scan_id"].is_null()
                    || chunks
                        .iter()
                        .enumerate()
                        .any(|(i, chunk)| chunk["chunk_id"] != i.to_string().as_str())))
        {
            return Err("c5_partial_observation");
        }
    }
    if document.get("loader_receipt").is_some() || document.get("range_scope").is_some() {
        let scope =
            partial_range_scope(&document["range_scope"]).map_err(|_| "c5_observation_receipt")?;
        let receipt = &document["loader_receipt"];
        if receipt["protocol"] != "oo.pipeline.native-update-result/v1"
            || receipt["source_id"] != document["source_id"]
            || receipt["logical_schema"] != document["schema"]
            || receipt["logical_table"] != document["table"]
            || receipt["source_generation"] != document["expected_source_generation"]
            || receipt["jobs_lease_receipt"]["id"] != document["lease_id"]
            || receipt["jobs_lease_receipt"]["generation"] != document["lease_generation"]
            || receipt["target_generation"].as_i64().is_none_or(|n| n < 1)
            || receipt["receipt"].as_str().is_none_or(str::is_empty)
            || scope["source_id"] != document["source_id"]
            || scope["schema"] != document["schema"]
            || scope["table"] != document["table"]
            || scope["source_generation"] != document["expected_source_generation"]
            || scope["schema_fingerprint"] != observation["schema_fingerprint"]
            || scope["chunk_topology_revision"] != observation["chunk_topology_revision"]
            || scope["chunk_id"] != chunk["chunk_id"]
            || scope["arrow_sha256"] != chunk["arrow_sha256"]
            || scope["lo"] != chunk["lo"]
            || scope["hi"] != chunk["hi"]
            || scope["row_count"] != chunk["row_count"]
            || (scope["protocol"] == crate::native_c5_parts::RANGE_PROTOCOL
                && scope["parts_task"] != document["lease_id"])
            || scope["prior_membership_revision"]
                .as_i64()
                .and_then(|v| v.checked_add(1))
                != chunk["membership_revision"].as_i64()
        {
            return Err("c5_observation_receipt");
        }
    }
    Ok(())
}

fn widening_type_preimages(document: &Value) -> Result<Option<(String, String)>, &'static str> {
    let Some(proof) = document.get("widening_type_preimages") else {
        return Ok(None);
    };
    if document["operation"] != "initialize_published_if_absent"
        || proof
            .as_object()
            .is_none_or(|p| p.len() != 2 || !p.contains_key("old") || !p.contains_key("new"))
    {
        return Err("c5_widening_scope");
    }
    let read = |pin: &Value| -> Result<String, &'static str> {
        if pin
            .as_object()
            .is_none_or(|p| p.len() != 2 || !p.contains_key("path") || !p.contains_key("sha256"))
        {
            return Err("c5_widening_preimage");
        }
        let path = std::path::Path::new(text(pin, "path")?);
        if !path.is_absolute()
            || path
                .components()
                .any(|part| part == std::path::Component::ParentDir)
            || !std::fs::symlink_metadata(path)
                .map_err(|_| "c5_widening_preimage")?
                .is_file()
        {
            return Err("c5_widening_preimage");
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| "c5_widening_preimage")?
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .map_err(|_| "c5_widening_preimage")?;
        if bytes.len() > 1_048_576
            || crate::target_installer::sha256_hex(&bytes) != text(pin, "sha256")?
        {
            return Err("c5_widening_preimage_digest");
        }
        // SQL checks these exact bytes against the sealed old/new run digests.
        String::from_utf8(bytes).map_err(|_| "c5_widening_preimage")
    };
    Ok(Some((read(&proof["old"])?, read(&proof["new"])?)))
}

fn validate_multipart_initialize(document: &Value) -> Result<(), &'static str> {
    let object = document.as_object().ok_or("c5_state_request")?;
    if object.keys().any(|key| {
        ![
            "protocol",
            "operation",
            "source_id",
            "schema",
            "table",
            "target",
            "lease_id",
            "lease_generation",
            "schema_fingerprint",
            "chunk_topology_revision",
            "key_columns",
            "expected_source_generation",
            "widening_type_preimages",
        ]
        .contains(&key.as_str())
    }) {
        return Err("c5_multipart_initialize_request");
    }
    if document.get("widening_type_preimages").is_some()
        && document["operation"] != "initialize_published_if_absent"
    {
        return Err("c5_widening_scope");
    }
    for field in ["source_id", "schema", "table", "lease_id"] {
        if text(document, field)?.contains('\0') {
            return Err("c5_state_request");
        }
    }
    let fingerprint = text(document, "schema_fingerprint")?;
    if fingerprint.len() != 64
        || !fingerprint
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("c5_schema_fingerprint");
    }
    if document["chunk_topology_revision"].as_i64() != Some(1) {
        return Err("c5_topology");
    }
    if document["expected_source_generation"]
        .as_i64()
        .is_none_or(|n| n < 1)
    {
        return Err("c5_state_scope");
    }
    if document["lease_generation"].as_i64().is_none_or(|n| n < 1) {
        return Err("lease_required");
    }
    let keys = document["key_columns"].as_array().ok_or("c5_keys")?;
    let empty_only = document["operation"] == "initialize_empty_if_absent";
    if (if empty_only {
        !keys.is_empty()
    } else {
        keys.is_empty()
    }) || keys.len() > 64
        || keys.iter().enumerate().any(|(i, key)| {
            key.as_str()
                .is_none_or(|s| s.is_empty() || s.contains('\0'))
                || keys[..i].contains(key)
        })
    {
        return Err("c5_keys");
    }
    Ok(())
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, &'static str> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("c5_state_request")
}

pub async fn stdio() -> Result<(), u8> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|_| 2u8)?;
    if bytes.len() > 1_048_576 {
        return Err(2);
    }
    let document: Value = serde_json::from_slice(&bytes).map_err(|_| 2u8)?;
    let result = match execute(&document).await {
        Ok(value) => value,
        Err(reason) => {
            eprintln!("oo-pipeline: {reason}");
            return Err(1);
        }
    };
    serde_json::to_writer(std::io::stdout(), &result).map_err(|_| 1u8)?;
    std::io::stdout().write_all(b"\n").map_err(|_| 1u8)
}

#[cfg(test)]
mod initialization_limits {
    use super::*;

    #[test]
    fn oracle_state_dispatch_is_closed_and_preserves_source_names() {
        let value = json!({"protocol":"oo.pipeline.native-c5-state/v1",
            "operation":"initialize_oracle_ranges","source_id":"source","schema":"OWNER","table":"ORDERS",
            "expected_source_generation":1,"run_id":"run","schema_fingerprint":"a".repeat(64),
            "key_column":"ID","chunks":[{"chunk_id":"0","lo":null,"hi":null}],"target":{},
            "oracle_snapshot":{"identity":"owned","scn":"2255350","manifest_sha256":"b".repeat(64),
                "source_schema":"OWNER","source_table":"ORDERS"}});
        assert!(validate_oracle_state(&value).is_ok());
        for (key, v) in [
            ("schema", json!("owner")),
            ("expected_source_generation", json!(0)),
            ("chunks", json!([])),
            ("fake_lease", json!(true)),
        ] {
            let mut bad = value.clone();
            bad[key] = v;
            assert!(validate_oracle_state(&bad).is_err(), "{key}");
        }
        let mut observation = value.clone();
        for key in ["run_id", "schema_fingerprint", "key_column", "chunks"] {
            observation.as_object_mut().unwrap().remove(key);
        }
        observation["operation"] = json!("observe_oracle_unchanged");
        observation["chunk_id"] = json!("0");
        observation["rows"] = json!([]);
        assert!(validate_oracle_state(&observation).is_ok());
        observation["oracle_snapshot"]["scn"] = json!("0");
        assert!(validate_oracle_state(&observation).is_err());
    }

    #[test]
    fn hints_only_poll_cannot_claim_a_scan_or_ignore_a_running_scan() {
        let authority = json!({"source_id":"fixture","schema":"app","table":"items",
            "schema_fingerprint":"a".repeat(64),"source_generation":7,
            "input_digest":"b".repeat(64),"scan_id":null,"chunk_topology_revision":1});
        let observation = json!({"source_id":"fixture","schema":"app","table":"items",
            "schema_fingerprint":"a".repeat(64),"chunk_topology_revision":1,
            "chunks":[],"next_chunk_index":0,"scan_id":null,"expected_chunks":null,
            "full_digest_scan_completed":false,"work":{"digest_rows":0,"captured_rows":0}});
        let document = json!({"source_id":"fixture","schema":"app","table":"items",
            "schema_fingerprint":"a".repeat(64),"expected_source_generation":7,
            "nochange":{"source_generation":7,"parent_digest":"b".repeat(64),
                "c5_completed_layers":[true,true,false,false],"c5_observation":observation}});
        assert!(validate_metadata_observation(&document, &authority).is_ok());
        for (pointer, value) in [
            ("/nochange/c5_completed_layers/2", json!(true)),
            ("/nochange/c5_completed_layers/3", json!(true)),
            (
                "/nochange/c5_observation/full_digest_scan_completed",
                json!(true),
            ),
            ("/nochange/c5_observation/work/digest_rows", json!(1)),
            ("/nochange/parent_digest", json!("c".repeat(64))),
        ] {
            let mut bad = document.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(
                validate_metadata_observation(&bad, &authority).is_err(),
                "{pointer}"
            );
        }
        let mut active = authority.clone();
        active["scan_id"] = json!("running");
        assert!(validate_metadata_observation(&document, &active).is_err());
    }

    #[tokio::test]
    async fn mapped_names_are_refused_before_target_connection() {
        for field in ["schema", "table"] {
            let mut document = json!({"protocol":"oo.pipeline.native-c5-state/v1","operation":"initialize_full_if_absent","source_id":"fixture","schema":"sample","table":"items","target":{}});
            document[field] = json!("select");
            assert_eq!(execute(&document).await, Err("c5_mapped_names_unsupported"));
        }
    }

    #[tokio::test]
    async fn bounded_initializer_requests_are_explicitly_refused() {
        for operation in ["initialize_full", "initialize_full_if_absent"] {
            for field in ["lo", "hi", "chunks", "expected_chunks", "boundaries"] {
                let mut document = json!({"protocol":"oo.pipeline.native-c5-state/v1","operation":operation,"source_id":"fixture","schema":"sample","table":"items","target":{}});
                document[field] = json!([]);
                assert_eq!(
                    execute(&document).await,
                    Err("c5_bounded_baseline_unsupported")
                );
            }
        }
    }

    fn multipart_request() -> Value {
        json!({
            "protocol":"oo.pipeline.native-c5-state/v1",
            "operation":"initialize_multipart_if_absent",
            "source_id":"shared","schema":"sample","table":"items","target":{},
            "lease_id":"11111111-1111-4111-8111-111111111111","lease_generation":1,
            "schema_fingerprint":"a".repeat(64),"chunk_topology_revision":1,
            "key_columns":["id"],"expected_source_generation":47
        })
    }

    #[test]
    fn multipart_request_has_a_source_cas_not_a_caller_table_generation() {
        assert_eq!(validate_multipart_initialize(&multipart_request()), Ok(()));
        for (field, value, reason) in [
            (
                "schema_fingerprint",
                json!("not-a-digest"),
                "c5_schema_fingerprint",
            ),
            ("chunk_topology_revision", json!(2), "c5_topology"),
            ("expected_source_generation", json!(0), "c5_state_scope"),
            ("expected_source_generation", json!("47"), "c5_state_scope"),
            ("lease_generation", json!(0), "lease_required"),
            ("key_columns", json!([]), "c5_keys"),
            ("key_columns", json!(["id", "id"]), "c5_keys"),
            ("key_columns", json!([""]), "c5_keys"),
            ("key_columns", json!([null]), "c5_keys"),
        ] {
            let mut request = multipart_request();
            request[field] = value;
            assert_eq!(
                validate_multipart_initialize(&request),
                Err(reason),
                "{field}"
            );
        }
    }

    #[tokio::test]
    async fn multipart_never_accepts_caller_topology_before_connecting() {
        for field in [
            "lo",
            "hi",
            "chunks",
            "expected_chunks",
            "boundaries",
            "multipart_scope",
            "manifest",
            "chunk_id",
            "baseline_generation",
            "table_source_generation",
            "key_kinds",
            "collation",
        ] {
            let mut request = multipart_request();
            request[field] = Value::Null;
            assert_eq!(
                execute(&request).await,
                Err("c5_multipart_initialize_request"),
                "{field}"
            );
        }
    }

    #[tokio::test]
    async fn partial_observations_require_scan_and_digest_identity_before_connecting() {
        let mut request = multipart_request();
        request["operation"] = json!("observe_partial");
        request["observation"] = json!({
            "scan_id":"scan1","schema_fingerprint":"a".repeat(64),
            "chunk_topology_revision":1,"checked_at_ms":1,
            "chunk":{"chunk_id":"0","row_count":2,"digest":[2,"0000000000000001","0000000000000002"],
                "digest_codec":"mysql-md5-xor/v1","projection_sha256":"b".repeat(64),
                "arrow_sha256":"c".repeat(64)}
        });
        assert_eq!(validate_partial_observation(&request), Ok(()));
        for field in ["digest_codec", "row_count", "digest"] {
            let mut invalid = request.clone();
            invalid["observation"]["chunk"][field] = Value::Null;
            assert_eq!(
                execute(&invalid).await,
                Err("c5_partial_observation"),
                "{field}"
            );
        }
        request["observation"]["scan_id"] = Value::Null;
        assert_eq!(execute(&request).await, Err("c5_state_request"));
    }

    fn retained_part() -> (Value, Vec<u8>) {
        retained_part_named("code")
    }

    fn retained_part_named(name: &str) -> (Value, Vec<u8>) {
        use crate::native_update::{FixtureCell, FixtureColumn, FixtureKind, encode_fixture_arrow};
        let mapped = crate::native_arrow::mapping_ident(name).unwrap();
        let bytes = encode_fixture_arrow(
            &[
                FixtureColumn {
                    name: "id".into(),
                    nullable: false,
                    kind: FixtureKind::Int64,
                },
                FixtureColumn {
                    name: name.into(),
                    nullable: false,
                    kind: FixtureKind::Utf8,
                },
            ],
            &[
                vec![FixtureCell::Int(-2), FixtureCell::Text("Z".into())],
                vec![FixtureCell::Int(4), FixtureCell::Text("a".into())],
            ],
        )
        .unwrap();
        let sha = crate::native_c5_multipart::digest(&bytes);
        let chunks = json!([{"table":"items","chunk_id":0,"lo":null,
            "hi":[{"Int":4},{"Text":[97]}],"rows":2,"bytes":bytes.len(),"arrow_sha256":sha}]);
        let manifest_sha =
            crate::native_c5_multipart::digest(&serde_json::to_vec(&chunks).unwrap());
        (
            json!({
                "chunk_id":"0","arrow_sha256":sha,"row_count":2,
                "columns":[{"name":"id","target_oid":20,"target_typmod":-1,"nullable":false},
                    {"name":mapped,"target_oid":25,"target_typmod":-1,"nullable":false}],
                "baseline":{"source":"shared","schema_name":"app","table_name":"items","source_generation":3,
                    "schema_digest":"a".repeat(64),"selected_columns":["id",mapped],
                    "key_columns":["id",mapped],"key_kinds":["Int","Text"]},
                "multipart_scope":{"protocol":"oo.mysql.c5-multipart/v1","source_id":"shared",
                    "source_generation":3,"occurrence":"sealed","schema":"app","table":"items",
                    "schema_sha256":"a".repeat(64),"capture_config_sha256":"b".repeat(64),
                    "manifest_sha256":manifest_sha,"chunks":chunks,"part_count":1,
                    "parts":[{"index":0,"total":1,"chunk_id":0,"rows":2,"arrow_sha256":sha}]}
            }),
            bytes,
        )
    }

    #[test]
    fn retained_source_fields_match_their_canonical_target_names() {
        for name in ["refEnum", "select", "q_literal"] {
            let (mut authority, bytes) = retained_part_named(name);
            assert_eq!(hydration_members(&authority, &bytes).unwrap().len(), 2);
            assert!(archive_digest(&authority, &bytes).unwrap().is_some());
            authority["baseline"]["selected_columns"][1] = json!(name);
            assert_eq!(
                hydration_members(&authority, &bytes),
                Err("c5_hydration_schema")
            );
        }
    }

    #[test]
    fn hydration_decodes_only_receipted_arrow_and_preserves_typed_source_membership() {
        use crate::native_c5_range::KeyComponent::{Int, Text};
        let (authority, bytes) = retained_part();
        assert_eq!(
            hydration_members(&authority, &bytes).unwrap(),
            vec![
                vec![Int(-2), Text("5a".into())],
                vec![Int(4), Text("61".into())]
            ]
        );
        let mut corrupted = bytes.clone();
        corrupted[0] ^= 1;
        assert_eq!(
            hydration_members(&authority, &corrupted),
            Err("multipart_digest")
        );
        let mut wrong = authority.clone();
        wrong["baseline"]["key_kinds"] = json!(["Int", "Bytes"]);
        assert_eq!(hydration_members(&wrong, &bytes), Err("c5_hydration_keys"));
        wrong = authority.clone();
        wrong["baseline"]["selected_columns"] = json!(["code", "id"]);
        assert_eq!(
            hydration_members(&wrong, &bytes),
            Err("c5_hydration_schema")
        );
        wrong = authority.clone();
        wrong["baseline"]["source_generation"] = json!(7);
        assert_eq!(
            hydration_members(&wrong, &bytes),
            Err("c5_hydration_baseline")
        );
        wrong = authority;
        wrong["multipart_scope"]["chunks"][0]["hi"] = json!([{"Int":100},{"Text":[122]}]);
        assert_eq!(hydration_members(&wrong, &bytes), Err("multipart_scope"));
    }

    #[test]
    fn archive_sha_seed_is_recomputed_from_trusted_cells_not_a_caller_digest() {
        let (mut authority, bytes) = retained_part();
        authority["digest"] = json!([2, "0000000000000000", "0000000000000000"]);
        assert_eq!(
            archive_digest(&authority, &bytes).unwrap(),
            Some(json!([2, "6dc760e30e3ce935", "8339f976cdca5410"]))
        );
        authority["columns"][1]["target_oid"] = json!(114);
        assert_eq!(archive_digest(&authority, &bytes).unwrap(), None);
        authority["arrow_sha256"] = json!("0".repeat(64));
        assert_eq!(archive_digest(&authority, &bytes), Err("c5_archive_digest"));
    }

    fn retained_float(value: Option<f64>) -> (Value, Vec<u8>) {
        use crate::native_update::{FixtureCell, FixtureColumn, FixtureKind, encode_fixture_arrow};
        let mut bytes = encode_fixture_arrow(
            &[FixtureColumn {
                name: "value".into(),
                nullable: true,
                kind: FixtureKind::Int64,
            }],
            &[vec![value.map_or(FixtureCell::Null, |v| {
                FixtureCell::Int(v.to_bits() as i64)
            })]],
        )
        .unwrap();
        fn indirect(bytes: &[u8], at: usize) -> usize {
            at + u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
        }
        fn slot(bytes: &[u8], table: usize, index: usize) -> usize {
            let vtable =
                table - i32::from_le_bytes(bytes[table..table + 4].try_into().unwrap()) as usize;
            let at = vtable + 4 + 2 * index;
            table + u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as usize
        }
        // Reuse the fixture's 64-bit body/validity buffers, declaring Double
        // instead of Int in its schema. No float conversion touches the bits.
        let message = indirect(&bytes, 16);
        let schema = indirect(&bytes, slot(&bytes, message, 2));
        let fields = indirect(&bytes, slot(&bytes, schema, 1));
        let field = indirect(&bytes, fields + 4);
        let tag = slot(&bytes, field, 2);
        bytes[tag] = 3;
        let kind = indirect(&bytes, slot(&bytes, field, 3));
        let precision = slot(&bytes, kind, 0);
        bytes[precision..precision + 4].copy_from_slice(&2i32.to_le_bytes());
        (
            json!({"arrow_sha256":crate::target_installer::sha256_hex(&bytes),
                "columns":[{"name":"value","nullable":true,"target_oid":701,"target_typmod":-1}]}),
            bytes,
        )
    }

    #[test]
    fn archive_float_digest_preserves_double_and_widened_float32_bits() {
        use sha2::{Digest, Sha256};
        for (value, canonical) in [
            (0.0, "0e0"),
            (-0.0, "-0e0"),
            (0.1, "1e-1"),
            (f64::from(0.1_f32), "10000000149011612e-17"),
            (f64::from_bits(1), "5e-324"),
            (
                f64::from_bits(1.0_f64.to_bits() + 1),
                "10000000000000002e-16",
            ),
        ] {
            let (authority, bytes) = retained_float(Some(value));
            let frame = canonical
                .bytes()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>()
                + "#0";
            let hash = Sha256::digest(frame.as_bytes());
            assert_eq!(
                archive_digest(&authority, &bytes).unwrap(),
                Some(json!([
                    1,
                    format!("{:016x}", u64::from_be_bytes(hash[..8].try_into().unwrap())),
                    format!(
                        "{:016x}",
                        u64::from_be_bytes(hash[8..16].try_into().unwrap())
                    )
                ])),
                "{value:?}"
            );
        }
        let (authority, bytes) = retained_float(None);
        let null = archive_digest(&authority, &bytes).unwrap().unwrap();
        let (authority, bytes) = retained_float(Some(0.0));
        assert_ne!(Some(null), archive_digest(&authority, &bytes).unwrap());
    }

    #[test]
    fn archive_float_digest_refuses_nonfinite_and_corrupted_arrow() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let (authority, bytes) = retained_float(Some(value));
            assert_eq!(
                archive_digest(&authority, &bytes),
                Err("c5_float_nonfinite")
            );
        }
        let (authority, mut bytes) = retained_float(Some(0.1));
        bytes[0] ^= 1;
        assert_eq!(archive_digest(&authority, &bytes), Err("c5_archive_digest"));
    }

    fn single_authority(mut authority: Value) -> Value {
        authority["baseline_protocol"] = json!("oo.c5.published-baseline/v1");
        authority["baseline_kind"] = json!("single");
        authority["baseline"]["baseline_kind"] = json!("single");
        authority["baseline"]["run_id"] = json!("22222222-2222-4222-8222-222222222222");
        authority["baseline"]["physical_name"] = json!("pt_".to_owned() + &"a".repeat(60));
        authority["baseline"]["config_digest"] = json!("b".repeat(64));
        authority["baseline"]["type_digest"] = json!("c".repeat(64));
        authority["single_receipt"] = json!({
            "run_id":authority["baseline"]["run_id"],"source_id":authority["baseline"]["source"],
            "schema":authority["baseline"]["schema_name"],"table":authority["baseline"]["table_name"],
            "source_generation":authority["baseline"]["source_generation"],
            "physical_name":authority["baseline"]["physical_name"],
            "config_digest":authority["baseline"]["config_digest"],"schema_digest":authority["baseline"]["schema_digest"],
            "type_digest":authority["baseline"]["type_digest"],"input_digest":authority["arrow_sha256"],
            "row_count":authority["row_count"]
        });
        authority["multipart_scope"] = Value::Null;
        authority["capture_hi"] = Value::Null;
        authority
    }

    #[test]
    fn genuine_single_receipt_uses_raw_input_not_a_synthetic_manifest() {
        let (authority, bytes) = retained_part();
        let single = single_authority(authority.clone());
        assert_eq!(
            hydration_members(&single, &bytes),
            hydration_members(&authority, &bytes)
        );
        let mut invalid = single.clone();
        invalid["multipart_scope"] = authority["multipart_scope"].clone();
        assert_eq!(
            hydration_members(&invalid, &bytes),
            Err("c5_hydration_baseline")
        );
        invalid = single.clone();
        invalid["single_receipt"]["row_count"] = json!(3);
        assert_eq!(
            hydration_members(&invalid, &bytes),
            Err("c5_hydration_baseline")
        );
        invalid = single;
        invalid["single_receipt"]["source_generation"] = json!(47);
        assert_eq!(
            hydration_members(&invalid, &bytes),
            Err("c5_hydration_baseline")
        );
    }

    #[test]
    fn published_multipart_requires_source_tuple_to_match_real_high_edge() {
        let (mut authority, bytes) = retained_part();
        authority["baseline_protocol"] = json!("oo.c5.published-baseline/v1");
        authority["baseline"]["baseline_kind"] = json!("multipart");
        authority["capture_hi"] = json!([{"kind":"int","value":4},{"kind":"text","value":"61"}]);
        assert!(hydration_members(&authority, &bytes).is_ok());
        authority["baseline"]["key_columns"] = json!(["code", "id"]);
        authority["baseline"]["key_kinds"] = json!(["Text", "Int"]);
        assert_eq!(
            hydration_members(&authority, &bytes),
            Err("c5_hydration_source_key_order")
        );
    }

    #[tokio::test]
    async fn published_initializer_accepts_only_pinned_keys_not_caller_manifests() {
        let mut request = multipart_request();
        request["operation"] = json!("initialize_published_if_absent");
        assert_eq!(validate_multipart_initialize(&request), Ok(()));
        request["multipart_scope"] = Value::Null;
        assert_eq!(
            execute(&request).await,
            Err("c5_multipart_initialize_request")
        );
    }

    #[test]
    fn empty_initializer_requires_explicit_zero_key_mode_without_caller_proof() {
        let mut request = multipart_request();
        request["operation"] = json!("initialize_empty_if_absent");
        assert_eq!(validate_multipart_initialize(&request), Err("c5_keys"));
        request["key_columns"] = json!([]);
        assert_eq!(validate_multipart_initialize(&request), Ok(()));
        for field in ["row_count", "baseline_receipt", "multipart_scope", "chunks"] {
            let mut bad = request.clone();
            bad[field] = Value::Null;
            assert_eq!(
                validate_multipart_initialize(&bad),
                Err("c5_multipart_initialize_request")
            );
        }
        request["operation"] = json!("initialize_published_if_absent");
        assert_eq!(validate_multipart_initialize(&request), Err("c5_keys"));
    }

    #[tokio::test]
    async fn empty_observation_refuses_nonzero_incomplete_or_unbound_probe_before_connecting() {
        let mut request = multipart_request();
        request["operation"] = json!("observe");
        request["nochange"] = json!({
            "source_generation":47,"acknowledged_source_generation":47,
            "c5_observation":{
                "protocol":"oo.c5.empty-observation/v1","source_id":"shared","schema":"sample","table":"items",
                "source_generation":47,"table_source_generation":3,"schema_fingerprint":"a".repeat(64),
                "baseline_receipt":{},"observation_id":"22222222-2222-4222-8222-222222222222",
                "lease_id":request["lease_id"],"lease_generation":1,
                "started_at_ms":1,"checked_at_ms":2,"source_query":"SELECT 1 FROM `sample`.`items` LIMIT 1",
                "rows_returned":0,"row_limit":1,"snapshot_complete":true,"read_only":true,
                "schema_checked_before":true,"schema_checked_after":true
            }
        });
        assert_eq!(validate_empty_observation(&request), Ok(()));
        for (field, value) in [
            ("rows_returned", json!(1)),
            ("rows_returned", json!("0")),
            ("row_limit", json!(2)),
            ("read_only", json!(false)),
            ("schema_checked_after", Value::Null),
            ("checked_at_ms", json!(0)),
            ("source_generation", json!(48)),
            ("table_source_generation", json!(48)),
            ("lease_generation", json!(2)),
        ] {
            let mut bad = request.clone();
            bad["nochange"]["c5_observation"][field] = value;
            assert_eq!(execute(&bad).await, Err("c5_empty_observation"), "{field}");
        }
        request["nochange"]["c5_observation"]["full_digest_scan_completed"] = json!(true);
        assert_eq!(execute(&request).await, Err("c5_empty_observation"));
    }

    #[tokio::test]
    async fn hydration_refuses_supplied_members_or_bounds_before_connection() {
        let request = json!({
            "protocol":"oo.pipeline.native-c5-state/v1","operation":"hydrate_multipart",
            "source_id":"shared","schema":"app","table":"items","target":{},
            "expected_source_generation":7,"chunk_id":"0","arrow_path":"/private/0.arrow"
        });
        assert_eq!(validate_hydration_request(&request), Ok(()));
        for field in [
            "members",
            "lo",
            "hi",
            "key_columns",
            "multipart_scope",
            "arrow_sha256",
        ] {
            let mut invalid = request.clone();
            invalid[field] = json!([]);
            assert_eq!(
                execute(&invalid).await,
                Err("c5_hydration_request"),
                "{field}"
            );
        }
        for chunk in ["-1", "01", "1.0", ""] {
            let mut invalid = request.clone();
            invalid["chunk_id"] = json!(chunk);
            assert!(validate_hydration_request(&invalid).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn retained_parts_must_be_private_regular_bounded_files() {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let path = std::env::temp_dir().join(format!(
            "c5-retained-part-{}-{}.arrow",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        file.write_all(b"private part").unwrap();
        assert_eq!(
            read_private_part(path.to_str().unwrap()).unwrap(),
            b"private part"
        );
        file.set_permissions(std::fs::Permissions::from_mode(0o644))
            .unwrap();
        assert_eq!(
            read_private_part(path.to_str().unwrap()),
            Err("c5_hydration_path")
        );
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .unwrap();
        file.set_len(32 * 1024 * 1024 + 1).unwrap();
        assert_eq!(
            read_private_part(path.to_str().unwrap()),
            Err("c5_hydration_path")
        );
        drop(file);
        std::fs::remove_file(path).unwrap();
    }
}
