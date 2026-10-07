//! One connection, one transaction, one LOGGED shadow.
//!
//! `stage_replace` stops after `full_import_write_batch`. Dropping that
//! connection closes the socket and PostgreSQL aborts the transaction.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use oo_connectors::{ChildRequest, FailureKind, ParentConfig, ParentError, VerifiedBatch};
use oo_core::limits::DOCUMENT_MAX_BYTES;
use oo_pipeline::copy_encoder::ExactDecimal;
use oo_pipeline::copy_sink::{self, SinkError};
use oo_pipeline::full_import::{self, Recovery};
use oo_pipeline::physical_mapping::{plan_physical_mapping, quote_physical_name, Level, MappingRequest};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection};

use crate::digest::{
    canonical_preimage, config_preimage, digest_text, hook_digest, payload_preimage, schema_preimage,
    type_preimage,
};
use crate::project::{
    applied_pos, copy_columns, empty_batch, positions_for, project_verified, sink_error, COPY_BUDGETS,
};
use crate::request::{physical_plan, ImportRequest, PhysicalNames};
use crate::{
    hex_encode, parse_request, ImportError, ImportResult, RESULT_PROTOCOL,
};

const WORKER_HASH_CAP: u64 = 512 * 1024 * 1024;
const PASSWORD_MAX: u64 = 4096;

/// Replay needs no transaction. Ready holds one open transaction.
pub enum Stage {
    /// Every batch receipt was already stored. The fence was not adopted.
    Replay(Box<ImportResult>),
    /// COPY and `write_batch` are done. `note_table` has not run.
    Ready(Box<StagedImport>),
}

/// Open replace. Drop aborts the transaction by closing the socket.
pub struct StagedImport {
    conn: Option<PgConnection>,
    backend_pid: i32,
    rows: u64,
    child_reaped: bool,
    names: PhysicalNames,
    source: String,
    shard: String,
    owner: String,
    generation: i64,
    run_id: String,
    object_sha256: String,
    config_digest: String,
    type_digest: String,
    schema_digest: String,
    input_digest: String,
    canonical: String,
    namespace: String,
    table: String,
}

struct ReadyBatch {
    batch_id: String,
    typed_rows: u64,
    payload_digest: String,
    applied_pos: String,
    copy_sha: String,
    positions: Vec<ExactDecimal>,
    typed: oo_pipeline::copy_encoder::TypedBatch,
}

struct ParsedFile {
    batches: Vec<ReadyBatch>,
    rows: u64,
    canonical: String,
    child_reaped: bool,
}

impl StagedImport {
    /// Backend that holds the open transaction.
    pub fn backend_pid(&self) -> i32 {
        self.backend_pid
    }

    /// Physical shadow name. The table is visible only after commit.
    pub fn physical_table(&self) -> &str {
        &self.names.table
    }

    /// `note_table`, `seal_run`, and `COMMIT`.
    pub async fn commit(mut self) -> Result<ImportResult, ImportError> {
        let Some(mut conn) = self.conn.take() else {
            return Err(child(ImportError::new("database", "connection")));
        };
        let finished = self.finish_tx(&mut conn).await;
        if finished.is_err() {
            let _ = sqlx::query("ROLLBACK").execute(&mut conn).await;
        }
        finished
    }

    async fn finish_tx(&self, conn: &mut PgConnection) -> Result<ImportResult, ImportError> {
        // The shadow's deferred insert trigger requires state `open`. Fire it
        // before seal moves the run to `awaiting_publish`.
        sqlx::raw_sql(sqlx::AssertSqlSafe("SET CONSTRAINTS ALL IMMEDIATE"))
            .execute(&mut *conn)
            .await
            .map_err(db)?;
        let rows = i64::try_from(self.rows).unwrap_or(i64::MAX);
        sqlx::query("SELECT _oo.full_import_note_table($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8)")
            .bind(&self.shard)
            .bind(&self.owner)
            .bind(self.generation)
            .bind(&self.run_id)
            .bind(0_i32)
            .bind(rows)
            .bind(&self.canonical)
            .bind(&self.source)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
        let schemas = vec![self.namespace.clone()];
        let tables = vec![self.table.clone()];
        let sealed: String = sqlx::query_scalar(
            "SELECT _oo.full_import_seal_run($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::text[])",
        )
        .bind(&self.shard)
        .bind(&self.owner)
        .bind(self.generation)
        .bind(&self.run_id)
        .bind(&self.source)
        .bind(1_i64)
        .bind(&self.config_digest)
        .bind(&self.type_digest)
        .bind(&self.input_digest)
        .bind(&self.schema_digest)
        .bind(&schemas)
        .bind(&tables)
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?;
        if sealed != "awaiting_publish" {
            return Err(child(ImportError::new("database", "seal")));
        }
        sqlx::query("COMMIT").execute(&mut *conn).await.map_err(db)?;
        Ok(self.result(false, Some(self.run_id.clone()), Some(self.generation)))
    }

    fn result(&self, replay: bool, run_id: Option<String>, generation: Option<i64>) -> ImportResult {
        ImportResult {
            protocol: RESULT_PROTOCOL,
            state: "awaiting_review",
            published: false,
            role_grant: false,
            stored_run_state: "awaiting_publish",
            replay,
            run_id,
            physical_namespace: self.names.namespace.clone(),
            physical_table: self.names.table.clone(),
            physical_columns: self.names.columns.clone(),
            rows: self.rows,
            object_sha256: self.object_sha256.clone(),
            config_digest: self.config_digest.clone(),
            parsing_mode: "trusted_public_fixture",
            kernel_restriction: "unsupported",
            child_reaped: self.child_reaped,
            stored_position_codec: "snapshot-zero/v1",
            stored_position_lineage: "full-import.snapshot",
            target_generation: generation,
            source: self.source.clone(),
        }
    }
}

/// Parse, copy, and commit one replace.
pub async fn import_replace(request: &ImportRequest, password: &str) -> Result<ImportResult, ImportError> {
    match stage_replace(request, password).await? {
        Stage::Replay(result) => Ok(*result),
        Stage::Ready(staged) => staged.commit().await,
    }
}

/// Parse and either replay or stop before `note_table`.
pub async fn stage_replace(request: &ImportRequest, password: &str) -> Result<Stage, ImportError> {
    if password.is_empty() {
        return Err(ImportError::new("closed_input", "password"));
    }
    let owned = request.clone();
    let parsed = tokio::task::spawn_blocking(move || parse_file(&owned))
        .await
        .map_err(|_| ImportError::new("worker", "io"))??;
    let names = physical_plan(request)?;
    let mut conn = connect(request, password).await?;
    admit_session(&mut conn, request).await?;
    match classify(&mut conn, request, &names, &parsed.batches).await? {
        Class::Replay => {
            Ok(Stage::Replay(Box::new(replay_result(request, &names, &parsed))))
        }
        Class::Insert => {
            let staged = insert(conn, request, &names, &parsed).await?;
            Ok(Stage::Ready(Box::new(staged)))
        }
    }
}

/// Read the two CLI paths and import. The password value is file contents.
pub async fn run_paths(request_path: &Path, password_path: &Path) -> Result<ImportResult, ImportError> {
    let password = read_password(password_path)?;
    let text = read_request(request_path)?;
    let request = parse_request(&text)?;
    import_replace(&request, &password).await
}

fn read_password(path: &Path) -> Result<String, ImportError> {
    let bytes = read_regular(path, PASSWORD_MAX, "password")?;
    let text = std::str::from_utf8(trim_newline(&bytes)).map_err(|_| ImportError::new("closed_input", "password"))?;
    if text.is_empty() {
        return Err(ImportError::new("closed_input", "password"));
    }
    Ok(text.to_string())
}

fn read_request(path: &Path) -> Result<String, ImportError> {
    let max = u64::try_from(DOCUMENT_MAX_BYTES).unwrap_or(u64::MAX);
    let bytes = read_regular(path, max, "json")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| ImportError::new("closed_input", "encoding"))?;
    Ok(text.to_string())
}

fn read_regular(path: &Path, max: u64, detail: &'static str) -> Result<Vec<u8>, ImportError> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| ImportError::new("closed_input", detail))?;
    if !meta.file_type().is_file() {
        return Err(ImportError::new("closed_input", "not_regular"));
    }
    if meta.len() > max {
        return Err(ImportError::new("closed_input", detail));
    }
    let mut file = File::open(path).map_err(|_| ImportError::new("closed_input", detail))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|_| ImportError::new("closed_input", detail))?;
    if bytes.len() as u64 > max {
        return Err(ImportError::new("closed_input", detail));
    }
    Ok(bytes)
}

fn trim_newline(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    if end > 0 && bytes[end - 1] == b'\n' {
        end -= 1;
        if end > 0 && bytes[end - 1] == b'\r' {
            end -= 1;
        }
    }
    &bytes[..end]
}

enum Class {
    Replay,
    Insert,
}

fn parse_file(request: &ImportRequest) -> Result<ParsedFile, ImportError> {
    let object_hash = hash_regular(request.object_path(), request.max_total_bytes(), false)?;
    if object_hash != request.object_sha256() {
        return Err(ImportError::new("hash_drift", "object"));
    }
    let worker_hash = hash_regular(request.worker_path(), WORKER_HASH_CAP, true)?;
    if worker_hash != request.worker_sha256() {
        return Err(ImportError::new("worker_hash", "worker"));
    }
    let config = ParentConfig::try_new(request.worker_path(), request.parent_deadline_ms())
        .map_err(|error| map_parent(error, false))?;
    let worker_request = ChildRequest::try_new(
        request.object_path(),
        request.object_sha256(),
        request.profile().clone(),
        request.budget().clone(),
    )
    .map_err(|error| map_parent(error, false))?;
    let mut pull = config.start(&worker_request).map_err(|error| map_parent(error, true))?;
    let mut raw = Vec::new();
    loop {
        match pull.next_batch() {
            Ok(Some(batch)) => raw.push(batch),
            Ok(None) => break,
            Err(error) => return Err(map_parent(error, true)),
        }
    }
    let complete = pull.finish().map_err(|error| map_parent(error, true))?;
    if complete.object_sha256() != request.object_sha256() {
        return Err(child(ImportError::new("hash_drift", "object")));
    }
    let again = hash_regular(request.object_path(), request.max_total_bytes(), false).map_err(|error| error.with_child(true))?;
    if again != request.object_sha256() {
        return Err(child(ImportError::new("hash_drift", "object")));
    }
    if complete.batches() != raw.len() as u64 {
        return Err(child(ImportError::new("malformed", "row_count")));
    }
    let listed: u64 = raw.iter().map(|batch| batch.rows.len() as u64).sum();
    if complete.rows() != listed || (raw.is_empty() && complete.rows() != 0) {
        return Err(child(ImportError::new("malformed", "row_count")));
    }
    if u64::from(complete.columns()) != request.columns().len() as u64 {
        return Err(child(ImportError::new("malformed", "row_width")));
    }
    let (batches, rows, canonical) = project_all(request, &raw)?;
    Ok(ParsedFile {
        batches,
        rows,
        canonical,
        child_reaped: true,
    })
}

fn project_all(
    request: &ImportRequest,
    raw: &[VerifiedBatch],
) -> Result<(Vec<ReadyBatch>, u64, String), ImportError> {
    let mut typed = Vec::with_capacity(raw.len().max(1));
    if raw.is_empty() {
        typed.push(empty_batch(
            request.columns(),
            request.source(),
            request.namespace(),
            request.table(),
            request.object_sha256(),
        ).map_err(|error| error.with_child(true))?);
    } else {
        for (ordinal, batch) in raw.iter().enumerate() {
            if batch.batch_index != ordinal as u64 {
                return Err(child(ImportError::new("malformed", "batch_index")));
            }
            typed.push(
                project_verified(
                    request.columns(),
                    batch,
                    request.source(),
                    request.namespace(),
                    request.table(),
                    request.object_sha256(),
                )
                .map_err(|error| error.with_child(true))?,
            );
        }
    }
    let mut next = 0u64;
    let mut hashes = Vec::new();
    let mut ready = Vec::with_capacity(typed.len());
    for (ordinal, batch) in typed.into_iter().enumerate() {
        let positions = positions_for(next, batch.rows.len()).map_err(|error| error.with_child(true))?;
        let columns = copy_columns(&batch);
        let hashed = copy_sink::hash_batch(&batch, &columns, &positions, COPY_BUDGETS)
            .map_err(|error| sink_error(error).with_child(true))?;
        let copy_sha = hex_encode(hashed.hasher.clone().finalize().as_ref());
        let rows = u64::try_from(batch.rows.len()).unwrap_or(u64::MAX);
        for row in &batch.rows {
            hashes.push(hex_encode(&row.canonical_hash));
        }
        let payload = digest_text(&payload_preimage(request.object_sha_hex(), &copy_sha, rows, ordinal as u64));
        ready.push(ReadyBatch {
            batch_id: format!("b{ordinal}"),
            typed_rows: rows,
            payload_digest: payload,
            applied_pos: applied_pos(&positions),
            copy_sha,
            positions,
            typed: batch,
        });
        next = next.saturating_add(rows);
    }
    let canonical = digest_text(&canonical_preimage(&hashes));
    Ok((ready, next, canonical))
}

fn hash_regular(path: &Path, max_bytes: u64, worker: bool) -> Result<[u8; 32], ImportError> {
    let budget = if worker { "worker_bytes" } else { "object_bytes" };
    let io = if worker { "worker" } else { "object" };
    let meta = std::fs::symlink_metadata(path).map_err(|_| ImportError::new("input_io", io))?;
    if !meta.file_type().is_file() {
        return Err(ImportError::new("closed_input", "not_regular"));
    }
    if meta.len() > max_bytes {
        return Err(ImportError::new("budget", budget));
    }
    let mut file = File::open(path).map_err(|_| ImportError::new("input_io", io))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buf).map_err(|_| ImportError::new("input_io", io))?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > max_bytes {
            return Err(ImportError::new("budget", budget));
        }
        hasher.update(&buf[..read]);
    }
    Ok(hasher.finalize().into())
}

fn map_parent(error: ParentError, started: bool) -> ImportError {
    match error {
        ParentError::Config(token) => ImportError::new("closed_input", token).with_child(started),
        ParentError::Failed(failure) => map_failure(failure.token, failure.kind).with_child(started),
    }
}

/// Worker stderr keeps `truncated` and collapses other protocol facts to `protocol`.
fn map_failure(token: Option<&str>, kind: FailureKind) -> ImportError {
    if token == Some("truncated") {
        return ImportError::new("truncated", "truncated");
    }
    if token == Some("hash_mismatch") {
        return ImportError::new("hash_drift", "object");
    }
    if token == Some("protocol") {
        return ImportError::new("malformed", "protocol");
    }
    match kind {
        FailureKind::Ended | FailureKind::Malformed | FailureKind::Trailer => {
            ImportError::new("truncated", "truncated")
        }
        FailureKind::HashMismatch => ImportError::new("hash_drift", "object"),
        FailureKind::Protocol | FailureKind::Worker => ImportError::new("malformed", "protocol"),
        FailureKind::Spawn => ImportError::new("worker", "spawn"),
        FailureKind::TimedOut => ImportError::new("worker", "timed_out"),
        FailureKind::Io => ImportError::new("worker", "io"),
    }
}

async fn connect(request: &ImportRequest, password: &str) -> Result<PgConnection, ImportError> {
    let ca = std::fs::read(request.ca_path()).map_err(|_| child(ImportError::new("tls", "ca_path")))?;
    let options = PgConnectOptions::new()
        .host(request.host())
        .port(request.port())
        .username(request.user())
        .password(password)
        .database(request.database())
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca)
        .disable_statement_logging();
    PgConnection::connect_with(&options)
        .await
        .map_err(|_| child(ImportError::new("connect", "target")))
}

async fn admit_session(conn: &mut PgConnection, request: &ImportRequest) -> Result<(), ImportError> {
    let version: String = sqlx::query_scalar("SELECT version()").fetch_one(&mut *conn).await.map_err(db)?;
    if !version.contains("17.10") {
        return Err(child(ImportError::new("server_version", "version")));
    }
    let current: String = sqlx::query_scalar("SELECT current_user").fetch_one(&mut *conn).await.map_err(db)?;
    let session: String = sqlx::query_scalar("SELECT session_user").fetch_one(&mut *conn).await.map_err(db)?;
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?;
    if current != request.user() || session != request.user() || database != request.database() {
        return Err(child(ImportError::new("principal", "principal")));
    }
    let encoding: String = sqlx::query_scalar("SELECT current_setting('server_encoding')")
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?;
    if encoding != "UTF8" {
        return Err(child(ImportError::new("encoding", "server_encoding")));
    }
    Ok(())
}

async fn classify(
    conn: &mut PgConnection,
    request: &ImportRequest,
    names: &PhysicalNames,
    batches: &[ReadyBatch],
) -> Result<Class, ImportError> {
    let mut replay = false;
    let mut absent = false;
    for batch in batches {
        match full_import::recover(conn, request.source(), &names.table, &batch.batch_id, &batch.payload_digest).await {
            Ok(Recovery::Replay) => replay = true,
            Ok(Recovery::Absent) => absent = true,
            Ok(Recovery::Conflict) => return Err(child(ImportError::new("hash_conflict", "receipt"))),
            Err(error) => return Err(from_pipeline(error)),
        }
    }
    if replay && absent {
        Err(child(ImportError::new("incomplete_receipt", "mixed")))
    } else if replay {
        Ok(Class::Replay)
    } else {
        Ok(Class::Insert)
    }
}

fn replay_result(request: &ImportRequest, names: &PhysicalNames, parsed: &ParsedFile) -> ImportResult {
    ImportResult {
        protocol: RESULT_PROTOCOL,
        state: "awaiting_review",
        published: false,
        role_grant: false,
        stored_run_state: "awaiting_publish",
        replay: true,
        run_id: None,
        physical_namespace: names.namespace.clone(),
        physical_table: names.table.clone(),
        physical_columns: names.columns.clone(),
        rows: parsed.rows,
        object_sha256: request.object_sha_hex().to_string(),
        config_digest: digest_text(&config_preimage(request)),
        parsing_mode: "trusted_public_fixture",
        kernel_restriction: "unsupported",
        child_reaped: parsed.child_reaped,
        stored_position_codec: "snapshot-zero/v1",
        stored_position_lineage: "full-import.snapshot",
        target_generation: None,
        source: request.source().to_string(),
    }
}

async fn insert(
    mut conn: PgConnection,
    request: &ImportRequest,
    names: &PhysicalNames,
    parsed: &ParsedFile,
) -> Result<StagedImport, ImportError> {
    sqlx::query("BEGIN").execute(&mut conn).await.map_err(db)?;
    match insert_body(&mut conn, request, names, parsed).await {
        Ok(mut staged) => {
            staged.conn = Some(conn);
            Ok(staged)
        }
        Err(error) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut conn).await;
            Err(error)
        }
    }
}

async fn insert_body(
    conn: &mut PgConnection,
    request: &ImportRequest,
    names: &PhysicalNames,
    parsed: &ParsedFile,
) -> Result<StagedImport, ImportError> {
    let owner: String = sqlx::query_scalar("SELECT pg_catalog.gen_random_uuid()::text")
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?;
    let generation = full_import::adopt_generation(conn, request.shard(), &owner)
        .await
        .map_err(from_pipeline)?;
    sqlx::query("SELECT _oo.full_import_bind_session($1, $2::uuid, $3)")
        .bind(request.shard())
        .bind(&owner)
        .bind(generation)
        .execute(&mut *conn)
        .await
        .map_err(db)?;
    let config_digest = digest_text(&config_preimage(request));
    let type_digest = digest_text(&type_preimage(request.columns()));
    let schema_digest = digest_text(&schema_preimage(request.namespace(), request.table(), request.columns()));
    let input_digest = request.object_sha_hex().to_string();
    register_all(conn, request, &config_digest).await?;
    let run_id = open_run(
        conn,
        request,
        generation,
        &owner,
        &config_digest,
        &type_digest,
        &input_digest,
        &schema_digest,
        &names.table,
    )
    .await?;
    prepare_shadow(conn, request, generation, &owner, &run_id, &config_digest, &names.table).await?;
    let statement = copy_statement(&names.table, &names.columns)?;
    let hook = hook_digest();
    for batch in &parsed.batches {
        copy_one(conn, &statement, batch).await?;
        write_one(
            conn,
            request,
            generation,
            &owner,
            &run_id,
            &names.table,
            batch,
            &config_digest,
            &type_digest,
            &hook,
            &schema_digest,
            &input_digest,
        )
        .await?;
    }
    let backend_pid: i32 = sqlx::query_scalar("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?;
    Ok(StagedImport {
        conn: None,
        backend_pid,
        rows: parsed.rows,
        child_reaped: parsed.child_reaped,
        names: names.clone(),
        source: request.source().to_string(),
        shard: request.shard().to_string(),
        owner,
        generation,
        run_id,
        object_sha256: request.object_sha_hex().to_string(),
        config_digest,
        type_digest,
        schema_digest,
        input_digest,
        canonical: parsed.canonical.clone(),
        namespace: request.namespace().to_string(),
        table: request.table().to_string(),
    })
}

async fn register_all(conn: &mut PgConnection, request: &ImportRequest, config_digest: &str) -> Result<(), ImportError> {
    register_one(conn, request, config_digest, Level::Namespace, "", "").await?;
    register_one(conn, request, config_digest, Level::Table, request.table(), "").await?;
    for column in request.columns() {
        register_one(conn, request, config_digest, Level::Column, request.table(), column.name()).await?;
    }
    Ok(())
}

async fn register_one(
    conn: &mut PgConnection,
    request: &ImportRequest,
    config_digest: &str,
    level: Level,
    table: &str,
    column: &str,
) -> Result<(), ImportError> {
    let planned = plan_physical_mapping(&MappingRequest {
        source: request.source().to_string(),
        config_digest: config_digest.to_string(),
        run_generation: 1,
        level,
        logical_namespace: request.namespace().to_string(),
        logical_table: table.to_string(),
        logical_column: column.to_string(),
    })
    .map_err(|_| child(ImportError::new("closed_input", "mapping")))?;
    sqlx::query("SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(request.source())
        .bind(config_digest)
        .bind(1_i64)
        .bind(level.as_str())
        .bind(request.namespace())
        .bind(table)
        .bind(column)
        .bind(planned.identity_digest.as_slice())
        .execute(&mut *conn)
        .await
        .map_err(db)?;
    let stored: String = sqlx::query_scalar("SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)")
        .bind(request.source())
        .bind(config_digest)
        .bind(1_i64)
        .bind(level.as_str())
        .bind(request.namespace())
        .bind(table)
        .bind(column)
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?;
    if stored != planned.physical_name {
        return Err(child(ImportError::new("mapping_conflict", "mapping")));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn open_run(
    conn: &mut PgConnection,
    request: &ImportRequest,
    generation: i64,
    owner: &str,
    config_digest: &str,
    type_digest: &str,
    input_digest: &str,
    schema_digest: &str,
    physical: &str,
) -> Result<String, ImportError> {
    let schemas = vec![request.namespace().to_string()];
    let tables = vec![request.table().to_string()];
    let physical_names = vec![physical.to_string()];
    sqlx::query_scalar(
        "SELECT _oo.full_import_open_run($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
    )
    .bind(request.shard())
    .bind(owner)
    .bind(generation)
    .bind(request.source())
    .bind(1_i64)
    .bind(config_digest)
    .bind(type_digest)
    .bind(input_digest)
    .bind(schema_digest)
    .bind(&schemas)
    .bind(&tables)
    .bind(&physical_names)
    .fetch_one(&mut *conn)
    .await
    .map_err(db)
}

async fn prepare_shadow(
    conn: &mut PgConnection,
    request: &ImportRequest,
    generation: i64,
    owner: &str,
    run_id: &str,
    config_digest: &str,
    physical: &str,
) -> Result<(), ImportError> {
    let idents: Vec<String> = request.columns().iter().map(|column| column.name().to_string()).collect();
    let oids: Vec<i64> = request.columns().iter().map(|column| column.oid()).collect();
    let typmods: Vec<i32> = request.columns().iter().map(|column| column.typmod()).collect();
    let required: Vec<bool> = request.columns().iter().map(|column| !column.nullable()).collect();
    let shadow: String = sqlx::query_scalar(
        "SELECT _oo.full_import_prepare_shadow($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
    )
    .bind(request.shard())
    .bind(owner)
    .bind(generation)
    .bind(run_id)
    .bind(0_i32)
    .bind(request.source())
    .bind(config_digest)
    .bind(1_i64)
    .bind(request.namespace())
    .bind(request.table())
    .bind(&idents)
    .bind(&oids)
    .bind(&typmods)
    .bind(&required)
    .fetch_one(&mut *conn)
    .await
    .map_err(db)?;
    if shadow != physical {
        return Err(child(ImportError::new("mapping_conflict", "mapping")));
    }
    Ok(())
}

fn copy_statement(table: &str, columns: &[String]) -> Result<String, ImportError> {
    let quoted = quote_physical_name(table).map_err(|_| child(ImportError::new("mapping_conflict", "mapping")))?;
    let mut list = String::from("\"_oo_pos\"");
    for column in columns {
        list.push_str(", ");
        list.push_str(
            &quote_physical_name(column).map_err(|_| child(ImportError::new("mapping_conflict", "mapping")))?,
        );
    }
    Ok(format!("COPY _oo.{quoted} ({list}) FROM STDIN WITH (FORMAT binary)"))
}

async fn copy_one(conn: &mut PgConnection, statement: &str, batch: &ReadyBatch) -> Result<(), ImportError> {
    let columns = copy_columns(&batch.typed);
    let sent = copy_sink::send_batch(conn, statement, &batch.typed, &columns, &batch.positions, COPY_BUDGETS)
        .await
        .map_err(sink_of)?;
    let sent_sha = hex_encode(sent.hasher.finalize().as_ref());
    if sent_sha != batch.copy_sha || sent.rows != batch.typed_rows {
        return Err(child(ImportError::new("database", "copy_bytes")));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn write_one(
    conn: &mut PgConnection,
    request: &ImportRequest,
    generation: i64,
    owner: &str,
    run_id: &str,
    physical: &str,
    batch: &ReadyBatch,
    config_digest: &str,
    type_digest: &str,
    hook: &str,
    schema_digest: &str,
    input_digest: &str,
) -> Result<(), ImportError> {
    let rows = i64::try_from(batch.typed_rows).unwrap_or(i64::MAX);
    let wrote: String = sqlx::query_scalar(
        "SELECT _oo.full_import_write_batch($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15::numeric, $16, $17, $18::text[], $19::bigint[], $20::integer[], $21::bigint[], $22::text[])",
    )
    .bind(request.shard())
    .bind(owner)
    .bind(generation)
    .bind(run_id)
    .bind(0_i32)
    .bind(request.source())
    .bind(physical)
    .bind(&batch.batch_id)
    .bind(&batch.payload_digest)
    .bind(config_digest)
    .bind(type_digest)
    .bind(hook)
    .bind(schema_digest)
    .bind(input_digest)
    .bind(&batch.applied_pos)
    .bind(rows)
    .bind(0_i64)
    .bind(Vec::<String>::new())
    .bind(Vec::<i64>::new())
    .bind(Vec::<i32>::new())
    .bind(Vec::<i64>::new())
    .bind(Vec::<String>::new())
    .fetch_one(&mut *conn)
    .await
    .map_err(db)?;
    if wrote != "inserted" {
        return Err(child(ImportError::new("incomplete_receipt", "replay")));
    }
    Ok(())
}

fn sink_of(error: SinkError) -> ImportError {
    sink_error(error).with_child(true)
}

fn child(error: ImportError) -> ImportError {
    error.with_child(true)
}

fn db(error: sqlx::Error) -> ImportError {
    let state = error
        .as_database_error()
        .and_then(|database| database.code().map(|code| code.into_owned()))
        .unwrap_or_default();
    map_state(&state).with_child(true)
}

fn from_pipeline(error: full_import::ImportError) -> ImportError {
    match error.sqlstate() {
        Some(state) => map_state(state).with_child(true),
        None => child(ImportError::new("database", "sqlstate")),
    }
}

fn map_state(state: &str) -> ImportError {
    match state {
        "OO010" => ImportError::new("hash_conflict", "receipt"),
        "OO014" => ImportError::new("writer_gate", "writer"),
        "OO018" => ImportError::new("fence_session", "fence"),
        "OO022" => ImportError::new("mapping_conflict", "mapping"),
        "OO024" => ImportError::new("mapping_absent", "mapping"),
        "OO030" => ImportError::new("coverage", "applied_rows"),
        "OO031" => ImportError::new("run_closed", "run"),
        "23514" | "22023" => ImportError::new("closed_input", "sqlstate"),
        _ => ImportError::new("database", "sqlstate"),
    }
}


