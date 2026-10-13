use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mysql_async::consts::ColumnFlags;
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row, Value};
use serde_json::{json, Value as Json};
use tokio::time::timeout;
use zeroize::Zeroize;

use crate::decide::{self, NativeColumn};
use crate::encode::{self, Cell, ColumnOut};
use crate::fail::Fail;
use crate::identity;

pub const DRIVER: &str = "mysql_async 0.36.1";
pub const DEPLOYMENT: &str = "docker-official-image";
pub const IMAGE_DIGEST: &str =
    "sha256:0130d92c05fbf2d82adc2b86de742eaede65b2596c65d91786f8e03cd19e6a39";
pub const CATALOG_PROTOCOL: &str = "oo.mariadb-driver-preflight.catalog/v1";
pub const RUN_PROTOCOL: &str = "oo.mariadb-driver-preflight.run/v1";

#[derive(Clone, Copy, Debug)]
pub enum Command {
    Read,
    RejectTable,
    ZeroDate,
    DenySelect,
    Cancel,
    Broken,
    CapsRow,
    CapsByte,
    CapsDeadline,
}

pub struct Request {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password_file: PathBuf,
    pub out: PathBuf,
    pub command: Command,
    pub table: Option<String>,
    pub ready_file: Option<PathBuf>,
    pub go_file: Option<PathBuf>,
    pub max_rows: u64,
    pub max_bytes: u64,
    pub deadline: Duration,
    pub batch_rows: usize,
    pub image_digest: String,
}

pub async fn execute(request: Request) -> Result<(), Fail> {
    fs::create_dir_all(&request.out).map_err(|err| Fail::new("out", err.to_string()))?;
    if request.image_digest != IMAGE_DIGEST {
        let err = Fail::new("image_digest", request.image_digest.clone());
        fail_run(&request.out, &err);
        return Err(err);
    }
    let outcome = dispatch(&request).await;
    if let Err(err) = &outcome {
        fail_run(&request.out, err);
    }
    outcome
}

async fn dispatch(request: &Request) -> Result<(), Fail> {
    match request.command {
        Command::Read => read(request).await,
        Command::RejectTable => reject_table(request).await,
        Command::ZeroDate => zero_date(request).await,
        Command::DenySelect => deny_select(request).await,
        Command::Cancel => cancel(request).await,
        Command::Broken => broken(request).await,
        Command::CapsRow | Command::CapsByte => read(request).await,
        Command::CapsDeadline => caps_deadline(request).await,
    }
}

fn fail_run(out: &Path, err: &Fail) {
    discard_partial(out);
    let body = json!({
        "protocol": RUN_PROTOCOL,
        "state": "failed",
        "code": err.code,
        "fact": err.fact,
        "source_acceptance": false,
    });
    let _ = write_json(&out.join("failed.json"), &body);
}

fn discard_partial(dir: &Path) {
    let _ = fs::remove_file(dir.join("ready.json"));
    let _ = fs::remove_file(dir.join("ready.json.tmp"));
    let _ = fs::remove_file(dir.join("catalog.json"));
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("arrow") {
            let _ = fs::remove_file(path);
        }
    }
}

struct Planned {
    name: String,
    storage_engine: String,
    collation: String,
    keys: Vec<Json>,
    checks: Vec<Json>,
    columns: Vec<PlannedColumn>,
    primary: Vec<String>,
    refusal: Option<Fail>,
}

struct PlannedColumn {
    native: NativeColumn,
    plan: Option<crate::decide::Plan>,
    metadata: HashMap<String, String>,
    error: Option<String>,
}

async fn read(request: &Request) -> Result<(), Fail> {
    let mut conn = connect(request).await?;
    let body = timeout(request.deadline, read_loaded(request, &mut conn)).await;
    let _ = conn.disconnect().await;
    match body {
        Err(_) => Err(Fail::new("deadline", "read exceeded its deadline")),
        Ok(result) => result,
    }
}

async fn read_loaded(request: &Request, conn: &mut Conn) -> Result<(), Fail> {
    let loaded = load_catalog(request, conn).await?;
    if loaded.business_connections != 1 {
        return Err(Fail::new(
            "connection_count",
            format!("{}", loaded.business_connections),
        ));
    }
    let mut total_rows: u64 = 0;
    let mut encoded = Vec::new();
    for table in loaded.tables.iter().filter(|table| table.refusal.is_none()) {
        let rows = fetch_table(conn, table, request.max_rows, &mut total_rows).await?;
        let columns = table
            .columns
            .iter()
            .filter_map(|column| {
                let plan = column.plan.clone()?;
                Some(ColumnOut {
                    name: column.native.name.clone(),
                    nullable: column.native.nullable,
                    plan,
                    metadata: column.metadata.clone(),
                })
            })
            .collect::<Vec<_>>();
        let mut schema_meta = HashMap::new();
        schema_meta.insert("oo.mariadb.engine".to_string(), "MariaDB".to_string());
        schema_meta.insert("oo.mariadb.service_version".to_string(), loaded.service_version.clone());
        schema_meta.insert("oo.mariadb.driver".to_string(), DRIVER.to_string());
        schema_meta.insert("oo.mariadb.deployment".to_string(), DEPLOYMENT.to_string());
        schema_meta.insert("oo.mariadb.schema".to_string(), "oo_synth".to_string());
        schema_meta.insert("oo.mariadb.table".to_string(), table.name.clone());
        schema_meta.insert("oo.mariadb.storage_engine".to_string(), table.storage_engine.clone());
        schema_meta.insert("oo.mariadb.collation".to_string(), table.collation.clone());
        let ipc = encode::write_ipc(&columns, &rows, schema_meta, request.batch_rows)?;
        encoded.push((table.name.clone(), ipc));
    }
    let bytes: u64 = encoded.iter().map(|(_, ipc)| ipc.bytes.len() as u64).sum();
    if bytes > request.max_bytes {
        return Err(Fail::new("byte_cap", format!("{bytes} > {}", request.max_bytes)));
    }
    let catalog = catalog_json(&loaded);
    write_json(&request.out.join("catalog.json"), &catalog)?;
    let mut batches = 0usize;
    let mut names = Vec::new();
    for (name, ipc) in &encoded {
        fs::write(request.out.join(format!("{name}.arrow")), &ipc.bytes)
            .map_err(|err| Fail::new("ipc", err.to_string()))?;
        batches += ipc.batches;
        names.push(name.clone());
    }
    let refused = loaded
        .tables
        .iter()
        .filter_map(|table| {
            table.refusal.as_ref().map(|err| {
                json!({
                    "table": table.name,
                    "error": err.code,
                    "fact": err.fact,
                })
            })
        })
        .collect::<Vec<_>>();
    let ready = json!({
        "protocol": RUN_PROTOCOL,
        "state": "validated",
        "catalog_protocol": CATALOG_PROTOCOL,
        "engine": "MariaDB",
        "service_version": loaded.service_version,
        "driver": DRIVER,
        "deployment": DEPLOYMENT,
        "image_digest": IMAGE_DIGEST,
        "handshake_connection_id": loaded.handshake_id,
        "session_connection_id": loaded.session_id,
        "business_connections": loaded.business_connections,
        "rows": total_rows,
        "bytes": bytes,
        "batches": batches,
        "tables": names,
        "refused": refused,
        "source_acceptance": false,
        "builder": false,
        "pack": false,
    });
    let tmp = request.out.join("ready.json.tmp");
    write_json(&tmp, &ready)?;
    fs::rename(&tmp, request.out.join("ready.json")).map_err(|err| Fail::new("ready", err.to_string()))?;
    let _ = fs::remove_file(request.out.join("failed.json"));
    Ok(())
}

struct Loaded {
    service_version: String,
    handshake_id: u32,
    session_id: u64,
    business_connections: u64,
    identity: Json,
    grants: Vec<String>,
    tables: Vec<Planned>,
    tls_cipher: String,
}

async fn load_catalog(request: &Request, conn: &mut Conn) -> Result<Loaded, Fail> {
    let handshake_id = conn.id();
    let identity_rows = query_values(
        conn,
        "SELECT JSON_OBJECT( \
            'version', @@version, \
            'comment', @@version_comment, \
            'sql_mode', @@sql_mode, \
            'time_zone', @@time_zone, \
            'collation_server', @@collation_server, \
            'character_set_server', @@character_set_server, \
            'collation_connection', @@collation_connection, \
            'character_set_client', @@character_set_client, \
            'character_set_connection', @@character_set_connection, \
            'log_bin', @@log_bin + 0, \
            'gtid_domain_id', @@gtid_domain_id, \
            'gtid_current_pos', @@gtid_current_pos, \
            'connection_id', CONNECTION_ID())",
    )
    .await?;
    let identity = value_json(only_cell(&identity_rows)?)?;
    let version = json_string(&identity, "version")?;
    let comment = json_string(&identity, "comment")?;
    identity::require_mariadb(&version, &comment)?;
    identity::require_fixture_session(&json_string(&identity, "sql_mode")?, &json_string(&identity, "time_zone")?)?;
    let session_id = json_u64(&identity, "connection_id")?;
    if session_id != u64::from(handshake_id) {
        return Err(Fail::new(
            "connection_id",
            format!("handshake={handshake_id} session={session_id}"),
        ));
    }
    if json_u64(&identity, "log_bin")? != 0 {
        return Err(Fail::new("log_bin", json_string(&identity, "log_bin")?));
    }
    let user = ident_piece(&request.user)?;
    let business_connections = query_u64(
        conn,
        &format!("SELECT COUNT(*) FROM information_schema.PROCESSLIST WHERE USER = '{user}'"),
    )
    .await?;
    let column_rows = query_values(
        conn,
        "SELECT JSON_OBJECT( \
            'table_name', TABLE_NAME, \
            'column_name', COLUMN_NAME, \
            'ordinal', ORDINAL_POSITION, \
            'data_type', DATA_TYPE, \
            'column_type', COLUMN_TYPE, \
            'is_nullable', IS_NULLABLE, \
            'charset', CHARACTER_SET_NAME, \
            'collation', COLLATION_NAME, \
            'column_key', COLUMN_KEY, \
            'precision', NUMERIC_PRECISION, \
            'scale', NUMERIC_SCALE, \
            'datetime_precision', DATETIME_PRECISION) \
         FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = 'oo_synth' \
         ORDER BY TABLE_NAME, ORDINAL_POSITION",
    )
    .await?;
    let check_rows = query_values(
        conn,
        "SELECT JSON_OBJECT( \
            'table_name', TABLE_NAME, \
            'constraint_name', CONSTRAINT_NAME, \
            'check_clause', CHECK_CLAUSE) \
         FROM information_schema.CHECK_CONSTRAINTS \
         WHERE CONSTRAINT_SCHEMA = 'oo_synth'",
    )
    .await
    .map_err(|err| Fail::new("check_constraints", err.fact))?;
    let key_rows = query_values(
        conn,
        "SELECT JSON_OBJECT( \
            'table_name', TABLE_NAME, \
            'index_name', INDEX_NAME, \
            'non_unique', NON_UNIQUE, \
            'seq', SEQ_IN_INDEX, \
            'column_name', COLUMN_NAME, \
            'collation', COLLATION) \
         FROM information_schema.STATISTICS \
         WHERE TABLE_SCHEMA = 'oo_synth' \
         ORDER BY TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX",
    )
    .await?;
    let table_rows = query_values(
        conn,
        "SELECT JSON_OBJECT( \
            'table_name', TABLE_NAME, \
            'engine', ENGINE, \
            'collation', TABLE_COLLATION) \
         FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = 'oo_synth' AND TABLE_TYPE = 'BASE TABLE' \
         ORDER BY TABLE_NAME",
    )
    .await?;
    let grant_rows = query_values(conn, "SHOW GRANTS FOR CURRENT_USER()").await?;
    let mut grants = Vec::new();
    for row in &grant_rows {
        let line = value_string(only_row(row)?)?.unwrap_or_default();
        grants.push(identity::redact_grant(&line));
    }
    identity::grants_ok(&grants)?;
    let status_rows = query_values(conn, "SHOW STATUS LIKE 'Ssl_cipher'").await?;
    let tls_cipher = status_rows
        .first()
        .and_then(|row| row.get(1))
        .map(value_string)
        .transpose()?
        .flatten()
        .unwrap_or_default();
    if !tls_cipher.is_empty() {
        return Err(Fail::new("tls", tls_cipher));
    }
    let checks = json_rows(&check_rows)?;
    let keys = json_rows(&key_rows)?;
    let mut tables = Vec::new();
    for row in &table_rows {
        let spec = value_json(only_row(row)?)?;
        let name = json_string(&spec, "table_name")?;
        let mut columns = Vec::new();
        let mut refusal = None;
        for column_row in &column_rows {
            let column = value_json(only_row(column_row)?)?;
            if json_string(&column, "table_name")? != name {
                continue;
            }
            let column_name = json_string(&column, "column_name")?;
            let check_clause = matching_check(&checks, &name, &column_name);
            let native = NativeColumn {
                table: name.clone(),
                name: column_name,
                ordinal: json_i32(&column, "ordinal")?,
                data_type: json_string(&column, "data_type")?,
                column_type: json_string(&column, "column_type")?,
                nullable: json_string(&column, "is_nullable")?.eq_ignore_ascii_case("YES"),
                charset: json_opt_string(&column, "charset")?,
                collation: json_opt_string(&column, "collation")?,
                column_key: json_opt_string(&column, "column_key")?.unwrap_or_default(),
                precision: json_opt_u32(&column, "precision")?,
                scale: json_opt_i32(&column, "scale")?,
                datetime_precision: json_opt_u32(&column, "datetime_precision")?,
                check_clause,
            };
            match decide::decide(&native) {
                Ok(plan) => {
                    let metadata = decide::metadata(&native, &plan).into_iter().collect();
                    columns.push(PlannedColumn {
                        native,
                        plan: Some(plan),
                        metadata,
                        error: None,
                    });
                }
                Err(err) => {
                    if refusal.is_none() {
                        refusal = Some(err.clone());
                    }
                    columns.push(PlannedColumn {
                        native,
                        plan: None,
                        metadata: HashMap::new(),
                        error: Some(err.code.to_string()),
                    });
                }
            }
        }
        let mut primary = Vec::new();
        let mut table_keys = Vec::new();
        for key in &keys {
            if json_string(key, "table_name")? != name {
                continue;
            }
            if json_string(key, "index_name")?.eq_ignore_ascii_case("PRIMARY") {
                primary.push(json_string(key, "column_name")?);
            }
            table_keys.push(key.clone());
        }
        let table_checks = checks
            .iter()
            .filter(|check| json_string(check, "table_name").ok().as_deref() == Some(name.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        tables.push(Planned {
            name,
            storage_engine: json_string(&spec, "engine")?,
            collation: json_string(&spec, "collation")?,
            keys: table_keys,
            checks: table_checks,
            columns,
            primary,
            refusal,
        });
    }
    Ok(Loaded {
        service_version: version,
        handshake_id,
        session_id,
        business_connections,
        identity,
        grants,
        tables,
        tls_cipher,
    })
}

fn catalog_json(loaded: &Loaded) -> Json {
    let tables = loaded
        .tables
        .iter()
        .map(|table| {
            let columns = table
                .columns
                .iter()
                .map(|column| {
                    json!({
                        "name": column.native.name,
                        "ordinal": column.native.ordinal,
                        "data_type": column.native.data_type,
                        "column_type": column.native.column_type,
                        "nullable": column.native.nullable,
                        "charset": column.native.charset,
                        "collation": column.native.collation,
                        "key": column.native.column_key,
                        "precision": column.native.precision,
                        "scale": column.native.scale,
                        "datetime_precision": column.native.datetime_precision,
                        "check_clause": column.native.check_clause,
                        "arrow": column.metadata.get("oo.mariadb.arrow"),
                        "caveat": column.metadata.get("oo.mariadb.caveat"),
                        "provenance": column.metadata.get("oo.mariadb.provenance"),
                        "plan": if column.error.is_some() { "refused" } else { "supported" },
                        "error": column.error,
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "name": table.name,
                "storage_engine": table.storage_engine,
                "collation": table.collation,
                "keys": table.keys,
                "checks": table.checks,
                "primary_key": table.primary,
                "columns": columns,
                "plan": if table.refusal.is_some() { "refused" } else { "supported" },
                "error": table.refusal.as_ref().map(|err| err.code),
                "fact": table.refusal.as_ref().map(|err| err.fact.clone()),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "protocol": CATALOG_PROTOCOL,
        "engine": "MariaDB",
        "service_version": loaded.service_version,
        "version_comment": loaded.identity.get("comment"),
        "driver": DRIVER,
        "deployment": DEPLOYMENT,
        "image_digest": IMAGE_DIGEST,
        "schema": "oo_synth",
        "schema_collation": "utf8mb4_bin",
        "server_collation": loaded.identity.get("collation_server"),
        "server_charset": loaded.identity.get("character_set_server"),
        "client_charset": loaded.identity.get("character_set_client"),
        "connection_charset": loaded.identity.get("character_set_connection"),
        "connection_collation": loaded.identity.get("collation_connection"),
        "sql_mode": loaded.identity.get("sql_mode"),
        "time_zone": loaded.identity.get("time_zone"),
        "log_bin": loaded.identity.get("log_bin"),
        "gtid_domain_id": loaded.identity.get("gtid_domain_id"),
        "gtid_current_pos": loaded.identity.get("gtid_current_pos"),
        "cdc": "not_claimed",
        "tls_cipher": loaded.tls_cipher,
        "tls": "absent_fixture",
        "handshake_connection_id": loaded.handshake_id,
        "session_connection_id": loaded.session_id,
        "business_connections": loaded.business_connections,
        "grants_redacted": loaded.grants,
        "tables": tables,
        "source_acceptance": false,
    })
}

async fn fetch_table(conn: &mut Conn, table: &Planned, max_rows: u64, total_rows: &mut u64) -> Result<Vec<Vec<Cell>>, Fail> {
    if table.primary.is_empty() {
        return Err(Fail::new("primary_key", table.name.clone()));
    }
    let columns = table
        .columns
        .iter()
        .map(|column| ident_piece(&column.native.name))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let order = table
        .primary
        .iter()
        .map(|name| ident_piece(name))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let sql = format!(
        "SELECT {columns} FROM `oo_synth`.{} ORDER BY {order}",
        ident_piece(&table.name)?
    );
    let mut result = conn.exec_iter(sql, ()).await.map_err(|err| map_db(err, ""))?;
    let unsigned_flags: Vec<bool> = result
        .columns_ref()
        .iter()
        .map(|column| column.flags().contains(ColumnFlags::UNSIGNED_FLAG))
        .collect();
    let planned: Vec<&PlannedColumn> = table.columns.iter().filter(|column| column.plan.is_some()).collect();
    if unsigned_flags.len() != planned.len() {
        return Err(Fail::new(
            "row_width",
            format!("{} driver={} catalog={}", table.name, unsigned_flags.len(), planned.len()),
        ));
    }
    for (column, flag) in planned.iter().zip(&unsigned_flags) {
        let wants_unsigned = matches!(column.plan.as_ref().map(|plan| &plan.kind), Some(crate::decide::Kind::Int { unsigned: true, .. }));
        if wants_unsigned && !flag {
            return Err(Fail::new("unsigned_flag", format!("{}.{}", table.name, column.native.name)));
        }
    }
    let mut rows = Vec::new();
    loop {
        let next = result.next().await.map_err(|err| map_db(err, ""))?;
        let Some(mut row) = next else { break };
        if *total_rows >= max_rows {
            let _ = result.drop_result().await;
            return Err(Fail::new("row_cap", format!("{total_rows} would exceed {max_rows}")));
        }
        let mut cells = Vec::new();
        for (index, column) in planned.iter().enumerate() {
            let value: Value = row
                .take(index)
                .ok_or_else(|| Fail::new("row", format!("{}.{}", table.name, column.native.name)))?;
            let plan = column.plan.as_ref().ok_or_else(|| Fail::new("plan", column.native.name.clone()))?;
            cells.push(encode::cell_from_value(plan, &value)?);
        }
        rows.push(cells);
        *total_rows += 1;
    }
    result.drop_result().await.map_err(|err| map_db(err, ""))?;
    Ok(rows)
}

async fn reject_table(request: &Request) -> Result<(), Fail> {
    let mut conn = connect(request).await?;
    let loaded = load_catalog(request, &mut conn).await;
    let _ = conn.disconnect().await;
    let loaded = loaded?;
    let name = request.table.clone().unwrap_or_default();
    let Some(table) = loaded.tables.iter().find(|table| table.name == name) else {
        return Err(Fail::new("table", name));
    };
    match &table.refusal {
        Some(err) => Err(Fail::new(err.code, err.fact.clone())),
        None => Err(Fail::new("expected_refusal", name)),
    }
}

async fn zero_date(request: &Request) -> Result<(), Fail> {
    let mut conn = connect(request).await?;
    let selected = query_values(&mut conn, "SELECT CAST('0000-00-00' AS DATE)").await;
    let warnings = if selected.is_ok() {
        query_values(&mut conn, "SHOW WARNINGS").await
    } else {
        Err(Fail::new("zero_date", "warnings were not read"))
    };
    let _ = conn.disconnect().await;
    let selected = selected?;
    let warnings = warnings?;
    let value = only_cell(&selected)?;
    if !matches!(value, Value::NULL) {
        return Err(Fail::new("zero_date", "server returned a zero date value"));
    }
    let mut saw_warning = false;
    for row in &warnings {
        let code = row.get(1).map(value_string).transpose()?.flatten().unwrap_or_default();
        if code == "1292" {
            saw_warning = true;
        }
    }
    if !saw_warning {
        return Err(Fail::new("zero_date", "CAST returned NULL without warning 1292"));
    }
    write_json(
        &request.out.join("zero-date.json"),
        &json!({
            "protocol": RUN_PROTOCOL,
            "state": "negative_proven",
            "server_code": 1292,
            "severity": "warning",
            "result": null,
            "source_acceptance": false,
        }),
    )?;
    Ok(())
}

async fn deny_select(request: &Request) -> Result<(), Fail> {
    let mut conn = connect(request).await?;
    let result = conn.query_drop("SELECT COUNT(*) FROM mysql.global_priv").await;
    let _ = conn.disconnect().await;
    match result {
        Err(err) if server_code(&err) == Some(1142) => {
            write_json(
                &request.out.join("deny-select.json"),
                &json!({"protocol": RUN_PROTOCOL, "state": "negative_proven", "server_code": 1142}),
            )?;
            Ok(())
        }
        Err(err) => Err(Fail::new("deny_select", map_db(err, "").fact)),
        Ok(()) => Err(Fail::new("deny_select", "reader read mysql.global_priv")),
    }
}

async fn caps_deadline(request: &Request) -> Result<(), Fail> {
    let mut conn = connect(request).await?;
    let result = timeout(
        request.deadline,
        conn.query_drop("SELECT SLEEP(5) /* oo-dl01w1-39-deadline */"),
    )
    .await;
    let _ = conn.disconnect().await;
    match result {
        Err(_) => Err(Fail::new("deadline", "sleep outlived the deadline")),
        Ok(Err(err)) => Err(Fail::new("deadline", map_db(err, "").fact)),
        Ok(Ok(())) => Err(Fail::new("deadline", "sleep finished inside the deadline")),
    }
}

async fn broken(request: &Request) -> Result<(), Fail> {
    let ready = request
        .ready_file
        .clone()
        .ok_or_else(|| Fail::new("ready_file", "missing"))?;
    // docker network disconnect leaves the TCP socket ESTABLISHED and does not RST.
    // mysql_async applies this as TCP_KEEPIDLE. The pinned image then sends 9 probes
    // 75s apart, so a blackhole surfaces in about 676s. 900s is only the bound that
    // records broken_unobserved; it is not a successful broken-connection proof.
    let mut conn = connect_opts(request, Some(1_000)).await?;
    fs::write(&ready, format!("{}\n", conn.id())).map_err(|err| Fail::new("ready_file", err.to_string()))?;
    let result = timeout(
        Duration::from_secs(900),
        conn.query_drop("SELECT SLEEP(40) /* oo-dl01w1-39-broken */"),
    )
    .await;
    let _ = conn.disconnect().await;
    match result {
        Err(_) => Err(Fail::new(
            "broken_unobserved",
            "socket stayed established for 900s after the query started",
        )),
        Ok(Ok(())) => Err(Fail::new("broken_connection", "sleep completed on a live connection")),
        Ok(Err(err)) => Err(Fail::new("broken_connection", map_db(err, "").fact)),
    }
}

async fn cancel(request: &Request) -> Result<(), Fail> {
    let ready = request
        .ready_file
        .clone()
        .ok_or_else(|| Fail::new("ready_file", "missing"))?;
    let go = request.go_file.clone().ok_or_else(|| Fail::new("go_file", "missing"))?;
    let mut business = connect(request).await?;
    let business_id = business.id();
    let session_id = query_u64(&mut business, "SELECT CONNECTION_ID()").await?;
    if session_id != u64::from(business_id) {
        let _ = business.disconnect().await;
        return Err(Fail::new(
            "connection_id",
            format!("handshake={business_id} session={session_id}"),
        ));
    }
    let mut control = connect(request).await?;
    let control_id = control.id();
    let marker = "oo-dl01w1-39-cancel";
    let sleep_sql = format!("SELECT SLEEP(25) /* {marker} */");
    let (abort_tx, abort_rx) = tokio::sync::oneshot::channel::<()>();
    let sleep_task = tokio::spawn(async move { sleep_or_abort(business, sleep_sql, abort_rx).await });
    let user = ident_piece(&request.user)?;
    let count_sql = format!("SELECT COUNT(*) FROM information_schema.PROCESSLIST WHERE USER = '{user}'");
    let info_sql = format!(
        "SELECT IFNULL(INFO, '') FROM information_schema.PROCESSLIST WHERE ID = {business_id}"
    );
    let mut seen = false;
    for _ in 0..80 {
        let info = query_optional_string(&mut control, &info_sql).await?;
        if info.contains(marker) {
            seen = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if !seen {
        let _ = abort_tx.send(());
        let _ = sleep_task.await;
        let _ = control.disconnect().await;
        return Err(Fail::new("cancel", "sleep did not appear on the reader process list"));
    }
    let count_before = query_u64(&mut control, &count_sql).await?;
    if count_before != 2 {
        let _ = abort_tx.send(());
        let _ = sleep_task.await;
        let _ = control.disconnect().await;
        return Err(Fail::new("connection_count", format!("before={count_before}")));
    }
    fs::write(
        &ready,
        format!("business={business_id}\ncontrol={control_id}\ncount={count_before}\n"),
    )
    .map_err(|err| Fail::new("ready_file", err.to_string()))?;
    if let Err(err) = wait_for_file(&go, Duration::from_secs(30)).await {
        let _ = abort_tx.send(());
        let _ = sleep_task.await;
        let _ = control.disconnect().await;
        return Err(err);
    }
    let kill_sql = format!("KILL QUERY {business_id}");
    let kill = control.query_drop(kill_sql).await;
    let path = match &kill {
        Ok(()) => "kill_query",
        Err(err) if server_code(err) == Some(1095) => {
            let _ = abort_tx.send(());
            "connection_abort"
        }
        Err(err) => {
            let fact = map_db_ref(err);
            let _ = abort_tx.send(());
            let _ = sleep_task.await;
            let _ = control.disconnect().await;
            return Err(Fail::new("cancel", fact));
        }
    };
    let sleep_result = sleep_task
        .await
        .map_err(|err| Fail::new("cancel", err.to_string()))?;
    let count_after = query_u64(&mut control, &count_sql).await?;
    let info_after = query_optional_string(&mut control, &info_sql).await?;
    let _ = control.disconnect().await;
    if info_after.contains(marker) {
        return Err(Fail::new("cancel", "sleep marker still in the process list"));
    }
    let proved = match (path, sleep_result) {
        ("kill_query", Err(err)) if err.code == "server" && err.fact.starts_with("1317 ") => json!(1317),
        ("kill_query", Ok(1)) => json!(1),
        ("connection_abort", Err(err)) if err.code == "connection_abort" => Json::Null,
        (_, Ok(0)) => return Err(Fail::new("cancel", "sleep returned 0")),
        (_, Ok(other)) => return Err(Fail::new("cancel", format!("sleep returned {other}"))),
        (_, Err(err)) => return Err(err),
    };
    write_json(
        &request.out.join("cancel.json"),
        &json!({
            "protocol": RUN_PROTOCOL,
            "state": "cancel_proven",
            "cancel_path": path,
            "business_connection_id": business_id,
            "control_connection_id": control_id,
            "handshake_matches_session": true,
            "connections_before": count_before,
            "connections_after": count_after,
            "marker_cleared": true,
            "server_code": proved,
            "reader_account": user,
            "dba_kill_used_by_driver": false,
            "source_acceptance": false,
        }),
    )?;
    Ok(())
}

async fn sleep_or_abort(
    mut conn: Conn,
    sql: String,
    mut abort_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<i64, Fail> {
    let started = conn.query_iter(sql).await;
    let mut result = match started {
        Ok(result) => result,
        Err(err) => {
            let _ = conn.disconnect().await;
            return Err(map_db(err, ""));
        }
    };
    let polled = {
        let pending = result.next();
        tokio::pin!(pending);
        tokio::select! {
            row = &mut pending => row.map_err(|err| map_db(err, "")),
            _ = &mut abort_rx => Err(Fail::new("connection_abort", "driver closed the business connection")),
        }
    };
    let mut row = match polled {
        Ok(Some(row)) => row,
        Ok(None) => {
            let _ = result.drop_result().await;
            let _ = conn.disconnect().await;
            return Err(Fail::new("cancel", "sleep returned no row"));
        }
        Err(err) => {
            drop(result);
            let _ = conn.disconnect().await;
            return Err(err);
        }
    };
    let value: Value = row.take(0).ok_or_else(|| Fail::new("row", "sleep"))?;
    let text = value_string(&value)?.unwrap_or_default();
    let _ = result.drop_result().await;
    let _ = conn.disconnect().await;
    text.trim()
        .parse::<i64>()
        .map_err(|_| Fail::new("cancel", format!("sleep value {text}")))
}

async fn wait_for_file(path: &Path, limit: Duration) -> Result<(), Fail> {
    let start = std::time::Instant::now();
    while start.elapsed() < limit {
        if path.is_file() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(Fail::new("go_file", "timed out"))
}

async fn connect(request: &Request) -> Result<Conn, Fail> {
    connect_opts(request, None).await
}

async fn connect_opts(request: &Request, keepalive_ms: Option<u32>) -> Result<Conn, Fail> {
    ident_piece(&request.user)?;
    let mut password = read_password(&request.password_file)?;
    let mut opts = OptsBuilder::default()
        .ip_or_hostname(request.host.clone())
        .tcp_port(request.port)
        .user(Some(request.user.clone()))
        .pass(Some(password.clone()))
        .db_name(Some("oo_synth".to_string()))
        .prefer_socket(false)
        .stmt_cache_size(0);
    if let Some(ms) = keepalive_ms {
        opts = opts.tcp_keepalive(Some(ms));
    }
    let connected = timeout(Duration::from_secs(15), Conn::new(opts)).await;
    let result = match connected {
        Err(_) => Err(Fail::new("connect_timeout", request.host.clone())),
        Ok(Ok(conn)) => Ok(conn),
        Ok(Err(err)) => Err(map_db(err, &password)),
    };
    password.zeroize();
    result
}

fn read_password(path: &Path) -> Result<String, Fail> {
    let meta = fs::metadata(path).map_err(|err| Fail::new("password_file", err.to_string()))?;
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(Fail::new("secret_mode", format!("{:o}", meta.permissions().mode())));
    }
    let mut text = fs::read_to_string(path).map_err(|err| Fail::new("password_file", err.to_string()))?;
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    if text.is_empty() {
        return Err(Fail::new("password_file", "empty"));
    }
    Ok(text)
}

async fn query_values(conn: &mut Conn, sql: &str) -> Result<Vec<Vec<Value>>, Fail> {
    let mut result = conn.query_iter(sql).await.map_err(|err| map_db(err, ""))?;
    let mut rows = Vec::new();
    while let Some(row) = result.next().await.map_err(|err| map_db(err, ""))? {
        rows.push(take_row(row)?);
    }
    result.drop_result().await.map_err(|err| map_db(err, ""))?;
    Ok(rows)
}

async fn query_u64(conn: &mut Conn, sql: &str) -> Result<u64, Fail> {
    let rows = query_values(conn, sql).await?;
    let text = value_string(only_cell(&rows)?)?.ok_or_else(|| Fail::new("catalog", "null count"))?;
    text.trim()
        .parse::<u64>()
        .map_err(|_| Fail::new("catalog", text))
}

async fn query_optional_string(conn: &mut Conn, sql: &str) -> Result<String, Fail> {
    let rows = query_values(conn, sql).await?;
    if rows.is_empty() {
        return Ok(String::new());
    }
    Ok(value_string(only_cell(&rows)?)?.unwrap_or_default())
}

fn take_row(mut row: Row) -> Result<Vec<Value>, Fail> {
    let width = row.len();
    let mut values = Vec::with_capacity(width);
    for index in 0..width {
        values.push(row.take(index).ok_or_else(|| Fail::new("row", index.to_string()))?);
    }
    Ok(values)
}

fn only_cell(rows: &[Vec<Value>]) -> Result<&Value, Fail> {
    only_row(rows.first().ok_or_else(|| Fail::new("catalog", "no row"))?)
}

fn only_row(row: &[Value]) -> Result<&Value, Fail> {
    row.first().ok_or_else(|| Fail::new("catalog", "no column"))
}

fn value_string(value: &Value) -> Result<Option<String>, Fail> {
    match value {
        Value::NULL => Ok(None),
        Value::Bytes(bytes) => {
            let text = std::str::from_utf8(bytes).map_err(|_| Fail::new("utf8", "catalog"))?;
            Ok(Some(text.to_string()))
        }
        Value::Int(number) => Ok(Some(number.to_string())),
        Value::UInt(number) => Ok(Some(number.to_string())),
        _ => Err(Fail::new("catalog_value", "unexpected scalar")),
    }
}

fn value_json(value: &Value) -> Result<Json, Fail> {
    let text = value_string(value)?.ok_or_else(|| Fail::new("catalog", "null json"))?;
    serde_json::from_str(&text).map_err(|err| Fail::new("catalog", err.to_string()))
}

fn json_rows(rows: &[Vec<Value>]) -> Result<Vec<Json>, Fail> {
    rows.iter().map(|row| value_json(only_row(row)?)).collect()
}

fn json_string(value: &Json, key: &str) -> Result<String, Fail> {
    match value.get(key) {
        Some(Json::String(text)) => Ok(text.clone()),
        Some(Json::Number(number)) => Ok(number.to_string()),
        Some(Json::Bool(flag)) => Ok(flag.to_string()),
        _ => Err(Fail::new("catalog", key.to_string())),
    }
}

fn json_opt_string(value: &Json, key: &str) -> Result<Option<String>, Fail> {
    match value.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(text)) if text.is_empty() => Ok(None),
        Some(_) => Ok(Some(json_string(value, key)?)),
    }
}

fn json_u64(value: &Json, key: &str) -> Result<u64, Fail> {
    match value.get(key) {
        Some(Json::Number(number)) => number.as_u64().ok_or_else(|| Fail::new("catalog", key.to_string())),
        Some(Json::String(text)) => text.parse().map_err(|_| Fail::new("catalog", key.to_string())),
        _ => Err(Fail::new("catalog", key.to_string())),
    }
}

fn json_i32(value: &Json, key: &str) -> Result<i32, Fail> {
    match value.get(key) {
        Some(Json::Number(number)) => number
            .as_i64()
            .and_then(|item| i32::try_from(item).ok())
            .ok_or_else(|| Fail::new("catalog", key.to_string())),
        Some(Json::String(text)) => text.parse().map_err(|_| Fail::new("catalog", key.to_string())),
        _ => Err(Fail::new("catalog", key.to_string())),
    }
}

fn json_opt_u32(value: &Json, key: &str) -> Result<Option<u32>, Fail> {
    match value.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Number(number)) => Ok(Some(
            number
                .as_u64()
                .and_then(|item| u32::try_from(item).ok())
                .ok_or_else(|| Fail::new("catalog", key.to_string()))?,
        )),
        Some(Json::String(text)) => Ok(Some(text.parse().map_err(|_| Fail::new("catalog", key.to_string()))?)),
        _ => Err(Fail::new("catalog", key.to_string())),
    }
}

fn json_opt_i32(value: &Json, key: &str) -> Result<Option<i32>, Fail> {
    match value.get(key) {
        None | Some(Json::Null) => Ok(None),
        _ => Ok(Some(json_i32(value, key)?)),
    }
}

fn matching_check(checks: &[Json], table: &str, column: &str) -> Option<String> {
    let needle = format!("`{column}`").to_ascii_lowercase();
    let mut found = Vec::new();
    for check in checks {
        let Ok(check_table) = json_string(check, "table_name") else {
            continue;
        };
        let Ok(clause) = json_string(check, "check_clause") else {
            continue;
        };
        if check_table == table && clause.to_ascii_lowercase().contains(&needle) {
            found.push(clause);
        }
    }
    if found.is_empty() {
        None
    } else {
        Some(found.join(" "))
    }
}

fn ident_piece(name: &str) -> Result<String, Fail> {
    if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
        return Err(Fail::new("ident", "characters"));
    }
    Ok(name.to_string())
}

fn map_db(err: mysql_async::Error, secret: &str) -> Fail {
    let mut text = err.to_string();
    if !secret.is_empty() {
        text = text.replace(secret, "[redacted]");
    }
    match &err {
        mysql_async::Error::Server(server) => Fail::new("server", format!("{} {text}", server.code)),
        _ => Fail::new("driver", text),
    }
}

fn map_db_ref(err: &mysql_async::Error) -> String {
    match err {
        mysql_async::Error::Server(server) => format!("{} {err}", server.code),
        _ => err.to_string(),
    }
}

fn server_code(err: &mysql_async::Error) -> Option<u16> {
    match err {
        mysql_async::Error::Server(server) => Some(server.code),
        _ => None,
    }
}

fn write_json(path: &Path, value: &Json) -> Result<(), Fail> {
    let mut text = serde_json::to_vec_pretty(value).map_err(|err| Fail::new("json", err.to_string()))?;
    text.push(b'\n');
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| Fail::new("out", err.to_string()))?;
    }
    fs::write(path, text).map_err(|err| Fail::new("out", err.to_string()))
}
