//! Source fixture, init-pg, and import-pg. Grants and reads are in `read`.

use std::collections::BTreeMap;
use std::fs;

use serde_json::Value;
use sqlx::postgres::PgConnection;

use crate::docs;
use crate::fixture::{CUSTOMERS, ORDERS};
use crate::live::{Live, LOGIN, READER, RUNTIME, SCHEMA, SOURCE_DB, TARGET_DB, WRITER};
use crate::proc::{parse_json, require_exit, run_cmd, save_cmd};
use crate::sql::{
    compare_source_gold, create_database, create_roles, dial, exec_sql, expect_mapped_column, expect_mapped_table,
    expect_receipt, load_fixture, mapping_identity, physical_ok, reader_denied, record_server, reset_cluster,
    scalar_text, shadow_values, sqlstate, text_of,
};
use crate::stop::{disk, Stop};

pub struct Imported {
    pub run_id: String,
    pub source_generation: i64,
    pub target_generation: i64,
    pub digest: String,
    pub generation: i64,
    pub init_install: String,
    pub init_full_import: String,
    pub historical_v2: bool,
    pub publication: String,
    pub owner_member: String,
    pub tables: BTreeMap<String, String>,
    pub columns: BTreeMap<String, String>,
}

pub async fn prepare(live: &Live) -> Result<(), Stop> {
    let mut admin = dial(live, &live.admin_user, &live.admin_password, "postgres").await?;
    reset_cluster(&mut admin).await?;
    create_roles(live, &mut admin).await?;
    create_database(&mut admin, SOURCE_DB).await?;
    create_database(&mut admin, TARGET_DB).await?;
    exec_sql(
        &mut admin,
        &format!("GRANT CONNECT ON DATABASE {SOURCE_DB} TO {READER}, {WRITER}"),
    )
    .await?;
    exec_sql(
        &mut admin,
        &format!("GRANT CONNECT ON DATABASE {TARGET_DB} TO {RUNTIME}, {LOGIN}"),
    )
    .await?;
    record_server(live, &mut admin).await?;
    drop(admin);
    load_fixture(live).await?;
    reader_denied(live).await?;
    let mut source = dial(live, &live.admin_user, &live.admin_password, SOURCE_DB).await?;
    compare_source_gold(&mut source).await?;
    drop(source);
    Ok(())
}

pub async fn load_target(live: &Live) -> Result<Imported, Stop> {
    let early = run_cmd(&live.pipeline, &["import-pg"], &docs::import_request(live)?, 120)?;
    save_cmd(live, "import-pg-not-ready", &early)?;
    if early.code != 1 || !early.stdout.is_empty() || !crate::proc::stderr_text(&early.stderr).contains("target schema is not ready")
    {
        return Err(Stop::new("import-pg-not-ready", format!("exit {}", early.code)));
    }
    let init = run_cmd(&live.init_pg, &[], &docs::init_request(live)?, 120)?;
    save_cmd(live, "init-pg", &init)?;
    require_exit("init-pg", &init, 0)?;
    let init_value = parse_json(&init.stdout, "init-pg")?;
    check_init(&init_value)?;
    let mut admin = dial(live, &live.admin_user, &live.admin_password, "postgres").await?;
    exec_sql(
        &mut admin,
        &format!("GRANT CONNECT ON DATABASE {TARGET_DB} TO {RUNTIME}, {LOGIN}"),
    )
    .await?;
    exec_sql(&mut admin, &format!("GRANT {RUNTIME_ROLE} TO {RUNTIME}")).await?;
    drop(admin);
    let imported = run_cmd(&live.pipeline, &["import-pg"], &docs::import_request(live)?, 180)?;
    save_cmd(live, "import-pg", &imported)?;
    require_exit("import-pg", &imported, 0)?;
    let value = parse_json(&imported.stdout, "import-pg")?;
    let mut target = dial(live, &live.admin_user, &live.admin_password, TARGET_DB).await?;
    let facts = compare_import(&mut target, &value).await?;
    let publication = scalar_text(&mut target, "SELECT count(*)::text FROM pg_catalog.pg_publication").await?;
    let owner_member = scalar_text(
        &mut target,
        "SELECT pg_catalog.pg_has_role('oo_pipeline_runtime', 'oo_target_owner', 'MEMBER')::text",
    )
    .await?;
    if publication != "0" || (owner_member != "false" && owner_member != "f") {
        return Err(Stop::new("publication", format!("{publication}/{owner_member}")));
    }
    drop(target);
    note_writer(live).await?;
    Ok(Imported {
        run_id: facts.run_id,
        source_generation: facts.source_generation,
        target_generation: facts.target_generation,
        digest: facts.digest,
        generation: facts.generation,
        init_install: text_of(&init_value, "install")?,
        init_full_import: text_of(&init_value, "full_import")?,
        historical_v2: false,
        publication,
        owner_member,
        tables: facts.tables,
        columns: facts.columns,
    })
}

struct ImportCore {
    run_id: String,
    source_generation: i64,
    target_generation: i64,
    digest: String,
    generation: i64,
    tables: BTreeMap<String, String>,
    columns: BTreeMap<String, String>,
}

const RUNTIME_ROLE: &str = "oo_pipeline_runtime";

fn check_init(value: &Value) -> Result<(), Stop> {
    if value["protocol"] != "oo.pipeline.init-pg-result/v1"
        || value["install"] != "bootstrapped"
        || value["full_import"] != "installed"
        || value["historical_v2"] != false
    {
        return Err(Stop::new("init-pg", "result"));
    }
    Ok(())
}

async fn compare_import(conn: &mut PgConnection, doc: &Value) -> Result<ImportCore, Stop> {
    if doc["protocol"] != "oo.pipeline.import-pg-result/v1"
        || doc["state"] != "awaiting_publish"
        || doc["table_count"].as_i64() != Some(2)
        || doc["source_generation"].as_i64() != Some(crate::live::GENERATION)
    {
        return Err(Stop::new("import-pg", "summary"));
    }
    let source_generation = doc["source_generation"].as_i64().unwrap_or(-1);
    let target_generation = doc["target_generation"]
        .as_i64()
        .ok_or_else(|| Stop::new("import-pg", "target_generation"))?;
    if target_generation == source_generation {
        return Err(Stop::new("import-pg", "generations are not distinct"));
    }
    let run_id = text_of(doc, "run_id")?;
    expect_seal(conn, &run_id).await?;
    let (tables, columns) = shadow_tables(conn, doc, &run_id).await?;
    let (digest, generation) = mapping_identity(conn).await?;
    if generation != source_generation {
        return Err(Stop::new("mapping", "run_generation differs from source_generation"));
    }
    Ok(ImportCore {
        run_id,
        source_generation,
        target_generation,
        digest,
        generation,
        tables,
        columns,
    })
}

async fn expect_seal(conn: &mut PgConnection, run_id: &str) -> Result<(), Stop> {
    let state: String = sqlx::query_scalar(sqlx::AssertSqlSafe(
        "SELECT state FROM _oo.full_import_run WHERE run_id = $1::uuid".to_string(),
    ))
    .bind(run_id)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| Stop::new("seal", sqlstate(&err)))?;
    let sealed: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(
        "SELECT sealed_at IS NOT NULL FROM _oo.full_import_run WHERE run_id = $1::uuid".to_string(),
    ))
    .bind(run_id)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| Stop::new("seal", sqlstate(&err)))?;
    if state == "awaiting_publish" && sealed {
        Ok(())
    } else {
        Err(Stop::new("seal", "state"))
    }
}

async fn shadow_tables(
    conn: &mut PgConnection,
    doc: &Value,
    run_id: &str,
) -> Result<(BTreeMap<String, String>, BTreeMap<String, String>), Stop> {
    let listed = doc["tables"].as_array().ok_or_else(|| Stop::new("import-pg", "tables"))?;
    let mut tables = BTreeMap::new();
    let mut columns = BTreeMap::new();
    for table in listed {
        let name = text_of(table, "name")?;
        if table["schema"] != SCHEMA || table["rows"].as_i64() != Some(3) {
            return Err(Stop::new("import-pg", "table header"));
        }
        let physical = text_of(table, "physical_name")?;
        let digest = text_of(table, "canonical_digest")?;
        if !physical_ok(&physical, "pt_") || digest.len() != 64 {
            return Err(Stop::new("import-pg", "physical"));
        }
        expect_receipt(conn, run_id, &physical, &digest).await?;
        expect_mapped_table(conn, &name, &physical).await?;
        copy_columns(conn, table, &name, &physical, &mut columns).await?;
        tables.insert(name, physical);
    }
    if !tables.contains_key("customer") || !tables.contains_key("order_doc") {
        return Err(Stop::new("import-pg", "table names"));
    }
    Ok((tables, columns))
}

async fn copy_columns(
    conn: &mut PgConnection,
    table: &Value,
    name: &str,
    physical: &str,
    columns: &mut BTreeMap<String, String>,
) -> Result<(), Stop> {
    let listed = table["columns"].as_array().ok_or_else(|| Stop::new("import-pg", "columns"))?;
    let gold = if name == "customer" { customer_gold() } else { order_gold() };
    for (logical, expect) in gold {
        let spec = listed
            .iter()
            .find(|item| item["name"] == logical)
            .ok_or_else(|| Stop::new("import-pg", logical))?;
        let column_physical = text_of(spec, "physical_name")?;
        if !physical_ok(&column_physical, "pc_") {
            return Err(Stop::new("import-pg", "column physical"));
        }
        expect_mapped_column(conn, name, logical, &column_physical).await?;
        let got = shadow_values(conn, physical, &column_physical).await?;
        if got != expect {
            return Err(Stop::new("rows", format!("shadow {name}.{logical}")));
        }
        columns.insert(format!("{name}.{logical}"), column_physical);
    }
    Ok(())
}

fn customer_gold() -> Vec<(&'static str, Vec<Option<String>>)> {
    let mut rows: [Vec<Option<String>>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for row in CUSTOMERS {
        rows[0].push(Some(row.id.to_string()));
        rows[1].push(row.display.map(str::to_string));
        rows[2].push(Some(row.active.to_string()));
        rows[3].push(row.private_note.map(str::to_string));
    }
    vec![
        ("customer_id", rows[0].clone()),
        ("display_name", rows[1].clone()),
        ("active", rows[2].clone()),
        ("private_note", rows[3].clone()),
    ]
}

fn order_gold() -> Vec<(&'static str, Vec<Option<String>>)> {
    let mut rows: [Vec<Option<String>>; 6] = Default::default();
    for row in ORDERS {
        rows[0].push(Some(row.id.to_string()));
        rows[1].push(Some(row.customer_id.to_string()));
        rows[2].push(row.memo.map(str::to_string));
        rows[3].push(Some(row.posted.to_string()));
        rows[4].push(Some(row.amount.to_string()));
        rows[5].push(row.clerk_note.map(str::to_string));
    }
    vec![
        ("order_id", rows[0].clone()),
        ("customer_id", rows[1].clone()),
        ("memo", rows[2].clone()),
        ("posted", rows[3].clone()),
        ("amount", rows[4].clone()),
        ("clerk_note", rows[5].clone()),
    ]
}

async fn note_writer(live: &Live) -> Result<(), Stop> {
    let mut writer = dial(live, WRITER, &live.writer_password, SOURCE_DB).await?;
    exec_sql(
        &mut writer,
        "INSERT INTO shop.customer (customer_id, display_name, active) VALUES (99, 'after', true)",
    )
    .await?;
    drop(writer);
    let mut source = dial(live, &live.admin_user, &live.admin_password, SOURCE_DB).await?;
    let writer_audit = scalar_text(
        &mut source,
        "SELECT count(*)::text FROM shop.write_audit WHERE who = 'oo_a226_writer'",
    )
    .await?;
    let reader_audit = scalar_text(
        &mut source,
        "SELECT count(*)::text FROM shop.write_audit WHERE who = 'oo_a226_reader'",
    )
    .await?;
    let customers = scalar_text(&mut source, "SELECT count(*)::text FROM shop.customer").await?;
    let source_stats = scalar_text(
        &mut source,
        "SELECT string_agg(reltuples::text, ',' ORDER BY relname) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'shop' AND c.relname IN ('customer', 'order_doc')",
    )
    .await?;
    if writer_audit != "1" || reader_audit != "0" || customers != "4" || source_stats != "-1,-1" {
        return Err(Stop::new(
            "source-audit",
            format!("writer={writer_audit} reader={reader_audit} rows={customers} reltuples={source_stats}"),
        ));
    }
    fs::write(
        live.evidence.join("source-audit.txt"),
        format!("writer={writer_audit}\nreader={reader_audit}\nrows={customers}\nreltuples={source_stats}\n"),
    )
    .map_err(|err| disk("source-audit", &err))?;
    Ok(())
}
