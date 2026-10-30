//! Current source mode of the original receipt producer. No target writes or ACK.
use super::*;
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use futures_util::TryStreamExt;
use serde::Deserialize;
use crate::config::{EndpointFile, FieldName, ObjectName};

pub(crate) const CONFIG_PROTOCOL: &str = "oo.postgres-preview.capture-config/v1";
const MANIFEST_PROTOCOL: &str = "oo.postgres-preview.capture-set/v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    protocol: String,
    source_id: String,
    source: EndpointFile,
    approved_objects: Vec<ObjectName>,
    approved_fields: Vec<FieldName>,
    capture: SourceIdentity,
    chunk_rows: usize,
    chunk_bytes: usize,
    statement_ms: u32,
    out_dir: PathBuf,
    #[serde(default)]
    resume: Option<Artifact>,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct SourceIdentity {
    system_identifier: String,
    database_oid: u32,
    slot: String,
    publication: String,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    path: PathBuf,
    sha256: String,
}

fn fail(name: &'static str) -> Fail { Fail::new(name, 4) }
fn sql_error(_: tokio_postgres::Error) -> Fail { fail("current.sql") }

impl Settings {
    fn validate(&self) -> Result<(), Fail> {
        let objects: BTreeSet<_> = self.approved_objects.iter().map(|o| (&o.schema, &o.table)).collect();
        let fields: BTreeSet<_> = self.approved_fields.iter().map(|f| (&f.schema, &f.table, &f.column)).collect();
        if self.protocol != CONFIG_PROTOCOL || !ident_ok(&self.source_id)
            || objects.is_empty() || objects.len() != self.approved_objects.len()
            || fields.is_empty() || fields.len() != self.approved_fields.len()
            || self.approved_objects.iter().any(|o| !ident_ok(&o.schema) || !ident_ok(&o.table))
            || self.approved_fields.iter().any(|f| !ident_ok(&f.column) || !objects.contains(&(&f.schema, &f.table)))
            || !ident_ok(&self.capture.slot) || !ident_ok(&self.capture.publication)
            || self.capture.database_oid == 0
            || self.capture.system_identifier.parse::<u64>().is_err()
            || self.chunk_rows == 0 || self.chunk_bytes < 4096
            || self.chunk_rows > 1_000_000 || self.chunk_bytes > 64 * 1024 * 1024
            || !(1..=120_000).contains(&self.statement_ms)
            || !self.out_dir.is_absolute()
        { return Err(fail("current.config")); }
        Ok(())
    }
}

fn save(path: &Path, data: &[u8]) -> Result<(), Fail> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600)
        .open(path).map_err(|_| fail("current.immutable_output"))?;
    file.write_all(data).and_then(|_| file.sync_all()).map_err(|_| fail("current.io"))
}

fn save_json(path: &Path, body: &Value) -> Result<(), Fail> { save(path, &canonical_bytes(body)?) }

pub(crate) async fn run(command: &str, bytes: &[u8]) -> Result<(), Fail> {
    if !matches!(command, "capture-preflight" | "capture-receipt" | "capture-receipt-transaction") {
        return Err(fail("current.command"));
    }
    let settings: Settings = serde_json::from_slice(bytes).map_err(|_| fail("current.config"))?;
    settings.validate()?;
    if command != "capture-receipt-transaction" && settings.resume.is_some() {
        return Err(fail("current.resume_command"));
    }
    // Fail before connecting when a prior attempt already owns this directory.
    fs::create_dir(&settings.out_dir).map_err(|_| fail("current.output_exists"))?;
    fs::set_permissions(&settings.out_dir, fs::Permissions::from_mode(0o700)).map_err(|_| fail("current.io"))?;
    let source = &settings.source;
    let endpoint = endpoint_from_files(&source.host, source.port, &source.database,
        &source.username, &source.password_file, &source.ca_file)?;
    let session = connect_read_only(&endpoint).await?;
    let client = &session.client;
    client.batch_execute(&format!("SET statement_timeout={}; SET search_path=pg_catalog; SET TIME ZONE 'UTC'",
        settings.statement_ms)).await.map_err(sql_error)?;
    let (observation, shapes) = preflight(&settings, client).await?;
    save_json(&settings.out_dir.join("preflight.json"), &observation)?;
    if command == "capture-preflight" {
        println!("{}", serde_json::to_string(&observation).map_err(|_| fail("current.json"))?);
        return Ok(());
    }
    let mut manifest = json!({
        "protocol": MANIFEST_PROTOCOL, "source_id": settings.source_id,
        "source_identity": observation["source_identity"], "catalog": observation["catalog"],
        "config_sha256": sha256(bytes), "driver_sha256": driver_digest()?,
        "source_read_only": true, "source_dml": 0, "slot_advanced": false,
        "admission_decision": "not_issued", "target_applied": false, "worker_ack": false,
        "resume": settings.resume,
    });
    if command == "capture-receipt" {
        baseline(&settings, client, &shapes, &observation, &mut manifest).await?;
    } else {
        transactions(&settings, client, &observation, &mut manifest).await?;
    }
    manifest["complete"] = json!(true);
    let path = settings.out_dir.join("manifest.json");
    let data = canonical_bytes(&manifest)?;
    save(&path, &data)?;
    println!("{}", json!({"manifest": path, "sha256": sha256(&data), "complete": true,
        "target_applied": false, "worker_ack": false}));
    Ok(())
}

async fn preflight(s: &Settings, client: &tokio_postgres::Client) -> Result<(Value, Vec<TableData>), Fail> {
    let row = client.query_one(
        "SELECT current_setting('server_version_num')::int, current_setting('wal_level'), current_user::text, \
         current_setting('transaction_read_only'), r.rolsuper, r.rolreplication, r.rolcreatedb, r.rolcreaterole, \
         (SELECT system_identifier::text FROM pg_control_system()), \
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()) \
         FROM pg_roles r WHERE rolname=current_user", &[]).await.map_err(sql_error)?;
    let version: i32 = row.get(0);
    if !(170000..180000).contains(&version) || row.get::<_, String>(1) != "logical"
        || row.get::<_, String>(2) != s.source.username || row.get::<_, String>(3) != "on"
        || row.get::<_, bool>(4) || !row.get::<_, bool>(5) || row.get::<_, bool>(6) || row.get::<_, bool>(7)
        || row.get::<_, String>(8) != s.capture.system_identifier
        || row.get::<_, i64>(9) != i64::from(s.capture.database_oid)
    { return Err(fail("current.source_identity_or_privileges")); }
    let slot = client.query_opt(
        "SELECT plugin::text, database::text, active, confirmed_flush_lsn::text, restart_lsn::text, \
         wal_status::text, invalidation_reason::text, temporary, two_phase \
         FROM pg_replication_slots WHERE slot_name=$1", &[&s.capture.slot]).await.map_err(sql_error)?
        .ok_or(fail("current.slot_missing"))?;
    if slot.get::<_, String>(0) != "pgoutput" || slot.get::<_, String>(1) != s.source.database
        || slot.get::<_, bool>(2) || slot.get::<_, Option<String>>(3).is_none()
        || slot.get::<_, Option<String>>(4).is_none()
        || !matches!(slot.get::<_, Option<String>>(5).as_deref(), Some("reserved" | "extended"))
        || slot.get::<_, Option<String>>(6).is_some() || slot.get::<_, bool>(7) || slot.get::<_, bool>(8)
    { return Err(fail("current.slot_unavailable")); }
    let publication = client.query_opt(
        "SELECT oid::bigint, puballtables, pubinsert, pubupdate, pubdelete, pubtruncate, pubviaroot, \
         pg_get_userbyid(pubowner)::text FROM pg_publication WHERE pubname=$1",
        &[&s.capture.publication]).await.map_err(sql_error)?.ok_or(fail("current.publication_missing"))?;
    if publication.get::<_, bool>(1) || !publication.get::<_, bool>(2)
        || !publication.get::<_, bool>(3) || !publication.get::<_, bool>(4)
        || !publication.get::<_, bool>(5) || publication.get::<_, bool>(6)
        || publication.get::<_, String>(7) == s.source.username
    { return Err(fail("current.publication_shape")); }
    let members = client.query("SELECT schemaname::text, tablename::text, rowfilter FROM pg_publication_tables WHERE pubname=$1",
        &[&s.capture.publication]).await.map_err(sql_error)?;
    let expected: BTreeSet<_> = s.approved_objects.iter().map(|o| (o.schema.clone(), o.table.clone())).collect();
    let actual: BTreeSet<_> = members.iter().map(|r| (r.get::<_, String>(0), r.get::<_, String>(1))).collect();
    if actual != expected || members.iter().any(|r| r.get::<_, Option<String>>(2).is_some()) {
        return Err(fail("current.publication_members"));
    }
    let mut shapes = Vec::new();
    let mut catalog = Vec::new();
    for object in &s.approved_objects {
        let relation = format!("{}.{}", quote(&object.schema), quote(&object.table));
        let facts = client.query_one(
            "SELECT c.oid::bigint, c.relkind::text, c.relrowsecurity, c.relforcerowsecurity, \
             pg_get_userbyid(c.relowner)::text, has_table_privilege(current_user,c.oid,'INSERT,UPDATE,DELETE,TRUNCATE,TRIGGER'), \
             c.relreplident::text, EXISTS(SELECT 1 FROM pg_publication_rel pr JOIN pg_publication p ON p.oid=pr.prpubid \
             WHERE p.pubname=$2 AND pr.prrelid=c.oid AND pr.prattrs IS NOT NULL) \
             FROM pg_class c WHERE c.oid=$1::text::regclass",
            &[&relation, &s.capture.publication]).await.map_err(sql_error)?;
        if facts.get::<_, String>(1) != "r" || facts.get::<_, bool>(2) || facts.get::<_, bool>(3)
            || facts.get::<_, String>(4) == s.source.username || facts.get::<_, bool>(5)
            || !matches!(facts.get::<_, String>(6).as_str(), "d" | "f") || facts.get::<_, bool>(7)
        { return Err(fail("current.table_privileges_or_shape")); }
        let found = client.query(TABLE_SHAPE_SQL, &[&object.schema, &object.table]).await.map_err(sql_error)?;
        let shape = table_shape(&found, &s.approved_fields, &object.schema, &object.table)?;
        // Raw pgoutput contains every published column. Never leak unapproved columns.
        if !shape.excluded.is_empty() { return Err(fail("current.unapproved_wal_columns")); }
        for col in &shape.columns {
            let privileges = client.query_one(
                "SELECT has_column_privilege(current_user,$1::text,$2::text,'SELECT'), \
                 has_column_privilege(current_user,$1::text,$2::text,'INSERT,UPDATE')",
                &[&relation, &col.name]).await.map_err(sql_error)?;
            if !privileges.get::<_, bool>(0) || privileges.get::<_, bool>(1) {
                return Err(fail("current.column_privileges"));
            }
        }
        catalog.push(json!({"schema": object.schema, "table": object.table,
            "relation_oid": facts.get::<_, i64>(0), "primary_key": shape.pk,
            "columns": column_projection(&shape), "native_notes": native_notes(&shape)}));
        shapes.push(shape);
    }
    Ok((json!({
        "protocol": "oo.postgres-preview.preflight/v1", "metadata_only": true,
        "server_version_num": version, "reader": s.source.username, "read_only": true,
        "replication": true, "business_write_privileges": false, "source_dml": 0,
        "source_identity": {"source_id": s.source_id, "host": s.source.host, "port": s.source.port,
            "database": s.source.database, "system_identifier": s.capture.system_identifier,
            "database_oid": s.capture.database_oid, "slot": s.capture.slot,
            "publication": s.capture.publication, "publication_oid": publication.get::<_, i64>(0),
            "publication_owner": publication.get::<_, String>(7), "plugin": "pgoutput"},
        "confirmed_flush_lsn": slot.get::<_, String>(3), "restart_lsn": slot.get::<_, String>(4),
        "catalog": catalog, "slot_advanced": false, "admission_decision": "not_issued"
    }), shapes))
}

async fn baseline(s: &Settings, client: &tokio_postgres::Client, shapes: &[TableData],
    observation: &Value, manifest: &mut Value) -> Result<(), Fail> {
    client.batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY").await.map_err(sql_error)?;
    let (snapshot_metadata, _) = preflight(s, client).await?;
    if snapshot_metadata["catalog"] != observation["catalog"] {
        return Err(fail("current.schema_drift"));
    }
    let snap = client.query_one("SELECT pg_export_snapshot()::text, pg_current_wal_lsn()::text", &[]).await.map_err(sql_error)?;
    manifest["kind"] = json!("baseline");
    manifest["snapshot"] = json!({"kind": "pg_export_snapshot", "value": snap.get::<_, String>(0)});
    manifest["observed_wal_lsn"] = json!(snap.get::<_, String>(1));
    manifest["resume_lsn"] = observation["confirmed_flush_lsn"].clone();
    manifest["handoff"] = json!("reconcile_from_original_slot_required");
    manifest["cross_table_consistency"] = json!("snapshot");
    let mut tables = Vec::new();
    for shape in shapes {
        let query = data_sql(&shape.schema, &shape.table, &shape.pk, &shape.columns)?;
        let stream = client.query_raw(&query, std::iter::empty::<&(dyn tokio_postgres::types::ToSql + Sync)>())
            .await.map_err(sql_error)?;
        futures_util::pin_mut!(stream);
        let mut rows = Vec::new();
        let mut bytes = 0;
        let mut total = 0u64;
        let mut parts = Vec::new();
        while let Some(row) = stream.try_next().await.map_err(sql_error)? {
            let item = typed_row(&row, &shape.columns)?;
            bytes += item.1.iter().map(cell_bytes).sum::<usize>();
            rows.push(item);
            if rows.len() >= s.chunk_rows || bytes >= s.chunk_bytes {
                flush_arrow(s, shape, &mut rows, &mut parts, &mut total)?;
                bytes = 0;
            }
        }
        flush_arrow(s, shape, &mut rows, &mut parts, &mut total)?;
        tables.push(json!({"schema": shape.schema, "table": shape.table, "rows": total,
            "columns": column_projection(shape), "parts": parts, "complete": true}));
    }
    client.batch_execute("COMMIT").await.map_err(sql_error)?;
    unchanged_slot(s, client, observation).await?;
    manifest["tables"] = json!(tables);
    Ok(())
}

fn cell_bytes(cell: &CellValue) -> usize {
    match cell { CellValue::Text(t) => t.len() + 16, CellValue::Bytes(b) => b.len() + 16, _ => 32 }
}

fn flush_arrow(s: &Settings, shape: &TableData, rows: &mut Vec<(i64, Vec<CellValue>)>,
    parts: &mut Vec<Value>, total: &mut u64) -> Result<(), Fail> {
    if rows.is_empty() { return Ok(()); }
    let mut offset = 0;
    for bytes in split_pages(&shape.columns, rows, s.chunk_bytes)? {
        let count = page_row_count(&rows[offset..], &bytes, &shape.columns)?;
        let name = format!("{}.{}.part-{:08}.arrow", shape.schema, shape.table, parts.len());
        save(&s.out_dir.join(&name), &bytes)?;
        parts.push(json!({"path": name, "sha256": sha256(&bytes), "bytes": bytes.len(), "rows": count,
            "first_key": rows[offset].0.to_string(), "last_key": rows[offset + count - 1].0.to_string()}));
        offset += count;
        *total += count as u64;
    }
    if offset != rows.len() { return Err(fail("current.chunk_count")); }
    rows.clear();
    Ok(())
}

fn lsn(value: &str) -> Result<u64, Fail> {
    let (high, low) = value.split_once('/').ok_or(fail("current.lsn"))?;
    let high = u32::from_str_radix(high, 16).map_err(|_| fail("current.lsn"))?;
    let low = u32::from_str_radix(low, 16).map_err(|_| fail("current.lsn"))?;
    Ok((u64::from(high) << 32) | u64::from(low))
}

fn resume(s: &Settings, observation: &Value) -> Result<u64, Fail> {
    let artifact = s.resume.as_ref().ok_or(fail("current.baseline_required"))?;
    let data = fs::read(&artifact.path).map_err(|_| fail("current.resume_read"))?;
    if sha256(&data) != artifact.sha256 { return Err(fail("current.resume_digest")); }
    let prior: Value = serde_json::from_slice(&data).map_err(|_| fail("current.resume_json"))?;
    if prior["protocol"] != MANIFEST_PROTOCOL || prior["complete"] != true
        || prior["source_identity"] != observation["source_identity"] || prior["catalog"] != observation["catalog"]
        || prior["source_id"] != s.source_id || !matches!(prior["kind"].as_str(), Some("baseline" | "transactions"))
        || prior["slot_advanced"] != false || prior["worker_ack"] != false || prior["target_applied"] != false
    { return Err(fail("current.resume_identity")); }
    verify_parts(&artifact.path, &prior)?;
    let boundary = lsn(prior["resume_lsn"].as_str().ok_or(fail("current.resume_lsn"))?)?;
    let confirmed = lsn(observation["confirmed_flush_lsn"].as_str().ok_or(fail("current.slot_lsn"))?)?;
    if confirmed > boundary { return Err(fail("current.slot_ahead_of_capture")); }
    Ok(boundary)
}

fn verify_parts(manifest: &Path, body: &Value) -> Result<(), Fail> {
    let groups = if body["kind"] == "transactions" { vec![body] }
        else { body["tables"].as_array().ok_or(fail("current.resume_parts"))?.iter().collect() };
    let root = manifest.parent().ok_or(fail("current.resume_path"))?.canonicalize().map_err(|_| fail("current.resume_path"))?;
    for group in groups {
        for part in group["parts"].as_array().ok_or(fail("current.resume_parts"))? {
            let name = part["path"].as_str().ok_or(fail("current.resume_parts"))?;
            if Path::new(name).components().count() != 1 { return Err(fail("current.resume_path")); }
            let path = root.join(name).canonicalize().map_err(|_| fail("current.resume_path"))?;
            if path.parent() != Some(root.as_path()) { return Err(fail("current.resume_path")); }
            let mut file = fs::File::open(path).map_err(|_| fail("current.resume_parts"))?;
            if Some(file.metadata().map_err(|_| fail("current.resume_parts"))?.len()) != part["bytes"].as_u64() {
                return Err(fail("current.resume_part_digest"));
            }
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 65536];
            loop {
                let count = file.read(&mut buffer).map_err(|_| fail("current.resume_parts"))?;
                if count == 0 { break; }
                hash.update(&buffer[..count]);
            }
            if Some(hex(&hash.finalize()).as_str()) != part["sha256"].as_str() {
                return Err(fail("current.resume_part_digest"));
            }
        }
    }
    Ok(())
}

async fn transactions(s: &Settings, client: &tokio_postgres::Client,
    observation: &Value, manifest: &mut Value) -> Result<(), Fail> {
    let after = resume(s, observation)?;
    let upper: String = client.query_one("SELECT pg_current_wal_lsn()::text", &[]).await.map_err(sql_error)?.get(0);
    // NULL is an unbounded message count, NOT a total-row cap. RowStream drains
    // complete transactions through the observed WAL ceiling into bounded files.
    let query = "SELECT lsn::text, data FROM pg_logical_slot_peek_binary_changes($1, $2::text::pg_lsn, NULL, \
        'proto_version','1','publication_names',$3)";
    let stream = client.query_raw(query, [&s.capture.slot, &upper, &s.capture.publication])
        .await.map_err(sql_error)?;
    futures_util::pin_mut!(stream);
    let mut relations = HashMap::new();
    let mut relation_messages = BTreeMap::new();
    let approved: BTreeSet<_> = s.approved_objects.iter().map(|o| (o.schema.clone(), o.table.clone())).collect();
    let mut open: Option<(i32, String, u64)> = None;
    let mut spool = Vec::new();
    let mut spool_rows = 0;
    let mut parts = Vec::new();
    let mut scopes = Vec::new();
    let mut counts = [0u64; 3];
    let mut tables = BTreeSet::new();
    let mut last = after;
    let mut selected = false;
    while let Some(row) = stream.try_next().await.map_err(sql_error)? {
        let message_lsn: String = row.get(0);
        let data: Vec<u8> = row.get(1);
        let decoded = decode_message(&data)?;
        match &decoded {
            Msg::Begin { xid, final_lsn } => {
                if open.is_some() { return Err(fail("current.nested_transaction")); }
                selected = *final_lsn >= after;
                open = Some((*xid, message_lsn.clone(), *final_lsn));
                counts = [0; 3]; tables.clear();
            }
            Msg::Relation(rel) => {
                if !approved.contains(&(rel.namespace.clone(), rel.name.clone())) {
                    return Err(fail("current.unapproved_relation"));
                }
                let shape = observation["catalog"].as_array().and_then(|items| items.iter().find(|item|
                    item["schema"] == rel.namespace && item["table"] == rel.name))
                    .ok_or(fail("current.relation_shape"))?;
                let columns = shape["columns"].as_array().ok_or(fail("current.relation_shape"))?;
                if shape["relation_oid"].as_u64() != Some(u64::from(rel.oid))
                    || columns.len() != rel.columns.len()
                    || columns.iter().zip(&rel.columns).any(|(a, b)| a["name"] != b.name || a["oid"] != b.type_oid)
                { return Err(fail("current.schema_drift")); }
                relation_messages.insert(rel.oid, json!({"lsn": message_lsn, "data_hex": hex(&data)}));
                relations.insert(rel.oid, rel.clone());
            }
            Msg::Insert { oid, .. } | Msg::Update { oid, .. } | Msg::Delete { oid, .. } => {
                if open.is_none() { return Err(fail("current.event_outside_transaction")); }
                let rel = relations.get(oid).ok_or(fail("current.unknown_relation"))?;
                tables.insert((rel.namespace.clone(), rel.name.clone()));
                let index = if matches!(decoded, Msg::Insert { .. }) { 0 } else if matches!(decoded, Msg::Update { .. }) { 1 } else { 2 };
                counts[index] += 1;
            }
            Msg::Truncate { .. } => return Err(fail("current.truncate_requires_target_contract")),
            _ => {}
        }
        if selected {
            let line = canonical_bytes(&json!({"lsn": message_lsn, "data_hex": hex(&data)}))?;
            if line.len() > s.chunk_bytes { return Err(fail("current.single_message_exceeds_chunk")); }
            if spool.len() + line.len() > s.chunk_bytes || spool_rows >= s.chunk_rows {
                flush_wal(s, &mut spool, &mut spool_rows, &mut parts)?;
            }
            spool.extend_from_slice(&line);
            spool_rows += 1;
        }
        if let Msg::Commit { commit_lsn, end_lsn, commit_us } = decoded {
            let (xid, begin_lsn, final_lsn) = open.take().ok_or(fail("current.commit_without_begin"))?;
            if end_lsn <= commit_lsn || final_lsn != commit_lsn { return Err(fail("current.commit_boundary")); }
            if selected {
                if end_lsn <= last { return Err(fail("current.nonmonotonic_lsn")); }
                scopes.push(json!({"xid": xid, "begin_message_lsn": begin_lsn,
                    "begin_final_lsn": lsn_pg(final_lsn), "commit_lsn": lsn_pg(commit_lsn),
                    "end_lsn": lsn_pg(end_lsn), "commit_time_pg_us": commit_us,
                    "inserts": counts[0], "updates": counts[1], "deletes": counts[2],
                    "tables": tables, "complete": true}));
                last = end_lsn;
            }
            selected = false;
        }
    }
    if open.is_some() { return Err(fail("current.open_transaction")); }
    flush_wal(s, &mut spool, &mut spool_rows, &mut parts)?;
    unchanged_slot(s, client, observation).await?;
    manifest["kind"] = json!("transactions");
    manifest["after_lsn"] = json!(lsn_pg(after));
    manifest["observed_wal_lsn"] = json!(upper);
    manifest["resume_lsn"] = json!(lsn_pg(last));
    manifest["parts"] = json!(parts);
    manifest["transactions"] = json!(scopes);
    // pgoutput may have sent a relation only in a skipped, already captured
    // transaction. Keep its ORIGINAL bytes so the resumed spool is self-contained.
    manifest["relation_messages"] = json!(relation_messages);
    manifest["encoding"] = json!("original_pgoutput_v1_hex_ndjson");
    manifest["target_contract"] = json!("ordered_pgoutput_parts_required");
    Ok(())
}

async fn unchanged_slot(s: &Settings, client: &tokio_postgres::Client, observation: &Value) -> Result<(), Fail> {
    let row = client.query_opt(
        "SELECT confirmed_flush_lsn::text FROM pg_replication_slots WHERE slot_name=$1 AND NOT active AND invalidation_reason IS NULL",
        &[&s.capture.slot]).await.map_err(sql_error)?.ok_or(fail("current.slot_changed"))?;
    if observation["confirmed_flush_lsn"] != row.get::<_, String>(0) {
        return Err(fail("current.slot_changed"));
    }
    Ok(())
}

fn flush_wal(s: &Settings, spool: &mut Vec<u8>, rows: &mut usize, parts: &mut Vec<Value>) -> Result<(), Fail> {
    if spool.is_empty() { return Ok(()); }
    let name = format!("wal.part-{:08}.ndjson", parts.len());
    save(&s.out_dir.join(&name), spool)?;
    parts.push(json!({"path": name, "sha256": sha256(spool), "bytes": spool.len(), "messages": rows}));
    spool.clear(); *rows = 0;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lsn_is_exact_unsigned_64_bit() {
        assert_eq!(lsn("FFFFFFFF/FFFFFFFF").unwrap(), u64::MAX);
        assert!(lsn("100000000/0").is_err());
        assert!(lsn("0/100000000").is_err());
        assert!(lsn("bogus").is_err());
    }

    #[test]
    fn config_has_no_model_dba_or_target_and_no_total_row_cap() {
        let mut value = json!({
            "protocol": CONFIG_PROTOCOL, "source_id": "owned",
            "source": {"host":"localhost","port":5432,"database":"owned","username":"reader",
                "password_file":"/private/reader","ca_file":"/private/ca"},
            "approved_objects":[{"schema":"owned","table":"items"}],
            "approved_fields":[{"schema":"owned","table":"items","column":"id"}],
            "capture":{"system_identifier":"1","database_oid":1,"slot":"owned_slot","publication":"owned_pub"},
            "chunk_rows":1024,"chunk_bytes":1048576,"statement_ms":120000,"out_dir":"/owned/output"
        });
        serde_json::from_value::<Settings>(value.clone()).unwrap().validate().unwrap();
        value["max_rows"] = json!(100000);
        assert!(serde_json::from_value::<Settings>(value.clone()).is_err());
        value.as_object_mut().unwrap().remove("max_rows");
        value["capture"]["slot"] = json!("foreign'; SELECT 1");
        assert!(serde_json::from_value::<Settings>(value).unwrap().validate().is_err());
    }
}
