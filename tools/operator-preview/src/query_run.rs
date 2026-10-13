//! One guarded select on the bound target. User SQL is not executed raw.

use std::fs;
use std::path::Path;
use std::time::Duration;

use gateway_audit_wal::{DiskWal, WalConfig};
use oo_gateway::policy::catalog::{load_policy_catalog, CatalogError};
use oo_gateway::query_executor::{
    CancelHandle, CancelOutcome, Cell, CompletionStatus, DedicatedSession, ExecError, QuerySummary,
    SelectRequest, StopClass,
};
use oo_gateway::{
    compile_column_acl, connect_require_tls, rustls_connector, PolicyIdentity, TlsEndpoint,
};
use serde_json::{json, Map, Value};

use crate::authority::{load_authority, load_policy, load_query, Authority};
use crate::describe::{bind_preview, import_value, refusal, Bound};
use crate::durable::{execution_id, write_durable};
use crate::output::{finished_error, finished_value, rows_value, shell, Fail, Finished};
use crate::RESULT_PROTOCOL;

const CANCEL_REQUEST_PROTOCOL: &str = "oo.operator-preview.cancel-request/v1";
const EXECUTION_PROTOCOL: &str = "oo.operator-preview.execution/v1";

pub fn query(authority_path: &Path, query_path: &Path) -> Finished {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return finished_error(Fail::Io),
    };
    runtime.block_on(query_inner(authority_path, query_path))
}

async fn query_inner(authority_path: &Path, query_path: &Path) -> Finished {
    let id = match execution_id() {
        Ok(id) => id,
        Err(fail) => return finished_error(fail),
    };
    let authority = match load_authority(authority_path) {
        Ok(authority) => authority,
        Err(fail) => return finished_error(fail),
    };
    let requested = match load_query(query_path) {
        Ok(requested) => requested,
        Err(fail) => return finished_error(fail),
    };
    let bound = match bind_preview(&authority) {
        Ok(bound) => bound,
        Err(Fail::Pack) => {
            let value = refusal(&authority, &id, "pack_mismatch", RESULT_PROTOCOL);
            return emit(&authority, &id, Some(Fail::Pack), &value);
        }
        Err(fail) => return finished_error(fail),
    };
    let policy = match load_policy(&authority.policy_path) {
        Ok(policy) => policy,
        Err(fail) => return finished_error(fail),
    };
    let compiled = match compile_column_acl(&policy.acl) {
        Ok(compiled) => compiled,
        Err(err) => return finished_error(crate::authority::policy_compile_error(err)),
    };
    let identity = PolicyIdentity {
        project: authority.project.clone(),
        role: authority.role.clone(),
        policy_id: policy.config_digest.clone(),
    };
    let password = match read_secret(&authority.password_path) {
        Ok(password) => password,
        Err(fail) => return finished_error(fail),
    };
    let ca = match read_ca(&authority.ca_path) {
        Ok(ca) => ca,
        Err(fail) => return finished_error(fail),
    };
    let endpoint = TlsEndpoint {
        host: authority.host.clone(),
        port: authority.port,
        database: authority.database.clone(),
        user: authority.user.clone(),
    };
    let tls = match rustls_connector(&ca) {
        Ok(tls) => tls,
        Err(err) => return finished_error(crate::authority::policy_compile_error(err)),
    };
    let (mut client, mut connection) = match connect_require_tls(&endpoint, &password, &ca).await {
        Ok(pair) => pair,
        Err(_) => return finished_error(Fail::Tls),
    };
    // Drop the loader future before moving the client. A catalog the shared
    // executor cannot decode goes to the native consumer. UnsupportedType is
    // refused here, before a WAL segment is opened.
    let loaded = {
        let mut load = std::pin::pin!(load_policy_catalog(
            &mut client,
            &identity,
            std::slice::from_ref(&compiled),
            &authority.limits,
        ));
        tokio::select! {
            biased;
            result = &mut load => result,
            _ = &mut connection => return finished_error(Fail::Tls),
        }
    };
    let catalog = match loaded {
        Err(
            CatalogError::MappingAbsent
            | CatalogError::AmbiguousMapping
            | CatalogError::StaleGeneration,
        ) => {
            let value = refusal(&authority, &id, "pack_mismatch", RESULT_PROTOCOL);
            return emit(&authority, &id, Some(Fail::Pack), &value);
        }
        Ok(catalog) if !shared_executor_decodes(&catalog) => {
            return crate::native_query::run(
                &authority,
                &id,
                &bound,
                &policy,
                &identity,
                &requested,
                &password,
                &ca,
                &endpoint,
                client,
                connection,
            )
            .await;
        }
        Ok(catalog) => catalog,
        Err(CatalogError::UnsupportedType) => {
            let mut value = refusal(&authority, &id, "unsupported_type", RESULT_PROTOCOL);
            if let Some(map) = value.as_object_mut() {
                map.insert("type_path".to_string(), json!("native_contract"));
                map.insert("shared_catalog".to_string(), json!("unsupported_type"));
            }
            return emit(&authority, &id, Some(Fail::Policy), &value);
        }
        Err(_) => return finished_error(Fail::Policy),
    };
    let wal = match DiskWal::open(
        &authority.audit_dir,
        WalConfig {
            max_bytes: authority.audit_max_bytes,
        },
    ) {
        Ok(wal) => wal,
        Err(_) => {
            let mut value = document(&authority, &id, "refused", Some(&bound), None, None);
            insert_str(&mut value, "class", "audit");
            return emit(&authority, &id, Some(Fail::Audit), &value);
        }
    };
    let mut session = match DedicatedSession::bind(client, connection, tls).await {
        Ok(session) => session,
        Err(_) => return finished_error(Fail::Query),
    };
    let request = SelectRequest {
        identity: &identity,
        catalog: &catalog,
        original_sql: &requested.sql,
        role: &identity.role,
        max_rows: requested.max_rows,
        max_result_bytes: requested.max_result_bytes,
        wall_clock: Duration::from_millis(requested.wall_clock_ms),
    };
    let mut stream = match session.execute_trusted_select(request, wal).await {
        Ok(stream) => stream,
        Err(ExecError::Guard(_)) => return guard_refusal(&authority, &id, &bound),
        Err(ExecError::AuditUnavailable) => {
            let mut value = document(&authority, &id, "refused", Some(&bound), None, None);
            insert_str(&mut value, "class", "audit");
            return emit(&authority, &id, Some(Fail::Audit), &value);
        }
        Err(_) => {
            let mut value = document(&authority, &id, "failed", Some(&bound), None, None);
            insert_str(&mut value, "class", "query");
            return emit(&authority, &id, Some(Fail::Query), &value);
        }
    };
    let generation = session.generation();
    let backend_pid = session.backend_pid();
    let backend_start = session.backend_start();
    let handle = stream.cancel_handle();
    if let Err(fail) = write_state(
        &authority,
        &id,
        "running",
        backend_pid,
        &backend_start,
        generation,
        "none",
        None,
        None,
    ) {
        drop(stream);
        let _ = session.wait_idle().await;
        return finished_error(fail);
    }
    let mut rows = Vec::new();
    let mut stream_error = None;
    let mut cancel_mark = "none";
    loop {
        tokio::select! {
            biased;
            row = stream.next_row() => {
                match row {
                    Ok(Some(row)) => rows.push(row),
                    Ok(None) => break,
                    Err(err) => {
                        stream_error = Some(err);
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)), if cancel_mark == "none" => {
                if cancel_requested(&authority.state_dir, &id) {
                    cancel_mark = match cancel_observed(&endpoint, &password, &ca, &handle).await {
                        Ok(CancelOutcome::Sent) => "sent",
                        _ => "not_sent",
                    };
                }
            }
        }
    }
    drop(stream);
    let _ = session.wait_idle().await;
    if cancel_mark == "none" && cancel_requested(&authority.state_dir, &id) {
        cancel_mark = "not_sent";
    }
    let Some(summary) = session.summary() else {
        let _ = write_state(
            &authority,
            &id,
            "failed",
            backend_pid,
            &backend_start,
            generation,
            cancel_mark,
            None,
            None,
        );
        let mut value = document(&authority, &id, "failed", Some(&bound), None, None);
        insert_str(&mut value, "class", "query");
        return emit(&authority, &id, Some(Fail::Query), &value);
    };
    finish_stream(
        &authority,
        &id,
        &bound,
        &summary,
        &rows,
        stream_error.as_ref(),
        cancel_mark,
        backend_pid,
        &backend_start,
        generation,
    )
}

fn finish_stream(
    authority: &Authority,
    id: &str,
    bound: &Bound,
    summary: &QuerySummary,
    rows: &[Vec<Cell>],
    stream_error: Option<&ExecError>,
    cancel_mark: &str,
    backend_pid: i32,
    backend_start: &str,
    generation: u64,
) -> Finished {
    let sqlstate = summary
        .sqlstate()
        .map(str::to_string)
        .or_else(|| stream_error.and_then(ExecError::sqlstate).map(str::to_string));
    let canceled = cancel_mark == "sent"
        && (matches!(stream_error, Some(ExecError::Canceled { .. }))
            || summary.stop() == Some(StopClass::Canceled));
    let status = if canceled {
        "canceled"
    } else if summary.status() == CompletionStatus::Succeeded && stream_error.is_none() {
        "finished"
    } else {
        "failed"
    };
    if write_state(
        authority,
        id,
        status,
        backend_pid,
        backend_start,
        generation,
        if canceled {
            "sent"
        } else if cancel_mark == "sent" {
            "not_sent"
        } else {
            cancel_mark
        },
        sqlstate.as_deref(),
        Some(summary.rollback_confirmed()),
    )
    .is_err()
    {
        return finished_error(Fail::Io);
    }
    if !summary.end_audit_ok() {
        let mut value = document(authority, id, "refused", Some(bound), Some(summary), None);
        insert_str(&mut value, "class", "audit");
        return emit(authority, id, Some(Fail::Audit), &value);
    }
    if canceled {
        let mut value = document(authority, id, "canceled", Some(bound), Some(summary), None);
        insert_str(&mut value, "class", "canceled");
        insert_u64(&mut value, "generation", generation);
        return emit(authority, id, None, &value);
    }
    if summary.status() == CompletionStatus::Succeeded && stream_error.is_none() {
        let value = document(
            authority,
            id,
            "succeeded",
            Some(bound),
            Some(summary),
            Some(rows),
        );
        return emit(authority, id, None, &value);
    }
    let class = match stream_error {
        Some(ExecError::Timeout { .. }) => "timeout",
        Some(ExecError::TypeRefused { .. }) => "type_refused",
        Some(ExecError::Canceled { .. }) => "canceled",
        _ => match summary.stop() {
            Some(StopClass::Timeout) => "timeout",
            Some(StopClass::TypeRefused) => "type_refused",
            Some(StopClass::Canceled) => "canceled",
            _ => "query",
        },
    };
    let outcome = if class == "timeout" { "timeout" } else { "failed" };
    let mut value = document(authority, id, outcome, Some(bound), Some(summary), None);
    insert_str(&mut value, "class", class);
    emit(authority, id, Some(Fail::Query), &value)
}

fn shared_executor_decodes(catalog: &oo_gateway::PolicyCatalog) -> bool {
    catalog.relations.iter().all(|relation| {
        relation.columns.iter().all(|column| {
            matches!(column.type_oid, 16 | 20 | 21 | 23 | 25 | 1700)
        })
    })
}

pub(crate) fn guard_refusal(authority: &Authority, id: &str, bound: &Bound) -> Finished {
    let recorded = match DiskWal::open(
        &authority.audit_dir,
        WalConfig {
            max_bytes: authority.audit_max_bytes,
        },
    ) {
        Ok(mut wal) => wal
            .record_not_admitted(&authority.project, &authority.role)
            .is_ok(),
        Err(_) => false,
    };
    if !recorded {
        let mut value = document(authority, id, "refused", Some(bound), None, None);
        insert_str(&mut value, "class", "audit");
        return emit(authority, id, Some(Fail::Audit), &value);
    }
    let mut value = document(authority, id, "refused", Some(bound), None, None);
    insert_str(&mut value, "class", "guard");
    emit(authority, id, Some(Fail::Guard), &value)
}

async fn cancel_observed(
    endpoint: &TlsEndpoint,
    password: &str,
    ca: &[u8],
    handle: &CancelHandle,
) -> Result<CancelOutcome, Fail> {
    let (observer, connection) = connect_require_tls(endpoint, password, ca)
        .await
        .map_err(|_| Fail::Tls)?;
    let driver = tokio::spawn(async move {
        let _ = connection.await;
    });
    let outcome = handle.cancel(&observer).await.map_err(|_| Fail::Query);
    drop(observer);
    driver.abort();
    outcome
}

pub(crate) fn cancel_requested(state_dir: &Path, id: &str) -> bool {
    let path = state_dir.join("cancel").join(format!("{id}.request"));
    let Ok(meta) = fs::symlink_metadata(&path) else {
        return false;
    };
    if !meta.file_type().is_file() || meta.len() == 0 || meta.len() > 4096 {
        return false;
    }
    let Ok(bytes) = fs::read(&path) else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    value.get("protocol").and_then(Value::as_str) == Some(CANCEL_REQUEST_PROTOCOL)
        && value.get("execution_id").and_then(Value::as_str) == Some(id)
}

pub(crate) fn write_state(
    authority: &Authority,
    id: &str,
    status: &str,
    backend_pid: i32,
    backend_start: &str,
    generation: u64,
    cancel: &str,
    sqlstate: Option<&str>,
    rollback_confirmed: Option<bool>,
) -> Result<(), Fail> {
    let mut body = Map::new();
    body.insert("protocol".to_string(), json!(EXECUTION_PROTOCOL));
    body.insert("execution_id".to_string(), json!(id));
    body.insert("status".to_string(), json!(status));
    body.insert("backend_pid".to_string(), json!(backend_pid));
    body.insert("backend_start".to_string(), json!(backend_start));
    body.insert("generation".to_string(), json!(generation));
    body.insert("cancel".to_string(), json!(cancel));
    if let Some(sqlstate) = sqlstate {
        body.insert("sqlstate".to_string(), json!(sqlstate));
    }
    if let Some(rollback) = rollback_confirmed {
        body.insert("rollback_confirmed".to_string(), json!(rollback));
    }
    let bytes = serde_json::to_vec(&Value::Object(body)).map_err(|_| Fail::Io)?;
    write_durable(
        &authority.state_dir.join("executions"),
        &format!("{id}.json"),
        &bytes,
    )
}

fn document(
    authority: &Authority,
    id: &str,
    outcome: &str,
    bound: Option<&Bound>,
    summary: Option<&QuerySummary>,
    rows: Option<&[Vec<Cell>]>,
) -> Value {
    let mut fields = Map::new();
    fields.insert("protocol".to_string(), json!(RESULT_PROTOCOL));
    fields.insert("execution_id".to_string(), json!(id));
    fields.insert("outcome".to_string(), json!(outcome));
    fields.insert("import".to_string(), import_value(&authority.import));
    if let Some(bound) = bound {
        fields.insert("knowledge".to_string(), bound.knowledge.clone());
        fields.insert("policy".to_string(), bound.policy.clone());
        fields.insert("draft_pack".to_string(), bound.draft.clone());
    }
    if let Some(summary) = summary {
        fields.insert("truncated".to_string(), json!(summary.truncated()));
        fields.insert("returned_rows".to_string(), json!(summary.returned_rows()));
        fields.insert("result_bytes".to_string(), json!(summary.result_bytes()));
        fields.insert(
            "rollback_confirmed".to_string(),
            json!(summary.rollback_confirmed()),
        );
        fields.insert("end_audit_ok".to_string(), json!(summary.end_audit_ok()));
        if let Some(sqlstate) = summary.sqlstate() {
            fields.insert("sqlstate".to_string(), json!(sqlstate));
        }
        if let Some(stop) = summary.stop() {
            fields.insert("stop".to_string(), json!(stop_name(stop)));
        }
    }
    if let Some(rows) = rows {
        fields.insert("rows".to_string(), rows_value(rows));
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

fn stop_name(stop: StopClass) -> &'static str {
    match stop {
        StopClass::Timeout => "timeout",
        StopClass::Canceled => "canceled",
        StopClass::TypeRefused => "type_refused",
        StopClass::Database => "database",
        StopClass::Protocol => "protocol",
    }
}

pub(crate) fn emit(authority: &Authority, id: &str, fail: Option<Fail>, value: &Value) -> Finished {
    let bytes = match serde_json::to_vec_pretty(value) {
        Ok(bytes) => bytes,
        Err(_) => return finished_error(Fail::Io),
    };
    if write_durable(&authority.receipt_dir, &format!("{id}.json"), &bytes).is_err() {
        return finished_error(Fail::Io);
    }
    finished_value(fail, value)
}

fn read_secret(path: &Path) -> Result<String, Fail> {
    let bytes = crate::durable::read_capped(path, 4096)?;
    let mut text = std::str::from_utf8(&bytes)
        .map_err(|_| Fail::Request)?
        .to_string();
    while text.ends_with('\n') || text.ends_with('\r') {
        text.pop();
    }
    if text.is_empty() || text.contains('\0') {
        return Err(Fail::Request);
    }
    Ok(text)
}

fn read_ca(path: &Path) -> Result<Vec<u8>, Fail> {
    crate::durable::read_capped(path, 65_536)
}
