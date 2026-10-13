//! Native producer, import, builder, and preview. Each stage is a frozen binary.
//!
//! The product admission is `oo-source-admission-pg-budget` over evidence the
//! producer wrote. A legacy `validate` report is not this admission. Preview
//! receives the builder's real pack. A protocol refusal is returned as
//! `preview.contract` with the raw stage output left on disk.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use zeroize::Zeroizing;

use crate::admit::{decision_admitted, required_w38_input, sha256_bytes};
use crate::config::{Config, ObjectName};
use crate::error::Fail;
use crate::pg::{apply_sql, endpoint_from_files, grant_runtime, read_sealed_run, SealedRun};
use crate::plan::{plan_run, Plan, PlanInput};
use crate::spawn::{
    catalog_argv, catalog_import_request, contains_secret, hook_digest, import_request, init_pg_path, init_request,
    install_argv, install_sealed_request, pipeline_argv, run_binary, CatalogImport, ImportInput,
};

const PRODUCER_MAX_BYTES: u64 = 33_554_432;
const OWNER: &str = "ad13411e-d04e-4c91-8a20-49caf867aa23";

pub struct ChainIn<'a> {
    pub config: &'a Config,
    pub secrets: &'a [Zeroizing<String>],
    pub run_id: &'a str,
    pub rows: u64,
    pub bytes: u64,
    pub session_sha: &'a str,
    pub target_tables: u64,
    pub commit: Option<&'a str>,
}

pub async fn execute(input: &ChainIn<'_>) -> Result<(), Fail> {
    let commit = ready_commit(input)?;
    let bound = produce_all(input).await?;
    let plan = plan_run(PlanInput {
        run_id: input.run_id,
        prior_run_id: None,
        partial_open: false,
        identity_ok: true,
        type_ok: true,
        rows_observed: input.rows,
        bytes_observed: input.bytes,
        max_rows: input.config.budget.max_rows,
        max_bytes: input.config.budget.max_bytes,
        admitted: bound.admitted,
        budget_unit_unknown: bound.unknown,
        w38_commit: Some(commit),
        decision_source_id: &bound.source_id,
        config_source_id: &input.config.source_id,
        decision_report_sha: &bound.report_sha,
        session_report_sha: &bound.evidence_sha,
    });
    write_plan(input, commit, &bound, &plan)?;
    match plan {
        Plan::Stop(fail) => Err(fail),
        Plan::Proceed { .. } => import_build_preview(input).await,
    }
}

struct Bound {
    admitted: bool,
    unknown: bool,
    source_id: String,
    report_sha: String,
    evidence_sha: String,
}

fn ready_commit<'a>(input: &'a ChainIn<'_>) -> Result<&'a str, Fail> {
    let Some(commit) = input.commit else {
        pending(input, "coordinator record has no author-complete source sha")?;
        return Err(Fail::new("dependency_pending", 3));
    };
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        pending(input, "coordinator source sha is not 40 hex")?;
        return Err(Fail::new("dependency_pending", 3));
    }
    if crate::spawn::file_is_present(&input.config.bins.budget).is_err()
        || crate::spawn::file_is_present(&input.config.bins.admission).is_err()
    {
        pending(input, "budget producer or admit-pg-budget binary is missing")?;
        return Err(Fail::new("binary.missing", 3));
    }
    Ok(commit)
}

fn pending(input: &ChainIn<'_>, fact: &str) -> Result<(), Fail> {
    write_json(
        input,
        "dependency-pending.json",
        &json!({
            "fact": fact,
            "required": required_w38_input(),
            "w38_commit": input.commit,
            "session_report_sha256": input.session_sha,
            "target_relations": input.target_tables,
            "model_called": false
        }),
    )?;
    write_phase(input, "admission_pending", false)
}

async fn produce_all(input: &ChainIn<'_>) -> Result<Bound, Fail> {
    let endpoint = endpoint_from_files(&input.config.source.host, input.config.source.port,
        &input.config.source.database, &input.config.source.username,
        &input.config.source.password_file, &input.config.source.ca_file)?;
    let session = crate::pg::connect_read_only(&endpoint).await?;
    let mut first = None;
    let mut bindings = Vec::new();
    for object in &input.config.approved_objects {
        if !input.config.approved_fields.is_empty() {
            let columns = crate::pg::object_columns(&session, object).await?;
            let approved: Vec<&str> = input.config.approved_fields.iter().filter(|field| field.schema == object.schema && field.table == object.table).map(|field| field.column.as_str()).collect();
            let same = columns.len() == approved.len() && columns.iter().all(|column| approved.contains(&column.as_str()));
            if !same {
                return Err(Fail::new("unapproved", 6));
            }
        }
        let measured = crate::pg::measure_objects(&session, std::slice::from_ref(object)).await?;
        let produced = produce_one(input, object, measured.rows)?;
        if !produced.admitted || produced.unknown || produced.source_id != input.config.source_id {
            return Err(Fail::new("admission_denied", 2));
        }
        bindings.push(json!({"schema": object.schema, "table": object.table,
            "rows": measured.rows, "admitted": produced.admitted,
            "report_sha256": produced.report_sha, "evidence_sha256": produced.evidence_sha}));
        if first.is_none() { first = Some(produced); }
    }
    write_json(input, "object-admission-bindings.json", &json!({"objects": bindings}))?;
    first.ok_or(Fail::new("producer.rows", 8))
}

fn produce_one(input: &ChainIn<'_>, object: &ObjectName, expected: u64) -> Result<Bound, Fail> {
    let name = if object.schema == "public" { object.table.clone() } else { format!("{}--{}", object.schema, object.table) };
    let dir = Path::new(&input.config.out_dir).join("producer").join(name);
    fs::create_dir_all(&dir).map_err(|_| Fail::new("config.read", 1))?;
    let evidence = dir.join("evidence.json");
    let access = dir.join("access.json");
    let budget_config = dir.join("config.json");
    let decision = dir.join("decision.json");
    let max_bytes = input.config.budget.max_bytes.min(PRODUCER_MAX_BYTES);
    let arrow = dir.join(format!("{}.arrow", object.table));
    let snapshot = dir.join("snapshot.txt");
    let _ = fs::remove_file(&arrow);
    let _ = fs::remove_file(&snapshot);
    let args = producer_args(input, object, &evidence, &access, &budget_config, &decision, (max_bytes, expected));
    let spawned = run_binary(Path::new(&input.config.bins.budget), &args, b"")?;
    save_stage(input, &producer_stage(&object.schema, &object.table), &spawned)?;
    if spawned.status != 0 {
        return producer_failure(&decision);
    }
    if bounded_rows(&spawned.stdout) != Some(expected) {
        return Err(Fail::new("producer.rows", 8));
    }
    bind_decision(&decision, &evidence)
}

fn producer_args(
    input: &ChainIn<'_>,
    object: &ObjectName,
    evidence: &Path,
    access: &Path,
    budget_config: &Path,
    decision: &Path,
    bounds: (u64, u64),
) -> Vec<String> {
    let (max_bytes, expected_rows) = bounds;
    let source = &input.config.source;
    vec![
        "--phase".to_owned(),
        "bounded-read".to_owned(),
        "--source-id".to_owned(),
        input.config.source_id.clone(),
        "--host".to_owned(),
        source.host.clone(),
        "--port".to_owned(),
        source.port.to_string(),
        "--database".to_owned(),
        source.database.clone(),
        "--username".to_owned(),
        source.username.clone(),
        "--password-file".to_owned(),
        source.password_file.clone(),
        "--ca-file".to_owned(),
        source.ca_file.clone(),
        "--secret-ref".to_owned(),
        input.config.secret_ref.clone(),
        "--schema".to_owned(),
        object.schema.clone(),
        "--table".to_owned(),
        object.table.clone(),
        "--admit-bin".to_owned(),
        input.config.bins.admission.clone(),
        "--access-out".to_owned(),
        access.display().to_string(),
        "--evidence-out".to_owned(),
        evidence.display().to_string(),
        "--config-out".to_owned(),
        budget_config.display().to_string(),
        "--decision-out".to_owned(),
        decision.display().to_string(),
        "--statement-ms".to_owned(),
        input.config.budget.statement_ms.to_string(),
        "--lock-ms".to_owned(),
        "5000".to_owned(),
        "--connect-ms".to_owned(),
        "30000".to_owned(),
        "--max-rows".to_owned(),
        input.config.budget.max_rows.to_string(),
        "--max-bytes".to_owned(),
        max_bytes.to_string(),
        "--expect-rows".to_owned(),
        expected_rows.to_string(),
        "--arrow-out".to_owned(),
        evidence.with_file_name(format!("{}.arrow", object.table)).display().to_string(),
        "--snapshot-file".to_owned(),
        evidence.with_file_name("snapshot.txt").display().to_string(),
        "--page-rows".to_owned(),
        "4096".to_owned(),
        "--deadline-ms".to_owned(),
        "900000".to_owned(),
    ]
}

fn bounded_rows(stdout: &[u8]) -> Option<u64> {
    let text = String::from_utf8_lossy(stdout);
    let line = text.lines().find(|line| line.contains("pages="))?;
    line.split_whitespace().find_map(|token| token.strip_prefix("rows=")?.parse().ok())
}

fn producer_stage(schema: &str, table: &str) -> String {
    if schema == "public" {
        format!("producer-{table}")
    } else {
        format!("producer-{schema}--{table}")
    }
}

fn producer_leaf(schema: &str, table: &str) -> String {
    if schema == "public" {
        table.to_owned()
    } else {
        format!("{schema}--{table}")
    }
}

fn producer_failure(decision: &Path) -> Result<Bound, Fail> {
    if let Ok(bound) = bind_decision(decision, &decision.with_file_name("evidence.json")) {
        if !bound.admitted {
            return Err(Fail::new("admission_denied", 2));
        }
        return Err(Fail::new("producer.rows", 8));
    }
    Err(Fail::new("producer.exit", 1))
}

fn bind_decision(decision: &Path, evidence: &Path) -> Result<Bound, Fail> {
    let stdout = fs::read(decision).map_err(|_| Fail::new("admission.output", 2))?;
    let evidence_bytes = fs::read(evidence).map_err(|_| Fail::new("admission.output", 2))?;
    let (admitted, unknown, source_id, report_sha) = decision_admitted(&stdout)?;
    let evidence_sha = sha256_bytes(&evidence_bytes);
    if report_sha != evidence_sha {
        return Err(Fail::new("admission.output", 2));
    }
    Ok(Bound { admitted, unknown, source_id, report_sha, evidence_sha })
}

fn write_plan(input: &ChainIn<'_>, commit: &str, bound: &Bound, plan: &Plan) -> Result<(), Fail> {
    let token = match plan {
        Plan::Stop(fail) => fail.token,
        Plan::Proceed { .. } => "proceed",
    };
    write_json(
        input,
        "plan.json",
        &json!({
            "admitted": bound.admitted,
            "budget_unit_unknown": bound.unknown,
            "w38_commit": commit,
            "preflight_session_sha256": input.session_sha,
            "evidence_sha256": bound.evidence_sha,
            "decision_report_sha256": bound.report_sha,
            "target_relations_before_import": input.target_tables,
            "producer_max_bytes": PRODUCER_MAX_BYTES,
            "model_called": false,
            "plan": token
        }),
    )
}

fn pack_role_name(config: &Config) -> &str {
    if config.pack_role.is_empty() {
        "oo_r_reader"
    } else {
        config.pack_role.as_str()
    }
}

async fn import_build_preview(input: &ChainIn<'_>) -> Result<(), Fail> {
    require_product_bins(input)?;
    write_phase(input, "importing", true)?;
    let init_result = init_target(input);
    let grant_result = if init_result.is_ok() {
        grant_target(input).await
    } else {
        Err(Fail::new("target.schema", 1))
    };
    let import_result = if grant_result.is_ok() { import_pg(input) } else { Err(Fail::new("import.rejected", 1)) };
    let install_result = if import_result.is_ok() { install_sealed(input) } else { Err(Fail::new("target.schema", 1)) };
    let owner_result = if install_result.is_ok() {
        install_owner_migration(input).await
    } else {
        Err(Fail::new("target.schema", 1))
    };
    let stored = if import_result.is_ok() { read_identity(input, import_result.as_ref().ok()).await.ok() } else { None };
    let catalog_result = if owner_result.is_ok() {
        send_catalog(input, import_result.as_ref().ok(), stored.as_ref()).await
    } else {
        Err(Fail::new("target.schema", 1))
    };
    let stops = json!({
        "init-pg": token_of(&init_result),
        "grant": token_of(&grant_result),
        "import-pg": token_of(&import_result),
        "install-sealed": token_of(&install_result),
        "owner-migration": token_of(&owner_result),
        "catalog-import": if owner_result.is_ok() { token_of(&catalog_result) } else { "not_called" },
        "pack_role": pack_role_name(input.config),
        "configured_role": input.config.role
    });
    write_json(input, "chain-stops.json", &stops)?;
    if catalog_result.as_ref().is_err_and(|fail| fail.token == "catalog.engine") {
        return catalog_result;
    }
    init_result?;
    grant_result?;
    import_result?;
    install_result?;
    owner_result?;
    catalog_result?;
    write_phase(input, "sealed", false)?;
    seal_pack(input)?;
    seal_all_authorities(input)?;
    record_consumer_deferred(input)
}

fn token_of<T>(result: &Result<T, Fail>) -> &'static str {
    match result {
        Ok(_) => "ok",
        Err(fail) => fail.token,
    }
}

fn require_product_bins(input: &ChainIn<'_>) -> Result<(), Fail> {
    for bin in [&input.config.bins.pipeline, &input.config.bins.builder, &input.config.bins.preview] {
        crate::spawn::file_is_present(bin)?;
    }
    let init = init_pg_path(&input.config.bins.pipeline);
    crate::spawn::file_is_present(init.to_str().unwrap_or(""))
}

fn super_endpoint(input: &ChainIn<'_>) -> Result<crate::pg::Endpoint, Fail> {
    let target = &input.config.target;
    let parent = Path::new(&target.password_file).parent().ok_or(Fail::new("target.schema", 1))?;
    let password = parent.join("super");
    let text = password.to_str().ok_or(Fail::new("target.schema", 1))?;
    endpoint_from_files(&target.host, target.port, &target.database, "postgres", text, &target.ca_file)
}

fn init_target(input: &ChainIn<'_>) -> Result<(), Fail> {
    let body = init_request(&input.config.target)?;
    ensure_clean(input, &body)?;
    let bin = init_pg_path(&input.config.bins.pipeline);
    let spawned = run_binary(&bin, &[], &body)?;
    save_stage(input, "init-pg", &spawned)?;
    if spawned.status == 0 { Ok(()) } else { Err(Fail::new("target.schema", 1)) }
}

async fn grant_target(input: &ChainIn<'_>) -> Result<(), Fail> {
    grant_runtime(&super_endpoint(input)?, &input.config.target.username).await
}

async fn read_identity(input: &ChainIn<'_>, imported: Option<&Value>) -> Result<SealedRun, Fail> {
    let run_id = imported.and_then(|doc| doc.get("run_id")).and_then(Value::as_str).unwrap_or("");
    read_sealed_run(&super_endpoint(input)?, &input.config.source_id, run_id).await
}

fn import_pg(input: &ChainIn<'_>) -> Result<Value, Fail> {
    let body = import_request(&ImportInput {
        owner: OWNER,
        shard: "oo-dl01w1-46",
        source_id: &input.config.source_id,
        secret_ref: &input.config.secret_ref,
        source: &input.config.source,
        target: &input.config.target,
        tables: &input.config.approved_objects,
        generation: input.config.source_generation,
    })?;
    ensure_clean(input, &body)?;
    let spawned = run_binary(Path::new(&input.config.bins.pipeline), &pipeline_argv(), &body)?;
    save_stage(input, "import-pg", &spawned)?;
    if spawned.status != 0 {
        return Err(Fail::new("import.rejected", 1));
    }
    let value: Value = serde_json::from_slice(&spawned.stdout).map_err(|_| Fail::new("import.state", 1))?;
    if value.get("protocol").and_then(Value::as_str) == Some("oo.pipeline.import-pg-result/v1")
        && value.get("state").and_then(Value::as_str) == Some("awaiting_publish")
    {
        Ok(value)
    } else {
        Err(Fail::new("import.state", 1))
    }
}

fn install_sealed(input: &ChainIn<'_>) -> Result<(), Fail> {
    let body = install_sealed_request(&input.config.target)?;
    ensure_clean(input, &body)?;
    let spawned = run_binary(Path::new(&input.config.bins.pipeline), &install_argv(), &body)?;
    save_stage(input, "install-sealed", &spawned)?;
    if spawned.status == 0 { Ok(()) } else { Err(Fail::new("target.schema", 1)) }
}

const OWNER_MIGRATION: &str = "/work/0007_sealed_import_owner.sql";

fn prefix_owner_migration(text: &str) -> Result<String, Fail> {
    if !text.contains("CREATE FUNCTION _oo.full_import_sealed_owner(")
        || !text.contains("CREATE FUNCTION _oo.full_import_sealed_columns(")
    {
        return Err(Fail::new("target.schema", 1));
    }
    Ok(format!(
        "DROP FUNCTION IF EXISTS _oo.full_import_sealed_owner(text, text, bigint, uuid);\nDROP FUNCTION IF EXISTS _oo.full_import_sealed_columns(text, text, bigint);\n{text}"
    ))
}

/// The pinned installer applies sealed catalog through migration 0006.
/// catalog-import-pg calls the functions defined in 0007.
async fn install_owner_migration(input: &ChainIn<'_>) -> Result<(), Fail> {
    let bytes = fs::read(OWNER_MIGRATION).map_err(|_| Fail::new("target.schema", 1))?;
    let text = String::from_utf8(bytes).map_err(|_| Fail::new("target.schema", 1))?;
    let sql = prefix_owner_migration(&text)?;
    match apply_sql(&super_endpoint(input)?, &sql).await {
        Ok(()) => write_json(
            input,
            "owner-migration.json",
            &json!({
                "applied": true,
                "source": "5d5547882bdb20df2ac96db6c5727c823667fa1d:crates/oo-pipeline/migrations/0007_sealed_import_owner.sql"
            }),
        ),
        Err(sqlstate) => {
            write_json(input, "owner-migration.json", &json!({"applied": false, "sqlstate": sqlstate}))?;
            Err(Fail::new("target.schema", 1))
        }
    }
}

async fn send_catalog(input: &ChainIn<'_>, imported: Option<&Value>, stored: Option<&SealedRun>) -> Result<(), Fail> {
    let Some(row) = stored else {
        write_json(
            input,
            "catalog-request-meta.json",
            &json!({
                "engine": "postgresql",
                "source_id": input.config.source_id,
                "identity": "absent",
                "tables": "not_sent"
            }),
        )?;
        return Err(Fail::new("catalog.identity", 1));
    };
    let hook = hook_digest();
    let config_digest = row.config_digest.clone();
    let schema_digest = row.schema_digest.clone();
    let type_digest = row.type_digest.clone();
    let input_digest = row.input_digest.clone();
    let run_id = row.run_id.clone();
    let source_generation = u64::try_from(row.source_generation).unwrap_or(0);
    let target_generation = row.target_generation;
    let mut tables = manifest_tables(imported)?;
    let source = &input.config.source;
    let source_endpoint = endpoint_from_files(&source.host, source.port, &source.database,
        &source.username, &source.password_file, &source.ca_file)?;
    let session = crate::pg::connect_read_only(&source_endpoint).await?;
    for table in &mut tables {
        let schema = table["logical_schema"].as_str().ok_or(Fail::new("catalog.identity", 1))?.to_owned();
        let name = table["logical_table"].as_str().ok_or(Fail::new("catalog.identity", 1))?.to_owned();
        let columns = table["columns"].as_array_mut().ok_or(Fail::new("catalog.identity", 1))?;
        for column in columns {
            let logical = column["logical_name"].as_str().ok_or(Fail::new("catalog.identity", 1))?.to_owned();
            let rows = session.client.query("SELECT a.atttypid::bigint, a.atttypmod::integer FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 AND a.attname=$3 AND a.attnum>0 AND NOT a.attisdropped", &[&schema, &name, &logical]).await.map_err(|_| Fail::new("catalog.identity", 1))?;
            if rows.len() != 1 || rows[0].get::<_, i64>(0) != column["oid"].as_i64().unwrap_or(-1) {
                return Err(Fail::new("catalog.identity", 1));
            }
            column["typmod"] = json!(rows[0].get::<_, i32>(1));
        }
    }
    let body = catalog_import_request(&CatalogImport {
        source_id: &input.config.source_id,
        config_digest: &config_digest,
        schema_digest: &schema_digest,
        type_digest: &type_digest,
        input_digest: &input_digest,
        hook_digest: &hook,
        run_id: &run_id,
        owner: OWNER,
        source_generation,
        target_generation,
        tables: &tables,
        target: &input.config.target,
    })?;
    ensure_clean(input, &body)?;
    let request_path = stage_dir(input).join("catalog-import.request.json");
    write_bytes(input, &request_path, &body)?;
    write_json(
        input,
        "catalog-request-meta.json",
        &json!({
            "engine": "postgresql",
            "source_id": input.config.source_id,
            "identity": "stored",
            "owner": OWNER,
            "tables": "import_stdout"
        }),
    )?;
    let spawned = run_binary(Path::new(&input.config.bins.pipeline), &catalog_argv(), &body)?;
    save_stage(input, "catalog-import", &spawned)?;
    if spawned.stderr.windows(b"source engine".len()).any(|window| window == b"source engine") {
        return Err(Fail::new("catalog.engine", 1));
    }
    if spawned.stdout.windows(b"oo.source.pgsql/v1".len()).any(|window| window == b"oo.source.pgsql/v1") {
        return Err(Fail::new("catalog.engine", 1));
    }
    if spawned.status != 0 {
        return Err(Fail::new("import.rejected", 1));
    }
    let value: Value = serde_json::from_slice(&spawned.stdout).map_err(|_| Fail::new("import.state", 1))?;
    if value.get("protocol").and_then(Value::as_str) != Some("oo.pipeline.sealed-import-catalog/v1") {
        return Err(Fail::new("catalog.engine", 1));
    }
    match value.get("engine").and_then(Value::as_str) {
        Some("postgresql" | "postgres") => Ok(()),
        _ => Err(Fail::new("catalog.engine", 1)),
    }
}

fn manifest_tables(imported: Option<&Value>) -> Result<Vec<Value>, Fail> {
    let Some(doc) = imported else {
        return Err(Fail::new("import.shape", 1));
    };
    tables_from_import(doc)
}

fn tables_from_import(doc: &Value) -> Result<Vec<Value>, Fail> {
    let Some(tables) = doc.get("tables").and_then(Value::as_array) else {
        return Err(Fail::new("import.shape", 1));
    };
    let mut built = Vec::new();
    for table in tables {
        built.push(manifest_table(table).ok_or(Fail::new("import.shape", 1))?);
    }
    if built.is_empty() {
        return Err(Fail::new("import.shape", 1));
    }
    Ok(built)
}

fn manifest_table(table: &Value) -> Option<Value> {
    let columns = table.get("columns")?.as_array()?;
    let mapped: Vec<Value> = columns.iter().filter_map(manifest_column).collect();
    if mapped.len() != columns.len() {
        return None;
    }
    Some(json!({
        "logical_schema": table.get("schema")?.as_str()?,
        "logical_table": table.get("name")?.as_str()?,
        "accepted_rows": table.get("rows")?.as_u64()?,
        "canonical_digest": table.get("canonical_digest")?.as_str()?,
        "columns": mapped,
        "rejects": []
    }))
}

fn manifest_column(column: &Value) -> Option<Value> {
    let name = column.get("name")?.as_str()?;
    let oid = column.get("type_oid")?.as_i64()?;
    Some(json!({
        "logical_name": name,
        "oid": oid,
        "typmod": -1,
        "caveat": null,
        "key": false
    }))
}

#[cfg(test)]
fn column_typmod(name: &str, oid: i64) -> Option<i32> {
    // numeric(precision, scale) is stored as ((precision << 16) | scale) + 4.
    const NUMERIC_38_4: i32 = ((38 << 16) | 4) + 4;
    match oid {
        1700 if matches!(name, "latitude" | "longitude" | "cost_amount") => Some(NUMERIC_38_4),
        20 if matches!(
            name,
            "hub_id"
                | "capacity_grams"
                | "shipment_id"
                | "origin_hub_id"
                | "dest_hub_id"
                | "weight_grams"
                | "event_id"
                | "recorded_weight_grams"
        ) =>
        {
            Some(-1)
        }
        2950 if matches!(name, "hub_token" | "shipment_token" | "event_token") => Some(-1),
        25 if matches!(
            name,
            "hub_code" | "hub_name" | "region_code" | "note" | "status" | "transport_mode" | "comment"
        ) =>
        {
            Some(-1)
        }
        1184 if matches!(name, "opened_at" | "created_at" | "promised_at" | "event_at") => Some(-1),
        17 if matches!(name, "badge" | "seal" | "payload") => Some(-1),
        _ => None,
    }
}

fn seal_pack(input: &ChainIn<'_>) -> Result<(), Fail> {
    let catalog_path = stage_dir(input).join("catalog-import.stdout");
    let catalog = read_json(&catalog_path)?;
    let engine = catalog.get("engine").and_then(Value::as_str).unwrap_or("");
    if !matches!(engine, "postgresql" | "postgres") {
        return Err(Fail::new("catalog.engine", 1));
    }
    let source_id = required_source_id(&catalog)?;
    if source_id != input.config.source_id {
        return Err(Fail::new("catalog.identity", 1));
    }
    let (settings, visibility, dataset) = sealed_docs(&catalog, input.config, pack_role_name(input.config))?;
    let dir = stage_dir(input);
    write_bytes(input, &dir.join("settings.json"), &json_vec(&settings)?)?;
    write_bytes(input, &dir.join("visibility.json"), &json_vec(&visibility)?)?;
    fs::write(Path::new(&input.config.out_dir).join("model-called"), b"sealed\n").map_err(|_| Fail::new("config.read", 1))?;
    let price = json!({"status": "declared_non_billing", "currency": "USD", "per_total_token": "0"});
    let rejected = dir.join("rejected-bearer");
    write_bytes(input, &rejected, b"rejected-not-a-customer-token")?;
    let rejected_config = dir.join("builder-config-401.json");
    write_bytes(input, &rejected_config, &builder_config(input, &price, path(&rejected), 180_000)?)?;
    let out_401 = dir.join("sealed-401");
    let cache_401 = dir.join("model-cache-401");
    let _status_401 = run_sealed_dated(input, &SealedLaunch {
        config: &rejected_config,
        settings: &dir.join("settings.json"),
        visibility: &dir.join("visibility.json"),
        out: &out_401,
        dataset: &dataset,
        reviewed_at: "2026-10-11",
        cache: &cache_401,
    })?;
    let doc_401 = read_json(&out_401.join("result.json")).unwrap_or(Value::Null);
    let ok_401 = doc_401.get("http_status").and_then(Value::as_u64) == Some(401)
        && doc_401.get("pack").and_then(Value::as_str) == Some("not_produced")
        && doc_401.get("model").and_then(Value::as_str) == Some("called");
    let timeout_config = dir.join("builder-config-timeout.json");
    write_bytes(input, &timeout_config, &builder_config(input, &price, &input.config.model.bearer_file, 800)?)?;
    let out_timeout = dir.join("sealed-timeout");
    let cache_timeout = dir.join("model-cache-timeout");
    let _status_timeout = run_sealed_dated(input, &SealedLaunch {
        config: &timeout_config,
        settings: &dir.join("settings.json"),
        visibility: &dir.join("visibility.json"),
        out: &out_timeout,
        dataset: &dataset,
        reviewed_at: "2026-10-11",
        cache: &cache_timeout,
    })?;
    let doc_timeout = read_json(&out_timeout.join("result.json")).unwrap_or(Value::Null);
    let ok_timeout = doc_timeout.get("reason").and_then(Value::as_str) == Some("timeout")
        && doc_timeout.get("http_status").and_then(Value::as_u64) == Some(0)
        && doc_timeout.get("pack").and_then(Value::as_str) == Some("not_produced")
        && doc_timeout.get("model").and_then(Value::as_str) == Some("called");
    let unknown_ok = unknown_price(input, &dir, &dataset).is_ok();
    let declared = dir.join("builder-config.json");
    write_bytes(input, &declared, &builder_config(input, &price, &input.config.model.bearer_file, 180_000)?)?;
    let out = dir.join("sealed");
    let status = run_sealed(input, &declared, &dir.join("settings.json"), &dir.join("visibility.json"), &out, &dataset)?;
    let sealed_doc = read_json(&out.join("result.json")).unwrap_or(Value::Null);
    let authority = read_json(&out.join("authority.json")).unwrap_or(Value::Null);
    let project = authority.get("project").and_then(Value::as_str).unwrap_or("");
    let imported = authority.pointer("/import/source_id").and_then(Value::as_str).unwrap_or("");
    let packed = status == 0
        && sealed_doc.get("cache").and_then(Value::as_str) == Some("miss")
        && sealed_doc.get("model").and_then(Value::as_str) == Some("called")
        && sealed_doc.get("pack").and_then(Value::as_str) == Some("produced")
        && sealed_doc.get("outcome").and_then(Value::as_str) == Some("packed")
        && project == source_id
        && imported == source_id;
    if !ok_401 || !ok_timeout || !unknown_ok || !packed {
        return Err(Fail::new("builder.sealed", 1));
    }
    Ok(())
}

/// Produce one genuine compiler authority per dataset, reusing the same real generation.
pub fn seal_all_authorities(input: &ChainIn<'_>) -> Result<(), Fail> {
    let dir = stage_dir(input);
    let catalog = read_json(&dir.join("catalog-import.stdout"))?;
    let settings = dir.join("settings.json");
    let visibility = dir.join("visibility.json");
    let config = dir.join("builder-config.json");
    let mut receipts = Vec::new();
    for (index, (dataset, _)) in dataset_rows(&catalog)?.iter().enumerate() {
        let out = dir.join(format!("sealed-table-{index}"));
        let exit = run_sealed(input, &config, &settings, &visibility, &out, dataset)?;
        let result = read_json(&out.join("result.json")).unwrap_or(Value::Null);
        receipts.push(json!({"dataset": dataset, "exit": exit, "result": result}));
        if exit != 0 || result.get("outcome").and_then(Value::as_str) != Some("packed") || !out.join("authority.json").is_file() {
            write_json(input, "dataset-authorities.json", &json!({"datasets": receipts, "complete": false}))?;
            return Err(Fail::new("builder.dataset", 1));
        }
    }
    write_json(input, "dataset-authorities.json", &json!({"datasets": receipts, "complete": true}))
}

fn unknown_price(input: &ChainIn<'_>, dir: &Path, dataset: &str) -> Result<(), Fail> {
    let config = dir.join("builder-config-unknown.json");
    write_bytes(input, &config, &builder_config(input, &json!({"status": "unknown"}), &input.config.model.bearer_file, 180_000)?)?;
    let out = dir.join("sealed-unknown");
    let _status = run_sealed(input, &config, &dir.join("settings.json"), &dir.join("visibility.json"), &out, dataset)?;
    let doc = read_json(&out.join("result.json")).unwrap_or(Value::Null);
    if doc.get("outcome").and_then(Value::as_str) == Some("cost_unknown")
        && doc.get("pack").and_then(Value::as_str) == Some("not_produced")
    {
        return Ok(());
    }
    Err(Fail::new("builder.sealed", 1))
}

pub fn rebind_catalog(input: &ChainIn<'_>) -> Result<(), Fail> {
    // Same sealed capture. New settings only. The historical seal directory stays.
    let catalog_path = stage_dir(input).join("catalog-import.stdout");
    let catalog = read_json(&catalog_path)?;
    let engine = catalog.get("engine").and_then(Value::as_str).unwrap_or("");
    if !matches!(engine, "postgresql" | "postgres") {
        return Err(Fail::new("catalog.engine", 1));
    }
    let source_id = required_source_id(&catalog)?;
    if source_id != input.config.source_id {
        return Err(Fail::new("catalog.identity", 1));
    }
    let (settings, visibility, dataset) = sealed_docs(&catalog, input.config, pack_role_name(input.config))?;
    let dir = stage_dir(input).join("rebind-input");
    let settings_path = dir.join("settings.json");
    let visibility_path = dir.join("visibility.json");
    let declared = dir.join("builder-config.json");
    write_bytes(input, &settings_path, &json_vec(&settings)?)?;
    write_bytes(input, &visibility_path, &json_vec(&visibility)?)?;
    write_bytes(
        input,
        &declared,
        &builder_config(input, &json!({"status": "declared_non_billing", "currency": "USD", "per_total_token": "0"}), &input.config.model.bearer_file, 180_000)?,
    )?;
    let out = stage_dir(input).join("sealed-rebind");
    let cache = stage_dir(input).join("model-cache");
    let status = run_sealed_dated(input, &SealedLaunch {
        config: &declared,
        settings: &settings_path,
        visibility: &visibility_path,
        out: &out,
        dataset: &dataset,
        reviewed_at: "2026-10-11",
        cache: &cache,
    })?;
    if status != 0 || !out.join("authority.json").is_file() {
        return Err(Fail::new("builder.sealed", 1));
    }
    let authority = read_json(&out.join("authority.json"))?;
    let project = authority.get("project").and_then(Value::as_str).unwrap_or("");
    let imported = authority.pointer("/import/source_id").and_then(Value::as_str).unwrap_or("");
    if project != source_id || imported != source_id {
        return Err(Fail::new("builder.sealed", 1));
    }
    Ok(())
}

pub fn reseal_arrow(input: &ChainIn<'_>) -> Result<(), Fail> {
    let catalog = read_json(&stage_dir(input).join("catalog-import.stdout"))?;
    let source_id = required_source_id(&catalog)?;
    if source_id != input.config.source_id {
        return Err(Fail::new("catalog.identity", 1));
    }
    let batches = arrow_batches(input)?;
    let document = native_arrow_document(&source_id, &catalog, &batches)?;
    let bytes = json_vec(&document)?;
    let spawned = run_binary(Path::new(&input.config.bins.pipeline), &["import-native".to_owned()], &bytes)?;
    save_stage(input, "arrow-reseal", &spawned)?;
    let parsed: Value = serde_json::from_slice(&spawned.stdout).unwrap_or(Value::Null);
    let reason = parsed.get("reason").and_then(Value::as_str).unwrap_or("");
    let new_run = parsed.get("run_id").and_then(Value::as_str).unwrap_or("");
    let identity = json!({
        "protocol": "oo.postgres-preview.arrow-reseal/v1",
        "source_id": source_id,
        "source_engine": "postgresql",
        "prior_seal_run_id": catalog.get("run_id").and_then(Value::as_str).unwrap_or(""),
        "pipeline_reason": reason,
        "pipeline_exit": spawned.status,
        "new_run_id": if new_run.is_empty() { Value::Null } else { json!(new_run) },
        "impersonates_prior_audit": false,
        "tables": batches.iter().map(|batch| json!({"table": batch.table, "rows": batch.rows, "arrow_sha256": batch.sha256})).collect::<Vec<_>>()
    });
    write_bytes(input, &stage_dir(input).join("arrow-reseal-identity.json"), &json_vec(&identity)?)?;
    if spawned.status == 0 && parsed.get("load_state").and_then(Value::as_str) == Some("awaiting_publish") && !new_run.is_empty() {
        return Ok(());
    }
    Err(Fail::new("arrow.restore", 1))
}

fn required_source_id(catalog: &Value) -> Result<String, Fail> {
    match catalog.get("source_id").and_then(Value::as_str) {
        Some(value) if !value.is_empty() => Ok(value.to_owned()),
        _ => Err(Fail::new("catalog.identity", 1)),
    }
}

#[derive(Debug)]
struct ArrowBatch {
    schema: String,
    table: String,
    rows: u64,
    sha256: String,
    path: String,
}

fn arrow_batches(input: &ChainIn<'_>) -> Result<Vec<ArrowBatch>, Fail> {
    let catalog = read_json(&stage_dir(input).join("catalog-import.stdout"))?;
    let source_id = required_source_id(&catalog)?;
    if source_id != input.config.source_id {
        return Err(Fail::new("catalog.identity", 1));
    }
    let tables = catalog.get("tables").and_then(Value::as_array).ok_or(Fail::new("arrow.restore", 1))?;
    if tables.is_empty() {
        return Err(Fail::new("arrow.restore", 1));
    }
    let mut batches = Vec::new();
    for table in tables {
        let schema = table.get("logical_schema").and_then(Value::as_str).ok_or(Fail::new("arrow.restore", 1))?;
        let name = table.get("logical_table").and_then(Value::as_str).ok_or(Fail::new("arrow.restore", 1))?;
        if !input.config.approved_objects.iter().any(|object| object.schema == schema && object.table == name) {
            return Err(Fail::new("unapproved", 6));
        }
        batches.push(read_arrow_batch(input, schema, name)?);
    }
    Ok(batches)
}

fn read_arrow_batch(input: &ChainIn<'_>, schema: &str, table: &str) -> Result<ArrowBatch, Fail> {
    let stdout = fs::read_to_string(stage_dir(input).join(format!("{}.stdout", producer_stage(schema, table)))).map_err(|_| Fail::new("arrow.restore", 1))?;
    let rows = field_u64(&stdout, "rows=").ok_or(Fail::new("arrow.restore", 1))?;
    let sha = field_token(&stdout, "arrow_sha256=").ok_or(Fail::new("arrow.restore", 1))?;
    if rows == 0 || sha.len() != 64 {
        return Err(Fail::new("arrow.restore", 1));
    }
    let path = Path::new(&input.config.out_dir).join("producer").join(producer_leaf(schema, table)).join(format!("{table}.arrow"));
    let bytes = fs::read(&path).map_err(|_| Fail::new("arrow.restore", 1))?;
    if sha256_bytes(&bytes) != sha {
        return Err(Fail::new("arrow.restore", 1));
    }
    Ok(ArrowBatch {
        schema: schema.to_owned(),
        table: table.to_owned(),
        rows,
        sha256: sha,
        path: path.to_str().unwrap_or("").to_owned(),
    })
}

fn field_token(text: &str, key: &str) -> Option<String> {
    let rest = text.split(key).nth(1)?;
    Some(rest.split_whitespace().next()?.trim().to_owned())
}

fn field_u64(text: &str, key: &str) -> Option<u64> {
    field_token(text, key)?.parse().ok()
}

fn native_arrow_document(source_id: &str, catalog: &Value, batches: &[ArrowBatch]) -> Result<Value, Fail> {
    if source_id.is_empty() || batches.is_empty() {
        return Err(Fail::new("arrow.restore", 1));
    }
    let tables: Vec<Value> = batches
        .iter()
        .map(|batch| {
            json!({
                "logical_schema": batch.schema,
                "logical_table": batch.table,
                "row_count": batch.rows,
                "batch_sha256": batch.sha256,
                "arrow_path": batch.path
            })
        })
        .collect();
    Ok(json!({
        "protocol": "oo.pipeline.native-arrow-import/v1",
        "source_engine": "postgresql",
        "target_storage": "postgresql",
        "source_id": source_id,
        "source_generation": catalog.get("source_generation").cloned().unwrap_or(json!(1)),
        "snapshot": {"kind": "token", "value": "genuine-arrow"},
        "catalog_identity": {
            "config_digest": catalog.get("config_digest").cloned().unwrap_or(Value::Null),
            "schema_digest": catalog.get("schema_digest").cloned().unwrap_or(Value::Null),
            "type_digest": catalog.get("type_digest").cloned().unwrap_or(Value::Null),
            "input_digest": catalog.get("input_digest").cloned().unwrap_or(Value::Null)
        },
        "tables": tables
    }))
}

fn sealed_docs(catalog: &Value, config: &Config, role: &str) -> Result<(Value, Value, String), Fail> {
    let recorded = required_source_id(catalog)?;
    if recorded != config.source_id {
        return Err(Fail::new("catalog.identity", 1));
    }
    let rows = dataset_rows(catalog)?;
    let dataset = rows.first().map(|(name, _)| name.clone()).ok_or(Fail::new("builder.sealed", 1))?;
    let schemas: Vec<&str> = {
        let mut seen = Vec::new();
        for (name, _) in &rows {
            let schema = name.split('.').next().unwrap_or("");
            if schema.is_empty() {
                return Err(Fail::new("catalog.identity", 1));
            }
            if !seen.contains(&schema) {
                seen.push(schema);
            }
        }
        seen
    };
    if schemas.len() != 1 {
        return Err(Fail::new("catalog.identity", 1));
    }
    let generation = catalog_generation(catalog)?;
    let digest = catalog.get("config_digest").and_then(Value::as_str).unwrap_or("");
    if !crate::config::digest64(digest) {
        return Err(Fail::new("catalog.identity", 1));
    }
    let compiled = crate::config::compiled_registry(schemas[0], &generation)?;
    if config.registry.as_ref() != Some(&compiled) {
        return Err(Fail::new("catalog.identity", 1));
    }
    let knowledge_id = compiled.knowledge_id.clone();
    let knowledge_version = compiled.knowledge_version.clone();
    let policy_id = compiled.policy_id.clone();
    let policy_version = compiled.policy_version.clone();
    let ontology_name = compiled.ontology_name.clone();
    let ontology_version = compiled.ontology_version.clone();
    let dataset_values: Vec<Value> = rows
        .iter()
        .map(|(name, columns)| {
            json!({
                "columns": columns,
                "dataset": name,
                "global_stats": "denied",
                "row_security": "none"
            })
        })
        .collect();
    let settings = json!({
        "catalog_version": digest,
        "knowledge": {"id": knowledge_id.clone(), "version": knowledge_version.clone()},
        "ontology": {
            "classification": {"auto_accept": "rule_high_confidence"},
            "compile": {
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 4000},
                "per_role": true,
                "sample_values": {"default": "masked", "max_per_column": 0}
            },
            "default_tier_preference": ["raw"],
            "include_schemas": schemas,
            "lifecycle": {"deprecation_min_releases": 2},
            "locale": "en-US",
            "name": ontology_name.clone(),
            "publish": {"include_unreviewed": []},
            "spec_version": "oos/v1"
        },
        "ontology_version": ontology_version.clone(),
        "policy": {"id": policy_id.clone(), "version": policy_version.clone()},
        "project": config.source_id
    });
    let visibility = json!({
        "catalog_version": digest,
        "datasets": dataset_values,
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "knowledge": {"id": knowledge_id, "version": knowledge_version},
        "limits": {"max_entries": 128, "max_file_bytes": 262144, "max_total_bytes": 1048576},
        "ontology_version": ontology_version,
        "policy": {"id": policy_id, "version": policy_version},
        "principal": "operator",
        "project": config.source_id,
        "protocol": "oo.builder.role-visibility/v1",
        "role": role
    });
    Ok((settings, visibility, dataset))
}

fn catalog_generation(catalog: &Value) -> Result<String, Fail> {
    let text = match catalog.get("source_generation") {
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::String(text)) => text.clone(),
        _ => return Err(Fail::new("catalog.identity", 1)),
    };
    if text.is_empty() || text.bytes().any(|byte| byte == b'.') {
        return Err(Fail::new("catalog.identity", 1));
    }
    Ok(text)
}

fn dataset_rows(catalog: &Value) -> Result<Vec<(String, Vec<String>)>, Fail> {
    let tables = catalog.get("tables").and_then(Value::as_array).ok_or(Fail::new("builder.sealed", 1))?;
    let mut rows = Vec::new();
    for table in tables {
        let schema = table.get("logical_schema").and_then(Value::as_str).ok_or(Fail::new("builder.sealed", 1))?;
        let name = table.get("logical_table").and_then(Value::as_str).ok_or(Fail::new("builder.sealed", 1))?;
        let columns = table.get("columns").and_then(Value::as_array).ok_or(Fail::new("builder.sealed", 1))?;
        let names: Vec<String> = columns
            .iter()
            .filter_map(|column| column.get("logical_name").and_then(Value::as_str).map(str::to_owned))
            .collect();
        if names.is_empty() {
            return Err(Fail::new("builder.sealed", 1));
        }
        rows.push((format!("{schema}.{name}"), names));
    }
    if rows.is_empty() {
        return Err(Fail::new("builder.sealed", 1));
    }
    Ok(rows)
}

struct SealedLaunch<'a> {
    config: &'a Path,
    settings: &'a Path,
    visibility: &'a Path,
    out: &'a Path,
    dataset: &'a str,
    reviewed_at: &'a str,
    cache: &'a Path,
}

fn run_sealed(input: &ChainIn<'_>, config: &Path, settings: &Path, visibility: &Path, out: &Path, dataset: &str) -> Result<i32, Fail> {
    let cache = stage_dir(input).join("model-cache");
    run_sealed_dated(input, &SealedLaunch {
        config,
        settings,
        visibility,
        out,
        dataset,
        reviewed_at: "2026-10-10",
        cache: &cache,
    })
}

fn run_sealed_dated(input: &ChainIn<'_>, launch: &SealedLaunch<'_>) -> Result<i32, Fail> {
    let dir = stage_dir(input);
    let catalog = dir.join("catalog-import.stdout");
    let target = &input.config.target;
    let port = target.port.to_string();
    let args = [
        "sealed",
        "--config",
        path(launch.config),
        "--settings",
        path(launch.settings),
        "--visibility",
        path(launch.visibility),
        "--catalog",
        path(&catalog),
        "--cache",
        path(launch.cache),
        "--out",
        path(launch.out),
        "--reviewer",
        "operator",
        "--reviewed-at",
        launch.reviewed_at,
        "--dataset",
        launch.dataset,
        "--target-host",
        &target.host,
        "--target-port",
        port.as_str(),
        "--target-database",
        &target.database,
        "--target-user",
        &target.username,
        "--login",
        &target.username,
        "--target-ca",
        &target.ca_file,
        "--target-password",
        &target.password_file,
    ];
    let owned: Vec<String> = args.iter().map(|item| (*item).to_owned()).collect();
    let name = launch.out.file_name().and_then(|item| item.to_str()).unwrap_or("sealed");
    let spawned = run_binary(Path::new(&input.config.bins.builder), &owned, b"")?;
    save_stage(input, name, &spawned)?;
    Ok(spawned.status)
}

fn builder_config(input: &ChainIn<'_>, price: &Value, bearer_file: &str, deadline_ms: u64) -> Result<Vec<u8>, Fail> {
    let target = &input.config.target;
    let tables: Vec<Value> = input.config.approved_objects.iter().map(|object| json!({"schema": object.schema, "table": object.table})).collect();
    let body = json!({
        "protocol": "oo.pg-builder.config/v1",
        "adapter": "loopback_http_local_dev",
        "namespace": "pg-builder",
        "purpose": "generate",
        "sample_policy": "none",
        "temperature": 0,
        "endpoint": {"origin": "http://127.0.0.1:8317", "path": "/v1/chat/completions", "model_id": "grok-4.7"},
        "secret": {"bearer_file": bearer_file},
        "declared_price": price,
        // Pin ceilings. A valid completion exceeded 4000 completion tokens, and a later call hit 120s.
        "budget": {
            "deadline_ms": deadline_ms,
            "max_completion_tokens": 100000,
            "max_in_flight": 1,
            "max_prompt_tokens": 100000,
            "max_request_bytes": 262144,
            "max_response_bytes": 262144,
            "max_tables": 16,
            "max_total_tokens": 200000
        },
        "postgres": {
            "host": target.host,
            "port": target.port,
            "database": target.database,
            "username": target.username,
            "password_file": target.password_file,
            "ca_file": target.ca_file,
            "catalog_bin": input.config.bins.pipeline,
            "tables": tables
        }
    });
    serde_json::to_vec(&body).map_err(|_| Fail::new("config.schema", 1))
}

fn record_consumer_deferred(input: &ChainIn<'_>) -> Result<(), Fail> {
    // The typed consumer runs only after the source container is stopped.
    // This run keeps the sealed files and does not call the model again.
    write_json(
        input,
        "consumer-wait.json",
        &json!({
            "protocol": "oo.postgres-preview.consumer-wait/v1",
            "token": "source_still_open",
            "called": false,
            "reason": "typed consumer runs after the source is closed",
            "consumer_commit": crate::consume::CONSUMER_COMMIT,
            "preview_binary_sha256": crate::consume::CONSUMER_SHA256
        }),
    )
}

fn json_vec(value: &Value) -> Result<Vec<u8>, Fail> {
    serde_json::to_vec(value).map_err(|_| Fail::new("config.schema", 1))
}

fn stage_dir(input: &ChainIn<'_>) -> PathBuf {
    Path::new(&input.config.out_dir).join("stages")
}

fn path(value: &Path) -> &str {
    value.to_str().unwrap_or("")
}

fn read_json(path: &Path) -> Result<Value, Fail> {
    let bytes = fs::read(path).map_err(|_| Fail::new("builder.review_input", 1))?;
    serde_json::from_slice(&bytes).map_err(|_| Fail::new("builder.review_input", 1))
}

fn save_stage(input: &ChainIn<'_>, name: &str, spawned: &crate::spawn::Spawned) -> Result<(), Fail> {
    let dir = stage_dir(input);
    fs::create_dir_all(&dir).map_err(|_| Fail::new("config.read", 1))?;
    write_bytes(input, &dir.join(format!("{name}.stdout")), &spawned.stdout)?;
    write_bytes(input, &dir.join(format!("{name}.stderr")), &spawned.stderr)?;
    write_bytes(input, &dir.join(format!("{name}.status")), spawned.status.to_string().as_bytes())
}

fn write_json(input: &ChainIn<'_>, name: &str, value: &Value) -> Result<(), Fail> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| Fail::new("config.read", 1))?;
    write_bytes(input, &Path::new(&input.config.out_dir).join(name), &bytes)
}

fn write_bytes(input: &ChainIn<'_>, path: &Path, bytes: &[u8]) -> Result<(), Fail> {
    ensure_clean(input, bytes)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| Fail::new("config.read", 1))?;
    }
    fs::write(path, bytes).map_err(|_| Fail::new("config.read", 1))
}

fn ensure_clean(input: &ChainIn<'_>, bytes: &[u8]) -> Result<(), Fail> {
    if input.secrets.iter().any(|secret| contains_secret(bytes, secret)) {
        Err(Fail::new("secret.leak", 1))
    } else {
        Ok(())
    }
}

fn write_phase(input: &ChainIn<'_>, phase: &str, partial: bool) -> Result<(), Fail> {
    write_json(
        input,
        "state.json",
        &json!({
            "protocol": "oo.postgres-preview.state/v1",
            "run_id": input.run_id,
            "phase": phase,
            "partial": partial
        }),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::config::{compiled_registry, Bins, Budget, Config, EndpointFile, Model, ObjectName, Registry};

    use serde_json::json;

    use super::{bounded_rows, column_typmod, prefix_owner_migration, producer_args, ChainIn};

    fn endpoint() -> EndpointFile {
        EndpointFile {
            host: "src".to_owned(),
            port: 5432,
            database: "src".to_owned(),
            username: "reader".to_owned(),
            password_file: "/run/secrets/reader".to_owned(),
            ca_file: "/run/secrets/ca.pem".to_owned(),
        }
    }

    #[test]
    fn bounded_read_requests_the_full_table() {
        let config = Config {
            protocol: "oo.postgres-preview.config/v1".to_owned(),
            source_id: "oo_dl01w1_46".to_owned(),
            secret_ref: "secret://oo-dl01w1-46/reader".to_owned(),
            role: "oo_dl01w1_46_reader".to_owned(),
            approved_objects: vec![
                ObjectName { schema: "public".to_owned(), table: "hubs".to_owned() },
                ObjectName { schema: "public".to_owned(), table: "shipments".to_owned() },
                ObjectName { schema: "public".to_owned(), table: "tracking_events".to_owned() },
            ],
            budget: Budget { max_rows: 100_000, max_bytes: 134_217_728, statement_ms: 120_000 },
            model: Model {
                origin: "http://127.0.0.1:8317".to_owned(),
                path: "/v1/chat/completions".to_owned(),
                model_id: "grok-4.7".to_owned(),
                bearer_file: "/tmp/token".to_owned(),
            },
            source: endpoint(),
            dba: endpoint(),
            target: endpoint(),
            bins: Bins {
                admission: "/opt/oo-source-admission".to_owned(),
                budget: "/opt/oo-source-admission-pg-budget".to_owned(),
                pipeline: "/opt/oo-pipeline".to_owned(),
                builder: "/opt/pg-builder".to_owned(),
                preview: "/opt/operator-preview".to_owned(),
            },
            coordinator_state: "/ontology/execution/pg-budget-ready.json".to_owned(),
            out_dir: "/work-out/run".to_owned(),
            denied: None,
            approved_fields: Vec::new(),
            pack_role: String::new(),
            registry: None,
            source_generation: 1,
            capture: None,
        };
        let input = ChainIn {
            config: &config,
            secrets: &[],
            run_id: "run",
            rows: 100_000,
            bytes: 1,
            session_sha: "abc",
            target_tables: 0,
            commit: None,
        };
        let object = config.approved_objects.iter().find(|item| item.table == "tracking_events").unwrap();
        let evidence = Path::new("/out/producer/tracking_events/evidence.json");
        let args = producer_args(
            &input,
            object,
            evidence,
            Path::new("/out/access.json"),
            Path::new("/out/config.json"),
            Path::new("/out/decision.json"),
            (33_554_432, 90_000),
        );
        let flag = |name: &str| args.windows(2).find(|pair| pair[0] == name).map(|pair| pair[1].as_str());
        assert_eq!(flag("--phase"), Some("bounded-read"));
        assert_eq!(flag("--expect-rows"), Some("90000"));
        assert_eq!(flag("--max-rows"), Some("100000"));
        assert_eq!(flag("--max-bytes"), Some("33554432"));
        assert_eq!(flag("--page-rows"), Some("4096"));
        assert_eq!(flag("--deadline-ms"), Some("900000"));
        assert_eq!(flag("--arrow-out"), Some("/out/producer/tracking_events/tracking_events.arrow"));
        assert_eq!(flag("--snapshot-file"), Some("/out/producer/tracking_events/snapshot.txt"));
        assert!(args.windows(2).all(|pair| !(pair[0] == "--phase" && pair[1] == "gate-read")));
        assert!(args.iter().all(|item| item != "read_probe"));
        assert_eq!(column_typmod("cost_amount", 1700), Some(2_490_376));
        assert_eq!(column_typmod("hub_token", 2950), Some(-1));
        assert_eq!(column_typmod("hub_token", 1700), None);
        assert_eq!(((2_490_376 - 4) >> 16) & 0xFFFF, 38);
        assert_eq!((2_490_376 - 4) & 0xFFFF, 4);
        let line = b"rows=90000 pages=22 bytes=8027821 windows_used=11 max_rows_in_window=9219 rate_waits=0 snapshot=0000000C-0000002C-1 arrow_sha256=4c081d20\n";
        assert_eq!(bounded_rows(line), Some(90_000));
        assert_eq!(bounded_rows(b"rows=50000\n"), None);
    }

    #[test]
    fn owner_migration_keeps_the_frozen_functions() {
        let frozen = "CREATE FUNCTION _oo.full_import_sealed_owner(\nCREATE FUNCTION _oo.full_import_sealed_columns(\n";
        let sql = prefix_owner_migration(frozen).expect("both functions");
        assert!(sql.starts_with(
            "DROP FUNCTION IF EXISTS _oo.full_import_sealed_owner(text, text, bigint, uuid);\nDROP FUNCTION IF EXISTS _oo.full_import_sealed_columns(text, text, bigint);\n"
        ));
        assert!(sql.ends_with(frozen));
        assert!(prefix_owner_migration("CREATE FUNCTION _oo.full_import_sealed_owner(\n").is_err());
    }

    #[test]
    fn settings_project_matches_the_catalog_source() {
        let catalog = json!({
            "source_id": "oo_dl01w1_46",
            "source_generation": 7,
            "config_digest": "df690a6f697d3f78d95d3bf878df7254e16665a15e0e8407bb3dc64f64368306",
            "tables": [{
                "logical_schema": "public",
                "logical_table": "hubs",
                "columns": [{"logical_name": "hub_id"}, {"logical_name": "hub_token"}]
            }]
        });
        let config = docs_config("oo_dl01w1_46", "public", "hubs", "7", "");
        let (settings, visibility, dataset) = super::sealed_docs(&catalog, &config, "oo_r_reader").expect("docs");
        assert_eq!(settings["project"], "oo_dl01w1_46");
        assert_eq!(visibility["project"], "oo_dl01w1_46");
        assert_eq!(settings["ontology"]["name"], "public");
        assert_eq!(settings["ontology_version"], "ont-public");
        assert_eq!(settings["knowledge"]["id"], "know-public");
        assert_eq!(settings["knowledge"]["version"], "7");
        assert_eq!(settings["policy"]["id"], "pol-public");
        assert_eq!(settings["policy"]["version"], "7");
        assert_eq!(settings["catalog_version"], catalog["config_digest"]);
        assert_eq!(visibility["knowledge"]["id"], "know-public");
        assert_eq!(visibility["ontology_version"], "ont-public");
        assert_eq!(visibility["catalog_version"], catalog["config_digest"]);
        let text = settings.to_string();
        assert!(!text.contains("logistics"));
        assert!(!text.contains("pending"));
        assert_eq!(dataset, "public.hubs");
        assert_eq!(visibility["role"], "oo_r_reader");
        let mut forged = config.clone();
        forged.registry = Some(Registry {
            knowledge_id: "know-logistics".to_owned(),
            knowledge_version: "1".to_owned(),
            policy_id: "pol-logistics".to_owned(),
            policy_version: "1".to_owned(),
            ontology_name: "logistics".to_owned(),
            ontology_version: "ont-1".to_owned(),
        });
        assert!(super::sealed_docs(&catalog, &forged, "oo_r_reader").is_err());
        let mut other = config.clone();
        other.source_id = "logistics".to_owned();
        assert!(super::sealed_docs(&catalog, &other, "oo_r_reader").is_err());
        let mut bare = catalog.clone();
        bare.as_object_mut().expect("object").remove("source_generation");
        assert!(super::sealed_docs(&bare, &config, "oo_r_reader").is_err());
        let batch = super::ArrowBatch {
            schema: "public".to_owned(),
            table: "hubs".to_owned(),
            rows: 2000,
            sha256: "0a6bbd29c1a6ac77f0e9b4e8db72dc4a4d9b5ac0eac0cb3b7185f2553ff1a55e".to_owned(),
            path: "/work-out/run/producer/hubs/hubs.arrow".to_owned(),
        };
        let document = super::native_arrow_document("oo_dl01w1_46", &catalog, &[batch]).expect("arrow");
        assert_eq!(document["protocol"], "oo.pipeline.native-arrow-import/v1");
        assert_eq!(document["source_engine"], "postgresql");
        assert_eq!(document["source_id"], "oo_dl01w1_46");
        assert_eq!(document["tables"][0]["row_count"], 2000);
        assert!(super::native_arrow_document("logistics", &catalog, &[]).is_err());
    }

    #[test]
    fn customer_reseal_uses_the_measured_arrow_count() {
        let root = std::env::temp_dir().join(format!("oo-dl01w1-46-arrow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bytes = b"customer-east-delivery-notes";
        let sha = crate::admit::sha256_bytes(bytes);
        let leaf = root.join("producer").join("customer_east--delivery_notes");
        std::fs::create_dir_all(root.join("stages")).unwrap();
        std::fs::create_dir_all(&leaf).unwrap();
        std::fs::write(leaf.join("delivery_notes.arrow"), bytes).unwrap();
        std::fs::write(
            root.join("stages").join("producer-customer_east--delivery_notes.stdout"),
            format!("rows=17 pages=1 bytes=27 arrow_sha256={sha}\n"),
        ).unwrap();
        let catalog = json!({
            "source_id": "oo_dl01w1_46",
            "tables": [
                {"logical_schema": "customer_east", "logical_table": "delivery_notes", "columns": [{"logical_name": "note_id"}]},
                {"logical_schema": "customer_east", "logical_table": "payroll", "columns": [{"logical_name": "internal_note"}]}
            ]
        });
        std::fs::write(root.join("stages").join("catalog-import.stdout"), serde_json::to_vec(&catalog).unwrap()).unwrap();
        let out = root.to_str().unwrap();
        {
            let config = customer_config(out);
            let input = customer_input(&config);
            assert_eq!(super::arrow_batches(&input).unwrap_err().token, "unapproved");
            assert!(!leaf.join("payroll.arrow").exists());
        }
        let only = json!({
            "source_id": "oo_dl01w1_46",
            "source_generation": 4,
            "config_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "tables": [{"logical_schema": "customer_east", "logical_table": "delivery_notes", "columns": [{"logical_name": "note_id"}]}]
        });
        std::fs::write(root.join("stages").join("catalog-import.stdout"), serde_json::to_vec(&only).unwrap()).unwrap();
        {
            let config = customer_config(out);
            let input = customer_input(&config);
            let batches = super::arrow_batches(&input).expect("measured");
            assert_eq!(batches.len(), 1);
            assert_eq!(batches[0].schema, "customer_east");
            assert_eq!(batches[0].table, "delivery_notes");
            assert_eq!(batches[0].rows, 17);
        }
        let config = customer_config(out);
        let (settings, visibility, dataset) = super::sealed_docs(&only, &config, "oo_r_east").expect("east");
        assert_eq!(dataset, "customer_east.delivery_notes");
        assert_eq!(visibility["role"], "oo_r_east");
        assert_eq!(settings["ontology"]["include_schemas"][0], "customer_east");
        assert_eq!(settings["ontology"]["name"], "customer_east");
        assert_eq!(settings["knowledge"]["id"], "know-customer_east");
        assert_eq!(settings["policy"]["id"], "pol-customer_east");
        assert_eq!(settings["ontology_version"], "ont-customer_east");
        assert_eq!(settings["knowledge"]["version"], "4");
        assert_eq!(settings["catalog_version"], only["config_digest"]);
        let text = settings.to_string();
        assert!(!text.contains("logistics"));
        assert!(!text.contains("pending"));
        std::fs::write(
            root.join("stages").join("producer-customer_east--delivery_notes.stdout"),
            format!("rows=0 pages=0 bytes=0 arrow_sha256={sha}\n"),
        ).unwrap();
        {
            let config = customer_config(out);
            let input = customer_input(&config);
            assert_eq!(super::arrow_batches(&input).unwrap_err().token, "arrow.restore");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    fn customer_input<'a>(config: &'a Config) -> ChainIn<'a> {
        ChainIn {
            config,
            secrets: &[],
            run_id: "customer",
            rows: 17,
            bytes: 27,
            session_sha: "abc",
            target_tables: 0,
            commit: None,
        }
    }

    fn customer_config(out_dir: &str) -> Config {
        Config {
            protocol: "oo.postgres-preview.config/v1".to_owned(),
            source_id: "oo_dl01w1_46".to_owned(),
            secret_ref: "secret://oo-dl01w1-46/reader".to_owned(),
            role: "oo_dl01w1_46_reader".to_owned(),
            approved_objects: vec![ObjectName { schema: "customer_east".to_owned(), table: "delivery_notes".to_owned() }],
            approved_fields: vec![],
            pack_role: "oo_r_east".to_owned(),
            budget: Budget { max_rows: 100_000, max_bytes: 134_217_728, statement_ms: 120_000 },
            model: Model {
                origin: "http://127.0.0.1:8317".to_owned(),
                path: "/v1/chat/completions".to_owned(),
                model_id: "grok-4.7".to_owned(),
                bearer_file: "/tmp/token".to_owned(),
            },
            source: endpoint(),
            dba: endpoint(),
            target: endpoint(),
            bins: Bins {
                admission: "/opt/oo-source-admission".to_owned(),
                budget: "/opt/oo-source-admission-pg-budget".to_owned(),
                pipeline: "/opt/oo-pipeline".to_owned(),
                builder: "/opt/pg-builder".to_owned(),
                preview: "/opt/operator-preview".to_owned(),
            },
            coordinator_state: "/ontology/execution/pg-budget-ready.json".to_owned(),
            out_dir: out_dir.to_owned(),
            denied: None,
            registry: Some(compiled_registry("customer_east", "4").expect("registry")),
            source_generation: 4,
            capture: None,
        }
    }

    fn docs_config(source_id: &str, schema: &str, table: &str, generation: &str, pack_role: &str) -> Config {
        Config {
            protocol: "oo.postgres-preview.config/v1".to_owned(),
            source_id: source_id.to_owned(),
            secret_ref: "secret://oo-dl01w1-46/reader".to_owned(),
            role: "oo_dl01w1_46_reader".to_owned(),
            approved_objects: vec![ObjectName { schema: schema.to_owned(), table: table.to_owned() }],
            approved_fields: Vec::new(),
            pack_role: pack_role.to_owned(),
            budget: Budget { max_rows: 100_000, max_bytes: 134_217_728, statement_ms: 120_000 },
            model: Model {
                origin: "http://127.0.0.1:8317".to_owned(),
                path: "/v1/chat/completions".to_owned(),
                model_id: "grok-4.7".to_owned(),
                bearer_file: "/tmp/token".to_owned(),
            },
            source: endpoint(),
            dba: endpoint(),
            target: endpoint(),
            bins: Bins {
                admission: "/opt/oo-source-admission".to_owned(),
                budget: "/opt/oo-source-admission-pg-budget".to_owned(),
                pipeline: "/opt/oo-pipeline".to_owned(),
                builder: "/opt/pg-builder".to_owned(),
                preview: "/opt/operator-preview".to_owned(),
            },
            coordinator_state: "/ontology/execution/pg-budget-ready.json".to_owned(),
            out_dir: "/work-out/run".to_owned(),
            denied: None,
            registry: Some(compiled_registry(schema, generation).expect("registry")),
            source_generation: generation.parse().expect("generation"),
            capture: None,
        }
    }
}
