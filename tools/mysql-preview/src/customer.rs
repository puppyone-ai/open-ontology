//! Customer source entry. Schema and table names come from the request.
//! The fixture loader is a different command. This path never chooses
//! `logistics` or `oo_synth` on its own.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row, SslOpts, Value};
use oo_connectors::PG17_RESERVED;
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{Map, Value as JsonValue, json};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row as PgRow};
use tokio::time::timeout;

use crate::config::{mysql_ident, read_ca, read_secret};
use crate::error::{PreviewError, redact};
use crate::map::{MysqlColumn, grant_is_read_only, map_table, mariadb_comment, session_mode_ok};

const SLEEP_SECONDS: u64 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomerStop {
    Access,
    Cancel,
    Deadline,
    Product,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomerObject {
    pub schema: String,
    pub table: String,
    pub columns: Vec<String>,
}

#[derive(Debug)]
pub struct CustomerInput {
    pub secret_ref: String,
    pub role: String,
    pub stop: CustomerStop,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub secret_file: PathBuf,
    pub ca_file: PathBuf,
    pub approved: Vec<CustomerObject>,
    pub requested: Vec<CustomerObject>,
    pub connect_timeout_ms: u64,
    pub statement_timeout_ms: u64,
    pub deadline_ms: u64,
    pub batch_rows: u64,
    pub max_rows: u64,
    pub max_bytes: u64,
    pub boolean_tinyint1: bool,
    pub target_host: String,
    pub target_port: u16,
    pub target_database: String,
    pub target_admin_user: String,
    pub target_admin_secret_ref: String,
    pub target_admin_secret_file: PathBuf,
    pub target_runtime_user: String,
    pub target_runtime_secret_ref: String,
    pub target_ca_file: PathBuf,
    pub product: Option<crate::product::ProductSpec>,
}

struct Note {
    verb: String,
    sha256: String,
}

struct ReadHandle {
    conn: Conn,
    connection_id: u64,
}

struct StopProof {
    connection_id: u64,
    killed_connection_id: u64,
    other_connection_id: u64,
    other_still_running: bool,
    trigger: &'static str,
    partial_retired: bool,
}

/// Signal owned by one read task. This is not a `mysql_async` type.
struct CancelEvent {
    signaled: AtomicBool,
}

impl CancelEvent {
    fn new() -> Self {
        Self {
            signaled: AtomicBool::new(false),
        }
    }

    fn signal(&self) {
        self.signaled.store(true, Ordering::SeqCst);
    }

    fn is_signaled(&self) -> bool {
        self.signaled.load(Ordering::SeqCst)
    }

    async fn wait(&self) {
        let start = std::time::Instant::now();
        loop {
            if self.is_signaled() {
                return;
            }
            if start.elapsed() > Duration::from_secs(30) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

struct ReadCancel {
    event: CancelEvent,
    connection_id: u64,
}

impl ReadCancel {
    fn bind(connection_id: u64) -> Self {
        Self {
            event: CancelEvent::new(),
            connection_id,
        }
    }

    /// User-facing cancel. It only signals this read task.
    fn request(&self) {
        self.event.signal();
    }

    /// A signaled token may name only its own connection.
    fn authorize(&self, requested: u64) -> Result<(), PreviewError> {
        if !self.event.is_signaled() || requested != self.connection_id {
            return Err(PreviewError::new("privilege", "other_query"));
        }
        Ok(())
    }
}

type ReadTask = tokio::task::JoinHandle<Result<Vec<Row>, mysql_async::Error>>;

pub fn gate(input: &CustomerInput) -> Result<(), PreviewError> {
    if input.role != input.user {
        return Err(PreviewError::new("privilege", "role"));
    }
    let limit = if input.stop == CustomerStop::Product {
        4
    } else {
        1
    };
    if input.requested.is_empty() || input.requested.len() > limit {
        return Err(PreviewError::new("config", "objects"));
    }
    for object in &input.requested {
        if !approved_object(&input.approved, object) {
            return Err(PreviewError::new("privilege", "unapproved"));
        }
    }
    Ok(())
}

fn approved_object(approved: &[CustomerObject], object: &CustomerObject) -> bool {
    approved.iter().any(|item| {
        item.schema == object.schema
            && item.table == object.table
            && columns_allowed(&item.columns, &object.columns)
    })
}

fn columns_allowed(approved: &[String], requested: &[String]) -> bool {
    // An empty approval list names the whole table. An empty request list
    // means every column, so it is allowed only for that whole-table approval.
    if approved.is_empty() {
        return true;
    }
    if requested.is_empty() {
        return false;
    }
    requested
        .iter()
        .all(|name| approved.iter().any(|item| item == name))
}

pub async fn run_customer(config_path: &Path, out_dir: &Path) -> Result<(), PreviewError> {
    let bytes = fs::read(config_path).map_err(|_| PreviewError::new("config", "path"))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| PreviewError::new("config", "json"))?;
    let input = parse_customer(text)?;
    fs::create_dir_all(out_dir).map_err(|_| PreviewError::new("config", "out"))?;
    if let Err(err) = gate(&input) {
        write_outcome(out_dir, &failure_body(&input, &err, &[]))?;
        return Err(err);
    }
    let mut notes = Vec::new();
    match drive(&input, &mut notes, out_dir).await {
        Ok(body) => write_outcome(out_dir, &body),
        Err(err) => {
            write_outcome(out_dir, &failure_body(&input, &err, &notes))?;
            Err(err)
        }
    }
}

async fn drive(
    input: &CustomerInput,
    notes: &mut Vec<Note>,
    out_dir: &Path,
) -> Result<JsonValue, PreviewError> {
    if input.stop == CustomerStop::Product {
        return crate::product::close_loop(input, out_dir).await;
    }
    let password = read_secret(&input.secret_file)?;
    let mut business = connect(input, password.as_str()).await?;
    preflight(input, &mut business, notes).await?;
    let object = &input.requested[0];
    let columns = catalog_columns(&mut business, notes, object).await?;
    let reserved: BTreeSet<String> = PG17_RESERVED
        .iter()
        .map(|word| (*word).to_string())
        .collect();
    map_table(&columns, input.boolean_tinyint1, false, &reserved)?;
    let selected = selected_names(object, &columns)?;
    if input.stop != CustomerStop::Access {
        let column = selected[0].clone();
        drop(business);
        let proof = cancel_query(input, password.as_str(), notes, object, &column).await?;
        return Ok(success_body(
            input,
            notes,
            "canceled",
            0,
            false,
            Some(&proof),
        ));
    }
    let count = read_capped(input, &mut business, notes, object, &selected).await?;
    drop(business);
    verbs_ok(notes, false)?;
    let opened = probe_target(input).await?;
    Ok(success_body(input, notes, "read", count, opened, None))
}

async fn preflight(
    input: &CustomerInput,
    conn: &mut Conn,
    notes: &mut Vec<Note>,
) -> Result<(), PreviewError> {
    let version = scalar(conn, notes, "SELECT VERSION()").await?;
    let comment = scalar(conn, notes, "SELECT @@version_comment").await?;
    if mariadb_comment(&comment) {
        return Err(PreviewError::new("version", "mariadb"));
    }
    crate::version::ServerVersion::parse(&version)?;
    let sql_mode = scalar(conn, notes, "SELECT @@session.sql_mode").await?;
    if !session_mode_ok(&sql_mode) {
        return Err(PreviewError::new("session", "sql_mode"));
    }
    let transport = scalar(conn, notes, "SELECT @@require_secure_transport").await?;
    if transport != "1" {
        return Err(PreviewError::new("tls", "transport"));
    }
    let cipher = show_cipher(conn, notes).await?;
    if cipher.is_empty() {
        return Err(PreviewError::new("tls", "ssl_cipher"));
    }
    let current = scalar(conn, notes, "SELECT CURRENT_USER()").await?;
    let account = current.split('@').next().unwrap_or("");
    if account != input.role {
        return Err(PreviewError::new("privilege", "current_user"));
    }
    let grants = show_grants(conn, notes).await?;
    if grants.is_empty()
        || grants.iter().any(|line| !grant_is_read_only(line))
        || !grants
            .iter()
            .any(|line| line.to_ascii_uppercase().contains("SELECT"))
    {
        return Err(PreviewError::new("privilege", "grants"));
    }
    Ok(())
}

async fn catalog_columns(
    conn: &mut Conn,
    notes: &mut Vec<Note>,
    object: &CustomerObject,
) -> Result<Vec<MysqlColumn>, PreviewError> {
    let sql = "SELECT COLUMN_NAME, ORDINAL_POSITION, DATA_TYPE, COLUMN_TYPE, IS_NULLABLE, \
               CHARACTER_MAXIMUM_LENGTH, NUMERIC_PRECISION, NUMERIC_SCALE, CHARACTER_SET_NAME, COLUMN_KEY \
               FROM information_schema.COLUMNS \
               WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION";
    note(notes, sql);
    let rows: Vec<Row> = timeout(
        Duration::from_secs(30),
        conn.exec(sql, (object.schema.as_str(), object.table.as_str())),
    )
    .await
    .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
    .map_err(|_| PreviewError::new("catalog", "columns"))?;
    if rows.is_empty() {
        return Err(PreviewError::new("catalog", "missing_table"));
    }
    let mut columns = Vec::new();
    for row in rows {
        let name = cell_text(&row, 0)?;
        if !mysql_ident(&name) {
            return Err(PreviewError::new("ident", "ident"));
        }
        columns.push(MysqlColumn {
            name,
            ordinal: i32::try_from(cell_i64(&row, 1)?)
                .map_err(|_| PreviewError::new("catalog", "ordinal"))?,
            data_type: cell_text(&row, 2)?,
            column_type: cell_text(&row, 3)?,
            nullable: cell_text(&row, 4)? == "YES",
            char_len: cell_opt_i64(&row, 5)?,
            precision: cell_opt_i64(&row, 6)?.map(|value| i32::try_from(value).unwrap_or(i32::MAX)),
            scale: cell_opt_i64(&row, 7)?.map(|value| i32::try_from(value).unwrap_or(i32::MAX)),
            charset: cell_opt_text(&row, 8)?,
            column_key: cell_text(&row, 9)?,
        });
    }
    Ok(columns)
}

fn selected_names(
    object: &CustomerObject,
    columns: &[MysqlColumn],
) -> Result<Vec<String>, PreviewError> {
    let wanted = if object.columns.is_empty() {
        columns
            .iter()
            .map(|column| column.name.clone())
            .collect::<Vec<_>>()
    } else {
        object.columns.clone()
    };
    if wanted.is_empty()
        || wanted
            .iter()
            .any(|name| !columns.iter().any(|column| column.name == *name))
    {
        return Err(PreviewError::new("catalog", "missing_column"));
    }
    Ok(wanted)
}

async fn read_capped(
    input: &CustomerInput,
    conn: &mut Conn,
    notes: &mut Vec<Note>,
    object: &CustomerObject,
    selected: &[String],
) -> Result<u64, PreviewError> {
    let list = quote_list(selected)?;
    let limit = input.max_rows.saturating_add(1);
    let sql = format!(
        "SELECT {list} FROM `{}`.`{}` ORDER BY `{}` LIMIT {limit}",
        object.schema, object.table, selected[0]
    );
    note(notes, &sql);
    let rows: Vec<Row> = timeout(Duration::from_millis(input.deadline_ms), conn.query(sql))
        .await
        .map_err(|_| PreviewError::new("budget", "deadline"))?
        .map_err(|_| PreviewError::new("protocol", "query"))?;
    if rows.len() as u64 > input.max_rows {
        return Err(PreviewError::new("budget", "max_rows"));
    }
    let mut bytes = 0u64;
    for row in &rows {
        for index in 0..selected.len() {
            bytes = bytes.saturating_add(cell_len(row, index)?);
        }
    }
    if bytes > input.max_bytes {
        return Err(PreviewError::new("budget", "max_bytes"));
    }
    Ok(rows.len() as u64)
}

async fn cancel_query(
    input: &CustomerInput,
    password: &str,
    notes: &mut Vec<Note>,
    object: &CustomerObject,
    column: &str,
) -> Result<StopProof, PreviewError> {
    let owned = open_read(input, password, notes).await?;
    let other = open_read(input, password, notes).await?;
    if owned.connection_id == other.connection_id {
        return Err(PreviewError::new("protocol", "connection_id"));
    }
    let sql = format!(
        "SELECT `{column}` FROM `{}`.`{}` WHERE SLEEP({SLEEP_SECONDS}) = 0 LIMIT 1",
        object.schema, object.table
    );
    note(notes, &sql);
    let (owned_id, owned_task) = spawn_read(owned, sql.clone());
    note(notes, &sql);
    let (other_id, other_task) = spawn_read(other, sql);
    let mut control = connect(input, password).await?;
    let deadline = input.stop == CustomerStop::Deadline;
    let owned_visible = wait_running(&mut control, notes, owned_id, &input.user, 0, true).await?;
    let other_visible = wait_running(&mut control, notes, other_id, &input.user, 0, true).await?;
    if !owned_visible || !other_visible {
        retire(&mut control, notes, owned_id, owned_task).await;
        retire(&mut control, notes, other_id, other_task).await;
        return Err(PreviewError::new("cancel", "not_running"));
    }
    let cancel = Arc::new(ReadCancel::bind(owned_id));
    if cancel.authorize(other_id).is_ok() || cancel.authorize(owned_id).is_ok() {
        retire(&mut control, notes, owned_id, owned_task).await;
        retire(&mut control, notes, other_id, other_task).await;
        return Err(PreviewError::new("privilege", "other_query"));
    }
    let (notes_tx, mut notes_rx) = tokio::sync::mpsc::unbounded_channel();
    let watcher = tokio::spawn(watch_scoped_kill(
        ScopedKill {
            host: input.host.clone(),
            port: input.port,
            user: input.user.clone(),
            password: password.to_string(),
            database: input.database.clone(),
            ca_file: input.ca_file.clone(),
            connect_timeout_ms: input.connect_timeout_ms,
            statement_timeout_ms: input.statement_timeout_ms,
            bound: owned_id,
            deadline,
            deadline_ms: input.deadline_ms,
        },
        Arc::clone(&cancel),
        notes_tx,
    ));
    if !deadline {
        cancel.request();
    }
    if cancel.authorize(other_id).is_ok() {
        retire(&mut control, notes, owned_id, owned_task).await;
        retire(&mut control, notes, other_id, other_task).await;
        watcher.abort();
        return Err(PreviewError::new("privilege", "other_query"));
    }
    if !deadline && cancel.authorize(owned_id).is_err() {
        retire(&mut control, notes, owned_id, owned_task).await;
        retire(&mut control, notes, other_id, other_task).await;
        watcher.abort();
        return Err(PreviewError::new("cancel", "not_killed"));
    }
    let limit =
        input
            .statement_timeout_ms
            .saturating_add(if deadline { input.deadline_ms } else { 2_000 });
    let killed = matches!(
        timeout(Duration::from_millis(limit), watcher).await,
        Ok(Ok(Ok(())))
    );
    while let Ok(sql) = notes_rx.try_recv() {
        note(notes, &sql);
    }
    if !killed {
        retire(&mut control, notes, owned_id, owned_task).await;
        retire(&mut control, notes, other_id, other_task).await;
        return Err(PreviewError::new("cancel", "not_killed"));
    }
    let ended = match timeout(
        Duration::from_millis(input.statement_timeout_ms),
        owned_task,
    )
    .await
    {
        Ok(Ok(result)) => result,
        _ => {
            retire(&mut control, notes, other_id, other_task).await;
            return Err(PreviewError::new("cancel", "not_killed"));
        }
    };
    if ended.is_ok() {
        retire(&mut control, notes, other_id, other_task).await;
        return Err(PreviewError::new("cancel", "rows_escaped"));
    }
    let owned_gone = info_count(&mut control, notes, owned_id, &input.user).await?;
    let other_alive = info_count(&mut control, notes, other_id, &input.user).await?;
    if owned_gone != 0 || other_alive != 1 {
        retire(&mut control, notes, other_id, other_task).await;
        let fact = if owned_gone != 0 {
            "still_running"
        } else {
            "other_killed"
        };
        return Err(PreviewError::new("cancel", fact));
    }
    let proof = StopProof {
        connection_id: owned_id,
        killed_connection_id: owned_id,
        other_connection_id: other_id,
        other_still_running: true,
        trigger: if deadline { "deadline" } else { "user" },
        partial_retired: true,
    };
    scoped_kill(&mut control, notes, other_id, input.statement_timeout_ms).await?;
    let _ = timeout(
        Duration::from_millis(input.statement_timeout_ms),
        other_task,
    )
    .await;
    let other_gone = info_count(&mut control, notes, other_id, &input.user).await?;
    drop(control);
    if other_gone != 0 {
        return Err(PreviewError::new("cancel", "still_running"));
    }
    verbs_ok(notes, true)?;
    Ok(proof)
}

struct ScopedKill {
    host: String,
    port: u16,
    user: String,
    password: String,
    database: String,
    ca_file: PathBuf,
    connect_timeout_ms: u64,
    statement_timeout_ms: u64,
    bound: u64,
    deadline: bool,
    deadline_ms: u64,
}

async fn watch_scoped_kill(
    job: ScopedKill,
    cancel: Arc<ReadCancel>,
    notes_tx: tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<(), PreviewError> {
    if job.deadline {
        tokio::time::sleep(Duration::from_millis(job.deadline_ms)).await;
        if job.bound != cancel.connection_id {
            return Err(PreviewError::new("privilege", "other_query"));
        }
    } else {
        cancel.event.wait().await;
        cancel.authorize(job.bound)?;
    }
    let mut dedicated = connect_parts(
        &job.host,
        job.port,
        &job.user,
        &job.password,
        &job.database,
        &job.ca_file,
        job.connect_timeout_ms,
    )
    .await?;
    let kill = format!("KILL QUERY {}", job.bound);
    let _ = notes_tx.send(kill.clone());
    timeout(
        Duration::from_millis(job.statement_timeout_ms),
        dedicated.query_drop(kill),
    )
    .await
    .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
    .map_err(|_| PreviewError::new("cancel", "kill"))?;
    drop(dedicated);
    Ok(())
}

async fn open_read(
    input: &CustomerInput,
    password: &str,
    notes: &mut Vec<Note>,
) -> Result<ReadHandle, PreviewError> {
    let mut conn = connect(input, password).await?;
    let id_text = scalar(&mut conn, notes, "SELECT CAST(CONNECTION_ID() AS CHAR)").await?;
    let connection_id = id_text
        .parse()
        .map_err(|_| PreviewError::new("protocol", "connection_id"))?;
    Ok(ReadHandle {
        conn,
        connection_id,
    })
}

fn spawn_read(handle: ReadHandle, sql: String) -> (u64, ReadTask) {
    let id = handle.connection_id;
    let mut conn = handle.conn;
    let task = tokio::spawn(async move { conn.query(sql).await });
    (id, task)
}

async fn scoped_kill(
    control: &mut Conn,
    notes: &mut Vec<Note>,
    id: u64,
    timeout_ms: u64,
) -> Result<(), PreviewError> {
    let kill = format!("KILL QUERY {id}");
    note(notes, &kill);
    timeout(Duration::from_millis(timeout_ms), control.query_drop(kill))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("cancel", "kill"))?;
    Ok(())
}

async fn retire(control: &mut Conn, notes: &mut Vec<Note>, id: u64, task: ReadTask) {
    let kill = format!("KILL QUERY {id}");
    note(notes, &kill);
    let _ = control.query_drop(kill).await;
    let _ = timeout(Duration::from_secs(2), task).await;
}

async fn info_count(
    conn: &mut Conn,
    notes: &mut Vec<Note>,
    id: u64,
    user: &str,
) -> Result<u64, PreviewError> {
    let sql = format!(
        "SELECT CAST(COUNT(*) AS CHAR) FROM information_schema.processlist WHERE ID = {id} AND USER = '{user}' AND INFO IS NOT NULL"
    );
    let text = scalar(conn, notes, &sql).await?;
    text.parse()
        .map_err(|_| PreviewError::new("protocol", "query"))
}

async fn wait_running(
    conn: &mut Conn,
    notes: &mut Vec<Note>,
    id: u64,
    user: &str,
    sleep_ms: u64,
    poll: bool,
) -> Result<bool, PreviewError> {
    if sleep_ms > 0 {
        tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
    }
    if !poll {
        return Ok(info_count(conn, notes, id, user).await? == 1);
    }
    let mut waited = 0u64;
    loop {
        if info_count(conn, notes, id, user).await? == 1 {
            return Ok(true);
        }
        if waited >= 3_000 {
            return Ok(false);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        waited += 50;
    }
}

fn verbs_ok(notes: &[Note], allow_kill: bool) -> Result<(), PreviewError> {
    for note in notes {
        let ok =
            matches!(note.verb.as_str(), "SELECT" | "SHOW") || (allow_kill && note.verb == "KILL");
        if !ok {
            return Err(PreviewError::new("privilege", "write_verb"));
        }
    }
    Ok(())
}

async fn probe_target(input: &CustomerInput) -> Result<bool, PreviewError> {
    let password = read_secret(&input.target_admin_secret_file)?;
    let ca = read_ca(&input.target_ca_file)?;
    let options = PgConnectOptions::from_explicit()
        .host(&input.target_host)
        .port(input.target_port)
        .username(&input.target_admin_user)
        .password(password.as_str())
        .database(&input.target_database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca)
        .disable_statement_logging();
    let connect = timeout(
        Duration::from_millis(input.connect_timeout_ms),
        options.connect(),
    )
    .await;
    let mut conn = match connect {
        Ok(Ok(conn)) => conn,
        Ok(Err(_)) => return Err(PreviewError::new("connect", "target")),
        Err(_) => return Err(PreviewError::new("connect", "connect_timeout")),
    };
    let row = sqlx::query("SELECT current_user")
        .fetch_one(&mut conn)
        .await
        .map_err(|_| PreviewError::new("target", "probe"))?;
    let current: String = row
        .try_get(0)
        .map_err(|_| PreviewError::new("target", "probe"))?;
    if current != input.target_admin_user {
        return Err(PreviewError::new("target", "admin_user"));
    }
    let _ = conn.close().await;
    Ok(true)
}

async fn connect(input: &CustomerInput, password: &str) -> Result<Conn, PreviewError> {
    connect_parts(
        &input.host,
        input.port,
        &input.user,
        password,
        &input.database,
        &input.ca_file,
        input.connect_timeout_ms,
    )
    .await
}

async fn connect_parts(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    database: &str,
    ca_file: &Path,
    connect_timeout_ms: u64,
) -> Result<Conn, PreviewError> {
    let ssl = SslOpts::default()
        .with_root_certs(vec![ca_file.to_path_buf().into()])
        .with_danger_accept_invalid_certs(false)
        .with_danger_skip_domain_validation(false)
        .with_disable_built_in_roots(true);
    let opts = OptsBuilder::default()
        .ip_or_hostname(host)
        .tcp_port(port)
        .user(Some(user.to_string()))
        .pass(Some(password.to_string()))
        .db_name(Some(database.to_string()))
        .prefer_socket(false)
        .ssl_opts(ssl);
    match timeout(Duration::from_millis(connect_timeout_ms), Conn::new(opts)).await {
        Ok(Ok(conn)) => Ok(conn),
        Ok(Err(err)) => {
            if redact(&err.to_string(), password) {
                Err(PreviewError::new("connect", "redacted"))
            } else {
                Err(PreviewError::new("connect", "connect"))
            }
        }
        Err(_) => Err(PreviewError::new("connect", "connect_timeout")),
    }
}

async fn scalar(conn: &mut Conn, notes: &mut Vec<Note>, sql: &str) -> Result<String, PreviewError> {
    note(notes, sql);
    let row: Option<Row> = timeout(Duration::from_secs(30), conn.query_first(sql))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("protocol", "query"))?;
    let row = row.ok_or_else(|| PreviewError::new("protocol", "query"))?;
    cell_text(&row, 0)
}

async fn show_cipher(conn: &mut Conn, notes: &mut Vec<Note>) -> Result<String, PreviewError> {
    let sql = "SHOW STATUS LIKE 'Ssl_cipher'";
    note(notes, sql);
    let row: Option<Row> = timeout(Duration::from_secs(30), conn.query_first(sql))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("tls", "ssl_cipher"))?;
    let row = row.ok_or_else(|| PreviewError::new("tls", "ssl_cipher"))?;
    cell_text(&row, 1)
}

async fn show_grants(conn: &mut Conn, notes: &mut Vec<Note>) -> Result<Vec<String>, PreviewError> {
    let sql = "SHOW GRANTS FOR CURRENT_USER()";
    note(notes, sql);
    let rows: Vec<Row> = timeout(Duration::from_secs(30), conn.query(sql))
        .await
        .map_err(|_| PreviewError::new("budget", "statement_timeout"))?
        .map_err(|_| PreviewError::new("privilege", "grants"))?;
    let mut lines = Vec::new();
    for row in rows {
        lines.push(cell_text(&row, 0)?);
    }
    Ok(lines)
}

fn note(notes: &mut Vec<Note>, sql: &str) {
    let verb = sql
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    notes.push(Note {
        verb,
        sha256: sha256_hex(sql.as_bytes()),
    });
}

fn cell_text(row: &Row, index: usize) -> Result<String, PreviewError> {
    match row
        .get(index)
        .ok_or_else(|| PreviewError::new("protocol", "cell"))?
    {
        Value::Bytes(bytes) => {
            String::from_utf8(bytes).map_err(|_| PreviewError::new("protocol", "cell"))
        }
        Value::Int(value) => Ok(value.to_string()),
        Value::UInt(value) => Ok(value.to_string()),
        _ => Err(PreviewError::new("protocol", "cell")),
    }
}

fn cell_opt_text(row: &Row, index: usize) -> Result<Option<String>, PreviewError> {
    match row
        .get(index)
        .ok_or_else(|| PreviewError::new("protocol", "cell"))?
    {
        Value::NULL => Ok(None),
        Value::Bytes(bytes) => Ok(Some(
            String::from_utf8(bytes).map_err(|_| PreviewError::new("protocol", "cell"))?,
        )),
        _ => Err(PreviewError::new("protocol", "cell")),
    }
}

fn cell_i64(row: &Row, index: usize) -> Result<i64, PreviewError> {
    match row
        .get(index)
        .ok_or_else(|| PreviewError::new("protocol", "cell"))?
    {
        Value::Int(value) => Ok(value),
        Value::UInt(value) => {
            i64::try_from(value).map_err(|_| PreviewError::new("protocol", "cell"))
        }
        Value::Bytes(bytes) => {
            let text =
                String::from_utf8(bytes).map_err(|_| PreviewError::new("protocol", "cell"))?;
            text.parse()
                .map_err(|_| PreviewError::new("protocol", "cell"))
        }
        _ => Err(PreviewError::new("protocol", "cell")),
    }
}

fn cell_opt_i64(row: &Row, index: usize) -> Result<Option<i64>, PreviewError> {
    match row
        .get(index)
        .ok_or_else(|| PreviewError::new("protocol", "cell"))?
    {
        Value::NULL => Ok(None),
        other => {
            let row_value = match other {
                Value::Int(value) => value,
                Value::UInt(value) => {
                    i64::try_from(value).map_err(|_| PreviewError::new("protocol", "cell"))?
                }
                Value::Bytes(bytes) => String::from_utf8(bytes)
                    .map_err(|_| PreviewError::new("protocol", "cell"))?
                    .parse()
                    .map_err(|_| PreviewError::new("protocol", "cell"))?,
                _ => return Err(PreviewError::new("protocol", "cell")),
            };
            Ok(Some(row_value))
        }
    }
}

fn cell_len(row: &Row, index: usize) -> Result<u64, PreviewError> {
    match row
        .get(index)
        .ok_or_else(|| PreviewError::new("protocol", "cell"))?
    {
        Value::NULL => Ok(0),
        Value::Bytes(bytes) => Ok(bytes.len() as u64),
        _ => Ok(8),
    }
}

fn quote_list(names: &[String]) -> Result<String, PreviewError> {
    let mut list = String::new();
    for name in names {
        if !mysql_ident(name) {
            return Err(PreviewError::new("ident", "ident"));
        }
        if !list.is_empty() {
            list.push_str(", ");
        }
        list.push('`');
        list.push_str(name);
        list.push('`');
    }
    Ok(list)
}

fn success_body(
    input: &CustomerInput,
    notes: &[Note],
    state: &str,
    rows: u64,
    target_opened: bool,
    proof: Option<&StopProof>,
) -> JsonValue {
    json!({
        "protocol": "oo.mysql-customer/v1",
        "state": state,
        "role": input.role,
        "database": input.database,
        "objects": input.requested.iter().map(object_json).collect::<Vec<_>>(),
        "secret_ref_sha256": sha256_hex(input.secret_ref.as_bytes()),
        "target_admin_secret_ref_sha256": sha256_hex(input.target_admin_secret_ref.as_bytes()),
        "target_runtime_secret_ref_sha256": sha256_hex(input.target_runtime_secret_ref.as_bytes()),
        "target_database": input.target_database,
        "target_runtime_user": input.target_runtime_user,
        "rows_retained": if state == "read" { rows } else { 0 },
        "row_count": rows,
        "target_opened": target_opened,
        "import_run_opened": false,
        "query_killed": proof.is_some(),
        "connection_id": proof.map(|item| item.connection_id),
        "killed_connection_id": proof.map(|item| item.killed_connection_id),
        "other_connection_id": proof.map(|item| item.other_connection_id),
        "other_still_running": proof.map(|item| item.other_still_running),
        "cancel_trigger": proof.map(|item| item.trigger),
        "partial_retired": proof.map(|item| item.partial_retired).unwrap_or(false),
        "half_ready": false,
        "statements": notes.iter().map(|item| json!({"verb": item.verb, "sha256": item.sha256})).collect::<Vec<_>>()
    })
}

fn failure_body(input: &CustomerInput, err: &PreviewError, notes: &[Note]) -> JsonValue {
    json!({
        "protocol": "oo.mysql-customer/v1",
        "state": "failed",
        "code": err.code,
        "fact": err.fact,
        "role": input.role,
        "secret_ref_sha256": sha256_hex(input.secret_ref.as_bytes()),
        "rows_retained": 0,
        "target_opened": false,
        "import_run_opened": false,
        "half_ready": false,
        "statements": notes.iter().map(|item| json!({"verb": item.verb, "sha256": item.sha256})).collect::<Vec<_>>()
    })
}

fn object_json(object: &CustomerObject) -> JsonValue {
    json!({"schema": object.schema, "table": object.table, "columns": object.columns})
}

fn write_outcome(out_dir: &Path, body: &JsonValue) -> Result<(), PreviewError> {
    fs::write(
        out_dir.join("outcome.json"),
        serde_json::to_vec_pretty(body).map_err(|_| PreviewError::new("config", "out"))?,
    )
    .map_err(|_| PreviewError::new("config", "out"))
}

pub fn parse_customer(text: &str) -> Result<CustomerInput, PreviewError> {
    let value: JsonValue =
        serde_json::from_str(text).map_err(|_| PreviewError::new("config", "json"))?;
    reject_secret_keys(&value)?;
    let map = value
        .as_object()
        .ok_or_else(|| PreviewError::new("config", "json"))?;
    if map.contains_key("product") {
        exact(
            map,
            &[
                "approved_objects",
                "boolean_tinyint1",
                "budgets",
                "connection",
                "objects",
                "product",
                "protocol",
                "role",
                "secret_ref",
                "stop",
                "target",
            ],
        )?;
    } else {
        exact(
            map,
            &[
                "approved_objects",
                "boolean_tinyint1",
                "budgets",
                "connection",
                "objects",
                "protocol",
                "role",
                "secret_ref",
                "stop",
                "target",
            ],
        )?;
    }
    if text_of(map, "protocol")? != "oo.mysql-customer/v1" {
        return Err(PreviewError::new("config", "version"));
    }
    let source_secret_ref = secret_ref(text_of(map, "secret_ref")?)?;
    let role = ident(text_of(map, "role")?)?;
    let stop = match text_of(map, "stop")? {
        "access" => CustomerStop::Access,
        "cancel" => CustomerStop::Cancel,
        "deadline" => CustomerStop::Deadline,
        "product" => CustomerStop::Product,
        _ => return Err(PreviewError::new("config", "stop")),
    };
    if (stop == CustomerStop::Product) != map.contains_key("product") {
        return Err(PreviewError::new("config", "stop"));
    }
    let product = if stop == CustomerStop::Product {
        Some(crate::product::parse_product(object(map, "product")?)?)
    } else {
        None
    };
    let connection = object(map, "connection")?;
    exact(
        connection,
        &["ca_file", "database", "host", "port", "secret_file", "user"],
    )?;
    let target = object(map, "target")?;
    exact(
        target,
        &[
            "admin_secret_file",
            "admin_secret_ref",
            "admin_user",
            "ca_file",
            "database",
            "host",
            "port",
            "runtime_secret_ref",
            "runtime_user",
        ],
    )?;
    let budgets = object(map, "budgets")?;
    exact(
        budgets,
        &[
            "batch_rows",
            "connect_timeout_ms",
            "deadline_ms",
            "max_bytes",
            "max_rows",
            "statement_timeout_ms",
        ],
    )?;
    let approved = objects(map, "approved_objects")?;
    let requested = objects(map, "objects")?;
    if approved.is_empty() {
        return Err(PreviewError::new("config", "approved_objects"));
    }
    Ok(CustomerInput {
        secret_ref: source_secret_ref,
        role,
        stop,
        host: host(text_of(connection, "host")?)?,
        port: port(connection)?,
        database: ident(text_of(connection, "database")?)?,
        user: ident(text_of(connection, "user")?)?,
        secret_file: absolute(text_of(connection, "secret_file")?)?,
        ca_file: absolute(text_of(connection, "ca_file")?)?,
        approved,
        requested,
        connect_timeout_ms: bounded(budgets, "connect_timeout_ms", 1, 30_000)?,
        statement_timeout_ms: bounded(budgets, "statement_timeout_ms", 1, 30_000)?,
        deadline_ms: bounded(budgets, "deadline_ms", 200, 30_000)?,
        batch_rows: bounded(budgets, "batch_rows", 1, 1_000)?,
        max_rows: bounded(budgets, "max_rows", 1, 100_000)?,
        max_bytes: bounded(budgets, "max_bytes", 1, 33_554_432)?,
        boolean_tinyint1: map
            .get("boolean_tinyint1")
            .and_then(JsonValue::as_bool)
            .ok_or_else(|| PreviewError::new("config", "boolean_tinyint1"))?,
        target_host: host(text_of(target, "host")?)?,
        target_port: port(target)?,
        target_database: ident(text_of(target, "database")?)?,
        target_admin_user: ident(text_of(target, "admin_user")?)?,
        target_admin_secret_ref: secret_ref(text_of(target, "admin_secret_ref")?)?,
        target_admin_secret_file: absolute(text_of(target, "admin_secret_file")?)?,
        target_runtime_user: ident(text_of(target, "runtime_user")?)?,
        target_runtime_secret_ref: secret_ref(text_of(target, "runtime_secret_ref")?)?,
        target_ca_file: absolute(text_of(target, "ca_file")?)?,
        product,
    })
}

fn objects(map: &Map<String, JsonValue>, key: &str) -> Result<Vec<CustomerObject>, PreviewError> {
    let rows = map
        .get(key)
        .and_then(JsonValue::as_array)
        .ok_or_else(|| PreviewError::config_field(key))?;
    let mut objects = Vec::new();
    for row in rows {
        let item = row
            .as_object()
            .ok_or_else(|| PreviewError::config_field(key))?;
        exact(item, &["columns", "schema", "table"])?;
        let mut columns = Vec::new();
        let list = item
            .get("columns")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| PreviewError::config_field(key))?;
        for column in list {
            columns.push(ident(
                column
                    .as_str()
                    .ok_or_else(|| PreviewError::config_field(key))?,
            )?);
        }
        objects.push(CustomerObject {
            schema: ident(text_of(item, "schema")?)?,
            table: ident(text_of(item, "table")?)?,
            columns,
        });
    }
    Ok(objects)
}

fn secret_ref(value: &str) -> Result<String, PreviewError> {
    let bytes = value.as_bytes();
    if !(12..=160).contains(&bytes.len())
        || !value.starts_with("secret://")
        || value.contains(char::is_whitespace)
    {
        return Err(PreviewError::new("config", "secret_ref"));
    }
    if !bytes[9..].iter().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(*byte, b'/' | b'_' | b'-' | b'.' | b':')
    }) {
        return Err(PreviewError::new("config", "secret_ref"));
    }
    Ok(value.to_string())
}

fn ident(value: &str) -> Result<String, PreviewError> {
    if mysql_ident(value) {
        Ok(value.to_string())
    } else {
        Err(PreviewError::new("ident", "ident"))
    }
}

fn host(value: &str) -> Result<String, PreviewError> {
    if value.is_empty()
        || value.len() > 253
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
    {
        Err(PreviewError::new("config", "host"))
    } else {
        Ok(value.to_string())
    }
}

fn port(map: &Map<String, JsonValue>) -> Result<u16, PreviewError> {
    map.get("port")
        .and_then(JsonValue::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|port| *port != 0)
        .ok_or_else(|| PreviewError::new("config", "port"))
}

fn bounded(
    map: &Map<String, JsonValue>,
    key: &str,
    low: u64,
    high: u64,
) -> Result<u64, PreviewError> {
    let value = map
        .get(key)
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| PreviewError::new("budget", "range"))?;
    if (low..=high).contains(&value) {
        Ok(value)
    } else {
        Err(PreviewError::new("budget", "range"))
    }
}

fn absolute(value: &str) -> Result<PathBuf, PreviewError> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(PreviewError::new("config", "path"))
    }
}

fn object<'a>(
    map: &'a Map<String, JsonValue>,
    key: &str,
) -> Result<&'a Map<String, JsonValue>, PreviewError> {
    map.get(key)
        .and_then(JsonValue::as_object)
        .ok_or_else(|| PreviewError::config_field(key))
}

fn text_of<'a>(map: &'a Map<String, JsonValue>, key: &str) -> Result<&'a str, PreviewError> {
    map.get(key)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| PreviewError::config_field(key))
}

fn exact(map: &Map<String, JsonValue>, allowed: &[&str]) -> Result<(), PreviewError> {
    if map.len() == allowed.len() && allowed.iter().all(|key| map.contains_key(*key)) {
        Ok(())
    } else {
        Err(PreviewError::new("config", "keys"))
    }
}

fn reject_secret_keys(value: &JsonValue) -> Result<(), PreviewError> {
    match value {
        JsonValue::Object(map) => {
            for (key, child) in map {
                if matches!(
                    key.as_str(),
                    "password" | "passwd" | "secret" | "token" | "private_key" | "ca_pem"
                ) {
                    return Err(PreviewError::new("config", "secret_key"));
                }
                reject_secret_keys(child)?;
            }
            Ok(())
        }
        JsonValue::Array(items) => {
            for item in items {
                reject_secret_keys(item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(objects: &str, approved: &str) -> String {
        format!(
            r#"{{
            "protocol":"oo.mysql-customer/v1",
            "secret_ref":"secret://oo-dl01w1-06/reader",
            "role":"yard_reader",
            "stop":"access",
            "boolean_tinyint1":false,
            "connection":{{"host":"db","port":3306,"database":"yard","user":"yard_reader","secret_file":"/secrets/reader","ca_file":"/secrets/ca"}},
            "approved_objects":{approved},
            "objects":{objects},
            "budgets":{{"connect_timeout_ms":5000,"statement_timeout_ms":8000,"deadline_ms":1500,"batch_rows":2,"max_rows":10,"max_bytes":1048576}},
            "target":{{"host":"pg","port":5432,"database":"previewdb","admin_user":"oo_admin","admin_secret_ref":"secret://oo-dl01w1-06/pg-admin","admin_secret_file":"/secrets/admin","runtime_user":"oo_runtime","runtime_secret_ref":"secret://oo-dl01w1-06/pg-runtime","ca_file":"/secrets/pg-ca"}}
            }}"#
        )
    }

    #[test]
    fn unapproved_object_is_refused_before_a_connection() {
        let text = sample(
            r#"[{"schema":"logistics","table":"shipments","columns":[]}]"#,
            r#"[{"schema":"yard","table":"dock","columns":["dock_id"]}]"#,
        );
        let input = parse_customer(&text).expect("parse");
        let err = gate(&input).expect_err("unapproved");
        assert_eq!((err.code, err.fact), ("privilege", "unapproved"));
    }

    #[test]
    fn approved_yard_object_passes_the_gate() {
        let text = sample(
            r#"[{"schema":"yard","table":"dock","columns":["dock_id"]}]"#,
            r#"[{"schema":"yard","table":"dock","columns":["dock_id","name"]}]"#,
        );
        let input = parse_customer(&text).expect("parse");
        assert!(gate(&input).is_ok());
        assert_ne!(input.database, "logistics");
    }

    #[test]
    fn password_key_is_refused() {
        let err = parse_customer(r#"{"password":"nope"}"#).expect_err("secret");
        assert_eq!(err.fact, "secret_key");
    }

    #[test]
    fn an_empty_column_list_does_not_widen_a_partial_approval() {
        let text = sample(
            r#"[{"schema":"yard","table":"dock","columns":[]}]"#,
            r#"[{"schema":"yard","table":"dock","columns":["dock_id"]}]"#,
        );
        let input = parse_customer(&text).expect("parse");
        let err = gate(&input).expect_err("partial");
        assert_eq!((err.code, err.fact), ("privilege", "unapproved"));
    }

    #[test]
    fn user_cancel_authorizes_only_its_own_connection() {
        let cancel = ReadCancel::bind(41);
        assert_eq!(
            cancel.authorize(41).expect_err("before request").fact,
            "other_query"
        );
        assert_eq!(
            cancel.authorize(99).expect_err("other id").fact,
            "other_query"
        );
        cancel.request();
        assert!(cancel.authorize(41).is_ok());
        assert_eq!(
            cancel.authorize(99).expect_err("still other").fact,
            "other_query"
        );
    }

    #[test]
    fn missing_column_request_is_not_an_unapproved_object() {
        let text = sample(
            r#"[{"schema":"yard","table":"mark","columns":["mark_id","shape"]}]"#,
            r#"[{"schema":"yard","table":"mark","columns":["mark_id","shape"]}]"#,
        );
        let input = parse_customer(&text).expect("parse");
        assert!(gate(&input).is_ok());
    }

    #[test]
    fn product_stop_accepts_two_approved_tables() {
        let mut text = sample(
            r#"[{"schema":"yard","table":"dock","columns":["dock_id","label","note"]},{"schema":"yard","table":"berth","columns":["berth_id","depth","name"]}]"#,
            r#"[{"schema":"yard","table":"dock","columns":["dock_id","label","note"]},{"schema":"yard","table":"berth","columns":["berth_id","depth","name"]}]"#,
        );
        text = text.replacen("\"stop\":\"access\"", "\"stop\":\"product\"", 1);
        let product = r#""product":{"audit_secret_file":"/secrets/root_password","audit_secret_ref":"secret://oo-dl01w1-06/mysql-root","audit_user":"root","builder_bin":"/work/pg-builder","generation":1,"hidden_column":"note","login_secret_file":"/secrets/preview_login_password","login_user":"oo_preview_login","model_bearer_file":"/run/llm-token","operator_bin":"/work/operator-preview","owner":"b5ac4bc8-1872-4fe4-a56a-04a70f7103a2","pg_role":"oo_r_reader","pipeline_bin":"/work/oo-pipeline","project":"yardcustomer","reviewed_at":"2026-10-11","reviewer":"yard.owner","runtime_secret_file":"/secrets/pg_runtime_password","shard":"yardcustomer","source_id":"yardcustomer"},"#;
        text = text.replacen("\"protocol\"", &format!("{product}\"protocol\""), 1);
        let input = parse_customer(&text).expect("parse");
        assert!(gate(&input).is_ok());
        assert_eq!(input.requested.len(), 2);
        assert!(input.product.is_some());
    }
}
