//! One dedicated target connection for one sealed spool.
//!
//! Static refusals return before the password file is read. Capability and
//! mapping checks run before `adopt_target_generation`. The receipt insert
//! and the row writes share one SQL transaction.

use std::collections::BTreeMap;
use std::time::Duration;

use cdc_spool::{encode_position, parse_commit_lsn, recover, Cell, Op, Recovery};
use oo_pipeline::physical_mapping::{
    plan_physical_mapping, quote_physical_name, Level, MappingRequest, LOOKUP_SQL,
};
use oo_pipeline::target_installer::{
    compiled_digests, HISTORICAL_V2_SHA256, VERSION_BOOTSTRAP, VERSION_FENCE, VERSION_MAPPING,
    VERSION_PREFLIGHT, VERSION_SEGMENT_V2,
};
use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, Error, Transaction};

use crate::model::{
    binding_from, config_preimage, convert_cell, digest_text, hook_preimage, manifest_matches,
    payload_preimage, plan_tables, schema_preimage, sql_text, type_preimage, Output, PgType,
    Request, Stored, TablePlan, APPLY_LIMIT,
};
use crate::tls::{hide_commit_connector, rustls_connector};

const CODEC: &str = "pg-commit-lsn/v1";
const CONNECT_WAIT: Duration = Duration::from_secs(20);

struct Prepared {
    source: String,
    txn_id: String,
    applied_pos: String,
    tables: Vec<TablePlan>,
    names: Vec<String>,
    config_digest: String,
    schema_digest: String,
    type_digest: String,
    hook_digest: String,
    payload_digest: String,
    work: Vec<Work>,
}

struct Work {
    table: String,
    op: Op,
    keys: Vec<Option<String>>,
    payload: Vec<Stored>,
}

struct Phys {
    namespace: String,
    table: String,
    keys: Vec<Col>,
    columns: Vec<Col>,
}

struct Col {
    name: String,
    pg_type: PgType,
    nullable: bool,
}

pub async fn apply(request: &Request) -> Output {
    let requested = Some(request.generation);
    if let Err(fault) = crate::model::request_closed(request) {
        return Output::refused(fault, requested);
    }
    if request.segment.is_some() {
        return Output::refused("needs_segments", requested);
    }
    let prepared = match prepare(request) {
        Ok(prepared) => prepared,
        Err(fault) => return Output::refused(fault, requested),
    };
    let password = match read_secret(&request.endpoint.password_file) {
        Ok(text) if !text.is_empty() => text,
        _ => return Output::refused("endpoint", requested),
    };
    let ca = match std::fs::read(&request.endpoint.ca_file) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        _ => return Output::refused("endpoint", requested),
    };
    let hide = std::env::var("CDC_APPLY_INJECT").ok().as_deref() == Some("hide_commit");
    let mut client = match connect(request, &password, &ca, hide).await {
        Ok(client) => client,
        Err(()) => return Output::refused("connect", requested),
    };
    if let Err(fault) = capabilities(&client).await {
        return tagged(Output::refused(fault, requested), &prepared, None, false);
    }
    let physical = match mappings(&client, request, &prepared).await {
        Ok(physical) => physical,
        Err(fault) => return tagged(Output::refused(fault, requested), &prepared, None, false),
    };
    let adopted = match adopt(&client, &request.shard, &request.owner).await {
        Ok(generation) => generation,
        Err(err) => {
            let fault = if sqlstate(&err).as_deref() == Some("55P03") {
                "fence_held"
            } else {
                "fence_adopt"
            };
            let mut out = tagged(Output::refused(fault, requested), &prepared, None, false);
            out.sqlstate = sqlstate(&err);
            return out;
        }
    };
    let advanced = adopted != request.generation;
    let outcome = write_transaction(&mut client, request, &prepared, &physical, adopted).await;
    match outcome {
        WriteResult::Applied(kind) => {
            park_for_kill("after_commit_before_response").await;
            applied_output(&prepared, kind, adopted, request.generation)
        }
        WriteResult::Refused { fault, sqlstate } => {
            let mut out = tagged(Output::refused(&fault, requested), &prepared, Some(adopted), advanced);
            out.sqlstate = sqlstate;
            out
        }
        WriteResult::Lost => {
            drop(client);
            reconcile(request, &password, &ca, &prepared, &physical).await
        }
    }
}

#[allow(clippy::cognitive_complexity)]
fn prepare(request: &Request) -> Result<Prepared, &'static str> {
    let committed = match recover(&request.spool_dir) {
        Ok(Recovery::Committed(committed)) => committed,
        Ok(Recovery::RolledBack(_)) => return Err("spool_rolled_back"),
        Ok(Recovery::Incomplete(_)) => return Err("spool_incomplete"),
        Ok(Recovery::Unknown { .. }) => return Err("spool_unknown"),
        Err(_) => return Err("spool_io"),
    };
    let manifest = &committed.manifest;
    if manifest.count > APPLY_LIMIT {
        return Err("needs_segments");
    }
    if manifest.selected_tables.is_empty() {
        return Err("binding_mismatch");
    }
    let tables = plan_tables(request, &manifest.selected_tables)?;
    manifest_matches(request, manifest, &tables)?;
    let lsn = parse_commit_lsn(&manifest.commit_lsn).map_err(|_| "spool_position")?;
    let applied_pos = encode_position(manifest.epoch, lsn).map_err(|_| "spool_position")?;
    if applied_pos != manifest.commit_position {
        return Err("spool_position");
    }
    let mut hashes = Vec::new();
    let mut work = Vec::new();
    let mut expect = 1u64;
    let iter = committed.iter().map_err(|_| "spool_io")?;
    for item in iter {
        let event = item.map_err(|_| "spool_io")?;
        if event.global_txn_sequence != expect {
            return Err("spool_sequence");
        }
        expect = expect.checked_add(1).ok_or("spool_sequence")?;
        let hashed = cdc_spool::event_hash_hex(&event.preimage).map_err(|_| "spool_io")?;
        if hashed != event.event_hash {
            return Err("spool_io");
        }
        hashes.push(event.event_hash);
        if let Some(table) = tables.iter().find(|table| table.logical == event.table) {
            work.push(plan_event(table, &event.key, event.op, &event.payload)?);
        }
    }
    if manifest.count == 0 {
        if expect != 1 {
            return Err("spool_sequence");
        }
    } else if expect != manifest.count.saturating_add(1) {
        return Err("spool_sequence");
    }
    let binding = binding_from(request, &tables);
    Ok(Prepared {
        source: manifest.source.clone(),
        txn_id: manifest.txn_id.clone(),
        applied_pos,
        names: tables.iter().map(|table| table.logical.clone()).collect(),
        config_digest: digest_text(&config_preimage(&binding)),
        schema_digest: digest_text(&schema_preimage(&tables)),
        type_digest: digest_text(&type_preimage(&tables)),
        hook_digest: digest_text(&hook_preimage(&manifest.hook_hashes)),
        payload_digest: digest_text(&payload_preimage(&hashes)),
        tables,
        work,
    })
}

fn plan_event(table: &TablePlan, key: &[Cell], op: Op, payload: &[Cell]) -> Result<Work, &'static str> {
    if key.len() != table.keys.len() {
        return Err("key_required");
    }
    let mut keys = Vec::with_capacity(key.len());
    for (cell, field) in key.iter().zip(&table.keys) {
        if cell.ordinal() != field.ordinal {
            return Err("ordinal_mismatch");
        }
        match convert_cell(cell, field.pg_type)? {
            Stored::Value(text) => keys.push(Some(text)),
            Stored::Null | Stored::Unavailable => return Err("key_required"),
        }
    }
    let stored_payload = if op == Op::Delete {
        if !payload.is_empty() {
            return Err("payload_incomplete");
        }
        Vec::new()
    } else {
        if payload.len() != table.columns.len() {
            return Err("payload_incomplete");
        }
        let mut stored = Vec::with_capacity(payload.len());
        for (cell, field) in payload.iter().zip(&table.columns) {
            if cell.ordinal() != field.ordinal {
                return Err("ordinal_mismatch");
            }
            stored.push(convert_cell(cell, field.pg_type)?);
        }
        stored
    };
    Ok(Work {
        table: table.logical.clone(),
        op,
        keys,
        payload: stored_payload,
    })
}

fn read_secret(path: &std::path::Path) -> Result<String, ()> {
    let bytes = std::fs::read(path).map_err(|_| ())?;
    let text = String::from_utf8(bytes).map_err(|_| ())?;
    Ok(text.trim_end_matches(['\r', '\n']).to_owned())
}

async fn connect(request: &Request, password: &str, ca: &[u8], hide: bool) -> Result<Client, ()> {
    let mut config = tokio_postgres::Config::new();
    config.host(&request.endpoint.host);
    config.port(request.endpoint.port);
    config.user(&request.endpoint.user);
    config.password(password);
    config.dbname(&request.endpoint.database);
    config.ssl_mode(tokio_postgres::config::SslMode::Require);
    let connect = async {
        if hide {
            let tls = hide_commit_connector(ca).map_err(|_| ())?;
            let (client, connection) = config.connect(tls).await.map_err(|_| ())?;
            tokio::spawn(async move {
                let _ = connection.await;
            });
            Ok(client)
        } else {
            let tls = rustls_connector(ca).map_err(|_| ())?;
            let (client, connection) = config.connect(tls).await.map_err(|_| ())?;
            tokio::spawn(async move {
                let _ = connection.await;
            });
            Ok(client)
        }
    };
    tokio::time::timeout(CONNECT_WAIT, connect).await.map_err(|_| ())?
}

async fn capabilities(client: &Client) -> Result<(), &'static str> {
    let rows = client
        .query(
            "SELECT version, checksum, phase FROM _oo.schema_migrations",
            &[],
        )
        .await
        .map_err(|_| "capability_absent")?;
    let mut seen: BTreeMap<String, (String, String)> = BTreeMap::new();
    for row in rows {
        let version: String = row.try_get(0).map_err(|_| "capability_absent")?;
        let checksum: String = row.try_get(1).map_err(|_| "capability_absent")?;
        let phase: String = row.try_get(2).map_err(|_| "capability_absent")?;
        seen.insert(version, (checksum, phase));
    }
    let digests = compiled_digests();
    let required = [
        (VERSION_BOOTSTRAP, digests.bootstrap()),
        (VERSION_FENCE, digests.fence()),
        (VERSION_PREFLIGHT, digests.preflight()),
        (VERSION_MAPPING, digests.mapping()),
    ];
    for (version, checksum) in required {
        match seen.get(version) {
            Some((stored, phase)) if stored == checksum && phase == "expand" => {}
            Some(_) => return Err("migration_drift"),
            None => return Err("capability_absent"),
        }
    }
    if let Some((stored, phase)) = seen.get(VERSION_SEGMENT_V2)
        && (stored != HISTORICAL_V2_SHA256 || phase != "expand")
    {
        return Err("migration_drift");
    }
    let functions = client
        .query(
            "SELECT p.proname::text, r.rolname::text \
             FROM pg_catalog.pg_proc AS p \
             INNER JOIN pg_catalog.pg_namespace AS n ON n.oid = p.pronamespace \
             INNER JOIN pg_catalog.pg_roles AS r ON r.oid = p.proowner \
             WHERE n.nspname = '_oo' \
               AND p.proname IN ( \
                 'adopt_target_generation', \
                 'assert_target_fence', \
                 'commit_apply_receipt', \
                 'lookup_physical_mapping' \
               )",
            &[],
        )
        .await
        .map_err(|_| "capability_absent")?;
    let mut names = BTreeMap::<String, String>::new();
    for row in functions {
        let name: String = row.try_get(0).map_err(|_| "capability_absent")?;
        let owner: String = row.try_get(1).map_err(|_| "capability_absent")?;
        if owner != "oo_target_owner" {
            return Err("capability_owner");
        }
        names.insert(name, owner);
    }
    if names.len() != 4 {
        return Err("capability_absent");
    }
    Ok(())
}

async fn mappings(
    client: &Client,
    request: &Request,
    prepared: &Prepared,
) -> Result<BTreeMap<String, Phys>, &'static str> {
    let mut out = BTreeMap::new();
    for table in &prepared.tables {
        let namespace = planned(
            request,
            &prepared.config_digest,
            Level::Namespace,
            &table.namespace,
            "",
            "",
        )?;
        let relation = planned(
            request,
            &prepared.config_digest,
            Level::Table,
            &table.namespace,
            &table.table,
            "",
        )?;
        let mut keys = Vec::new();
        for field in &table.keys {
            let column = planned(
                request,
                &prepared.config_digest,
                Level::Column,
                &table.namespace,
                &table.table,
                &field.name,
            )?;
            let stored = lookup(client, request, &prepared.config_digest, &column).await?;
            if stored != column.physical_name {
                return Err("mapping_foreign");
            }
            keys.push(Col {
                name: quote_physical_name(&stored).map_err(|_| "mapping_foreign")?,
                pg_type: field.pg_type,
                nullable: false,
            });
        }
        let mut columns = Vec::new();
        for field in &table.columns {
            let column = planned(
                request,
                &prepared.config_digest,
                Level::Column,
                &table.namespace,
                &table.table,
                &field.name,
            )?;
            let stored = lookup(client, request, &prepared.config_digest, &column).await?;
            if stored != column.physical_name {
                return Err("mapping_foreign");
            }
            columns.push(Col {
                name: quote_physical_name(&stored).map_err(|_| "mapping_foreign")?,
                pg_type: field.pg_type,
                nullable: field.nullable,
            });
        }
        let ns_name = lookup(client, request, &prepared.config_digest, &namespace).await?;
        let table_name = lookup(client, request, &prepared.config_digest, &relation).await?;
        if ns_name != namespace.physical_name || table_name != relation.physical_name {
            return Err("mapping_foreign");
        }
        out.insert(
            table.logical.clone(),
            Phys {
                namespace: quote_physical_name(&ns_name).map_err(|_| "mapping_foreign")?,
                table: quote_physical_name(&table_name).map_err(|_| "mapping_foreign")?,
                keys,
                columns,
            },
        );
    }
    Ok(out)
}

fn planned<'a>(
    request: &'a Request,
    config_digest: &'a str,
    level: Level,
    namespace: &'a str,
    table: &'a str,
    column: &'a str,
) -> Result<oo_pipeline::physical_mapping::PhysicalPlan, &'static str> {
    plan_physical_mapping(&MappingRequest {
        source: request.source.clone(),
        config_digest: config_digest.to_owned(),
        run_generation: request.run_generation,
        level,
        logical_namespace: namespace.to_owned(),
        logical_table: table.to_owned(),
        logical_column: column.to_owned(),
    })
    .map_err(|_| "mapping_name")
}

async fn lookup(
    client: &Client,
    request: &Request,
    config_digest: &str,
    plan: &oo_pipeline::physical_mapping::PhysicalPlan,
) -> Result<String, &'static str> {
    let generation = request.run_generation;
    let level = plan.level.as_str();
    let params: [&(dyn ToSql + Sync); 7] = [
        &request.source,
        &config_digest,
        &generation,
        &level,
        &plan.logical_namespace,
        &plan.logical_table,
        &plan.logical_column,
    ];
    let row = client.query_one(LOOKUP_SQL, &params).await.map_err(|err| {
        if sqlstate(&err).as_deref() == Some("OO024") {
            "mapping_absent"
        } else {
            "mapping_lookup"
        }
    })?;
    row.try_get(0).map_err(|_| "mapping_lookup")
}

async fn adopt(client: &Client, shard: &str, owner: &str) -> Result<i64, Error> {
    let row = client
        .query_one(
            "SELECT _oo.adopt_target_generation($1::text, $2::text::uuid)",
            &[&shard, &owner],
        )
        .await?;
    row.try_get(0)
}

enum WriteResult {
    Applied(&'static str),
    Refused { fault: String, sqlstate: Option<String> },
    Lost,
}

#[allow(clippy::cognitive_complexity)]
async fn write_transaction(
    client: &mut Client,
    request: &Request,
    prepared: &Prepared,
    physical: &BTreeMap<String, Phys>,
    adopted: i64,
) -> WriteResult {
    let Ok(epoch) = i32::try_from(request.epoch) else {
        return WriteResult::Refused {
            fault: "epoch_range".to_owned(),
            sqlstate: None,
        };
    };
    let tx = match client.transaction().await {
        Ok(tx) => tx,
        Err(err) => {
            return WriteResult::Refused {
                fault: "connect".to_owned(),
                sqlstate: sqlstate(&err),
            };
        }
    };
    if let Err(err) = assert_fence(&tx, &request.shard, &request.owner, adopted).await {
        let state = sqlstate(&err);
        let _ = tx.rollback().await;
        return WriteResult::Refused {
            fault: "fence_adopt".to_owned(),
            sqlstate: state,
        };
    }
    let outcome = match receipt(&tx, request, prepared, adopted, epoch).await {
        Ok(outcome) => outcome,
        Err(err) => {
            let state = sqlstate(&err);
            let fault = if state.as_deref() == Some("OO010") {
                "payload_conflict"
            } else {
                "receipt"
            };
            let _ = tx.rollback().await;
            return WriteResult::Refused {
                fault: fault.to_owned(),
                sqlstate: state,
            };
        }
    };
    if outcome == "replay" {
        return match tx.commit().await {
            Ok(()) => WriteResult::Applied("replay"),
            Err(err) if err.code().is_none() => WriteResult::Lost,
            Err(err) => WriteResult::Refused {
                fault: "receipt".to_owned(),
                sqlstate: sqlstate(&err),
            },
        };
    }
    if outcome != "inserted" {
        let _ = tx.rollback().await;
        return WriteResult::Refused {
            fault: "receipt".to_owned(),
            sqlstate: None,
        };
    }
    if let Err(fault) = write_rows(&tx, physical, &prepared.work).await {
        let _ = tx.rollback().await;
        return WriteResult::Refused {
            fault: fault.to_owned(),
            sqlstate: None,
        };
    }
    if std::env::var("CDC_APPLY_INJECT").ok().as_deref() == Some("row_failure") {
        let _ = tx.batch_execute("SELECT pg_catalog.div(1, 0)").await;
        let _ = tx.rollback().await;
        return WriteResult::Refused {
            fault: "row_failure".to_owned(),
            sqlstate: Some("22012".to_owned()),
        };
    }
    park_for_kill("before_commit").await;
    match tx.commit().await {
        Ok(()) => WriteResult::Applied("inserted"),
        Err(err) if err.code().is_none() => WriteResult::Lost,
        Err(err) => WriteResult::Refused {
            fault: "receipt".to_owned(),
            sqlstate: sqlstate(&err),
        },
    }
}

async fn adopt_after_release(client: &Client, shard: &str, owner: &str) -> Result<i64, ()> {
    for _ in 0..20 {
        match adopt(client, shard, owner).await {
            Ok(generation) => return Ok(generation),
            Err(err) if sqlstate(&err).as_deref() == Some("55P03") => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(_) => return Err(()),
        }
    }
    Err(())
}

async fn assert_fence(tx: &Transaction<'_>, shard: &str, owner: &str, generation: i64) -> Result<(), Error> {
    tx.execute(
        "SELECT _oo.assert_target_fence($1::text, $2::text::uuid, $3::int8)",
        &[&shard, &owner, &generation],
    )
    .await
    .map(|_| ())
}

async fn receipt(
    tx: &Transaction<'_>,
    request: &Request,
    prepared: &Prepared,
    generation: i64,
    epoch: i32,
) -> Result<String, Error> {
    let partial = false;
    let cross = true;
    let complete = true;
    let segment: Option<i32> = None;
    let bitmap: Vec<u8> = Vec::new();
    let params: [&(dyn ToSql + Sync); 21] = [
        &request.shard,
        &request.owner,
        &generation,
        &prepared.source,
        &request.table_generation,
        &prepared.txn_id,
        &prepared.config_digest,
        &prepared.type_digest,
        &prepared.hook_digest,
        &prepared.schema_digest,
        &prepared.payload_digest,
        &CODEC,
        &request.lineage,
        &epoch,
        &prepared.applied_pos,
        &partial,
        &cross,
        &complete,
        &segment,
        &segment,
        &bitmap,
    ];
    let row = tx
        .query_one(
            "SELECT _oo.commit_apply_receipt( \
                $1::text, $2::text::uuid, $3::int8, $4::text, $5::text, $6::text, \
                $7::text, $8::text, $9::text, $10::text, $11::text, $12::text, $13::text, \
                $14::int4, $15::text::numeric, $16::bool, $17::bool, $18::bool, \
                $19::int4, $20::int4, $21::bytea)",
            &params,
        )
        .await?;
    row.try_get(0)
}

async fn write_rows(
    tx: &Transaction<'_>,
    physical: &BTreeMap<String, Phys>,
    work: &[Work],
) -> Result<(), &'static str> {
    for event in work {
        let table = physical.get(&event.table).ok_or("binding_mismatch")?;
        match event.op {
            Op::Delete => delete_row(tx, table, &event.keys).await?,
            Op::Upsert => upsert_row(tx, table, &event.keys, &event.payload).await?,
        }
    }
    Ok(())
}

async fn delete_row(tx: &Transaction<'_>, table: &Phys, keys: &[Option<String>]) -> Result<(), &'static str> {
    let namespace = &table.namespace;
    let relation = &table.table;
    let pred = predicate(&table.keys, 1);
    let sql = format!("DELETE FROM {namespace}.{relation} WHERE {pred}");
    tx.execute(&sql, &bind(keys)).await.map_err(|_| "row_write")?;
    Ok(())
}

async fn upsert_row(
    tx: &Transaction<'_>,
    table: &Phys,
    keys: &[Option<String>],
    payload: &[Stored],
) -> Result<(), &'static str> {
    let namespace = &table.namespace;
    let relation = &table.table;
    let pred = predicate(&table.keys, 1);
    let exists_sql = format!("SELECT 1 FROM {namespace}.{relation} WHERE {pred} FOR UPDATE");
    let exists = tx
        .query_opt(&exists_sql, &bind(keys))
        .await
        .map_err(|_| "row_write")?
        .is_some();
    if exists {
        update_row(tx, table, keys, payload).await
    } else {
        for (column, stored) in table.columns.iter().zip(payload) {
            match stored {
                Stored::Unavailable => return Err("unavailable_insert"),
                Stored::Null if !column.nullable => return Err("null_required"),
                Stored::Null | Stored::Value(_) => {}
            }
        }
        insert_row(tx, table, keys, payload).await
    }
}

async fn update_row(
    tx: &Transaction<'_>,
    table: &Phys,
    keys: &[Option<String>],
    payload: &[Stored],
) -> Result<(), &'static str> {
    let mut sets = Vec::new();
    let mut params = Vec::new();
    let mut index = 1i32;
    for (column, stored) in table.columns.iter().zip(payload) {
        match stored {
            Stored::Unavailable => {}
            Stored::Null if !column.nullable => return Err("null_required"),
            Stored::Null => {
                let name = &column.name;
                let cast = column.pg_type.cast();
                sets.push(format!("{name} = ${index}{cast}"));
                params.push(None);
                index += 1;
            }
            Stored::Value(text) => {
                let name = &column.name;
                let cast = column.pg_type.cast();
                sets.push(format!("{name} = ${index}{cast}"));
                params.push(Some(text.clone()));
                index += 1;
            }
        }
    }
    if sets.is_empty() {
        let key = table.keys.first().ok_or("key_required")?;
        let name = &key.name;
        sets.push(format!("{name} = {name}"));
    }
    params.extend(keys.iter().cloned());
    let namespace = &table.namespace;
    let relation = &table.table;
    let assignments = sets.join(", ");
    let pred = predicate(&table.keys, index);
    let sql = format!("UPDATE {namespace}.{relation} SET {assignments} WHERE {pred}");
    tx.execute(&sql, &bind(&params)).await.map_err(|_| "row_write")?;
    Ok(())
}

async fn insert_row(
    tx: &Transaction<'_>,
    table: &Phys,
    keys: &[Option<String>],
    payload: &[Stored],
) -> Result<(), &'static str> {
    let mut names = Vec::new();
    let mut places = Vec::new();
    let mut params = Vec::new();
    let mut index = 1i32;
    for (column, value) in table.keys.iter().zip(keys) {
        names.push(column.name.clone());
        let cast = column.pg_type.cast();
        places.push(format!("${index}{cast}"));
        params.push(value.clone());
        index += 1;
    }
    for (column, stored) in table.columns.iter().zip(payload) {
        let Some(value) = sql_text(stored) else {
            return Err("unavailable_insert");
        };
        names.push(column.name.clone());
        let cast = column.pg_type.cast();
        places.push(format!("${index}{cast}"));
        params.push(value);
        index += 1;
    }
    let namespace = &table.namespace;
    let relation = &table.table;
    let columns = names.join(", ");
    let values = places.join(", ");
    let sql = format!("INSERT INTO {namespace}.{relation} ({columns}) VALUES ({values})");
    tx.execute(&sql, &bind(&params)).await.map_err(|_| "row_write")?;
    Ok(())
}

fn predicate(keys: &[Col], start: i32) -> String {
    keys.iter()
        .enumerate()
        .map(|(offset, column)| {
            let name = &column.name;
            let index = start + i32::try_from(offset).unwrap_or(0);
            let cast = column.pg_type.cast();
            format!("{name} = ${index}{cast}")
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn bind(values: &[Option<String>]) -> Vec<&(dyn ToSql + Sync)> {
    values.iter().map(|value| value as &(dyn ToSql + Sync)).collect()
}

#[allow(clippy::cognitive_complexity)]
async fn reconcile(
    request: &Request,
    password: &str,
    ca: &[u8],
    prepared: &Prepared,
    physical: &BTreeMap<String, Phys>,
) -> Output {
    if std::env::var("CDC_APPLY_RECONCILE").ok().as_deref() == Some("down") {
        return unknown(prepared, request.generation);
    }
    let Ok(epoch) = i32::try_from(request.epoch) else {
        return unknown(prepared, request.generation);
    };
    let mut client = match connect(request, password, ca, false).await {
        Ok(client) => client,
        Err(()) => return unknown(prepared, request.generation),
    };
    let adopted = match adopt_after_release(&client, &request.shard, &request.owner).await {
        Ok(generation) => generation,
        Err(()) => return unknown(prepared, request.generation),
    };
    let tx = match client.transaction().await {
        Ok(tx) => tx,
        Err(_) => return unknown(prepared, request.generation),
    };
    if assert_fence(&tx, &request.shard, &request.owner, adopted).await.is_err() {
        let _ = tx.rollback().await;
        return unknown(prepared, request.generation);
    }
    let outcome = match receipt(&tx, request, prepared, adopted, epoch).await {
        Ok(outcome) => outcome,
        Err(err) => {
            let state = sqlstate(&err);
            let _ = tx.rollback().await;
            if state.as_deref() == Some("OO010") {
                let mut out = tagged(
                    Output::refused("payload_conflict", Some(request.generation)),
                    prepared,
                    Some(adopted),
                    adopted != request.generation,
                );
                out.sqlstate = state;
                return out;
            }
            return unknown(prepared, request.generation);
        }
    };
    if outcome == "replay" {
        return if tx.commit().await.is_ok() {
            applied_output(prepared, "replay", adopted, request.generation)
        } else {
            unknown(prepared, request.generation)
        };
    }
    if outcome == "inserted" {
        if let Err(fault) = write_rows(&tx, physical, &prepared.work).await {
            let _ = tx.rollback().await;
            return tagged(
                Output::refused(fault, Some(request.generation)),
                prepared,
                Some(adopted),
                adopted != request.generation,
            );
        }
        return if tx.commit().await.is_ok() {
            applied_output(prepared, "inserted", adopted, request.generation)
        } else {
            unknown(prepared, request.generation)
        };
    }
    let _ = tx.rollback().await;
    unknown(prepared, request.generation)
}

async fn park_for_kill(kind: &str) {
    let Ok(want) = std::env::var("CDC_APPLY_CHECKPOINT") else {
        return;
    };
    if want != kind {
        return;
    }
    if let Ok(path) = std::env::var("CDC_APPLY_MARK") {
        let _ = std::fs::write(path, kind);
    }
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

fn sqlstate(err: &Error) -> Option<String> {
    err.code().map(|code| code.code().to_owned())
}

fn applied_output(prepared: &Prepared, outcome: &'static str, adopted: i64, requested: i64) -> Output {
    Output {
        contract: crate::model::CONTRACT,
        kind: "applied",
        fault: None,
        outcome: Some(outcome.to_owned()),
        sqlstate: None,
        source: Some(prepared.source.clone()),
        txn_id: Some(prepared.txn_id.clone()),
        generation: Some(adopted),
        requested_generation: Some(requested),
        generation_advanced: adopted != requested,
        applied_pos: Some(prepared.applied_pos.clone()),
        row_writes: if outcome == "replay" {
            0
        } else {
            u64::try_from(prepared.work.len()).unwrap_or(0)
        },
        tables: prepared.names.clone(),
        target_applied: true,
        ack_allowed: false,
        db_cross_table_consistent: true,
        source_cross_table_consistent: false,
        upstream_commit_lsn_verified: false,
        debezium_normalizer: false,
        p2_verified: false,
        p11_verified: false,
        control_lease_consulted: false,
        commit_position_provenance: "caller_declared",
    }
}

fn unknown(prepared: &Prepared, requested: i64) -> Output {
    let mut out = tagged(
        Output::refused("commit_unknown", Some(requested)),
        prepared,
        None,
        false,
    );
    out.kind = "commit_unknown";
    out
}

fn tagged(mut out: Output, prepared: &Prepared, generation: Option<i64>, advanced: bool) -> Output {
    out.source = Some(prepared.source.clone());
    out.txn_id = Some(prepared.txn_id.clone());
    out.applied_pos = Some(prepared.applied_pos.clone());
    out.tables = prepared.names.clone();
    out.generation = generation;
    out.generation_advanced = advanced;
    out.target_applied = false;
    out.db_cross_table_consistent = false;
    out
}
