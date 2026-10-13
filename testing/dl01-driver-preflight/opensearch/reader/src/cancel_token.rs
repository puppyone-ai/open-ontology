//! Explicit user cancel of one owned OpenSearch search.
//!
//! OpenSearch 3.9.0 `RestCancellableNodeClient.cancelTask` registers the search
//! on its own HTTP channel. When that channel closes, the server cancels that
//! task as the system `TASKS_ORIGIN` with reason `channel closed`. That is not
//! `cancel_after_time_interval` and it is not `cluster:admin/tasks/cancel`.
//! `QueryPhase` then throws `cancelled task with reason: ` plus that reason.
//! A closed socket alone is never proof. An independently captured server log
//! can bind the native reason to the observed task/node/index and cancel window.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use zeroize::Zeroize;

use super::http::{redact, Audit, Identities, Api};

pub struct CancelJob {
    pub index: String,
    pub marker: String,
    pub forbidden_marker: String,
    pub script: String,
    pub rejected_timer: String,
}

pub struct SearchOutcome {
    pub status: u16,
    pub body: String,
    pub transport_error: Option<String>,
    pub client_closed: bool,
    pub natural_before_trigger: bool,
    pub completed_unix_ms: Option<u64>,
    pub user_triggered_unix_ms: Option<u64>,
    pub elapsed_ms: u128,
}

pub fn business_search_path(index: &str) -> String {
    format!("/{index}/_search?request_cache=false")
}

pub fn classify_user_cancel(input: &ClassInput) -> &'static str {
    if input.natural_before_trigger || !input.user_triggered {
        return "late_or_absent";
    }
    let reason = input.terminal_reason.to_ascii_lowercase();
    let channel = (reason.contains("channel closed") || reason.contains("channel was closed")) && !reason.contains("expired");
    let proved = channel
        && ((input.reason_source == "owned_http_response" && input.server_status == 500)
            || input.reason_source == "bound_server_log")
        && !input.before_cancelled
        && input.after_cancelled
        && input.same_task_id
        && !input.task_id_present_after
        && input.marker_only
        && input.running_nanos > 0
        && !input.cancel_after_sent
        && !input.dba_cancel_used
        ;
    if proved { "user_cancel" } else { "unsupported" }
}

#[allow(clippy::struct_excessive_bools)]
pub struct ClassInput {
    pub natural_before_trigger: bool,
    pub user_triggered: bool,
    pub terminal_reason: String,
    pub server_status: u16,
    pub before_cancelled: bool,
    pub after_cancelled: bool,
    pub same_task_id: bool,
    pub task_id_present_after: bool,
    pub marker_only: bool,
    pub running_nanos: u64,
    pub cancel_after_sent: bool,
    pub dba_cancel_used: bool,
    pub reason_source: &'static str,
}

/// Accept only the shard failure of the exact observed node/index.
/// OpenSearch can write the QueryPhase exception several seconds after
/// `cancellation_time_millis`. Fifteen seconds still rejects an expired
/// deadline reason and a line from before the user trigger.
fn bound_log_line<'a>(raw: &'a str, task_id: &str, index: &str, trigger: u64, cancelled: u64) -> Option<&'a str> {
    if trigger == 0 || cancelled < trigger || cancelled - trigger > 5_000 { return None; }
    let (node, _) = task_id.split_once(':')?;
    if node.is_empty() || index.is_empty() { return None; }
    let shard = format!("[{node}][{index}][");
    let low = format_unix_ms(trigger);
    let high = format_unix_ms(cancelled + 15_000);
    raw.lines().find(|line| {
        let stamp = line.split_whitespace().next().unwrap_or("");
        // Docker RFC3339 timestamps have nanoseconds; compare millisecond prefix.
        let ms = stamp.get(..23).unwrap_or("");
        !ms.is_empty() && ms >= &low[..23] && ms <= &high[..23]
            && line.contains(&shard)
            && line.contains("TaskCancelledException[cancelled task with reason: channel closed]")
            && !line.to_ascii_lowercase().contains("expired")
    })
}

pub fn reason_from_http(raw: &str) -> String {
    let body = http_body(raw);
    if let Some(reason) = json_reason(&body) {
        return reason;
    }
    quoted_reason(raw)
}

fn json_reason(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let error = value.get("error").cloned().unwrap_or(value);
    walk_reason(Some(&error))
}

fn quoted_reason(raw: &str) -> String {
    let needle = "cancelled task with reason:";
    let Some(start) = raw.find(needle) else {
        return String::new();
    };
    let rest = &raw[start..];
    let end = rest.find(['"', '\\']).unwrap_or(rest.len());
    rest[..end].trim().to_string()
}

fn walk_reason(node: Option<&Value>) -> Option<String> {
    let node = node?;
    if node.get("type").and_then(Value::as_str) == Some("task_cancelled_exception") {
        return node.get("reason").and_then(Value::as_str).map(str::to_string);
    }
    if let Some(found) = walk_reason(node.get("caused_by")) {
        return Some(found);
    }
    for key in ["failed_shards", "root_cause"] {
        if let Some(items) = node.get(key).and_then(Value::as_array) {
            for item in items {
                if let Some(found) = walk_reason(item.get("reason").or(Some(item))) {
                    return Some(found);
                }
            }
        }
    }
    None
}

pub fn unix_ms_now() -> (String, u64) {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    (format_unix_ms(ms), ms)
}

pub fn format_unix_ms(ms: u64) -> String {
    let secs = ms / 1000;
    let millis = ms % 1000;
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let hour = tod / 3600;
    let minute = (tod % 3600) / 60;
    let second = tod % 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

fn civil_from_days(mut z: i64) -> (i32, u32, u32) {
    z += 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    (year as i32, month as u32, day as u32)
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity, clippy::collapsible_if)]
pub async fn execute_user_cancel(
    ident: &Identities,
    dba: &mut Api,
    reader: &mut Api,
    job: &CancelJob,
    out: &Path,
) -> Result<u8, String> {
    let path = business_search_path(&job.index);
    if path.contains("cancel_after_time_interval") || path.contains(&job.rejected_timer) {
        return Err("user cancel path includes a server timer".into());
    }
    let (priv_status, priv_raw) = reader
        .call("POST", "/_tasks/_cancel?actions=*search", Some(b"{}".to_vec()), "application/json", "denied_probe")
        .await?;
    let (open_before_status, open_before) = http_open(dba).await?;
    let (workers_before_status, workers_before) = search_active(dba).await?;
    let (pit_status, pit_raw) = reader
        .call(
            "POST",
            &format!("/{}/_search/point_in_time?keep_alive=2m", job.index),
            None,
            "application/json",
            "cursor_open",
        )
        .await?;
    let pit_json: Value = serde_json::from_slice(&pit_raw).unwrap_or(Value::Null);
    let pit_id = pit_json.get("pit_id").and_then(Value::as_str).unwrap_or("").to_string();
    let body = workload_query(&job.script)?;
    let body_len = body.len();
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let params = OwnedParams {
        endpoint: ident.endpoint.clone(),
        ca_pem: ident.ca_pem.clone(),
        user: ident.reader_user.clone(),
        password: ident.reader_password.clone(),
        path: path.clone(),
        body,
        host_header: host_header(&ident.endpoint)?,
    };
    let search = tokio::spawn(owned_search(params, cancel_rx, None));
    let seen = poll_marked(dba, &job.marker, &job.forbidden_marker, 400).await?;
    let Some(seen) = seen else {
        drop(cancel_tx);
        let outcome = join_search(search).await;
        write_pending(out, &path, priv_status, &priv_raw, &outcome, "running task was not observed")?;
        close_pit(reader, &pit_id).await?;
        return Ok(2);
    };
    let task_id = text_of(&seen.task, "task_id");
    let start_ms = seen.task.get("start_time_in_millis").and_then(Value::as_u64).unwrap_or(0);
    if task_id.is_empty() || start_ms == 0 {
        return Err("running task has no id or start time".into());
    }
    let seen = match hold_running(dba, &task_id, &job.marker, &job.forbidden_marker, start_ms).await? {
        Some(still) => still,
        None => {
            drop(cancel_tx);
            let outcome = join_search(search).await;
            write_pending(out, &path, priv_status, &priv_raw, &outcome, "task ended before the held user cancel")?;
            close_pit(reader, &pit_id).await?;
            return Ok(2);
        }
    };
    let (during_status, during_open) = http_open(dba).await?;
    let (workers_during_status, workers_during) = search_active(dba).await?;
    let held_status = if pit_id.is_empty() { 0 } else { pit_held(reader, &pit_id).await? };
    let task_id = text_of(&seen.task, "task_id");
    let other_ids = seen.other_ids.clone();
    if cancel_tx.send(()).is_err() {
        return Err("owned search dropped the cancel channel".into());
    }
    let after = poll_cancelled(dba, &task_id, &job.marker, 800).await?;
    let outcome = join_search(search).await;
    let mut final_state = task_present(dba, &task_id).await?;
    for _ in 0..200 {
        if final_state.0 == 200 && !final_state.1 { break; }
        tokio::time::sleep(Duration::from_millis(25)).await;
        final_state = task_present(dba, &task_id).await?;
    }
    let (after_list_status, still_listed) = final_state;
    let close_status = if pit_id.is_empty() { 0 } else { reader.delete_pit(&pit_id).await? };
    let reuse_status = if pit_id.is_empty() { 0 } else { pit_held(reader, &pit_id).await? };
    let (mut open_after_status, mut open_after) = http_open(dba).await?;
    let (mut workers_after_status, mut workers_after) = search_active(dba).await?;
    let settle = Instant::now() + Duration::from_secs(3);
    while workers_after >= workers_during && Instant::now() < settle {
        tokio::time::sleep(Duration::from_millis(50)).await;
        (workers_after_status, workers_after) = search_active(dba).await?;
        (open_after_status, open_after) = http_open(dba).await?;
    }
    let (mut reason, mut reason_source) = if outcome.body.contains("cancelled task with reason:") || outcome.status == 500 {
        let found = reason_from_http(&outcome.body);
        if found.is_empty() { (String::new(), "not_in_response") } else { (found, "owned_http_response") }
    } else {
        (String::new(), "not_in_response")
    };
    let error = server_error(&http_body(&outcome.body));
    let running_nanos = seen.task.get("running_time_in_nanos").and_then(Value::as_u64).unwrap_or(0);
    let before_cancelled = seen.task.get("cancelled").and_then(Value::as_bool).unwrap_or(true);
    let after_task = after.clone().unwrap_or(Value::Null);
    let after_cancelled = after_task.get("cancelled").and_then(Value::as_bool).unwrap_or(false);
    let after_id = text_of(&after_task, "task_id");
    let description = seen.task.get("description").and_then(Value::as_str).unwrap_or("");
    let marker_only = description.contains(&job.marker) && !description.contains(&job.forbidden_marker);
    let mut server_log_binding = Value::Null;
    if reason.is_empty() && other_ids.is_empty() {
        if let (Some(trigger), Some(log_path)) = (outcome.user_triggered_unix_ms, std::env::var_os("OO_OS_SERVER_LOG")) {
            let cancelled = after_task.get("cancellation_time_millis").and_then(Value::as_u64).unwrap_or(0);
            for _ in 0..600 {
                let raw = std::fs::read_to_string(&log_path).unwrap_or_default();
                if let Some(line) = bound_log_line(&raw, &task_id, &job.index, trigger, cancelled) {
                    reason = "cancelled task with reason: channel closed".to_string();
                    reason_source = "bound_server_log";
                    server_log_binding = json!({"line": line, "task_id": task_id, "index": job.index,
                        "marker": job.marker, "trigger_unix_ms": trigger, "cancelled_unix_ms": cancelled,
                        "other_business_search_tasks": other_ids});
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    }
    let class = classify_user_cancel(&ClassInput {
        natural_before_trigger: outcome.natural_before_trigger,
        user_triggered: outcome.user_triggered_unix_ms.is_some(),
        terminal_reason: reason.clone(),
        server_status: outcome.status,
        before_cancelled,
        after_cancelled,
        same_task_id: !task_id.is_empty() && task_id == after_id,
        task_id_present_after: still_listed,
        marker_only,
        running_nanos,
        cancel_after_sent: false,
        dba_cancel_used: false,
        reason_source,
    });
    let proved = class == "user_cancel"
        && during_status == 200
        && workers_during_status == 200
        && workers_before_status == 200
        && workers_after_status == 200
        && workers_during >= 1
        && workers_after < workers_during
        && open_before_status == 200
        && open_after_status == 200
        && held_status == 200
        && pit_status == 200
        && close_status == 200
        && reuse_status >= 400
        && after_list_status == 200;
    let (triggered_text, triggered_ms) = match outcome.user_triggered_unix_ms {
        Some(ms) => (format_unix_ms(ms), ms),
        None => (String::new(), 0),
    };
    push_search_audit(reader, &path, outcome.status, body_len, outcome.body.len(), outcome.transport_error.as_deref(), "user_cancel");
    let mut secrets = ident.secrets();
    let transport_error = outcome.transport_error.as_ref().map(|text| redact(text, &secrets));
    for secret in &mut secrets {
        secret.zeroize();
    }
    let document = json!({
        "outcome": if proved { "proved" } else { "pending" },
        "class": class,
        "mechanism": "RestCancellableNodeClient http channel close",
        "labeled_as_timeout": false,
        "client_closed": outcome.client_closed,
        "search_path": path,
        "cancel_after_time_interval_sent": false,
        "rejected_server_timer": job.rejected_timer,
        "response_framing": response_framing(&outcome.body),
        "owned_response_excerpt": truncate_n(&outcome.body, 1500),
        "transport_error": transport_error,
        "privilege": "reader search on the approved workload index; cluster task cancel is not granted",
        "workload_index": job.index,
        "marker": job.marker,
        "deadline_marker": job.forbidden_marker,
        "task_cancel_http_status": priv_status,
        "task_cancel_body": truncate(&String::from_utf8_lossy(&priv_raw)),
        "task": seen.task,
        "task_before": seen.task,
        "task_after": after_task,
        "other_search_task_ids_before": other_ids,
        "targeted_task_ids": [task_id],
        "server_error": error,
        "terminal_reason": reason,
        "terminal_reason_source": reason_source,
        "server_log_binding": server_log_binding,
        "server_status": outcome.status,
        "elapsed_ms": outcome.elapsed_ms,
        "natural_completion_before_trigger": outcome.natural_before_trigger,
        "user_triggered_at": triggered_text,
        "user_triggered_unix_ms": triggered_ms,
        "task_id_present_after": still_listed,
        "task_list_after_status": after_list_status,
        "http_current_open_before": open_before,
        "http_current_open_before_status": open_before_status,
        "http_current_open_during": during_open,
        "http_current_open_during_status": during_status,
        "http_current_open_after": open_after,
        "http_current_open_after_status": open_after_status,
        "search_workers_before": workers_before,
        "search_workers_before_status": workers_before_status,
        "search_workers_during": workers_during,
        "search_workers_during_status": workers_during_status,
        "search_workers_after": workers_after,
        "search_workers_after_status": workers_after_status,
        "pit_open_status": pit_status,
        "pit_held_status": held_status,
        "pit_close_status": close_status,
        "pit_reuse_status": reuse_status,
        "ipc_present": false,
        "dba_cancel_used": false,
        "owned_client": "dedicated-http1-channel",
    });
    write_json(&out.join("cancel.json"), &document)?;
    let _ = std::fs::remove_file(out.join("batch.arrow"));
    Ok(if proved { 0 } else { 2 })
}

pub async fn execute_late_cancel(ident: &Identities, dba: &mut Api, reader: &mut Api, job: &CancelJob, out: &Path) -> Result<u8, String> {
    let path = business_search_path(&job.index);
    if path.contains("cancel_after_time_interval") || path.contains(&job.rejected_timer) {
        return Err("late cancel path includes a server timer".into());
    }
    let body = workload_query(&job.script)?;
    let body_len = body.len();
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let params = OwnedParams {
        endpoint: ident.endpoint.clone(),
        ca_pem: ident.ca_pem.clone(),
        user: ident.reader_user.clone(),
        password: ident.reader_password.clone(),
        path: path.clone(),
        body,
        host_header: host_header(&ident.endpoint)?,
    };
    let search = tokio::spawn(owned_search(params, cancel_rx, Some(done_tx)));
    let completed = match tokio::time::timeout(Duration::from_secs(20), done_rx).await {
        Ok(Ok(status)) => status,
        _ => {
            drop(cancel_tx);
            let _ = join_search(search).await;
            return Ok(2);
        }
    };
    let (list_status, present) = marker_present(dba, &job.marker).await?;
    let (signaled_text, signaled_ms) = unix_ms_now();
    if cancel_tx.send(()).is_err() {
        return Err("late cancel channel dropped".into());
    }
    let outcome = join_search(search).await;
    let triggered_ms = outcome.user_triggered_unix_ms.unwrap_or(0);
    let completed_ms = outcome.completed_unix_ms.unwrap_or(0);
    let late = completed == 200
        && outcome.natural_before_trigger
        && !present
        && list_status == 200
        && triggered_ms >= completed_ms
        && triggered_ms > 0
        && completed_ms > 0;
    push_search_audit(reader, &path, outcome.status, body_len, outcome.body.len(), outcome.transport_error.as_deref(), "late_cancel");
    write_json(
        &out.join("late.json"),
        &json!({
            "class": if late { "late" } else { "not_late" },
            "counted_as_user_cancel": false,
            "natural_completion_before_trigger": outcome.natural_before_trigger,
            "server_status_before_trigger": completed,
            "task_id_present_at_trigger": present,
            "task_list_status": list_status,
            "completed_unix_ms": completed_ms,
            "completed_at": outcome.completed_unix_ms.map(format_unix_ms).unwrap_or_default(),
            "user_triggered_unix_ms": triggered_ms,
            "user_triggered_at": if triggered_ms == 0 { String::new() } else { format_unix_ms(triggered_ms) },
            "supervisor_signaled_at": signaled_text,
            "supervisor_signaled_unix_ms": signaled_ms,
            "marker": job.marker,
            "search_path": path,
            "cancel_after_time_interval_sent": false,
            "rejected_server_timer": job.rejected_timer,
            "dba_cancel_used": false,
        }),
    )?;
    Ok(if late { 0 } else { 2 })
}

struct OwnedParams {
    endpoint: String,
    ca_pem: Vec<u8>,
    user: String,
    password: String,
    path: String,
    body: Vec<u8>,
    host_header: String,
}

impl Drop for OwnedParams {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn owned_search(
    mut params: OwnedParams,
    mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
    completed_tx: Option<tokio::sync::oneshot::Sender<u16>>,
) -> SearchOutcome {
    let started = Instant::now();
    let mut outcome = SearchOutcome {
        status: 0,
        body: String::new(),
        transport_error: None,
        client_closed: false,
        natural_before_trigger: false,
        completed_unix_ms: None,
        user_triggered_unix_ms: None,
        elapsed_ms: 0,
    };
    let connect = connect_owned(&params).await;
    let mut tls = match connect {
        Ok(tls) => tls,
        Err(error) => {
            outcome.transport_error = Some(error);
            outcome.elapsed_ms = started.elapsed().as_millis();
            return outcome;
        }
    };
    let mut request = build_request(&params);
    params.password.zeroize();
    if let Err(error) = tls.write_all(&request).await {
        request.zeroize();
        outcome.transport_error = Some(error.to_string());
        outcome.elapsed_ms = started.elapsed().as_millis();
        return outcome;
    }
    request.zeroize();
    if let Err(error) = tls.flush().await {
        outcome.transport_error = Some(error.to_string());
        outcome.elapsed_ms = started.elapsed().as_millis();
        return outcome;
    }
    let mut raw = Vec::new();
    let mut tmp = [0_u8; 8192];
    let mut triggered = false;
    loop {
        if http_complete(&raw) || started.elapsed() > Duration::from_secs(45) {
            break;
        }
        tokio::select! {
            biased;
            signal = &mut cancel_rx, if !triggered => {
                if signal.is_ok() {
                    let (_, ms) = unix_ms_now();
                    outcome.user_triggered_unix_ms = Some(ms);
                    outcome.client_closed = true;
                }
                triggered = true;
                if let Err(error) = tls.shutdown().await {
                    outcome.transport_error = Some(error.to_string());
                }
            }
            read = tls.read(&mut tmp) => {
                match read {
                    Ok(0) => break,
                    Ok(n) => raw.extend_from_slice(&tmp[..n]),
                    Err(error) => {
                        outcome.transport_error = Some(error.to_string());
                        break;
                    }
                }
            }
        }
        if triggered {
            break;
        }
    }
    if outcome.user_triggered_unix_ms.is_some() {
        let deadline = Instant::now() + Duration::from_secs(25);
        while !http_complete(&raw) && Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_secs(2), tls.read(&mut tmp)).await {
                Ok(Ok(0)) => break,
                Ok(Ok(n)) => raw.extend_from_slice(&tmp[..n]),
                Ok(Err(error)) => {
                    outcome.transport_error = Some(error.to_string());
                    break;
                }
                Err(_) => {}
            }
        }
    }
    outcome.body = String::from_utf8_lossy(&raw).to_string();
    outcome.status = http_status(&outcome.body);
    outcome.elapsed_ms = started.elapsed().as_millis();
    if !triggered && outcome.status != 0 {
        outcome.natural_before_trigger = true;
        let (_, ms) = unix_ms_now();
        outcome.completed_unix_ms = Some(ms);
        if let Some(tx) = completed_tx {
            let _ = tx.send(outcome.status);
            if let Ok(Ok(())) = tokio::time::timeout(Duration::from_secs(5), cancel_rx).await {
                let (_, ms) = unix_ms_now();
                outcome.user_triggered_unix_ms = Some(ms);
                outcome.client_closed = true;
                let _ = tls.shutdown().await;
            }
        }
    }
    outcome
}

async fn connect_owned(params: &OwnedParams) -> Result<tokio_rustls::client::TlsStream<TcpStream>, String> {
    let (host, port) = parse_endpoint(&params.endpoint)?;
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();
    use tokio_rustls::rustls::pki_types::pem::PemObject;
    let der = tokio_rustls::rustls::pki_types::CertificateDer::from_pem_slice(&params.ca_pem).map_err(|error| format!("ca:{error}"))?;
    let mut roots = tokio_rustls::rustls::RootCertStore::empty();
    roots.add(der).map_err(|error| format!("ca:{error}"))?;
    let provider = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider();
    let config = tokio_rustls::rustls::ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("tls:{error}"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = tokio_rustls::rustls::pki_types::ServerName::try_from(host.clone()).map_err(|error| format!("sni:{error}"))?;
    let tcp = TcpStream::connect((host, port)).await.map_err(|error| format!("tcp:{error}"))?;
    tcp.set_nodelay(true).map_err(|error| format!("nodelay:{error}"))?;
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    connector.connect(name, tcp).await.map_err(|error| format!("handshake:{error}"))
}

fn build_request(params: &OwnedParams) -> Vec<u8> {
    let mut raw_token = format!("{}:{}", params.user, params.password);
    let mut token = base64::engine::general_purpose::STANDARD.encode(&raw_token);
    raw_token.zeroize();
    let head = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Basic {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\nUser-Agent: oo-dl01w1-44-cancel-token\r\n\r\n",
        params.path,
        params.host_header,
        token,
        params.body.len()
    );
    token.zeroize();
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(&params.body);
    bytes
}

fn parse_endpoint(endpoint: &str) -> Result<(String, u16), String> {
    let rest = endpoint.strip_prefix("https://").ok_or("endpoint scheme")?;
    let (host, port) = rest.rsplit_once(':').ok_or("endpoint port")?;
    let host = host.trim_matches(['[', ']']).to_string();
    if host.is_empty() {
        return Err("endpoint host".into());
    }
    let port = port.parse::<u16>().map_err(|_| "endpoint port".to_string())?;
    Ok((host, port))
}

fn host_header(endpoint: &str) -> Result<String, String> {
    let (host, port) = parse_endpoint(endpoint)?;
    Ok(format!("{host}:{port}"))
}

fn workload_query(script: &str) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&json!({
        "size": 0,
        "query": {"script": {"script": {"lang": "painless", "source": script}}}
    }))
    .map_err(|error| error.to_string())
}

fn http_status(raw: &str) -> u16 {
    let line = raw.lines().next().unwrap_or("");
    line.split_whitespace().nth(1).and_then(|code| code.parse().ok()).unwrap_or(0)
}

fn header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|window| window == b"\r\n\r\n")
}

fn http_body(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let Some(index) = header_end(bytes) else {
        return raw.trim().to_string();
    };
    let head = &raw[..index];
    let body = &bytes[index + 4..];
    if transfer_chunked(head) {
        return decode_chunks(body);
    }
    if let Some(len) = content_length(head) {
        let end = len.min(body.len());
        return String::from_utf8_lossy(&body[..end]).into_owned();
    }
    String::from_utf8_lossy(body).trim().to_string()
}

fn http_complete(raw: &[u8]) -> bool {
    let Some(index) = header_end(raw) else {
        return false;
    };
    let head = String::from_utf8_lossy(&raw[..index]);
    let body = &raw[index + 4..];
    if transfer_chunked(&head) {
        return chunked_complete(body);
    }
    if let Some(len) = content_length(&head) {
        return body.len() >= len;
    }
    false
}

fn transfer_chunked(head: &str) -> bool {
    head.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("transfer-encoding") && value.to_ascii_lowercase().contains("chunked")
        })
    })
}

fn response_framing(raw: &str) -> &'static str {
    let head = raw.split("\r\n\r\n").next().unwrap_or(raw);
    if raw.is_empty() {
        "empty"
    } else if transfer_chunked(head) {
        "chunked"
    } else if content_length(head).is_some() {
        "content-length"
    } else {
        "unknown"
    }
}

fn content_length(head: &str) -> Option<usize> {
    for line in head.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().ok();
        }
    }
    None
}

fn chunked_complete(body: &[u8]) -> bool {
    let mut index = 0;
    while index < body.len() {
        let Some(relative) = body[index..].windows(2).position(|window| window == b"\r\n") else {
            return false;
        };
        let line_end = index + relative;
        let line = std::str::from_utf8(&body[index..line_end]).unwrap_or("");
        let size_hex = line.split(';').next().unwrap_or("").trim();
        let Ok(size) = usize::from_str_radix(size_hex, 16) else {
            return false;
        };
        let next = line_end + 2;
        if size == 0 {
            return body.get(next..next + 2) == Some(b"\r\n") || body[next..].windows(4).any(|window| window == b"\r\n\r\n");
        }
        let end = next + size;
        if body.len() < end + 2 || body.get(end..end + 2) != Some(b"\r\n") {
            return false;
        }
        index = end + 2;
    }
    false
}

fn decode_chunks(body: &[u8]) -> String {
    let mut out = Vec::new();
    let mut index = 0;
    while index < body.len() {
        let Some(relative) = body[index..].windows(2).position(|window| window == b"\r\n") else {
            break;
        };
        let line_end = index + relative;
        let line = std::str::from_utf8(&body[index..line_end]).unwrap_or("");
        let size_hex = line.split(';').next().unwrap_or("").trim();
        let Ok(size) = usize::from_str_radix(size_hex, 16) else {
            break;
        };
        let next = line_end + 2;
        if size == 0 {
            break;
        }
        let end = next + size;
        if body.len() < end {
            break;
        }
        out.extend_from_slice(&body[next..end]);
        index = end + 2;
    }
    String::from_utf8_lossy(&out).into_owned()
}

struct Marked {
    task: Value,
    other_ids: Vec<String>,
}

async fn poll_marked(api: &mut Api, marker: &str, forbidden: &str, rounds: u32) -> Result<Option<Marked>, String> {
    for _ in 0..rounds {
        let value = task_list(api).await?;
        let found = marked_tasks(&value, marker);
        if found.len() > 1 {
            return Err("marker matched more than one search".into());
        }
        if let Some(task) = found.into_iter().next() {
            let description = task.get("description").and_then(Value::as_str).unwrap_or("");
            if description.contains(forbidden) {
                return Err("running task description also contains the other marker".into());
            }
            if task.get("cancelled").and_then(Value::as_bool) == Some(true) {
                tokio::time::sleep(Duration::from_millis(15)).await;
                continue;
            }
            return Ok(Some(Marked { other_ids: other_search_ids(&value, &text_of(&task, "task_id")), task }));
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    Ok(None)
}

/// Keep the owned search running until 300ms after its server start.
/// The snapshot returned is still uncancelled. `None` means the task left
/// the list before the user trigger, so this call must not be labeled cancel.
async fn hold_running(
    api: &mut Api,
    task_id: &str,
    marker: &str,
    forbidden: &str,
    start_ms: u64,
) -> Result<Option<Marked>, String> {
    let until = start_ms.saturating_add(300);
    loop {
        let value = task_list(api).await?;
        let Some(task) = task_by_id(&value, task_id) else {
            return Ok(None);
        };
        if task.get("cancelled").and_then(Value::as_bool) == Some(true) {
            return Err("task was cancelled before the user trigger".into());
        }
        let description = task.get("description").and_then(Value::as_str).unwrap_or("");
        if !description.contains(marker) || description.contains(forbidden) {
            return Err("held task marker changed".into());
        }
        if unix_ms_now().1 >= until {
            return Ok(Some(Marked { other_ids: other_search_ids(&value, task_id), task }));
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

async fn poll_cancelled(api: &mut Api, task_id: &str, marker: &str, rounds: u32) -> Result<Option<Value>, String> {
    for _ in 0..rounds {
        let value = task_list(api).await?;
        if let Some(task) = task_by_id(&value, task_id) {
            let description = task.get("description").and_then(Value::as_str).unwrap_or("");
            if task.get("cancelled").and_then(Value::as_bool) == Some(true) && description.contains(marker) {
                return Ok(Some(task));
            }
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    Ok(None)
}

async fn task_present(api: &mut Api, task_id: &str) -> Result<(u16, bool), String> {
    let (status, value) = task_list_status(api).await?;
    Ok((status, task_by_id(&value, task_id).is_some()))
}

async fn marker_present(api: &mut Api, marker: &str) -> Result<(u16, bool), String> {
    let (status, value) = task_list_status(api).await?;
    Ok((status, !marked_tasks(&value, marker).is_empty()))
}

async fn task_list(api: &mut Api) -> Result<Value, String> {
    let (status, value) = task_list_status(api).await?;
    if status != 200 {
        return Err(format!("task_list:{status}"));
    }
    Ok(value)
}

async fn task_list_status(api: &mut Api) -> Result<(u16, Value), String> {
    let (status, raw) = api
        .call("GET", "/_tasks?actions=*search*&detailed=true", None, "application/json", "control_stats")
        .await?;
    let value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    Ok((status, value))
}

fn marked_tasks(body: &Value, marker: &str) -> Vec<Value> {
    let mut found = Vec::new();
    let Some(nodes) = body.get("nodes").and_then(Value::as_object) else {
        return found;
    };
    for node in nodes.values() {
        let Some(tasks) = node.get("tasks").and_then(Value::as_object) else {
            continue;
        };
        for (key, task) in tasks {
            let description = task.get("description").and_then(Value::as_str).unwrap_or("");
            let action = task.get("action").and_then(Value::as_str).unwrap_or("");
            if description.contains(marker) && action.contains("indices:data/read/search") && !action.contains('[') {
                let mut saved = task.clone();
                if let Some(map) = saved.as_object_mut() {
                    map.insert("task_id".to_string(), Value::String(key.clone()));
                }
                found.push(saved);
            }
        }
    }
    found
}

fn other_search_ids(body: &Value, own_id: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let Some(nodes) = body.get("nodes").and_then(Value::as_object) else {
        return ids;
    };
    for node in nodes.values() {
        let Some(tasks) = node.get("tasks").and_then(Value::as_object) else {
            continue;
        };
        for (key, task) in tasks {
            let action = task.get("action").and_then(Value::as_str).unwrap_or("");
            if action.contains("indices:data/read/search") && !action.contains('[') && key != own_id {
                ids.push(key.clone());
            }
        }
    }
    ids.sort();
    ids
}

fn task_by_id(body: &Value, task_id: &str) -> Option<Value> {
    let nodes = body.get("nodes")?.as_object()?;
    for node in nodes.values() {
        let tasks = node.get("tasks")?.as_object()?;
        if let Some(task) = tasks.get(task_id) {
            let mut saved = task.clone();
            if let Some(map) = saved.as_object_mut() {
                map.insert("task_id".to_string(), Value::String(task_id.to_string()));
            }
            return Some(saved);
        }
    }
    None
}

async fn http_open(api: &mut Api) -> Result<(u16, i64), String> {
    sum_stat(api, "/_nodes/stats/http", "/http/current_open").await
}

async fn search_active(api: &mut Api) -> Result<(u16, i64), String> {
    sum_stat(api, "/_nodes/stats/thread_pool", "/thread_pool/search/active").await
}

async fn sum_stat(api: &mut Api, path: &str, pointer: &str) -> Result<(u16, i64), String> {
    let (status, raw) = api.call("GET", path, None, "application/json", "control_stats").await?;
    let value: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    let mut total = 0_i64;
    if let Some(nodes) = value.pointer("/nodes").and_then(Value::as_object) {
        for node in nodes.values() {
            total += node.pointer(pointer).and_then(Value::as_i64).unwrap_or(0);
        }
    }
    Ok((status, total))
}

async fn pit_held(reader: &mut Api, pit_id: &str) -> Result<u16, String> {
    let body = serde_json::to_vec(&json!({"size": 1, "pit": {"id": pit_id, "keep_alive": "1m"}})).map_err(|error| error.to_string())?;
    let (status, _) = reader.call("POST", "/_search", Some(body), "application/json", "read").await?;
    Ok(status)
}

async fn close_pit(reader: &mut Api, pit_id: &str) -> Result<(), String> {
    if !pit_id.is_empty() {
        let _ = reader.delete_pit(pit_id).await?;
    }
    Ok(())
}

fn server_error(body: &str) -> Value {
    serde_json::from_str::<Value>(body).ok().and_then(|value| value.get("error").cloned()).unwrap_or(Value::Null)
}

fn text_of(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn push_search_audit(reader: &mut Api, path: &str, status: u16, bytes_out: usize, bytes_in: usize, error: Option<&str>, class: &str) {
    reader.audit.push(Audit {
        n: reader.audit.len() as u64 + 1,
        identity: "reader".to_string(),
        method: "POST".to_string(),
        path: path.to_string(),
        class: class.to_string(),
        status,
        bytes_out: bytes_out as u64,
        bytes_in: bytes_in as u64,
        error: error.map(str::to_string),
    });
}

fn truncate(text: &str) -> String {
    truncate_n(text, 800)
}

fn truncate_n(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn write_json(path: &std::path::Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    std::fs::write(path, text + "\n").map_err(|error| error.to_string())
}

fn write_pending(
    out: &Path,
    path: &str,
    priv_status: u16,
    priv_raw: &[u8],
    outcome: &SearchOutcome,
    note: &str,
) -> Result<(), String> {
    write_json(
        &out.join("cancel.json"),
        &json!({
            "outcome": "pending",
            "class": "unsupported",
            "note": note,
            "search_path": path,
            "cancel_after_time_interval_sent": false,
            "labeled_as_timeout": false,
            "client_closed": outcome.client_closed,
            "task_cancel_http_status": priv_status,
            "task_cancel_body": truncate(&String::from_utf8_lossy(priv_raw)),
            "server_status": outcome.status,
            "terminal_reason": reason_from_http(&outcome.body),
            "terminal_reason_source": "not_in_response",
            "dba_cancel_used": false,
            "ipc_present": false,
            "natural_completion_before_trigger": outcome.natural_before_trigger,
        }),
    )
}

async fn join_search(search: tokio::task::JoinHandle<SearchOutcome>) -> SearchOutcome {
    match search.await {
        Ok(outcome) => outcome,
        Err(error) => SearchOutcome {
            status: 0,
            body: String::new(),
            transport_error: Some(error.to_string()),
            client_closed: false,
            natural_before_trigger: false,
            completed_unix_ms: None,
            user_triggered_unix_ms: None,
            elapsed_ms: 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proved_input(reason: &str) -> ClassInput {
        ClassInput {
            natural_before_trigger: false,
            user_triggered: true,
            terminal_reason: reason.to_string(),
            server_status: 500,
            before_cancelled: false,
            after_cancelled: true,
            same_task_id: true,
            task_id_present_after: false,
            marker_only: true,
            running_nanos: 6_000_000,
            cancel_after_sent: false,
            dba_cancel_used: false,
            reason_source: "owned_http_response",
        }
    }

    #[test]
    fn channel_close_is_user_cancel() {
        assert_eq!(classify_user_cancel(&proved_input("cancelled task with reason: channel closed")), "user_cancel");
    }

    #[test]
    fn timeout_expiry_is_not_user_cancel() {
        let mut input = proved_input("cancelled task with reason: Cancellation timeout of 800ms is expired");
        assert_eq!(classify_user_cancel(&input), "unsupported");
        input.terminal_reason = "cancelled task with reason: Cancellation timeout of 2s is expired".to_string();
        assert_eq!(classify_user_cancel(&input), "unsupported");
    }

    #[test]
    fn constant_flag_without_response_reason_is_unsupported() {
        let mut input = proved_input("cancelled task with reason: channel closed");
        input.reason_source = "not_in_response";
        assert_eq!(classify_user_cancel(&input), "unsupported");
    }

    #[test]
    fn completion_before_trigger_is_not_user_cancel() {
        let mut input = proved_input("cancelled task with reason: channel closed");
        input.natural_before_trigger = true;
        assert_eq!(classify_user_cancel(&input), "late_or_absent");
    }

    #[test]
    fn native_log_requires_node_index_and_current_window() {
        let log = "2026-10-10T23:37:50.333462553Z shardFailures {[NODE][approved][0]: TaskCancelledException[cancelled task with reason: channel closed]; }";
        assert!(bound_log_line(log, "NODE:308", "approved", 1791675469721, 1791675469724).is_some());
        assert!(bound_log_line(log, "OTHER:308", "approved", 1791675469721, 1791675469724).is_none());
        assert!(bound_log_line(log, "NODE:308", "other", 1791675469721, 1791675469724).is_none());
        assert!(bound_log_line(log, "NODE:308", "approved", 1791675479721, 1791675479724).is_none());
        let delayed = format!(
            "{} shardFailures {{[NODE][approved][0]: TaskCancelledException[cancelled task with reason: channel closed]; }}",
            format_unix_ms(1791675469724 + 6_400)
        );
        assert!(bound_log_line(&delayed, "NODE:308", "approved", 1791675469721, 1791675469724).is_some());
        let too_late = format!(
            "{} shardFailures {{[NODE][approved][0]: TaskCancelledException[cancelled task with reason: channel closed]; }}",
            format_unix_ms(1791675469724 + 20_000)
        );
        assert!(bound_log_line(&too_late, "NODE:308", "approved", 1791675469721, 1791675469724).is_none());
        assert!(bound_log_line(&log.replace("channel closed", "Cancellation timeout expired"), "NODE:308", "approved", 1791675469721, 1791675469724).is_none());
        let mut input = proved_input("cancelled task with reason: channel closed");
        input.reason_source = "bound_server_log";
        input.server_status = 0;
        assert_eq!(classify_user_cancel(&input), "user_cancel");
        input.task_id_present_after = true;
        assert_eq!(classify_user_cancel(&input), "unsupported");
    }

    #[test]
    fn search_path_has_no_server_timer() {
        let path = business_search_path("oo-dl01w1-44-workload");
        assert_eq!(path, "/oo-dl01w1-44-workload/_search?request_cache=false");
        assert!(!path.contains("cancel_after_time_interval"));
    }

    #[test]
    fn response_reason_comes_from_the_body() {
        let payload = r#"{"error":{"type":"task_cancelled_exception","reason":"cancelled task with reason: channel closed"}}"#;
        let raw = format!("HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\n\r\n{payload}", payload.len());
        assert_eq!(http_status(&raw), 500);
        assert_eq!(reason_from_http(&raw), "cancelled task with reason: channel closed");
        assert!(http_complete(raw.as_bytes()));
        assert_eq!(response_framing(&raw), "content-length");
    }

    #[test]
    fn chunked_response_reason_comes_from_the_body() {
        let payload = r#"{"error":{"type":"task_cancelled_exception","reason":"cancelled task with reason: channel closed"}}"#;
        let raw = format!("HTTP/1.1 500 X\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{payload}\r\n0\r\n\r\n", payload.len());
        assert!(http_complete(raw.as_bytes()));
        assert_eq!(reason_from_http(&raw), "cancelled task with reason: channel closed");
        assert_eq!(response_framing(&raw), "chunked");
        assert!(!http_complete(b"HTTP/1.1 500 X\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhel"));
    }

    #[test]
    fn timeout_body_is_not_relabeled() {
        let raw = "{\"error\":{\"type\":\"task_cancelled_exception\",\"reason\":\"cancelled task with reason: Cancellation timeout of 800ms is expired\"}}";
        let reason = reason_from_http(raw);
        assert!(reason.contains("expired"));
        assert!(!reason.contains("channel closed"));
    }

    #[test]
    fn unix_epoch_and_a_known_day() {
        assert_eq!(format_unix_ms(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(format_unix_ms(1_704_067_200_000), "2024-01-01T00:00:00.000Z");
    }
}
