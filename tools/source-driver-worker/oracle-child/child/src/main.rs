//! Oracle thin child for `oo.source.driver-oracle/v2`.
//!
//! One connection. The statement text is an allowlist in this binary.
//! The password is read from `/run/secrets/source-password` after the JSON
//! is accepted. The batch completion records `cancel_api=absent` because that
//! statement does not interrupt a call. `oracle:oci-cancel` calls
//! `Connection::request_cancel` and writes `cancel_api=oci_marker_interrupt`
//! only when the driver returns `ErrorKind::Cancelled`. A call timeout stays
//! a deadline and is not a container kill.
//! `oracle:readonly-audit` hashes the mounted capture files and counts rows
//! as of the manifest SCN. It does not attach another engine's receipt.

#![forbid(unsafe_code)]

mod sha256;

use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use arrow_array::RecordBatch;
use arrow_ipc::writer::StreamWriter;
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use oracledb::OracleNumber;
use serde_json::{json, Value};

use sha256::{hex, sha256};

const PROTOCOL: &str = "oo.source.driver-oracle/v2";
const PASSWORD_PATH: &str = "/run/secrets/source-password";
const HEAVY: &str = "SELECT /* oo-dl01w1-04-p2-cancel */ /*+ LEADING(a) USE_NL(b) NO_PARALLEL */ SUM(MOD(a.object_id + b.object_id, 9)) FROM all_objects a, all_objects b WHERE ROWNUM <= 8000000";

const STATEMENTS: &[&str] = &[
    "oracle:batch-asof",
    "oracle:deadline-select",
    "oracle:cancel-select",
    "oracle:oci-cancel",
    "oracle:readonly-audit",
    "oracle:text-wide",
    "oracle:text-ts",
    "oracle:fence",
    "oracle:reject-wide",
    "oracle:reject-ts",
    "oracle:reject-tz",
    "oracle:tamper",
    "oracle:secret",
    "oracle:dml",
];

fn main() -> ExitCode {
    let mut password = String::new();
    match run(&mut password) {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            let detail = redact(&err.detail, &password);
            eprintln!("oo-oracle-p2-child: {}", err.token);
            let _ = fs::write(
                "/audit/child-error.txt",
                format!("{}\n{}\n", err.token, detail),
            );
            ExitCode::from(err.code)
        }
    }
}

#[derive(Debug)]
struct ChildError {
    code: u8,
    token: &'static str,
    detail: String,
}

fn err(code: u8, token: &'static str, detail: impl Into<String>) -> ChildError {
    ChildError {
        code,
        token,
        detail: detail.into(),
    }
}

struct Request {
    statement_ref: String,
    generation: u64,
    max_rows: u64,
    source_host: String,
    source_user: String,
    source_service: String,
    source_object: String,
    image_id: String,
    decoy_host: String,
    lineage_pad: bool,
}

fn run(password: &mut String) -> Result<u8, ChildError> {
    let stdin = read_request_line()?;
    let request = parse_request(&stdin)?;
    if !STATEMENTS.contains(&request.statement_ref.as_str()) {
        return Err(err(2, "read_unsupported", "statement"));
    }
    gate(&request)?;
    *password = read_password()?;
    let conn = connect(&request, password)?;
    check_privilege(&conn, password)?;
    match request.statement_ref.as_str() {
        "oracle:batch-asof" => batch(&conn, &request, password),
        "oracle:deadline-select" => deadline(&conn, password),
        "oracle:cancel-select" => cancel_select(&conn, password),
        "oracle:oci-cancel" => oci_cancel(&conn, password),
        "oracle:readonly-audit" => readonly_audit(&conn, password),
        "oracle:text-wide" => text_object(&conn, password, &request, "wide-text"),
        "oracle:text-ts" => text_object(&conn, password, &request, "ts-text"),
        "oracle:fence" => fence(&conn, &request, password),
        "oracle:reject-wide" => reject_arrow(
            &conn,
            password,
            "wide-number",
            "SELECT n_free AS \"N_FREE\" FROM oo04_fix.t_wide WHERE id = 71",
            "Float64",
        ),
        "oracle:reject-ts" => reject_arrow(
            &conn,
            password,
            "timestamp9",
            "SELECT ts AS \"TS\" FROM oo04_fix.t_ts WHERE id = 81",
            "Timestamp(Microsecond,None)",
        ),
        "oracle:reject-tz" => reject_arrow(
            &conn,
            password,
            "timestamp-tz",
            "SELECT tstz AS \"TSTZ\" FROM oo04_fix.t_ts WHERE id = 81",
            "Timestamp(Microsecond,None)",
        ),
        "oracle:tamper" => tamper(&conn, password),
        "oracle:secret" => denied(
            &conn,
            password,
            "SELECT note FROM oo04_fix.t_secret",
            &["ORA-00942", "942"],
            false,
        ),
        "oracle:dml" => denied(
            &conn,
            password,
            "INSERT INTO oo04_fix.t_batch (id, n16, n1, d, c6, v2, r) VALUES (46, 0, 0, SYSDATE, 'X', 'x', NULL)",
            &["ORA-01031", "1031", "ORA-41900", "41900"],
            true,
        ),
        _ => Err(err(2, "read_unsupported", "statement")),
    }
}

/// One JSON object and its terminating newline. Docker attach half-close
/// does not deliver stdin EOF, so this does not wait for end of stream.
fn read_request_line() -> Result<Vec<u8>, ChildError> {
    let mut stdin = io::stdin();
    let mut buf = Vec::new();
    let mut tmp = [0u8; 512];
    loop {
        match stdin.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                if let Some(pos) = tmp[..n].iter().position(|byte| *byte == b'\n') {
                    buf.extend_from_slice(&tmp[..=pos]);
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                if buf.len() > 65_536 {
                    return Err(err(2, "protocol", "request too large"));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(err(2, "io", error.to_string())),
        }
    }
    if buf.is_empty() {
        return Err(err(2, "protocol", "empty request"));
    }
    Ok(buf)
}

fn parse_request(bytes: &[u8]) -> Result<Request, ChildError> {
    reject_duplicate_keys(bytes).map_err(|token| err(2, token, "json"))?;
    let value: Value = serde_json::from_slice(bytes).map_err(|e| err(2, "protocol", e.to_string()))?;
    let map = value.as_object().ok_or_else(|| err(2, "protocol", "object"))?;
    for key in map.keys() {
        if !matches!(
            key.as_str(),
            "protocol"
                | "statement_ref"
                | "generation"
                | "max_rows"
                | "source_host"
                | "source_user"
                | "source_service"
                | "source_object"
                | "image_id"
                | "decoy_host"
                | "lineage_pad"
        ) {
            return Err(err(2, "protocol", "key"));
        }
    }
    if map.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) {
        return Err(err(2, "protocol", "protocol"));
    }
    let generation = map.get("generation").and_then(Value::as_u64).ok_or_else(|| err(2, "protocol", "generation"))?;
    let max_rows = map.get("max_rows").and_then(Value::as_u64).ok_or_else(|| err(2, "protocol", "max_rows"))?;
    if !(1..=8).contains(&max_rows) {
        return Err(err(2, "budget", "max_rows"));
    }
    let source_host = required(map, "source_host")?;
    if !host_ok(&source_host) {
        return Err(err(2, "network_scope", "host"));
    }
    let image_id = required(map, "image_id")?;
    if !image_ok(&image_id) {
        return Err(err(2, "image_digest", "image"));
    }
    let decoy_host = match map.get("decoy_host") {
        None => String::new(),
        Some(Value::String(text)) => text.clone(),
        _ => return Err(err(2, "protocol", "decoy")),
    };
    if !decoy_host.is_empty() && !host_ok(&decoy_host) {
        return Err(err(2, "network_scope", "decoy"));
    }
    let lineage_pad = match map.get("lineage_pad") {
        None => false,
        Some(Value::Bool(value)) => *value,
        _ => return Err(err(2, "protocol", "lineage_pad")),
    };
    Ok(Request {
        statement_ref: required(map, "statement_ref")?,
        generation,
        max_rows,
        source_host,
        source_user: optional_identifier(map, "source_user")?,
        source_service: optional_identifier(map, "source_service")?,
        source_object: optional_object(map)?,
        image_id,
        decoy_host,
        lineage_pad,
    })
}

fn gate(request: &Request) -> Result<(), ChildError> {
    // Refuse before the password file or the Oracle connection.
    if request.statement_ref == "oracle:dml" {
        return Err(err(2, "read_unsupported", "product connection does not submit DML"));
    }
    if request.source_user.is_empty() || request.source_service.is_empty() {
        return Err(err(2, "protocol", "caller source identity"));
    }
    let needs_object = matches!(
        request.statement_ref.as_str(),
        "oracle:batch-asof" | "oracle:text-wide" | "oracle:text-ts"
    );
    if needs_object && request.source_object.is_empty() {
        return Err(err(2, "protocol", "source_object"));
    }
    Ok(())
}

fn optional_identifier(map: &serde_json::Map<String, Value>, key: &str) -> Result<String, ChildError> {
    match map.get(key).and_then(Value::as_str) {
        None => Ok(String::new()),
        Some(value) => identifier(value),
    }
}

fn optional_object(map: &serde_json::Map<String, Value>) -> Result<String, ChildError> {
    match map.get("source_object").and_then(Value::as_str) {
        None => Ok(String::new()),
        Some(value) => object_identifier(value),
    }
}

fn identifier(value: &str) -> Result<String, ChildError> {
    if value.is_empty() || value.len() > 128 || !value.as_bytes()[0].is_ascii_alphabetic()
        || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(err(2, "protocol", "identifier"));
    }
    Ok(value.to_ascii_uppercase())
}

fn object_identifier(value: &str) -> Result<String, ChildError> {
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 2 { return Err(err(2, "protocol", "source_object")); }
    Ok(format!("{}.{}", identifier(parts[0])?, identifier(parts[1])?))
}

fn required(map: &serde_json::Map<String, Value>, key: &str) -> Result<String, ChildError> {
    map.get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .ok_or_else(|| err(2, "protocol", key))
}

fn read_password() -> Result<String, ChildError> {
    let meta = fs::symlink_metadata(PASSWORD_PATH).map_err(|e| err(2, "secret_ref", e.to_string()))?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err(err(2, "secret_ref", "password file"));
    }
    let bytes = fs::read(PASSWORD_PATH).map_err(|e| err(2, "secret_ref", e.to_string()))?;
    if bytes.contains(&0) || bytes.is_empty() || bytes.len() > 1024 {
        return Err(err(2, "secret_ref", "password bytes"));
    }
    let text = String::from_utf8(bytes).map_err(|_| err(2, "secret_ref", "password utf8"))?;
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Err(err(2, "secret_ref", "password empty"));
    }
    Ok(trimmed)
}

fn connect(request: &Request, password: &str) -> Result<oracledb::Connection, ChildError> {
    let connect = format!("{}:1521/{}", request.source_host, request.source_service);
    let config = oracledb::Config::default()
        .set_credentials(&request.source_user, password)
        .set_driver_name("oracledb-26.0.0-beta.4")
        .set_connect_string(&connect)
        .map_err(|e| err(2, "io", e.to_string()))?;
    let conn = oracledb::connect(config).map_err(|e| err(2, "io", e.to_string()))?;
    conn.set_module("oo-dl01w1-04-p2-reader")
        .map_err(|e| err(2, "io", e.to_string()))?;
    conn.ping().map_err(|e| err(2, "io", e.to_string()))?;
    Ok(conn)
}

fn check_privilege(conn: &oracledb::Connection, password: &str) -> Result<(), ChildError> {
    let mut sqls = Vec::new();
    let grants = query_strings(
        conn,
        &mut sqls,
        "SELECT privilege || '|' || owner || '|' || table_name FROM user_tab_privs ORDER BY 1",
    )?;
    let sys_privs = query_strings(conn, &mut sqls, "SELECT privilege FROM user_sys_privs ORDER BY 1")?;
    clean(&sqls, password)?;
    let any = grants.iter().chain(sys_privs.iter()).any(|line| {
        let privilege = line.split('|').next().unwrap_or(line);
        privilege.contains(" ANY") || privilege == "LOGMINING"
    });
    let _ = fs::write(
        "/audit/privilege.json",
        serde_json::to_vec_pretty(&json!({
            "grants": grants,
            "sys_privs": sys_privs,
            "any_or_logmining": any
        }))
        .unwrap_or_default(),
    );
    if any {
        return Err(err(6, "denied", "privilege wider than the reader grant"));
    }
    Ok(())
}

fn batch(conn: &oracledb::Connection, request: &Request, password: &str) -> Result<u8, ChildError> {
    let mut sqls = Vec::new();
    let (owner, table) = request.source_object.split_once('.').ok_or_else(|| err(2, "protocol", "source_object"))?;
    let columns = query_strings(
        conn,
        &mut sqls,
        &format!("SELECT column_name || '|' || data_type || '|' || NVL(TO_CHAR(data_precision), '-') || '|' || NVL(TO_CHAR(data_scale), '-') || '|' || nullable FROM all_tab_columns WHERE owner = '{owner}' AND table_name = '{table}' ORDER BY column_id"),
    )?;
    let expect = [
        "ID|NUMBER|4|0|N",
        "N16|NUMBER|16|6|Y",
        "N1|NUMBER|1|0|Y",
        "D|DATE|-|-|Y",
        "C6|CHAR|-|-|Y",
        "V2|VARCHAR2|-|-|Y",
        "R|RAW|-|-|Y",
    ];
    let _ = fs::write(
        "/audit/column-metadata.json",
        serde_json::to_vec_pretty(&json!(columns)).unwrap_or_default(),
    );
    if columns.len() != expect.len() || columns.iter().zip(expect).any(|(got, want)| got != want) {
        return Err(err(4, "precision_loss", "column metadata"));
    }
    // The reader does not call DBMS_FLASHBACK. A DBA session publishes the SCN.
    // MAX(ora_rowscn) is before the table's last DDL, so AS OF that value is ORA-01466.
    let published = fs::read_to_string("/audit/published-scn").map_err(|e| err(2, "protocol", e.to_string()))?;
    let scn = published.trim().to_string();
    if scn.is_empty()
        || !scn.chars().all(|ch| ch.is_ascii_digit())
        || scn == "2175138"
    {
        return Err(err(2, "protocol", "published scn"));
    }
    sqls.push(format!("AS OF SCN {scn} from /audit/published-scn"));
    let mut ready = false;
    let mut last_probe = String::from("scn probe");
    let mut replacement: Option<oracledb::Connection> = None;
    for attempt in 1..=3 {
        let probe = format!("SELECT COUNT(*) FROM {} AS OF SCN {scn}", request.source_object);
        let outcome = {
            let active = replacement.as_ref().unwrap_or(conn);
            query_one_number(active, &mut sqls, &probe)
        };
        match outcome {
            Ok(count) if count != "0" => {
                ready = true;
                break;
            }
            Ok(_) => return Err(err(2, "budget", "rows")),
            Err(text) if scn_probe_retry(&text) => {
                last_probe = text;
                let _ = fs::write(format!("/audit/scn-probe-{attempt}.txt"), &last_probe);
                thread::sleep(Duration::from_secs(1));
                replacement = Some(connect(request, password)?);
            }
            Err(text) => return Err(err(2, "protocol", text)),
        }
    }
    clean(&sqls, password)?;
    if !ready {
        return Err(err(2, "protocol", last_probe));
    }
    let active = replacement.as_ref().unwrap_or(conn);
    fs::write("/audit/scn-ready", format!("{scn}\n")).map_err(|e| err(2, "io", e.to_string()))?;
    wait_flag("/audit/dba-mutated.flag", Duration::from_secs(180))?;
    let arrow_sql = format!(
        "SELECT id AS \"ID\", n16 AS \"N16\", n1 AS \"N1\", d AS \"D\", c6 AS \"C6\", v2 AS \"V2\", r AS \"R\" FROM {} AS OF SCN {scn} ORDER BY id FETCH FIRST {} ROWS ONLY",
        request.source_object, request.max_rows
    );
    sqls.push(arrow_sql.clone());
    clean(&sqls, password)?;
    let batch = active
        .query_arrow(&arrow_sql, oracledb::BindParameters::default())
        .map_err(|e| err(4, "precision_loss", e.to_string()))?;
    if !batch_schema_ok(batch.schema().as_ref()) {
        let _ = fs::write(
            "/audit/arrow-schema-rejected.json",
            serde_json::to_vec_pretty(&json!({
                "committed": false,
                "schema": schema_list(batch.schema().as_ref())
            }))
            .unwrap_or_default(),
        );
        return Err(err(4, "precision_loss", "arrow schema"));
    }
    if batch.num_rows() == 0 || batch.num_rows() > request.max_rows as usize {
        return Err(err(2, "budget", "rows"));
    }
    let fingerprint = fingerprint(request.generation, batch.schema().as_ref(), &request.source_object);
    let ipc = encode_stream(&batch).map_err(|e| err(2, "schema", e))?;
    fs::write("/audit/batch.arrow", &ipc).map_err(|e| err(2, "io", e.to_string()))?;
    let lineage = lineage_body(batch.num_rows(), ipc.len(), &fingerprint, request.lineage_pad);
    // This statement reads. It does not call request_cancel.
    let completion = completion_text(
        batch.num_rows(),
        &scn,
        &fingerprint,
        &request.image_id,
        request.generation,
        "absent",
    );
    fs::write("/audit/completion.txt", &completion).map_err(|e| err(2, "io", e.to_string()))?;
    let mut out = io::stdout().lock();
    write_frame(&mut out, 1, &ipc).map_err(|e| err(2, "io", e))?;
    out.flush().map_err(|e| err(2, "io", e.to_string()))?;
    write_frame(&mut out, 2, &lineage).map_err(|e| err(2, "io", e))?;
    out.flush().map_err(|e| err(2, "io", e.to_string()))?;
    fs::write("/audit/lineage-written", b"1\n").map_err(|e| err(2, "io", e.to_string()))?;
    write_frame(&mut out, 3, completion.as_bytes()).map_err(|e| err(2, "io", e))?;
    out.flush().map_err(|e| err(2, "io", e.to_string()))?;
    let _ = fs::write("/audit/statements.json", serde_json::to_vec_pretty(&sqls).unwrap_or_default());
    Ok(0)
}

fn deadline(conn: &oracledb::Connection, password: &str) -> Result<u8, ChildError> {
    let mut sqls = Vec::new();
    write_session(conn)?;
    conn.set_call_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| err(2, "io", e.to_string()))?;
    sqls.push(HEAVY.to_string());
    clean(&sqls, password)?;
    let started = Instant::now();
    let outcome = conn.execute(HEAVY, &[]);
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    match outcome {
        Ok(_) => Err(err(2, "deadline", "query completed before the driver deadline")),
        Err(error) => {
            let timeout = matches!(error.kind(), oracledb::ErrorKind::CallTimeoutExceeded)
                || error.to_string().contains("call timeout");
            let detail = redact(&format!("{} | {error}", error_kind(&error)), password);
            let body = json!({
                "kind": "deadline",
                "cancel_api": "absent",
                "native_cancel_capability": "not_called",
                "ready": false,
                "rows_committed": 0,
                "client_timeout": timeout,
                "not_a_cancel": true,
                "error_kind": error_kind(&error),
                "elapsed_ms": elapsed_ms,
                "session": session_text(),
                "published_scn": published_scn_text(),
                "error": detail,
                "business_select": false,
                "process_kill": false
            });
            emit_measurement("read-zero.json", &body)?;
            if timeout {
                Ok(3)
            } else {
                Err(err(2, "deadline", "driver error was not CallTimeoutExceeded"))
            }
        }
    }
}

fn oci_cancel(conn: &oracledb::Connection, password: &str) -> Result<u8, ChildError> {
    write_session(conn)?;
    conn.set_call_timeout(Some(Duration::from_secs(90)))
        .map_err(|e| err(2, "io", e.to_string()))?;
    let sqls = vec![HEAVY.to_string()];
    clean(&sqls, password)?;
    #[cfg(not(oci_cancel))]
    {
        let _ = (conn, sqls);
        let body = json!({
            "kind": "oci_cancel_unlinked",
            "cancel_api": "absent",
            "native_cancel_capability": "absent",
            "ready": false,
            "rows_committed": 0,
            "reason": "published oracledb 26.0.0-beta.4 has no request_cancel",
            "process_kill": false
        });
        emit_measurement("read-zero.json", &body)?;
        return Err(err(2, "cancel", "oci overlay is not linked"));
    }
    #[cfg(oci_cancel)]
    {
        conn.arm_cancel_poll(Duration::from_millis(200))
            .map_err(|e| err(2, "io", e.to_string()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_watch = Arc::clone(&stop);
        let outcome = thread::scope(|scope| {
            scope.spawn(|| {
                while !stop_watch.load(Ordering::SeqCst) {
                    if fs::metadata("/audit/cancel-now").is_ok() {
                        conn.request_cancel();
                        break;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
            });
            let started = Instant::now();
            let outcome = conn.execute(HEAVY, &[]);
            stop.store(true, Ordering::SeqCst);
            (
                outcome,
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            )
        });
        let (outcome, elapsed_ms) = outcome;
        match outcome {
            Ok(_) => {
                let _ = fs::write("/audit/cancel-too-fast", b"completed\n");
                Err(err(2, "cancel", "query completed before the OCI interrupt"))
            }
            Err(error) => {
                let kind = error_kind(&error);
                let cancelled = matches!(error.kind(), oracledb::ErrorKind::Cancelled);
                let timeout = matches!(error.kind(), oracledb::ErrorKind::CallTimeoutExceeded);
                let body = json!({
                    "kind": "oci_marker_interrupt",
                    "cancel_api": if cancelled { "oci_marker_interrupt" } else { "absent" },
                    "native_cancel_capability": "request_cancel",
                    "ready": false,
                    "rows_committed": 0,
                    "not_call_timeout": cancelled && !timeout,
                    "call_timeout": timeout,
                    "error_kind": kind,
                    "elapsed_ms": elapsed_ms,
                    "session": session_text(),
                    "published_scn": published_scn_text(),
                    "process_kill": false
                });
                emit_measurement("read-zero.json", &body)?;
                if cancelled && !timeout {
                    Ok(0)
                } else if timeout {
                    Err(err(3, "deadline", "OCI cancel was preempted by CallTimeoutExceeded"))
                } else {
                    Err(err(2, "cancel", "driver error was not Cancelled"))
                }
            }
        }
    }
}

fn text_sql(name: &str, object: &str, scn: &str, max_rows: u64) -> Result<String, ChildError> {
    match name {
        "wide-text" => Ok(format!(
            "SELECT id AS \"ID\", TO_CHAR(n_free, 'TME') AS \"N_FREE_TEXT\" FROM {object} AS OF SCN {scn} ORDER BY id FETCH FIRST {max_rows} ROWS ONLY"
        )),
        "ts-text" => Ok(format!(
            "SELECT id AS \"ID\", TO_CHAR(ts, 'YYYY-MM-DD HH24:MI:SS.FF9') AS \"TS_TEXT\", TO_CHAR(tstz, 'YYYY-MM-DD HH24:MI:SS.FF9 TZH:TZM') AS \"TSTZ_TEXT\" FROM {object} AS OF SCN {scn} ORDER BY id FETCH FIRST {max_rows} ROWS ONLY"
        )),
        _ => Err(err(2, "protocol", "text profile")),
    }
}

fn text_object(
    conn: &oracledb::Connection,
    password: &str,
    request: &Request,
    name: &str,
) -> Result<u8, ChildError> {
    let scn = read_published_scn()?;
    let sql = text_sql(name, &request.source_object, &scn, request.max_rows)?;
    let sqls = vec![sql.clone()];
    clean(&sqls, password)?;
    let batch = conn
        .query_arrow(&sql, oracledb::BindParameters::default())
        .map_err(|e| err(4, "precision_loss", e.to_string()))?;
    let schema = schema_list(batch.schema().as_ref());
    let lossy = schema.iter().any(|item| {
        item.contains("Float64")
            || item.contains("Timestamp")
            || item.contains("Decimal128")
            || item.contains("Decimal256")
    });
    if lossy || batch.num_rows() == 0 || batch.num_rows() > request.max_rows as usize {
        let _ = fs::write(
            "/audit/text-rejected.json",
            serde_json::to_vec_pretty(&json!({
                "name": name,
                "committed": false,
                "rows_committed": 0,
                "schema": schema,
                "lossy": lossy
            }))
            .unwrap_or_default(),
        );
        return Err(err(4, "precision_loss", "text capture was not canonical utf8"));
    }
    let ipc = encode_stream(&batch).map_err(|e| err(2, "schema", e))?;
    fs::write("/audit/text.arrow", &ipc).map_err(|e| err(2, "io", e.to_string()))?;
    let _ = fs::write(
        "/audit/text.json",
        serde_json::to_vec_pretty(&json!({
            "name": name,
            "scn": scn,
            "rows": batch.num_rows(),
            "schema": schema,
            "representation": "canonical_oracle_text",
            "arrow_decimal": false,
            "arrow_timestamp": false,
            "rows_committed": 0,
            "committed": false
        }))
        .unwrap_or_default(),
    );
    let _ = fs::write("/audit/statements.json", serde_json::to_vec_pretty(&sqls).unwrap_or_default());
    Ok(0)
}

fn read_published_scn() -> Result<String, ChildError> {
    let published = fs::read_to_string("/audit/published-scn").map_err(|e| err(2, "protocol", e.to_string()))?;
    let scn = published.trim();
    if scn.is_empty() || scn == "2175138" || !scn.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(err(2, "protocol", "published scn"));
    }
    Ok(scn.to_string())
}

fn write_session(conn: &oracledb::Connection) -> Result<(), ChildError> {
    let sid = conn.session_id().map_err(|e| err(2, "io", e.to_string()))?;
    let serial = conn.serial_num().map_err(|e| err(2, "io", e.to_string()))?;
    fs::write("/audit/session.txt", format!("{sid},{serial}\n")).map_err(|e| err(2, "io", e.to_string()))
}

fn cancel_select(conn: &oracledb::Connection, password: &str) -> Result<u8, ChildError> {
    let sid = conn.session_id().map_err(|e| err(2, "io", e.to_string()))?;
    let serial = conn.serial_num().map_err(|e| err(2, "io", e.to_string()))?;
    fs::write("/audit/session.txt", format!("{sid},{serial}\n")).map_err(|e| err(2, "io", e.to_string()))?;
    conn.set_call_timeout(Some(Duration::from_secs(90)))
        .map_err(|e| err(2, "io", e.to_string()))?;
    let sqls = vec![HEAVY.to_string()];
    clean(&sqls, password)?;
    match conn.execute(HEAVY, &[]) {
        Ok(_) => {
            let _ = fs::write("/audit/cancel-too-fast", b"completed\n");
            Err(err(2, "cancel", "query completed before connection shutdown"))
        }
        Err(error) => {
            let _ = fs::write(
                "/audit/cancel-client.json",
                redact(&format!("{} | {error}", error_kind(&error)), password),
            );
            Err(err(2, "cancel", "client returned before shutdown was observed"))
        }
    }
}

fn fence(conn: &oracledb::Connection, request: &Request, password: &str) -> Result<u8, ChildError> {
    let allowed = tcp_ok(&request.source_host, 1521, Duration::from_secs(5));
    let testnet = tcp_ok("192.0.2.1", 1521, Duration::from_secs(2));
    let public = tcp_ok("1.1.1.1", 443, Duration::from_secs(2));
    let decoy = tcp_ok(&request.decoy_host, 1521, Duration::from_secs(2));
    let route = fs::read_to_string("/proc/net/route").unwrap_or_default();
    let _ = fs::write("/audit/route.txt", &route);
    let mut sqls = Vec::new();
    let query_ok = query_strings(conn, &mut sqls, "SELECT 'ok' FROM dual")
        .map(|rows| rows.first().map(String::as_str) == Some("ok"))
        .unwrap_or(false);
    clean(&sqls, password)?;
    let forbidden_ok = testnet || public || decoy;
    let _ = fs::write(
        "/audit/fence.json",
        serde_json::to_vec_pretty(&json!({
            "import_receipt": false,
            "allowed_ok": allowed && query_ok && !forbidden_ok,
            "forbidden_ok": forbidden_ok,
            "allowed_connect": allowed,
            "forbidden_testnet": testnet,
            "forbidden_public": public,
            "forbidden_decoy": decoy,
            "query_ok": query_ok,
            "business_select": false
        }))
        .unwrap_or_default(),
    );
    if allowed && query_ok && !forbidden_ok {
        Ok(0)
    } else {
        Err(err(2, "network_scope", "fence"))
    }
}

fn reject_arrow(
    conn: &oracledb::Connection,
    password: &str,
    name: &str,
    sql: &str,
    marker: &str,
) -> Result<u8, ChildError> {
    let sqls = vec![sql.to_string()];
    clean(&sqls, password)?;
    let outcome = conn.query_arrow(sql, oracledb::BindParameters::default());
    let body = match outcome {
        Ok(batch) => {
            let schema = schema_list(batch.schema().as_ref());
            let lossy = schema.iter().any(|item| item.contains(marker));
            json!({
                "name": name,
                "committed": false,
                "operational_path": false,
                "schema": schema,
                "lossy": lossy,
                "rows_seen": batch.num_rows()
            })
        }
        Err(error) => json!({
            "name": name,
            "committed": false,
            "operational_path": false,
            "driver_error": redact(&format!("{} | {error}", error_kind(&error)), password),
            "lossy": true
        }),
    };
    let _ = fs::write(
        "/audit/reject.json",
        serde_json::to_vec_pretty(&body).unwrap_or_default(),
    );
    Ok(4)
}

fn tamper(conn: &oracledb::Connection, password: &str) -> Result<u8, ChildError> {
    let sql = "SELECT id AS \"ID\", n16 AS \"N16\", n1 AS \"N1\", d AS \"D\", c6 AS \"C6\", v2 AS \"V2\", r AS \"R\" FROM oo04_fix.t_batch WHERE id = 41";
    let sqls = vec![sql.to_string()];
    clean(&sqls, password)?;
    let real = conn
        .query_arrow(sql, oracledb::BindParameters::default())
        .map_err(|e| err(2, "schema", e.to_string()))?;
    let _ = fs::write(
        "/audit/tamper.json",
        serde_json::to_vec_pretty(&json!({
            "real_query": true,
            "real_rows": real.num_rows(),
            "committed": false,
            "operational_path": false
        }))
        .unwrap_or_default(),
    );
    let schema = std::sync::Arc::new(Schema::new(vec![Field::new("N_FREE", DataType::Float64, true)]));
    let column = arrow_array::Float64Array::from(vec![Some(1.0_f64)]);
    let batch = RecordBatch::try_new(schema, vec![std::sync::Arc::new(column)])
        .map_err(|e| err(2, "schema", e.to_string()))?;
    let ipc = encode_stream(&batch).map_err(|e| err(2, "schema", e))?;
    let mut out = io::stdout().lock();
    write_frame(&mut out, 1, &ipc).map_err(|e| err(2, "io", e))?;
    out.flush().map_err(|e| err(2, "io", e.to_string()))?;
    Ok(0)
}

fn denied(
    conn: &oracledb::Connection,
    password: &str,
    sql: &str,
    markers: &[&str],
    execute: bool,
) -> Result<u8, ChildError> {
    let sqls = vec![sql.to_string()];
    clean(&sqls, password)?;
    let result = if execute {
        conn.execute(sql, &[]).map(|_| ()).map_err(|e| e.to_string())
    } else {
        conn.query_row(sql, &[])
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    match result {
        Ok(()) => {
            let _ = fs::write(
                "/audit/denied.json",
                br#"{"operational_path":false,"denied":false,"unexpected_success":true}"#,
            );
            Err(err(2, "denied", "negative probe succeeded"))
        }
        Err(text) => {
            let safe = redact(&text, password);
            let matched = markers.iter().any(|marker| safe.contains(marker));
            let _ = fs::write(
                "/audit/denied.json",
                serde_json::to_vec_pretty(&json!({
                    "operational_path": false,
                    "denied": matched,
                    "error": safe
                }))
                .unwrap_or_default(),
            );
            if matched {
                Ok(6)
            } else {
                Err(err(2, "denied", "negative probe error was not the expected denial"))
            }
        }
    }
}

fn query_strings(
    conn: &oracledb::Connection,
    sqls: &mut Vec<String>,
    sql: &str,
) -> Result<Vec<String>, ChildError> {
    sqls.push(sql.to_string());
    let cursor = conn.query(sql, &[]).map_err(|e| err(2, "io", e.to_string()))?;
    let mut lines = Vec::new();
    for row in cursor {
        let row = row.map_err(|e| err(2, "io", e.to_string()))?;
        let value: Option<String> = row.get(0).map_err(|e| err(2, "io", e.to_string()))?;
        lines.push(value.unwrap_or_default());
    }
    let _ = fs::write("/audit/statements.json", serde_json::to_vec_pretty(sqls).unwrap_or_default());
    Ok(lines)
}

fn scn_probe_retry(text: &str) -> bool {
    text.contains("ORA-01466")
        || text.contains("1466")
        || text.contains("ORA-01003")
        || text.contains("1003")
        || text.contains("ORA-08181")
        || text.contains("8181")
}

fn query_one_number(
    conn: &oracledb::Connection,
    sqls: &mut Vec<String>,
    sql: &str,
) -> Result<String, String> {
    sqls.push(sql.to_string());
    let _ = fs::write("/audit/statements.json", serde_json::to_vec_pretty(sqls).unwrap_or_default());
    match conn.query_row(sql, &[]) {
        Ok(row) => {
            let value: Option<OracleNumber> = row.get(0).map_err(|e| e.to_string())?;
            Ok(value.map(|number| number.to_string()).unwrap_or_else(|| "NULL".into()))
        }
        Err(error) => Err(format!("{} | {error}", error_kind(&error))),
    }
}

fn wait_flag(path: &str, limit: Duration) -> Result<(), ChildError> {
    let started = Instant::now();
    while started.elapsed() < limit {
        if fs::metadata(path).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_secs(1));
    }
    Err(err(2, "budget", "mutate flag"))
}

fn encode_stream(batch: &RecordBatch) -> Result<Vec<u8>, String> {
    let mut writer = StreamWriter::try_new(Vec::<u8>::new(), batch.schema().as_ref())
        .map_err(|e| e.to_string())?;
    writer.write(batch).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    writer.into_inner().map_err(|e| e.to_string())
}

fn write_frame(mut out: impl Write, tag: u32, payload: &[u8]) -> Result<(), String> {
    if payload.is_empty() || payload.len() > u32::MAX as usize {
        return Err("frame".into());
    }
    let len = payload.len() as u32;
    out.write_all(&tag.to_le_bytes()).map_err(|e| e.to_string())?;
    out.write_all(&len.to_le_bytes()).map_err(|e| e.to_string())?;
    out.write_all(payload).map_err(|e| e.to_string())?;
    Ok(())
}

fn lineage_body(rows: usize, arrow_bytes: usize, fingerprint: &str, pad: bool) -> Vec<u8> {
    let mut text = format!(
        "batch_index=0\nrows={rows}\narrow_bytes={arrow_bytes}\nschema_fingerprint={fingerprint}\n"
    );
    if pad {
        text.push_str(&"x".repeat(8 * 1024 * 1024));
    }
    text.into_bytes()
}

fn emit_measurement(name: &str, body: &Value) -> Result<(), ChildError> {
    let mut bytes = serde_json::to_vec_pretty(body).map_err(|e| err(2, "io", e.to_string()))?;
    bytes.push(b'\n');
    fs::write(format!("/audit/{name}"), &bytes).map_err(|e| err(2, "io", e.to_string()))?;
    let mut out = io::stdout().lock();
    out.write_all(&bytes).map_err(|e| err(2, "io", e.to_string()))?;
    out.flush().map_err(|e| err(2, "io", e.to_string()))?;
    let kind = body.get("kind").and_then(Value::as_str).unwrap_or("measurement");
    let capability = body
        .get("native_cancel_capability")
        .and_then(Value::as_str)
        .unwrap_or("absent");
    eprintln!("measurement={kind} native_cancel_capability={capability}");
    Ok(())
}

fn session_text() -> String {
    fs::read_to_string("/audit/session.txt")
        .map(|text| text.trim().to_string())
        .unwrap_or_else(|_| "unread".to_string())
}

fn published_scn_text() -> String {
    fs::read_to_string("/audit/published-scn")
        .map(|text| text.trim().to_string())
        .unwrap_or_else(|_| "unread".to_string())
}

fn file_sha(path: &str) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    Ok(hex(&sha256(&bytes)))
}

fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn snapshot_scn(value: &str) -> Result<String, String> {
    let Some(digits) = value.strip_prefix("oo-dl01w1-04-scn-") else {
        return Err("snapshot token".to_string());
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("snapshot digits".to_string());
    }
    if matches!(digits, "2175138" | "2196583" | "2205514") {
        return Err("historical snapshot".to_string());
    }
    Ok(digits.to_string())
}

fn audit_relation(logical: &str) -> Result<(&'static str, &'static str), String> {
    match logical {
        "t_batch" => Ok(("oo04_fix.t_batch", "T_BATCH")),
        "t_wide_text" => Ok(("oo04_fix.t_wide", "T_WIDE")),
        "t_ts_text" => Ok(("oo04_fix.t_ts", "T_TS")),
        _ => Err("logical table".to_string()),
    }
}

fn counts_equal(oracle: &str, expected: u64) -> bool {
    let oracle = oracle.trim();
    if oracle.parse::<u64>().ok() == Some(expected) {
        return true;
    }
    let Some((head, fraction)) = oracle.split_once('.') else {
        return false;
    };
    fraction.bytes().all(|byte| byte == b'0') && head.parse::<u64>().ok() == Some(expected)
}

const FOREIGN_ADMISSION: &str = "65b7f22fe26686f8a1e61d705a78d5a1a69f2a123de0b9dfce827cce2ffb59ef";

fn readonly_audit(conn: &oracledb::Connection, password: &str) -> Result<u8, ChildError> {
    let _ = write_session(conn);
    match readonly_audit_body(conn, password, "not_called") {
        Ok((code, body)) => {
            emit_measurement("readonly-audit.json", &body)?;
            Ok(code)
        }
        Err(error) => {
            let body = json!({
                "protocol": "oo.oracle-capture.readonly-audit/v1",
                "ok": false,
                "kind": "readonly_audit",
                "native_cancel_capability": "not_called",
                "process_kill": false,
                "rows_committed": 0,
                "admits_import": false,
                "error": redact(&error.detail, password),
                "error_token": error.token
            });
            emit_measurement("readonly-audit.json", &body)?;
            Ok(2)
        }
    }
}

fn readonly_audit_body(conn: &oracledb::Connection, password: &str, capability: &str) -> Result<(u8, Value), ChildError> {
    let names = [
        "manifest.json",
        "registry.json",
        "catalog.json",
        "model-input.json",
        "decision.json",
        "batch.arrow",
    ];
    let mut hashes = serde_json::Map::new();
    let mut borrowed = false;
    for name in names {
        let path = format!("/audit/identity/{name}");
        let digest = file_sha(&path).map_err(|text| err(2, "identity", text))?;
        if digest == FOREIGN_ADMISSION {
            borrowed = true;
        }
        hashes.insert(name.to_string(), json!(digest));
    }
    let manifest: Value = serde_json::from_slice(&fs::read("/audit/identity/manifest.json").map_err(|e| err(2, "identity", e.to_string()))?)
        .map_err(|e| err(2, "identity", e.to_string()))?;
    let registry: Value = serde_json::from_slice(&fs::read("/audit/identity/registry.json").map_err(|e| err(2, "identity", e.to_string()))?)
        .map_err(|e| err(2, "identity", e.to_string()))?;
    let catalog: Value = serde_json::from_slice(&fs::read("/audit/identity/catalog.json").map_err(|e| err(2, "identity", e.to_string()))?)
        .map_err(|e| err(2, "identity", e.to_string()))?;
    let model: Value = serde_json::from_slice(&fs::read("/audit/identity/model-input.json").map_err(|e| err(2, "identity", e.to_string()))?)
        .map_err(|e| err(2, "identity", e.to_string()))?;
    let decision_bytes = fs::read("/audit/identity/decision.json").map_err(|e| err(2, "identity", e.to_string()))?;
    let snapshot = manifest
        .pointer("/snapshot/value")
        .and_then(Value::as_str)
        .unwrap_or("");
    let scn = snapshot_scn(snapshot).map_err(|text| err(2, "identity", text))?;
    let table = manifest
        .pointer("/tables/0")
        .ok_or_else(|| err(2, "identity", "manifest table"))?;
    let logical = table.get("logical_table").and_then(Value::as_str).unwrap_or("");
    let row_count = table
        .get("row_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| err(2, "identity", "row_count"))?;
    let (relation, dictionary_table) = audit_relation(logical).map_err(|text| err(2, "identity", text))?;
    let schema_digest = catalog.get("schema_digest").and_then(Value::as_str).unwrap_or("");
    let model_digest = model.get("schema_digest").and_then(Value::as_str).unwrap_or("");
    let input_digest = catalog.get("input_digest").and_then(Value::as_str).unwrap_or("");
    let arrow_sha = hashes
        .get("batch.arrow")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let registry_snapshot = registry.pointer("/bindings/0/snapshot/value").and_then(Value::as_str).unwrap_or("");
    let catalog_source = catalog.get("source_id").and_then(Value::as_str).unwrap_or("");
    let manifest_source = manifest.get("source_id").and_then(Value::as_str).unwrap_or("");
    let registry_source = registry.pointer("/bindings/0/source_id").and_then(Value::as_str).unwrap_or("");
    let run_id = catalog.get("run_id").and_then(Value::as_str).unwrap_or("");
    let published = published_scn_text();
    let decision_contains_scn = decision_bytes.windows(scn.len()).any(|window| window == scn.as_bytes());
    let mut sqls = Vec::new();
    let count_sql = format!("SELECT COUNT(*) FROM {relation} AS OF SCN {scn}");
    let count_outcome = query_one_number(conn, &mut sqls, &count_sql);
    let dictionary_sql = format!(
        "SELECT column_name || '|' || data_type || '|' || NVL(TO_CHAR(data_precision), '-') || '|' || NVL(TO_CHAR(data_scale), '-') || '|' || NVL(TO_CHAR(char_length), '-') FROM all_tab_columns WHERE owner = 'OO04_FIX' AND table_name = '{dictionary_table}' ORDER BY column_id"
    );
    let dictionary_outcome = query_strings(conn, &mut sqls, &dictionary_sql);
    let banner_outcome = query_strings(conn, &mut sqls, "SELECT banner FROM v$version WHERE ROWNUM = 1");
    clean(&sqls, password)?;
    let (dictionary, dictionary_error) = match dictionary_outcome {
        Ok(rows) => (Some(rows), String::new()),
        Err(error) => (None, redact(&error.detail, password)),
    };
    let (banner, banner_error) = match banner_outcome {
        Ok(rows) => (rows.first().cloned(), String::new()),
        Err(error) => (None, redact(&error.detail, password)),
    };
    let (count_text, count_error) = match count_outcome {
        Ok(text) => (text, String::new()),
        Err(text) => (String::new(), redact(&text, password)),
    };
    let identity_ok = !borrowed
        && hex64(schema_digest)
        && schema_digest == model_digest
        && input_digest == arrow_sha
        && registry_snapshot == snapshot
        && !catalog_source.is_empty()
        && catalog_source == manifest_source
        && catalog_source == registry_source
        && published == scn
        && !run_id.is_empty();
    let matched = identity_ok && count_error.is_empty() && counts_equal(&count_text, row_count);
    let body = json!({
        "protocol": "oo.oracle-capture.readonly-audit/v1",
        "ok": matched,
        "kind": "readonly_audit",
        "native_cancel_capability": capability,
        "process_kill": false,
        "rows_committed": 0,
        "admits_import": false,
        "replaces_admission_slot": false,
        "mounted_file_is_foreign_receipt": borrowed,
        "foreign_admission_sha256": FOREIGN_ADMISSION,
        "snapshot": snapshot,
        "scn": scn,
        "published_scn": published,
        "source_id": catalog_source,
        "run_id": run_id,
        "logical_table": logical,
        "relation": relation,
        "manifest_row_count": row_count,
        "oracle_count": count_text,
        "oracle_count_error": count_error,
        "schema_digest": schema_digest,
        "model_input_schema_digest": model_digest,
        "input_digest": input_digest,
        "arrow_sha256": arrow_sha,
        "registry_snapshot": registry_snapshot,
        "decision_contains_scn": decision_contains_scn,
        "session": session_text(),
        "dictionary_columns": dictionary,
        "dictionary_error": dictionary_error,
        "banner": banner,
        "banner_error": banner_error,
        "files": hashes,
        "statements": sqls
    });
    Ok((if matched { 0 } else { 2 }, body))
}

fn completion_text(rows: usize, scn: &str, fingerprint: &str, image_id: &str, generation: u64, cancel_api: &str) -> String {
    format!(
        "\
protocol={PROTOCOL}
admits_import=false
rows={rows}
batches=1
scn={scn}
schema_fingerprint={fingerprint}
image_id={image_id}
generation={generation}
vendor_library_loaded=false
cancel_api={cancel_api}
published=false
pg_run_complete=false
fallback=false
"
    )
}

fn fingerprint(generation: u64, schema: &Schema, object: &str) -> String {
    let mut text = format!(
        "oo.source.driver-schema/v2\ngeneration={generation}\nobject={object}\n"
    );
    for field in schema.fields() {
        let nullable = if field.is_nullable() { "true" } else { "false" };
        text.push_str(&format!(
            "{}:{}:arrow_nullable={}\n",
            field.name(),
            arrow_type(field.data_type()),
            nullable
        ));
    }
    text.push_str("db=ID:NUMBER(4,0):nullable=false\n");
    text.push_str("db=N16:NUMBER(16,6):nullable=true\n");
    text.push_str("db=N1:NUMBER(1,0):nullable=true\n");
    text.push_str("db=D:DATE:nullable=true\n");
    text.push_str("db=C6:CHAR:nullable=true\n");
    text.push_str("db=V2:VARCHAR2:nullable=true\n");
    text.push_str("db=R:RAW:nullable=true\n");
    text.push_str("precision_loss_key=blocked\n");
    hex(&sha256(text.as_bytes()))
}

fn batch_schema_ok(schema: &Schema) -> bool {
    let expect = [
        ("ID", "Int16"),
        ("N16", "Decimal128(16,6)"),
        ("N1", "Int8"),
        ("D", "Timestamp(Microsecond,None)"),
        ("C6", "Utf8"),
        ("V2", "Utf8"),
        ("R", "Binary"),
    ];
    schema.fields().len() == expect.len()
        && schema.fields().iter().zip(expect).all(|(field, (name, ty))| {
            field.name() == name
                && arrow_type(field.data_type()) == ty
                && !matches!(field.data_type(), DataType::Boolean | DataType::Float64)
        })
}

fn schema_list(schema: &Schema) -> Vec<String> {
    schema
        .fields()
        .iter()
        .map(|field| format!("{}:{}", field.name(), arrow_type(field.data_type())))
        .collect()
}

fn arrow_type(data_type: &DataType) -> String {
    match data_type {
        DataType::Int8 => "Int8".into(),
        DataType::Int16 => "Int16".into(),
        DataType::Boolean => "Boolean".into(),
        DataType::Utf8 => "Utf8".into(),
        DataType::Binary => "Binary".into(),
        DataType::Float64 => "Float64".into(),
        DataType::Decimal128(precision, scale) => format!("Decimal128({precision},{scale})"),
        DataType::Timestamp(TimeUnit::Microsecond, None) => "Timestamp(Microsecond,None)".into(),
        DataType::Timestamp(unit, zone) => format!("Timestamp({unit:?},{zone:?})"),
        other => format!("{other:?}"),
    }
}

fn tcp_ok(host: &str, port: u16, timeout: Duration) -> bool {
    let Ok(mut addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    let Some(addr) = addrs.next() else {
        return false;
    };
    TcpStream::connect_timeout(&addr, timeout).is_ok()
}

fn clean(sqls: &[String], password: &str) -> Result<(), ChildError> {
    if password.is_empty() {
        return Ok(());
    }
    if sqls.iter().any(|sql| sql.contains(password)) {
        return Err(err(2, "secret_ref", "sql contained the secret"));
    }
    Ok(())
}

fn redact(text: &str, password: &str) -> String {
    if password.is_empty() {
        text.to_string()
    } else {
        text.replace(password, "[redacted]")
    }
}

fn error_kind(error: &oracledb::Error) -> String {
    format!("{:?}", error.kind())
}

fn host_ok(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_lowercase() || ch.is_ascii_digit() => {}
        _ => return false,
    }
    text.len() <= 63 && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn image_ok(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("sha256:") else {
        return false;
    };
    rest.len() == 64
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn reject_duplicate_keys(bytes: &[u8]) -> Result<(), &'static str> {
    let mut index = 0usize;
    let value = serde_json::from_slice::<Value>(bytes).map_err(|_| "protocol")?;
    if !value.is_object() && !value.is_array() && !value.is_string() && !value.is_number() && !value.is_boolean() && !value.is_null() {
        return Err("protocol");
    }
    scan_object(bytes, &mut index)?;
    skip_ws(bytes, &mut index);
    if index != bytes.len() && !(index + 1 == bytes.len() && bytes[index] == b'\n') {
        return Err("protocol");
    }
    Ok(())
}

fn scan_object(bytes: &[u8], index: &mut usize) -> Result<(), &'static str> {
    skip_ws(bytes, index);
    if bytes.get(*index) != Some(&b'{') {
        *index = bytes.len();
        return Ok(());
    }
    *index += 1;
    let mut seen = std::collections::BTreeSet::new();
    skip_ws(bytes, index);
    if bytes.get(*index) == Some(&b'}') {
        *index += 1;
        return Ok(());
    }
    loop {
        skip_ws(bytes, index);
        let key = scan_string(bytes, index)?;
        if !seen.insert(key) {
            return Err("duplicate_key");
        }
        skip_ws(bytes, index);
        if bytes.get(*index) != Some(&b':') {
            return Err("protocol");
        }
        *index += 1;
        skip_value(bytes, index)?;
        skip_ws(bytes, index);
        match bytes.get(*index) {
            Some(b',') => *index += 1,
            Some(b'}') => {
                *index += 1;
                return Ok(());
            }
            _ => return Err("protocol"),
        }
    }
}

fn skip_value(bytes: &[u8], index: &mut usize) -> Result<(), &'static str> {
    skip_ws(bytes, index);
    match bytes.get(*index) {
        Some(b'{') => scan_object(bytes, index),
        Some(b'"') => scan_string(bytes, index).map(|_| ()),
        Some(b'[') => {
            *index += 1;
            loop {
                skip_ws(bytes, index);
                if bytes.get(*index) == Some(&b']') {
                    *index += 1;
                    return Ok(());
                }
                skip_value(bytes, index)?;
                skip_ws(bytes, index);
                match bytes.get(*index) {
                    Some(b',') => *index += 1,
                    Some(b']') => {
                        *index += 1;
                        return Ok(());
                    }
                    _ => return Err("protocol"),
                }
            }
        }
        Some(_) => {
            while !matches!(bytes.get(*index), Some(b' ' | b'\n' | b'\r' | b'\t' | b',' | b'}' | b']')) {
                *index += 1;
            }
            Ok(())
        }
        None => Err("protocol"),
    }
}

fn scan_string(bytes: &[u8], index: &mut usize) -> Result<String, &'static str> {
    if bytes.get(*index) != Some(&b'"') {
        return Err("protocol");
    }
    *index += 1;
    let mut out = String::new();
    loop {
        let byte = *bytes.get(*index).ok_or("protocol")?;
        *index += 1;
        match byte {
            b'"' => return Ok(out),
            b'\\' => {
                let esc = *bytes.get(*index).ok_or("protocol")?;
                *index += 1;
                out.push(esc as char);
            }
            byte => out.push(byte as char),
        }
    }
}

fn skip_ws(bytes: &[u8], index: &mut usize) {
    while matches!(bytes.get(*index), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        *index += 1;
    }
}

#[cfg(test)]
mod customer_identifiers {
    use super::{gate, identifier, object_identifier, parse_request};
    #[test]
    fn customer_object_is_not_fixture_bound() {
        assert_eq!(object_identifier("sales.orders").unwrap(), "SALES.ORDERS");
        assert_eq!(identifier("customer_reader").unwrap(), "CUSTOMER_READER");
    }
    #[test]
    fn arbitrary_sql_cannot_be_an_object() {
        for value in ["SALES.ORDERS;DELETE", "SALES.ORDERS WHERE 1=1", "SYS.X.Y", "X", "\"SYS\".X", ".X"] {
            assert!(object_identifier(value).is_err(), "accepted {value}");
        }
    }
    #[test]
    fn connection_identifiers_cannot_inject() {
        for value in ["", "FREEPDB1?SYSDBA", "A/B", "A:1521", "1USER", "USER\n"] {
            assert!(identifier(value).is_err(), "accepted {value}");
        }
    }
    #[test]
    fn missing_caller_fields_are_empty_and_dml_never_connects() {
        let body = br#"{"protocol":"oo.source.driver-oracle/v2","statement_ref":"oracle:batch-asof","generation":2,"max_rows":1,"source_host":"oracle-host","image_id":"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","decoy_host":"decoy-host","lineage_pad":false}"#;
        let request = parse_request(body).unwrap();
        assert!(request.source_user.is_empty());
        assert!(request.source_service.is_empty());
        assert!(request.source_object.is_empty());
        assert!(gate(&request).is_err());
        let dml = br#"{"protocol":"oo.source.driver-oracle/v2","statement_ref":"oracle:dml","generation":2,"max_rows":1,"source_host":"oracle-host","source_user":"customer_reader","source_service":"sales","source_object":"sales.orders","image_id":"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","decoy_host":"decoy-host","lineage_pad":false}"#;
        let request = parse_request(dml).unwrap();
        assert_eq!(request.source_object, "SALES.ORDERS");
        let error = gate(&request).unwrap_err();
        assert_eq!(error.token, "read_unsupported");
    }

    #[test]
    fn completion_frame_copies_the_reported_capability() {
        let text = super::completion_text(5, "6395124", "abc", "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", 2, "oci_marker_interrupt");
        assert!(text.contains("cancel_api=oci_marker_interrupt\n"));
        assert!(text.contains("admits_import=false\n"));
        assert!(!text.contains("cancel_api=absent"));
        let absent = super::completion_text(5, "6395124", "abc", "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", 2, "absent");
        assert!(absent.contains("cancel_api=absent\n"));
    }

    #[test]
    fn historical_snapshot_is_not_this_capture() {
        assert_eq!(super::snapshot_scn("oo-dl01w1-04-scn-6395124").unwrap(), "6395124");
        assert!(super::snapshot_scn("oo-dl01w1-04-scn-2205514").is_err());
        assert!(super::snapshot_scn("oo-dl01w1-04-scn-2196583").is_err());
        assert!(super::snapshot_scn("6395124").is_err());
    }
}
