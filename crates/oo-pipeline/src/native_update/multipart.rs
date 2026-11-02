use super::*;
use crate::native_c5_multipart::Manifest;

#[derive(Clone, Copy)]
struct Stage<'a> {
    plan: &'a UpdatePlan,
    manifest: &'a Manifest,
    directory: &'a Path,
    run: &'a str,
    physical: &'a str,
    fence: i64,
}

pub(super) async fn execute(document: &Value) -> Result<Value, &'static str> {
    let object = document.as_object().ok_or("multipart_request")?;
    if object.keys().any(|key| {
        ![
            "protocol",
            "native_request",
            "multipart_scope",
            "action",
            "part_index",
        ]
        .contains(&key.as_str())
    }) {
        return Err("multipart_request");
    }
    let request = &document["native_request"];
    let plan = classify_inner(request, false, true, false)?;
    if !plan.source_bound
        || plan.source_engine != "mysql"
        || plan.shape != "snapshot"
        || plan.consistency_grade != "B"
        || plan.cross_table
        || plan.position_codec != "snapshot-zero/v1"
        || plan.applied_pos_request != "0"
        || plan.mirror
    {
        return Err("multipart_request");
    }
    let registry: Value =
        serde_json::from_slice(&read_registry(request)?).map_err(|_| "nonadmitted")?;
    let binding = registry.pointer("/bindings/0").ok_or("nonadmitted")?;
    if binding.get("multipart_scope") != document.get("multipart_scope") {
        return Err("multipart_scope");
    }
    let manifest = Manifest::parse(&document["multipart_scope"])?;
    let capturer: Value = serde_json::from_slice(
        &read_capped(Path::new(text_field(binding, "capture_receipt_path")?))
            .map_err(|_| "nonadmitted")?,
    )
    .map_err(|_| "nonadmitted")?;
    if capturer.get("multipart_scope") != document.get("multipart_scope")
        || manifest.schema_sha256 != plan.schema_digest
        || manifest.source_id != plan.source_id
        || manifest.source_generation != plan.source_generation
        || manifest.occurrence != request["run_id"]
        || manifest.schema != plan.schema
        || manifest.table != plan.table
    {
        return Err("multipart_scope");
    }
    let directory = Path::new(text_field(request, "arrow_path")?)
        .parent()
        .ok_or("multipart_path")?;
    let action = text_field(document, "action")?;
    if !["part", "finalize", "all"].contains(&action) {
        return Err("multipart_action");
    }
    let index = if action == "part" {
        Some(
            document["part_index"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| *n < manifest.part_count)
                .ok_or("multipart_index")?,
        )
    } else {
        None
    };
    let mut conn = connect_target(&plan.target).await?;
    let installed: Option<String> =
        sqlx::query_scalar("SELECT checksum FROM _oo.schema_migrations WHERE version=$1")
            .bind(target_installer::VERSION_NATIVE_C5_MULTIPART)
            .fetch_optional(&mut conn)
            .await
            .map_err(sql_token)?;
    if installed.as_deref()
        != Some(target_installer::compiled_native_c5_multipart_digest().as_str())
    {
        return Err("multipart_schema_not_ready");
    }

    let cells_version: Option<String> =
        sqlx::query_scalar("SELECT checksum FROM _oo.schema_migrations WHERE version=$1")
            .bind(target_installer::VERSION_MULTIPART_CELLS)
            .fetch_optional(&mut conn)
            .await
            .map_err(sql_token)?;
    if cells_version.as_deref()
        != Some(target_installer::compiled_multipart_cells_digest().as_str())
    {
        return Err("multipart_schema_not_ready");
    }
    let mut tx = conn.begin().await.map_err(sql_token)?;
    lease_hold(
        &mut tx,
        plan.lease_id.as_deref().ok_or("lease_required")?,
        plan.lease_generation,
    )
    .await?;
    for (level, column) in [("namespace", ""), ("table", "")] {
        register_mapping(
            &mut tx,
            &plan.source_id,
            &plan.config_digest,
            plan.source_generation,
            level,
            &plan.schema,
            if level == "namespace" {
                ""
            } else {
                &plan.table
            },
            column,
        )
        .await
        .map_err(|_| "physical")?;
    }
    for column in &plan.column_idents {
        register_mapping(
            &mut tx,
            &plan.source_id,
            &plan.config_digest,
            plan.source_generation,
            "column",
            &plan.schema,
            &plan.table,
            column,
        )
        .await
        .map_err(|_| "physical")?;
    }
    let fence: i64 = sqlx::query_scalar("SELECT _oo.adopt_target_generation($1,$2::uuid)")
        .bind(&plan.shard)
        .bind(&plan.owner)
        .fetch_one(&mut *tx)
        .await
        .map_err(sql_token)?;
    sqlx::query("SELECT _oo.full_import_bind_session($1,$2::uuid,$3)")
        .bind(&plan.shard)
        .bind(&plan.owner)
        .bind(fence)
        .execute(&mut *tx)
        .await
        .map_err(sql_token)?;
    let physical = planned_table(&plan)?;
    let (oids, typmods, required) = declared_types(&plan.columns)?;
    let raw:String=sqlx::query_scalar("SELECT _oo.c5_multipart_open($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9,$10::jsonb,$11,$12::text[],$13::bigint[],$14::integer[],$15::boolean[])::text")
        .bind(&plan.shard).bind(&plan.owner).bind(fence).bind(&plan.source_id).bind(plan.source_generation)
        .bind(&plan.config_digest).bind(&plan.type_digest).bind(&plan.input_digest).bind(&plan.schema_digest)
        .bind(serde_json::to_string(&manifest).map_err(|_|"multipart_scope")?).bind(&physical).bind(&plan.column_idents).bind(&oids).bind(&typmods).bind(&required)
        .fetch_one(&mut *tx).await.map_err(sql_token)?;
    let state: Value = serde_json::from_str(&raw).map_err(|_| "multipart_state")?;
    let run = text_field(&state, "run_id")?.to_owned();
    if state["state"] != "open" {
        manifest.verify_files(directory)?;
        tx.commit().await.map_err(sql_token)?;
        return Ok(
            json!({"protocol":RESULT_PROTOCOL,"load_state":"replay","run_id":run,"physical_name":physical,"source_generation":plan.source_generation,"consistency_grade":"B","target_generation":state["target_generation"],"rows":manifest.parts.iter().map(|p|p.rows).sum::<u64>()}),
        );
    }
    let names = column_physical_names(&plan, None)?;
    let keys = plan
        .key_indexes
        .iter()
        .map(|i| names[*i].clone())
        .collect::<Vec<_>>();
    sqlx::query("SELECT _oo.c5_multipart_index($1,$2::uuid,$3,$4::uuid,$5::text[])")
        .bind(&plan.shard)
        .bind(&plan.owner)
        .bind(fence)
        .bind(&run)
        .bind(keys)
        .execute(&mut *tx)
        .await
        .map_err(sql_token)?;
    tx.commit().await.map_err(sql_token)?;
    let stage = Stage {
        plan: &plan,
        manifest: &manifest,
        directory,
        run: &run,
        physical: &physical,
        fence,
    };
    let result = if action == "all" {
        apply_all(&mut conn, &stage).await
    } else if let Some(index) = index {
        part(&mut conn, &stage, index).await
    } else {
        finish(&mut conn, &stage).await
    };
    if result.is_err() {
        let _ = sqlx::query("SELECT _oo.full_import_retire_run($1,$2::uuid,$3,$4::uuid)")
            .bind(&plan.shard)
            .bind(&plan.owner)
            .bind(fence)
            .bind(&run)
            .execute(&mut conn)
            .await;
    }
    result
}

async fn part(
    conn: &mut sqlx::PgConnection,
    stage: &Stage<'_>,
    index: usize,
) -> Result<Value, &'static str> {
    let Stage {
        plan,
        manifest,
        directory,
        run,
        physical,
        fence,
    } = *stage;
    let bytes = manifest.read_part(directory, index)?;
    check_arrow_declaration(&bytes, &plan.columns)?;
    let batch_id = format!("{}.part.{index}", manifest.occurrence);
    let payload = &manifest.parts[index].arrow_sha256;
    let mut tx = conn.begin().await.map_err(sql_token)?;
    lease_hold(
        &mut tx,
        plan.lease_id.as_deref().ok_or("lease_required")?,
        plan.lease_generation,
    )
    .await?;
    let hook = hook_digest();
    let prepared = prepare_table(
        &bytes,
        &plan.source_id,
        &plan.schema,
        &plan.table,
        u64::try_from(plan.source_generation).map_err(|_| "stale_run")?,
        &plan.config_digest,
        decode_hex32(&plan.input_digest).map_err(|_| "digest_mismatch")?,
        &batch_id,
        &columns_for_prepare(&plan.columns, &plan.column_sources)?,
        Some(&plan.trusted_fields),
    )
    .map_err(arrow_token)?;
    if prepared.batch.rows.len() as u64 != manifest.parts[index].rows {
        return Err("multipart_rows");
    }
    let decision: String = sqlx::query_scalar(
        "SELECT _oo.full_import_stage_decision($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9,$10,$11,0)",
    )
    .bind(&plan.shard)
    .bind(&plan.owner)
    .bind(fence)
    .bind(&plan.source_id)
    .bind(physical)
    .bind(&batch_id)
    .bind(payload)
    .bind(&plan.config_digest)
    .bind(&plan.type_digest)
    .bind(&hook)
    .bind(&plan.schema_digest)
    .fetch_one(&mut *tx)
    .await
    .map_err(sql_token)?;
    if decision == "replay" {
        reconcile_part(&mut tx, stage, &prepared, &bytes).await?;
        tx.commit().await.map_err(sql_token)?;
        return Ok(
            json!({"protocol":RESULT_PROTOCOL,"load_state":"part_acknowledged","replay":true,"part_index":index,"run_id":run,"physical_name":physical}),
        );
    }
    if index > 0 {
        let prior = format!("{}.part.{}", manifest.occurrence, index - 1);
        let prior_state: String = sqlx::query_scalar(
            "SELECT _oo.full_import_stage_decision($1,$2::uuid,$3,$4,$5,$6,$7,$8,$9,$10,$11,0)",
        )
        .bind(&plan.shard)
        .bind(&plan.owner)
        .bind(fence)
        .bind(&plan.source_id)
        .bind(physical)
        .bind(&prior)
        .bind(&manifest.parts[index - 1].arrow_sha256)
        .bind(&plan.config_digest)
        .bind(&plan.type_digest)
        .bind(&hook)
        .bind(&plan.schema_digest)
        .fetch_one(&mut *tx)
        .await
        .map_err(sql_token)?;
        if prior_state != "replay" {
            return Err("multipart_order");
        }
    }
    let mut columns = String::from("\"_oo_pos\"");
    for column in &prepared.verified_columns {
        columns.push_str(", ");
        columns.push_str(&quote_physical_name(&column.physical_name).map_err(|_| "physical")?);
    }
    let statement = format!(
        "COPY _oo.{} ({columns}) FROM STDIN WITH (FORMAT binary)",
        quote_physical_name(physical).map_err(|_| "physical")?
    );
    let positions = vec![
        ExactDecimal::new(false, "0".to_string(), 0).map_err(|_| "encode")?;
        prepared.batch.rows.len()
    ];
    let sent = copy_sink::send_batch(
        &mut tx,
        &statement,
        &prepared.batch,
        &prepared.copy_columns,
        &positions,
        CopyBudgets {
            max_value_bytes: 1_048_576,
            max_row_bytes: 1_048_576,
            max_chunk_bytes: 1_048_576,
            max_rows_per_chunk: 1024,
        },
    )
    .await
    .map_err(sink_token)?;
    if sent.rows != manifest.parts[index].rows {
        return Err("multipart_rows");
    }
    reconcile_part(&mut tx, stage, &prepared, &bytes).await?;
    write_ack(&mut tx, stage, &batch_id, payload, sent.rows as i64).await?;
    tx.commit().await.map_err(sql_token)?;
    Ok(
        json!({"protocol":RESULT_PROTOCOL,"load_state":"part_acknowledged","replay":false,"part_index":index,"run_id":run,"physical_name":physical,"rows":sent.rows}),
    )
}

async fn write_ack(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    stage: &Stage<'_>,
    batch: &str,
    payload: &str,
    rows: i64,
) -> Result<(), &'static str> {
    let Stage {
        plan,
        run,
        physical,
        fence,
        ..
    } = *stage;
    let state:String=sqlx::query_scalar("SELECT _oo.full_import_write_batch($1,$2::uuid,$3,$4::uuid,0,$5,$6,$7,$8,$9,$10,$11,$12,$13,0,$14,0,ARRAY[]::text[],ARRAY[]::bigint[],ARRAY[]::integer[],ARRAY[]::bigint[],ARRAY[]::text[])")
        .bind(&plan.shard).bind(&plan.owner).bind(fence).bind(run).bind(&plan.source_id).bind(physical).bind(batch).bind(payload)
        .bind(&plan.config_digest).bind(&plan.type_digest).bind(hook_digest()).bind(&plan.schema_digest).bind(&plan.input_digest).bind(rows)
        .fetch_one(&mut **tx).await.map_err(sql_token)?;
    if state != "inserted" {
        return Err("multipart_ack");
    }
    Ok(())
}

async fn finish(conn: &mut sqlx::PgConnection, stage: &Stage<'_>) -> Result<Value, &'static str> {
    let Stage {
        plan,
        manifest,
        directory,
        run,
        physical,
        fence,
    } = *stage;
    manifest.verify_files(directory)?;
    let truth = plan.truth.as_ref();
    let mut tx = conn.begin().await.map_err(sql_token)?;
    lease_hold(
        &mut tx,
        plan.lease_id.as_deref().ok_or("lease_required")?,
        plan.lease_generation,
    )
    .await?;
    position_ahead(&mut tx, plan).await?;
    let rows: i64 = sqlx::query_scalar("SELECT _oo.c5_multipart_complete($1,$2::uuid,$3,$4::uuid)")
        .bind(&plan.shard)
        .bind(&plan.owner)
        .bind(fence)
        .bind(run)
        .fetch_one(&mut *tx)
        .await
        .map_err(sql_token)?;
    let actual: i64 = sqlx::query_scalar("SELECT _oo.native_update_shadow_count($1)")
        .bind(physical)
        .fetch_one(&mut *tx)
        .await
        .map_err(sql_token)?;
    if actual != rows {
        return Err("multipart_rows");
    }
    if let Some(truth) = truth {
        let cells: String =
            sqlx::query_scalar("SELECT _oo.native_update_read_cells($1,$2::text[])")
                .bind(physical)
                .bind(column_physical_names(plan, None)?)
                .fetch_one(&mut *tx)
                .await
                .map_err(sql_token)?;
        if !cells_match(
            &serde_json::from_str::<Value>(&cells).map_err(|_| "cell_mismatch")?,
            truth,
        ) {
            return Err("cell_mismatch");
        }
    }
    write_ack(&mut tx, stage, &plan.batch_id, &plan.payload_digest, 0).await?;
    let canonical = canonical_digest(&plan.payload_digest, rows);
    sqlx::query("SELECT _oo.full_import_note_table($1,$2::uuid,$3,$4::uuid,0,$5,$6,$7)")
        .bind(&plan.shard)
        .bind(&plan.owner)
        .bind(fence)
        .bind(run)
        .bind(rows)
        .bind(&canonical)
        .bind(&plan.source_id)
        .execute(&mut *tx)
        .await
        .map_err(sql_token)?;
    sqlx::query("SELECT _oo.native_update_seal_quality($1,$2::uuid,$3,$4::uuid,$5,$6,$7,$8,$9,$10,$11,0,'snapshot-zero/v1',$12,'c5_diff','reconcile','B',false,ARRAY[]::text[],$13,'ok',0,'full')")
        .bind(&plan.shard).bind(&plan.owner).bind(fence).bind(run).bind(&plan.source_id).bind(plan.source_generation).bind(&plan.schema).bind(&plan.table).bind(physical).bind(&plan.batch_id).bind(&plan.payload_digest).bind(&plan.position_lineage).bind(rows)
        .execute(&mut *tx).await.map_err(sql_token)?;
    let state:String=sqlx::query_scalar("SELECT _oo.full_import_seal_run($1,$2::uuid,$3,$4::uuid,$5,$6,$7,$8,$9,$10,$11::text[],$12::text[])")
        .bind(&plan.shard).bind(&plan.owner).bind(fence).bind(run).bind(&plan.source_id).bind(plan.source_generation).bind(&plan.config_digest).bind(&plan.type_digest).bind(&plan.input_digest).bind(&plan.schema_digest).bind(vec![plan.schema.as_str()]).bind(vec![plan.table.as_str()])
        .fetch_one(&mut *tx).await.map_err(sql_token)?;
    tx.commit().await.map_err(sql_token)?;
    Ok(
        json!({"protocol":RESULT_PROTOCOL,"load_state":state,"run_id":run,"physical_name":physical,"target_generation":fence,"source_generation":plan.source_generation,"consistency_grade":"B","rows":rows,"canonical_digest":canonical,"reconcile":"ok"}),
    )
}

async fn reconcile_part(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    stage: &Stage<'_>,
    prepared: &native_arrow::PreparedTable,
    bytes: &[u8],
) -> Result<(), &'static str> {
    use crate::c1_parts::{canonical_row, logical_type_of_pg, pg_text_cell, source_cell};
    let plan = stage.plan;
    let columns = column_physical_names(plan, Some(prepared))?;
    let key_columns = plan
        .key_indexes
        .iter()
        .map(|i| columns[*i].clone())
        .collect::<Vec<_>>();
    let members = crate::multipart_cells::keys(
        &decode_arrow_stream(bytes).map_err(arrow_token)?,
        &plan.key_indexes,
    )?;
    let types = prepared
        .verified_columns
        .iter()
        .map(|column| {
            logical_type_of_pg(column.oid, column.typmod).map_err(|_| "multipart_cell_type")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut expected = std::collections::BTreeMap::<Vec<u8>, u64>::new();
    for row in &prepared.batch.rows {
        let cells = row
            .cells
            .iter()
            .map(source_cell)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "multipart_cell_type")?;
        let bytes = canonical_row(&types, &cells).map_err(|_| "multipart_cell_type")?;
        *expected.entry(bytes).or_default() += 1;
    }
    let actual: Vec<String> = sqlx::query_scalar(
        "SELECT cells::text FROM _oo.c5_multipart_cells($1,$2::text[],$3::text[],$4::jsonb) AS r(cells)",
    )
    .bind(stage.physical)
    .bind(columns)
    .bind(key_columns)
    .bind(members.to_string())
    .fetch_all(&mut **tx)
    .await
    .map_err(sql_token)?;
    let mut actual_rows = std::collections::BTreeMap::<Vec<u8>, u64>::new();
    for row in actual {
        let values: Vec<Option<String>> =
            serde_json::from_str(&row).map_err(|_| "multipart_cell_type")?;
        if values.len() != prepared.verified_columns.len() {
            return Err("cell_mismatch");
        }
        let cells = values
            .iter()
            .zip(&prepared.verified_columns)
            .map(|(text, column)| pg_text_cell(column.oid, text.as_deref()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "multipart_cell_type")?;
        let bytes = canonical_row(&types, &cells).map_err(|_| "multipart_cell_type")?;
        *actual_rows.entry(bytes).or_default() += 1;
    }
    if actual_rows != expected {
        return Err("cell_mismatch");
    }
    Ok(())
}

async fn apply_all(
    conn: &mut sqlx::PgConnection,
    stage: &Stage<'_>,
) -> Result<Value, &'static str> {
    for index in 0..stage.manifest.part_count {
        let started = std::time::Instant::now();
        part(conn, stage, index).await?;
        eprintln!(
            "oo-pipeline: multipart part={} rows={} elapsed_ms={}",
            index,
            stage.manifest.parts[index].rows,
            started.elapsed().as_millis()
        );
    }
    finish(conn, stage).await
}

pub(super) async fn execute_all(document: &Value) -> Result<Value, &'static str> {
    let mut native = document.clone();
    native["mirror"] = json!(false);
    let scope = document.get("multipart_scope").ok_or("multipart_scope")?;
    let result=execute(&json!({"protocol":"oo.pipeline.native-c5-multipart/v1","native_request":native,"multipart_scope":scope,"action":"all"})).await?;
    let plan = classify_inner(document, false, true, false)?;
    let mut conn = connect_target(&plan.target).await?;
    let mut tx = conn.begin().await.map_err(sql_token)?;
    let (lease, call) = lease_hold(
        &mut tx,
        plan.lease_id.as_deref().ok_or("lease_required")?,
        plan.lease_generation,
    )
    .await?;
    let batch = read_batch(&mut tx, &plan)
        .await?
        .ok_or("multipart_receipt")?;
    if batch.payload_digest != plan.payload_digest {
        return Err("OO010");
    }
    tx.commit().await.map_err(sql_token)?;
    let mirror = if plan.mirror {
        Some(
            mirror_call(
                &mut conn,
                &plan.source_id,
                &plan.batch_id,
                &batch.physical_name,
            )
            .await?,
        )
    } else {
        None
    };
    let jobs = jobs_regclass(&mut conn).await;
    let mut value = if result["load_state"] == "replay" {
        replay_result(&plan, &batch, mirror, jobs, lease, call)
    } else {
        success_result(
            &plan,
            &Applied {
                run_id: batch.run_id,
                target_generation: batch.target_generation,
                physical_name: batch.physical_name,
                row_count: result["rows"].as_i64().ok_or("multipart_rows")?,
                tombstone_count: 0,
                receipt: "inserted".to_string(),
                stage_select_granted: None,
                stage_freshness_select: None,
            },
            mirror,
            jobs,
            lease,
            call,
        )
    };
    value["part_count"] = scope["part_count"].clone();
    value["consistency_grade"] = json!("B");
    Ok(value)
}
