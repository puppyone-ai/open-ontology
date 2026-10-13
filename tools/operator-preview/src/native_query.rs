//! Trusted consumer for an admitted catalog the shared executor cannot decode.
//!
//! `query_run` enters this path only after `load_policy_catalog` returns Ok
//! and at least one visible OID is outside bool, int2, int4, int8, text, and
//! numeric. `UnsupportedType` never reaches here. This path decodes the
//! published cells, keeps the guard, and does not cast a column to text.
//! A visible column is consumed only when its physical base type is the
//! sealed pack `data_type` and the admission is `sealed_import`.

use std::path::PathBuf;
use std::time::Duration;

use oo_gateway::{
    connect_require_tls, guard_select, LiteralTypes, PolicyCatalog, PolicyIdentity, TlsConnection,
    TlsEndpoint, VisibleColumn, VisibleRelation,
};
use serde_json::{json, Map, Value};
use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, Error as PgError, IsolationLevel, Row, Transaction};

use crate::authority::{Authority, PolicyFile, QueryFile};
use crate::describe::{refusal, Bound};
use crate::native_types::{cell_len, classify, decode, decodable, stop_before_append};
use crate::native_wal::{begin, complete, AuditMeta, Completion};
use crate::output::{finished_error, shell, Fail, Finished};
use crate::query_run::{cancel_requested, emit, guard_refusal, write_state};
use crate::RESULT_PROTOCOL;

const GENERATION: u64 = 1;

struct ProbeOk {
    catalog: PolicyCatalog,
    backend_pid: i32,
    backend_start: String,
}

enum ProbeErr {
    Pack,
    Policy,
    Query,
    Tls,
    Undefined,
}

enum Stop {
    UserCancel,
    Timeout,
    TypeRefused,
    Database,
    Protocol,
}

struct Drive {
    rows: Vec<Vec<Value>>,
    bytes: u64,
    truncated: bool,
    stop: Option<Stop>,
    sqlstate: Option<String>,
}

pub(crate) async fn run(
    authority: &Authority,
    id: &str,
    bound: &Bound,
    policy: &PolicyFile,
    identity: &PolicyIdentity,
    requested: &QueryFile,
    password: &str,
    ca: &[u8],
    endpoint: &TlsEndpoint,
    mut client: Client,
    connection: TlsConnection,
) -> Finished {
    let driver = tokio::spawn(async move {
        let _ = connection.await;
    });
    let finished = consume(
        authority, id, bound, policy, identity, requested, password, ca, endpoint, &mut client,
    )
    .await;
    drop(client);
    driver.abort();
    let _ = driver.await;
    finished
}

async fn consume(
    authority: &Authority,
    id: &str,
    bound: &Bound,
    policy: &PolicyFile,
    identity: &PolicyIdentity,
    requested: &QueryFile,
    password: &str,
    ca: &[u8],
    endpoint: &TlsEndpoint,
    client: &mut Client,
) -> Finished {
    let ready = match probe(client, authority, bound, policy, identity).await {
        Ok(ready) => ready,
        Err(err) => return probe_finished(authority, id, err),
    };
    let plan = match guard_select(identity, &ready.catalog, &requested.sql) {
        Ok(plan) => plan,
        Err(_) => return guard_refusal(authority, id, bound),
    };
    let Some(quoted) = quote_role(&identity.role) else {
        return finished_error(Fail::Policy);
    };
    let meta = AuditMeta {
        original: plan.original_sha256.to_ascii_lowercase(),
        executed: plan.executed_sha256.to_ascii_lowercase(),
        fingerprint: plan.fingerprint_hex.clone(),
        role: identity.role.clone(),
        project: identity.project.clone(),
        policy: identity.policy_id.clone(),
        backend_pid: ready.backend_pid,
        backend_start: ready.backend_start.clone(),
    };
    let audit = match begin(&authority.audit_dir, authority.audit_max_bytes, &meta) {
        Ok(audit) => audit,
        Err(fail) => return audit_refused(authority, id, bound, fail),
    };
    if write_state(
        authority,
        id,
        "running",
        ready.backend_pid,
        &ready.backend_start,
        GENERATION,
        "none",
        None,
        None,
    )
    .is_err()
    {
        let _ = complete(
            &audit,
            &meta,
            &Completion {
                status: "failed",
                truncated: false,
                returned_rows: 0,
                result_bytes: 0,
                rollback: false,
                socket: false,
                stop: "protocol",
                sqlstate: "",
            },
        );
        return finished_error(Fail::Io);
    }
    let tx = match open_reader(client, &quoted, requested.wall_clock_ms).await {
        Ok(tx) => tx,
        Err((stop, rolled_back)) => {
            return finish(
                authority,
                id,
                bound,
                &audit,
                &meta,
                Drive {
                    rows: Vec::new(),
                    bytes: 0,
                    truncated: false,
                    stop: Some(stop),
                    sqlstate: None,
                },
                rolled_back,
                false,
            );
        }
    };
    let (done_tx, done_rx) = tokio::sync::watch::channel(false);
    let (sent_tx, mut sent_rx) = tokio::sync::oneshot::channel();
    let poller = tokio::spawn(poll_cancel(
        endpoint_clone(endpoint),
        password.to_string(),
        ca.to_vec(),
        authority.state_dir.clone(),
        id.to_string(),
        ready.backend_pid,
        done_rx,
        sent_tx,
    ));
    let (drive, rollback_ok) = fetch(tx, &plan.executed_sql, requested).await;
    let mut cancel_sent = false;
    if matches!(drive.stop, Some(Stop::UserCancel)) || cancel_requested(&authority.state_dir, id) {
        tokio::select! {
            biased;
            got = &mut sent_rx => {
                cancel_sent = matches!(got, Ok(true));
            }
            _ = tokio::time::sleep(Duration::from_millis(500)) => {}
        }
    }
    if matches!(sent_rx.try_recv(), Ok(true)) {
        cancel_sent = true;
    }
    let _ = done_tx.send(true);
    poller.abort();
    let _ = poller.await;
    finish(
        authority,
        id,
        bound,
        &audit,
        &meta,
        drive,
        rollback_ok,
        cancel_sent,
    )
}

fn probe_finished(authority: &Authority, id: &str, err: ProbeErr) -> Finished {
    match err {
        ProbeErr::Undefined => {
            let mut value = refusal(authority, id, "unsupported_type", RESULT_PROTOCOL);
            if let Some(map) = value.as_object_mut() {
                map.insert("type_path".to_string(), json!("native_contract"));
                map.insert("shared_catalog".to_string(), json!("unsupported_type"));
            }
            emit(authority, id, Some(Fail::Policy), &value)
        }
        ProbeErr::Pack => {
            let value = refusal(authority, id, "pack_mismatch", RESULT_PROTOCOL);
            emit(authority, id, Some(Fail::Pack), &value)
        }
        ProbeErr::Policy => finished_error(Fail::Policy),
        ProbeErr::Query => finished_error(Fail::Query),
        ProbeErr::Tls => finished_error(Fail::Tls),
    }
}

fn audit_refused(authority: &Authority, id: &str, bound: &Bound, fail: Fail) -> Finished {
    let mut value = document(authority, id, bound, "refused", None, false, 0, 0, false, false, None, None, None);
    insert_str(&mut value, "class", "audit");
    if let Some(map) = value.as_object_mut() {
        map.remove("compatibility_audit");
    }
    emit(authority, id, Some(fail), &value)
}

async fn probe(
    client: &mut Client,
    authority: &Authority,
    bound: &Bound,
    policy: &PolicyFile,
    identity: &PolicyIdentity,
) -> Result<ProbeOk, ProbeErr> {
    let tx = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await
        .map_err(|_| ProbeErr::Query)?;
    let result = probe_snapshot(&tx, authority, bound, policy, identity).await;
    if tx.rollback().await.is_err() {
        return Err(ProbeErr::Query);
    }
    result
}

async fn probe_snapshot(
    tx: &Transaction<'_>,
    authority: &Authority,
    bound: &Bound,
    policy: &PolicyFile,
    identity: &PolicyIdentity,
) -> Result<ProbeOk, ProbeErr> {
    tx.batch_execute("SET LOCAL search_path TO ''")
        .await
        .map_err(|_| ProbeErr::Query)?;
    let (backend_pid, backend_start) = read_backend(tx).await?;
    ensure_role(tx, &identity.role).await?;
    ensure_generation(tx, &policy.project, &policy.config_digest, policy.generation).await?;
    let namespace = policy.acl.table.namespace.as_str();
    let table = policy.acl.table.name.as_str();
    let physical_table = mapped_name(
        tx,
        &policy.project,
        &policy.config_digest,
        policy.generation,
        "t",
        "table",
        "pt_",
        namespace,
        table,
        "",
    )
    .await?;
    let mut published = Vec::with_capacity(policy.acl.table.columns.len());
    for column in &policy.acl.table.columns {
        let physical = mapped_name(
            tx,
            &policy.project,
            &policy.config_digest,
            policy.generation,
            "c",
            "column",
            "pc_",
            namespace,
            table,
            &column.name,
        )
        .await?;
        published.push((column.name.clone(), physical));
    }
    let relation = relation_oid(tx, &physical_table).await?;
    let attributes = fetch_attributes(tx, relation, authority.limits.max_columns).await?;
    let mut seen_names = std::collections::BTreeSet::new();
    for attribute in &attributes {
        if !seen_names.insert(attribute.name.clone()) {
            return Err(ProbeErr::Pack);
        }
    }
    let dataset = format!("{namespace}.{table}");
    let mut visible = Vec::new();
    for (logical, physical) in &published {
        let attribute = attributes
            .iter()
            .find(|attribute| &attribute.name == physical)
            .ok_or(ProbeErr::Pack)?;
        if classify(&attribute.type_name, attribute.oid, attribute.typmod, &attribute.typtype).is_none() {
            return Err(ProbeErr::Undefined);
        }
        // The catalog may publish the physical base type. The sealed pack
        // still has to name that same type. A json seal on a jsonb column,
        // or a jsonb seal on oid 114, is a fake seal.
        let sealed = sealed_base(bound, &dataset, logical)?;
        if sealed != attribute.type_name {
            return Err(ProbeErr::Pack);
        }
        if column_select(tx, &identity.role, relation, attribute.attnum).await? {
            visible.push((
                attribute.attnum,
                VisibleColumn {
                    name: logical.clone(),
                    physical_name: physical.clone(),
                    type_oid: attribute.oid,
                },
            ));
        }
    }
    visible.sort_by_key(|(attnum, _)| *attnum);
    let pending = u32::try_from(attributes.len() - visible.len()).map_err(|_| ProbeErr::Policy)?;
    let columns: Vec<VisibleColumn> = visible.into_iter().map(|(_, column)| column).collect();
    let relations = if columns.is_empty() {
        Vec::new()
    } else {
        vec![VisibleRelation {
            schema: namespace.to_string(),
            name: table.to_string(),
            physical_schema: "_oo".to_string(),
            physical_name: physical_table,
            columns,
            pending_unclassified: pending,
            estimated_rows: 0,
        }]
    };
    Ok(ProbeOk {
        catalog: PolicyCatalog {
            identity: identity.clone(),
            relations,
            functions: Vec::new(),
            operators: Vec::new(),
            literal_types: LiteralTypes::default(),
            null_test_allowed: true,
            large_table_row_threshold: authority.limits.large_table_row_threshold,
        },
        backend_pid,
        backend_start,
    })
}

fn sealed_base<'a>(bound: &'a Bound, dataset: &str, column: &str) -> Result<&'a str, ProbeErr> {
    let fields = bound
        .draft
        .get("admitted_fields")
        .and_then(Value::as_array)
        .ok_or(ProbeErr::Pack)?;
    let mut found = None;
    for field in fields {
        if field.get("dataset").and_then(Value::as_str) != Some(dataset) {
            continue;
        }
        if field.get("column").and_then(Value::as_str) != Some(column) {
            continue;
        }
        if field.get("admission").and_then(Value::as_str) != Some("sealed_import") {
            return Err(ProbeErr::Pack);
        }
        let data_type = field.get("data_type").and_then(Value::as_str).ok_or(ProbeErr::Pack)?;
        if data_type.is_empty() || found.replace(data_type).is_some() {
            return Err(ProbeErr::Pack);
        }
    }
    found.ok_or(ProbeErr::Pack)
}

struct Attribute {
    attnum: i16,
    name: String,
    oid: u32,
    typmod: i32,
    type_name: String,
    typtype: String,
}

async fn read_backend(tx: &Transaction<'_>) -> Result<(i32, String), ProbeErr> {
    let row = tx
        .query_one(
            "SELECT pg_catalog.pg_backend_pid(), \
             COALESCE(to_char(a.backend_start AT TIME ZONE 'UTC', 'YYYYMMDDHH24MISSUS'), ''), \
             s.ssl \
             FROM pg_catalog.pg_stat_activity AS a \
             JOIN pg_catalog.pg_stat_ssl AS s ON s.pid = a.pid \
             WHERE a.pid = pg_catalog.pg_backend_pid()",
            &[],
        )
        .await
        .map_err(|_| ProbeErr::Query)?;
    let pid: i32 = row.try_get(0).map_err(|_| ProbeErr::Query)?;
    let backend_start: String = row.try_get(1).map_err(|_| ProbeErr::Query)?;
    let ssl: bool = row.try_get(2).map_err(|_| ProbeErr::Tls)?;
    if !ssl {
        return Err(ProbeErr::Tls);
    }
    if pid <= 0 || backend_start.is_empty() || !backend_start.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ProbeErr::Query);
    }
    Ok((pid, backend_start))
}

async fn ensure_role(tx: &Transaction<'_>, role: &str) -> Result<(), ProbeErr> {
    let rows = tx
        .query(
            "SELECT rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, \
                    rolbypassrls, rolreplication \
             FROM pg_catalog.pg_roles WHERE rolname = $1",
            &[&role],
        )
        .await
        .map_err(|_| ProbeErr::Query)?;
    if rows.len() != 1 {
        return Err(ProbeErr::Policy);
    }
    let mut flags = [false; 7];
    for (index, flag) in flags.iter_mut().enumerate() {
        *flag = rows[0].try_get(index).map_err(|_| ProbeErr::Policy)?;
    }
    if flags.iter().any(|flag| *flag) {
        Err(ProbeErr::Policy)
    } else {
        Ok(())
    }
}

async fn ensure_generation(
    tx: &Transaction<'_>,
    source: &str,
    digest: &str,
    generation: i64,
) -> Result<(), ProbeErr> {
    let row = tx
        .query_one(
            "SELECT max(run_generation) FROM _oo.physical_mappings \
             WHERE source = $1 AND config_digest = $2",
            &[&source, &digest],
        )
        .await
        .map_err(|_| ProbeErr::Query)?;
    let current: Option<i64> = row.try_get(0).map_err(|_| ProbeErr::Query)?;
    if current == Some(generation) {
        Ok(())
    } else {
        Err(ProbeErr::Pack)
    }
}

async fn mapped_name(
    tx: &Transaction<'_>,
    source: &str,
    digest: &str,
    generation: i64,
    letter: &str,
    word: &str,
    prefix: &str,
    namespace: &str,
    table: &str,
    column: &str,
) -> Result<String, ProbeErr> {
    let count_row = tx
        .query_one(
            "SELECT count(*)::int8 FROM _oo.physical_mappings \
             WHERE source = $1 AND config_digest = $2 AND run_generation = $3 \
               AND level = $4 AND logical_namespace = $5 AND logical_table = $6 \
               AND logical_column = $7",
            &[&source, &digest, &generation, &letter, &namespace, &table, &column],
        )
        .await
        .map_err(|_| ProbeErr::Query)?;
    let count: i64 = count_row.try_get(0).map_err(|_| ProbeErr::Query)?;
    if count != 1 {
        return Err(ProbeErr::Pack);
    }
    let stored_row = tx
        .query_one(
            "SELECT physical_name FROM _oo.physical_mappings \
             WHERE source = $1 AND config_digest = $2 AND run_generation = $3 \
               AND level = $4 AND logical_namespace = $5 AND logical_table = $6 \
               AND logical_column = $7",
            &[&source, &digest, &generation, &letter, &namespace, &table, &column],
        )
        .await
        .map_err(|_| ProbeErr::Query)?;
    let stored: String = stored_row.try_get(0).map_err(|_| ProbeErr::Query)?;
    let looked_row = tx
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &[&source, &digest, &generation, &word, &namespace, &table, &column],
        )
        .await
        .map_err(|_| ProbeErr::Query)?;
    let looked: String = looked_row.try_get(0).map_err(|_| ProbeErr::Query)?;
    if stored != looked || !physical_ok(&stored, prefix) {
        return Err(ProbeErr::Pack);
    }
    Ok(stored)
}

fn physical_ok(value: &str, prefix: &str) -> bool {
    let Some(rest) = value.strip_prefix(prefix) else {
        return false;
    };
    value.len() == 63
        && rest.len() == 60
        && rest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

async fn relation_oid(tx: &Transaction<'_>, physical: &str) -> Result<i32, ProbeErr> {
    let count_row = tx
        .query_one(
            "SELECT count(*)::int8 \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = $1 AND relation.relname = $2 AND relation.relkind = 'r'",
            &[&"_oo", &physical],
        )
        .await
        .map_err(|_| ProbeErr::Query)?;
    let count: i64 = count_row.try_get(0).map_err(|_| ProbeErr::Query)?;
    if count != 1 {
        return Err(ProbeErr::Pack);
    }
    let row = tx
        .query_one(
            "SELECT relation.oid::int4 \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = $1 AND relation.relname = $2 AND relation.relkind = 'r'",
            &[&"_oo", &physical],
        )
        .await
        .map_err(|_| ProbeErr::Query)?;
    row.try_get(0).map_err(|_| ProbeErr::Query)
}

async fn fetch_attributes(tx: &Transaction<'_>, relation: i32, max_columns: u32) -> Result<Vec<Attribute>, ProbeErr> {
    let count_row = tx
        .query_one(
            "SELECT count(*)::int8 FROM pg_catalog.pg_attribute AS attribute \
             WHERE attribute.attrelid = ($1::int4)::oid \
               AND attribute.attnum > 0 \
               AND NOT attribute.attisdropped",
            &[&relation],
        )
        .await
        .map_err(|_| ProbeErr::Query)?;
    let count: i64 = count_row.try_get(0).map_err(|_| ProbeErr::Query)?;
    if count < 0 || u32::try_from(count).ok().is_none_or(|value| value > max_columns) {
        return Err(ProbeErr::Policy);
    }
    let rows = tx
        .query(
            "SELECT attribute.attnum::int2, attribute.attname::text, attribute.atttypid::int4, \
                    attribute.atttypmod::int4, type_row.typname::text, type_row.typtype::text \
             FROM pg_catalog.pg_attribute AS attribute \
             JOIN pg_catalog.pg_type AS type_row ON type_row.oid = attribute.atttypid \
             WHERE attribute.attrelid = ($1::int4)::oid \
               AND attribute.attnum > 0 \
               AND NOT attribute.attisdropped",
            &[&relation],
        )
        .await
        .map_err(|_| ProbeErr::Query)?;
    if i64::try_from(rows.len()).ok() != Some(count) {
        return Err(ProbeErr::Policy);
    }
    let mut attributes = Vec::with_capacity(rows.len());
    for row in rows {
        let oid_raw: i32 = row.try_get(2).map_err(|_| ProbeErr::Query)?;
        attributes.push(Attribute {
            attnum: row.try_get(0).map_err(|_| ProbeErr::Query)?,
            name: row.try_get(1).map_err(|_| ProbeErr::Query)?,
            oid: u32::try_from(oid_raw).unwrap_or(0),
            typmod: row.try_get(3).map_err(|_| ProbeErr::Query)?,
            type_name: row.try_get(4).map_err(|_| ProbeErr::Query)?,
            typtype: row.try_get(5).map_err(|_| ProbeErr::Query)?,
        });
    }
    Ok(attributes)
}

async fn column_select(tx: &Transaction<'_>, role: &str, relation: i32, attnum: i16) -> Result<bool, ProbeErr> {
    let row = tx
        .query_one(
            "SELECT pg_catalog.has_column_privilege(($1::text)::name, ($2::int4)::oid, $3::int2, 'SELECT')",
            &[&role, &relation, &attnum],
        )
        .await
        .map_err(|_| ProbeErr::Query)?;
    let allowed: Option<bool> = row.try_get(0).map_err(|_| ProbeErr::Query)?;
    Ok(allowed == Some(true))
}

async fn open_reader<'a>(
    client: &'a mut Client,
    quoted_role: &str,
    wall_ms: u64,
) -> Result<Transaction<'a>, (Stop, bool)> {
    let tx = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await
        .map_err(|_| (Stop::Database, false))?;
    let opened = apply_reader(&tx, quoted_role, wall_ms).await;
    if let Err(stop) = opened {
        let rolled_back = tx.rollback().await.is_ok();
        return Err((stop, rolled_back));
    }
    Ok(tx)
}

async fn apply_reader(tx: &Transaction<'_>, quoted_role: &str, wall_ms: u64) -> Result<(), Stop> {
    tx.batch_execute("SET LOCAL search_path TO ''")
        .await
        .map_err(|_| Stop::Database)?;
    tx.batch_execute(&format!("SET LOCAL ROLE {quoted_role}"))
        .await
        .map_err(|_| Stop::Database)?;
    tx.batch_execute(&format!("SET LOCAL statement_timeout = {wall_ms}"))
        .await
        .map_err(|_| Stop::Database)?;
    let settings = tx
        .query_one(
            "SELECT current_setting('transaction_isolation'), current_setting('transaction_read_only'), \
             current_user::text, current_setting('search_path')",
            &[],
        )
        .await
        .map_err(|_| Stop::Protocol)?;
    let timeout = tx
        .query_one(
            "SELECT setting FROM pg_catalog.pg_settings WHERE name = 'statement_timeout'",
            &[],
        )
        .await
        .map_err(|_| Stop::Protocol)?;
    let isolation: String = settings.try_get(0).map_err(|_| Stop::Protocol)?;
    let read_only: String = settings.try_get(1).map_err(|_| Stop::Protocol)?;
    let current_user: String = settings.try_get(2).map_err(|_| Stop::Protocol)?;
    let search_path: String = settings.try_get(3).map_err(|_| Stop::Protocol)?;
    let timeout_setting: String = timeout.try_get(0).map_err(|_| Stop::Protocol)?;
    if isolation != "repeatable read" || read_only != "on" || !search_path_empty(&search_path) {
        return Err(Stop::Protocol);
    }
    if current_user != quoted_role.trim_matches('"') || timeout_setting != wall_ms.to_string() {
        return Err(Stop::Protocol);
    }
    Ok(())
}

fn search_path_empty(value: &str) -> bool {
    matches!(value, "" | "\"\"")
}

fn quote_role(role: &str) -> Option<String> {
    let slug = role.strip_prefix("oo_r_")?;
    let bytes = slug.as_bytes();
    if bytes.is_empty() || bytes.len() > 58 || !bytes[0].is_ascii_lowercase() {
        return None;
    }
    if !bytes[1..]
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
    {
        return None;
    }
    if role.contains('"') {
        return None;
    }
    Some(format!("\"{role}\""))
}

async fn fetch(tx: Transaction<'_>, sql: &str, requested: &QueryFile) -> (Drive, bool) {
    let drive = match prepare_and_fetch(&tx, sql, requested).await {
        Ok(drive) => drive,
        Err(err) => classify_drive(err),
    };
    let rollback_ok = tx.rollback().await.is_ok();
    (drive, rollback_ok)
}

async fn prepare_and_fetch(tx: &Transaction<'_>, sql: &str, requested: &QueryFile) -> Result<Drive, PgError> {
    let statement = tx.prepare(sql).await?;
    if !statement.params().is_empty() {
        return Ok(Drive {
            rows: Vec::new(),
            bytes: 0,
            truncated: false,
            stop: Some(Stop::Protocol),
            sqlstate: None,
        });
    }
    for column in statement.columns() {
        if !decodable(column.type_().oid()) {
            return Ok(Drive {
                rows: Vec::new(),
                bytes: 0,
                truncated: false,
                stop: Some(Stop::TypeRefused),
                sqlstate: None,
            });
        }
    }
    let params: [&(dyn ToSql + Sync); 0] = [];
    let portal = tx.bind(&statement, &params).await?;
    let mut rows = Vec::new();
    let mut used = 0_u64;
    loop {
        let page = tx.query_portal(&portal, 1).await?;
        if page.is_empty() {
            break;
        }
        for row in page {
            let cells = match decode_row(&row) {
                Ok(cells) => cells,
                Err(()) => {
                    return Ok(Drive {
                        rows: Vec::new(),
                        bytes: 0,
                        truncated: false,
                        stop: Some(Stop::TypeRefused),
                        sqlstate: None,
                    });
                }
            };
            let row_bytes = cells.iter().fold(0_u64, |sum, cell| sum.saturating_add(cell_len(cell)));
            if stop_before_append(
                u32::try_from(rows.len()).unwrap_or(u32::MAX),
                requested.max_rows,
                used,
                row_bytes,
                requested.max_result_bytes,
            ) {
                return Ok(Drive {
                    rows,
                    bytes: used,
                    truncated: true,
                    stop: None,
                    sqlstate: None,
                });
            }
            used = used.saturating_add(row_bytes);
            rows.push(cells);
        }
    }
    Ok(Drive {
        rows,
        bytes: used,
        truncated: false,
        stop: None,
        sqlstate: None,
    })
}

fn decode_row(row: &Row) -> Result<Vec<Value>, ()> {
    let mut cells = Vec::with_capacity(row.len());
    for index in 0..row.len() {
        let oid = row.columns()[index].type_().oid();
        let raw: RawCell = row.try_get(index).map_err(|_| ())?;
        let cell = decode(oid, raw.0.as_deref()).map_err(|_| ())?;
        cells.push(cell);
    }
    Ok(cells)
}

struct RawCell(Option<Vec<u8>>);

impl<'a> tokio_postgres::types::FromSql<'a> for RawCell {
    fn from_sql(
        _ty: &tokio_postgres::types::Type,
        raw: &'a [u8],
    ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Self(Some(raw.to_vec())))
    }

    fn from_sql_null(
        _ty: &tokio_postgres::types::Type,
    ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Self(None))
    }

    fn accepts(_ty: &tokio_postgres::types::Type) -> bool {
        true
    }
}

fn classify_drive(err: PgError) -> Drive {
    let (stop, sqlstate) = classify_pg(&err);
    Drive {
        rows: Vec::new(),
        bytes: 0,
        truncated: false,
        stop: Some(stop),
        sqlstate,
    }
}

fn classify_pg(err: &PgError) -> (Stop, Option<String>) {
    let Some(db) = err.as_db_error() else {
        return (Stop::Database, None);
    };
    let state = db.code().code();
    let sqlstate = if state.len() == 5
        && state.bytes().all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        Some(state.to_string())
    } else {
        None
    };
    if state == "57014" && db.message().starts_with("canceling statement due to user request") {
        (Stop::UserCancel, sqlstate)
    } else if state == "57014" {
        (Stop::Timeout, sqlstate)
    } else {
        (Stop::Database, sqlstate)
    }
}

#[allow(clippy::too_many_arguments)]
fn finish(
    authority: &Authority,
    id: &str,
    bound: &Bound,
    audit: &crate::native_wal::OpenAudit,
    meta: &AuditMeta,
    drive: Drive,
    rollback_ok: bool,
    cancel_sent: bool,
) -> Finished {
    let user_canceled = cancel_sent && matches!(drive.stop, Some(Stop::UserCancel)) && rollback_ok;
    let success = drive.stop.is_none() && rollback_ok && !user_canceled;
    let (wal_status, wal_stop, outcome, class, fail) = if user_canceled {
        ("failed", "canceled", "canceled", Some("canceled"), None)
    } else if success {
        ("succeeded", "", "succeeded", None, None)
    } else if !rollback_ok {
        ("failed", "database", "failed", Some("query"), Some(Fail::Query))
    } else {
        match drive.stop {
            Some(Stop::Timeout) => ("failed", "timeout", "timeout", Some("timeout"), Some(Fail::Query)),
            Some(Stop::TypeRefused) => ("failed", "type_refused", "failed", Some("type_refused"), Some(Fail::Query)),
            Some(Stop::Protocol) => ("failed", "protocol", "failed", Some("query"), Some(Fail::Query)),
            Some(Stop::UserCancel | Stop::Database) | None => {
                ("failed", "database", "failed", Some("query"), Some(Fail::Query))
            }
        }
    };
    let returned = if success {
        u32::try_from(drive.rows.len()).unwrap_or(u32::MAX)
    } else {
        0
    };
    let bytes = if success { drive.bytes } else { 0 };
    let sqlstate = drive.sqlstate.as_deref().filter(|value| {
        value.len() == 5 && value.bytes().all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    });
    let wal_sqlstate = if wal_stop.is_empty() { "" } else { sqlstate.unwrap_or("") };
    let audit_ok = complete(
        audit,
        meta,
        &Completion {
            status: wal_status,
            truncated: success && drive.truncated,
            returned_rows: returned,
            result_bytes: bytes,
            rollback: rollback_ok,
            socket: false,
            stop: wal_stop,
            sqlstate: wal_sqlstate,
        },
    )
    .is_ok();
    let (outcome, class, fail, show_rows) = if !audit_ok {
        ("refused", Some("audit"), Some(Fail::Audit), false)
    } else {
        (outcome, class, fail, success)
    };
    let cancel_field = if user_canceled && audit_ok {
        "sent"
    } else if cancel_sent {
        "not_sent"
    } else {
        "none"
    };
    let state_status = if outcome == "canceled" {
        "canceled"
    } else if outcome == "succeeded" {
        "finished"
    } else {
        "failed"
    };
    if write_state(
        authority,
        id,
        state_status,
        meta.backend_pid,
        &meta.backend_start,
        GENERATION,
        cancel_field,
        if outcome == "canceled" || outcome == "timeout" {
            sqlstate
        } else {
            None
        },
        Some(rollback_ok),
    )
    .is_err()
    {
        return finished_error(Fail::Io);
    }
    let rows = if show_rows { Some(drive.rows.as_slice()) } else { None };
    let mut value = document(
        authority,
        id,
        bound,
        outcome,
        Some(meta.backend_pid),
        success && drive.truncated,
        returned,
        bytes,
        rollback_ok,
        audit_ok,
        if outcome == "canceled" || outcome == "timeout" { sqlstate } else { None },
        if wal_stop.is_empty() || !audit_ok { None } else { Some(wal_stop) },
        rows,
    );
    if let Some(class) = class {
        insert_str(&mut value, "class", class);
    }
    if matches!(outcome, "succeeded" | "canceled") {
        insert_u64(&mut value, "generation", GENERATION);
    }
    emit(authority, id, fail, &value)
}

#[allow(clippy::too_many_arguments)]
fn document(
    authority: &Authority,
    id: &str,
    bound: &Bound,
    outcome: &str,
    backend_pid: Option<i32>,
    truncated: bool,
    returned_rows: u32,
    result_bytes: u64,
    rollback: bool,
    end_audit_ok: bool,
    sqlstate: Option<&str>,
    stop: Option<&str>,
    rows: Option<&[Vec<Value>]>,
) -> Value {
    let mut fields = Map::new();
    fields.insert("protocol".to_string(), json!(RESULT_PROTOCOL));
    fields.insert("execution_id".to_string(), json!(id));
    fields.insert("outcome".to_string(), json!(outcome));
    fields.insert("import".to_string(), crate::describe::import_value(&authority.import));
    fields.insert("knowledge".to_string(), bound.knowledge.clone());
    fields.insert("policy".to_string(), bound.policy.clone());
    fields.insert("draft_pack".to_string(), bound.draft.clone());
    fields.insert("type_path".to_string(), json!("native_contract"));
    fields.insert("shared_catalog".to_string(), json!("admitted"));
    fields.insert("compatibility_audit".to_string(), json!("inspect_segment"));
    if let Some(pid) = backend_pid {
        fields.insert("backend_pid".to_string(), json!(pid));
    }
    fields.insert("truncated".to_string(), json!(truncated));
    fields.insert("returned_rows".to_string(), json!(returned_rows));
    fields.insert("result_bytes".to_string(), json!(result_bytes));
    fields.insert("rollback_confirmed".to_string(), json!(rollback));
    fields.insert("end_audit_ok".to_string(), json!(end_audit_ok));
    if let Some(sqlstate) = sqlstate {
        fields.insert("sqlstate".to_string(), json!(sqlstate));
    }
    if let Some(stop) = stop {
        fields.insert("stop".to_string(), json!(stop));
    }
    if let Some(rows) = rows {
        fields.insert(
            "rows".to_string(),
            Value::Array(rows.iter().map(|row| Value::Array(row.clone())).collect()),
        );
    }
    shell(fields)
}

fn insert_str(value: &mut Value, key: &str, text: &str) {
    if let Some(map) = value.as_object_mut() {
        map.insert(key.to_string(), json!(text));
    }
}

fn insert_u64(value: &mut Value, key: &str, number: u64) {
    if let Some(map) = value.as_object_mut() {
        map.insert(key.to_string(), json!(number));
    }
}

fn endpoint_clone(endpoint: &TlsEndpoint) -> TlsEndpoint {
    TlsEndpoint {
        host: endpoint.host.clone(),
        port: endpoint.port,
        database: endpoint.database.clone(),
        user: endpoint.user.clone(),
    }
}

async fn poll_cancel(
    endpoint: TlsEndpoint,
    password: String,
    ca: Vec<u8>,
    state_dir: PathBuf,
    id: String,
    pid: i32,
    mut done: tokio::sync::watch::Receiver<bool>,
    sent: tokio::sync::oneshot::Sender<bool>,
) {
    loop {
        tokio::select! {
            biased;
            changed = done.changed() => {
                if changed.is_err() || *done.borrow() {
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                if cancel_requested(&state_dir, &id) {
                    let ok = cancel_backend(&endpoint, &password, &ca, pid).await.unwrap_or(false);
                    let _ = sent.send(ok);
                    break;
                }
            }
        }
    }
}

async fn cancel_backend(endpoint: &TlsEndpoint, password: &str, ca: &[u8], pid: i32) -> Result<bool, Fail> {
    let (client, connection) = connect_require_tls(endpoint, password, ca).await.map_err(|_| Fail::Tls)?;
    let driver = tokio::spawn(async move {
        let _ = connection.await;
    });
    let result = client
        .query_one("SELECT pg_catalog.pg_cancel_backend($1)", &[&pid])
        .await;
    drop(client);
    driver.abort();
    let _ = driver.await;
    let row = result.map_err(|_| Fail::Query)?;
    row.try_get(0).map_err(|_| Fail::Query)
}

#[cfg(test)]
mod tests {
    use super::quote_role;

    #[test]
    fn reader_role_is_quoted_and_other_shapes_are_not() {
        assert_eq!(quote_role("oo_r_reader").as_deref(), Some("\"oo_r_reader\""));
        assert!(quote_role("oo_preview_login").is_none());
        assert!(quote_role("oo_r_").is_none());
    }
}
