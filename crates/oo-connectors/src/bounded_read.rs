//! Paced read of one selected table. Protocol [`WINDOW_PROTOCOL`].
//!
//! One business connection holds one repeatable-read, read-only transaction and
//! one named portal. Each `Execute` still asks for one row. The rate ledger is
//! unchanged: a full window waits for the next second instead of failing the
//! export. `read_typed` still refuses a single fetch above that window.
//!
//! The Arrow file is renamed into place only after the portal completes inside
//! the row cap, the byte cap, and the deadline. Cancel, deadline, byte cap, and
//! row cap delete the partial file. This path does not issue `SAVEPOINT`,
//! source DDL, or `pg_terminate_backend`.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;
use std::time::{Duration, Instant};

use arrow_ipc::MetadataVersion;
use arrow_ipc::writer::{IpcWriteOptions, StreamWriter};
use sqlx::postgres::{
    PgArguments, PgIncomingBounds, PgNamedPortal, PgPortalAllowance, PgPortalOutcome,
};
use sqlx::{Row, ValueRef};

use crate::error::{self, SourceError, SourceErrorKind, SqlState};
use crate::model::sha256_hex;
use crate::session::{self, PgSession};
use crate::snapshot::CLEANUP_OBSERVATION_BOUND_MS;
use crate::sql;
use crate::types::{
    ColumnPlan, DecodedCell, MapDecision, PgTypePolicy, RunBinding, assemble_batch,
    compose_presence_scan, compose_scan, decode_payload, guard_plans, map_column, require_binary,
};

/// Window-read protocol. This does not grow `oo.source.pgsql/v1`.
pub const WINDOW_PROTOCOL: &str = "oo.source.pgsql.window/v1";

/// Largest Arrow page this API will buffer before writing it.
pub const WINDOW_PAGE_ROWS_MAX: u32 = 4_096;

const WATCH_SLICE: Duration = Duration::from_millis(50);
const MESSAGE_FLOOR: u32 = 8_192;

/// Caller inputs for [`PgSession::read_window`].
pub struct WindowRequest<'a> {
    /// Source, schema, and table. The source must be this session.
    pub binding: &'a RunBinding,
    /// Same policy object `read_typed` accepts.
    pub policy: PgTypePolicy,
    /// Wall clock for the whole export. This is not `statement_timeout`.
    pub deadline: Instant,
    /// Set to interrupt an in-flight `Execute`.
    pub cancel: &'a AtomicBool,
    /// Final Arrow IPC path. A sibling partial file is renamed here at the end.
    pub arrow_out: &'a Path,
    /// Rows buffered in one Arrow batch. `1..=WINDOW_PAGE_ROWS_MAX`.
    pub page_rows: u32,
    /// Written and synced as soon as `pg_export_snapshot` returns.
    /// The id can be imported only while this call is still running.
    pub snapshot_file: Option<&'a Path>,
}

/// Counts observed from decoded rows and from the rate ledger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowReceipt {
    /// [`WINDOW_PROTOCOL`].
    pub protocol: &'static str,
    /// Decoded rows written to `arrow_out`.
    pub rows: u64,
    /// SQL text bytes plus binary payloads. Nulls add nothing.
    pub payload_bytes: u64,
    /// Arrow record batches written. An empty read is one empty batch.
    pub pages: u32,
    /// Distinct one-second rate windows that accepted a row.
    pub windows_used: u32,
    /// Largest debit count inside one of those windows.
    pub max_rows_in_window: u32,
    /// How many reserves waited for the next window.
    pub rate_waits: u32,
    /// `pg_export_snapshot` text for this transaction.
    pub snapshot: String,
    /// SHA-256 of the published Arrow file.
    pub arrow_sha256: String,
}

struct Plan {
    sql: String,
    plans: Vec<ColumnPlan>,
    key_attnums: Vec<i32>,
    width: usize,
}

struct Meter {
    windows: BTreeMap<Instant, u32>,
    waits: u32,
}

struct PartialFile {
    path: PathBuf,
    published: bool,
}

impl Drop for PartialFile {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl PgSession {
    /// Read one authenticated table into an Arrow IPC file.
    ///
    /// `guard` is a second session for the same source, user, and database. It
    /// only cancels this backend and reads `pg_stat_activity`. The snapshot and
    /// the portal stay on `self`.
    pub async fn read_window(
        &mut self,
        guard: &mut PgSession,
        request: WindowRequest<'_>,
    ) -> Result<WindowReceipt, SourceError> {
        self.precheck(guard, &request)?;
        let partial = PartialFile::create(request.arrow_out)?;
        let read = self.scan(guard, &request, &partial.path).await;
        match read {
            Ok(receipt) => {
                publish(&partial.path, request.arrow_out)?;
                let arrow_sha256 = match hash_file(request.arrow_out) {
                    Ok(hash) => hash,
                    Err(err) => {
                        let _ = fs::remove_file(request.arrow_out);
                        return Err(err);
                    }
                };
                let _keep = partial.published_ok();
                Ok(WindowReceipt {
                    arrow_sha256,
                    ..receipt
                })
            }
            Err(err) => Err(err),
        }
    }

    fn precheck(&self, guard: &PgSession, request: &WindowRequest<'_>) -> Result<(), SourceError> {
        self.ensure_open()?;
        guard.ensure_open()?;
        if self.in_transaction || guard.in_transaction {
            return Err(SourceError::protocol("nested_transaction"));
        }
        if request.binding.source_id != self.source_id {
            return Err(SourceError::config("binding_source"));
        }
        if !self.selection.tables().iter().any(|table| {
            table.schema() == request.binding.schema && table.table() == request.binding.table
        }) {
            return Err(SourceError::config("binding_table"));
        }
        if !self.identity_aligned || self.current_user != self.session_user {
            return Err(SourceError::config("session_user_mismatch"));
        }
        if !self.startup_clean {
            return Err(SourceError::config("startup_options"));
        }
        if self.admission != "tested" {
            return Err(SourceError::version(match self.admission {
                "below_minimum" => "version_below_minimum",
                "untested" => "version_untested",
                _ => "version_unknown",
            }));
        }
        if guard.source_id != self.source_id
            || guard.current_user != self.current_user
            || guard.session_user != self.session_user
            || guard.database_name != self.database_name
            || !guard.identity_aligned
            || !guard.startup_clean
            || guard.admission != "tested"
        {
            return Err(SourceError::config("guard_identity"));
        }
        if request.page_rows == 0 || request.page_rows > WINDOW_PAGE_ROWS_MAX {
            return Err(SourceError::config("page_rows"));
        }
        if request.arrow_out.as_os_str().is_empty() || request.arrow_out.exists() {
            return Err(SourceError::config("arrow_out"));
        }
        let Some(parent) = request.arrow_out.parent() else {
            return Err(SourceError::config("arrow_out"));
        };
        if !parent.as_os_str().is_empty() && !parent.is_dir() {
            return Err(SourceError::config("arrow_out"));
        }
        if Instant::now() >= request.deadline {
            return Err(SourceError::budget("deadline"));
        }
        Ok(())
    }

    async fn scan(
        &mut self,
        guard: &mut PgSession,
        request: &WindowRequest<'_>,
        partial: &Path,
    ) -> Result<WindowReceipt, SourceError> {
        self.begin().await?;
        let outcome = self.scan_open(guard, request, partial).await;
        let cleanup = self.finish_scan().await;
        prefer(outcome, cleanup)
    }

    async fn finish_scan(&mut self) -> Result<(), SourceError> {
        if self.rollback_skipped()
            || !self.conn.incoming_synchronized()
            || self.conn.named_portal_poisoned()
        {
            self.mark_skip_rollback();
            return Ok(());
        }
        let _ = self.conn.set_incoming_bounds(None);
        self.finish(Ok(())).await
    }

    async fn scan_open(
        &mut self,
        guard: &mut PgSession,
        request: &WindowRequest<'_>,
        partial: &Path,
    ) -> Result<WindowReceipt, SourceError> {
        let backend = self.backend_key().await?;
        let snapshot = self.export_snapshot().await?;
        if let Some(path) = request.snapshot_file {
            write_snapshot_file(path, &snapshot)?;
        }
        self.recheck_settings().await?;
        self.recheck_identity().await?;
        self.recheck_privilege(&request.binding.schema, &request.binding.table)
            .await?;
        let plan = self.plan_table(request).await?;
        let mut used = u64::try_from(plan.sql.len()).unwrap_or(u64::MAX);
        if used > self.budget.max_bytes() {
            return Err(SourceError::budget("max_bytes"));
        }
        self.arm_portal_bounds(plan.width)?;
        let statement_timeout = self.budget.statement_timeout();
        let mut portal = match watch(
            guard,
            request,
            &backend,
            statement_timeout,
            Box::pin(
                self.conn
                    .open_named_portal(&plan.sql, PgArguments::default()),
            ),
        )
        .await
        {
            Ok(Ok(portal)) => portal,
            Ok(Err(err)) => return Err(classify(error::map_sqlx(err), request)),
            Err(err) => return Err(err),
        };
        let pulled = pull_portal(
            &mut portal,
            guard,
            request,
            &plan,
            &backend,
            &self.source_id,
            self.budget.max_rows(),
            self.budget.max_bytes(),
            statement_timeout,
            &mut used,
            partial,
        )
        .await;
        let closed = portal.close().await;
        if closed.is_err() || self.conn.named_portal_poisoned() {
            self.mark_skip_rollback();
        }
        confirm_quiet(guard, &backend).await?;
        let mut receipt = pulled?;
        receipt.snapshot = snapshot;
        receipt.payload_bytes = used;
        Ok(receipt)
    }

    async fn backend_key(&mut self) -> Result<BackendKey, SourceError> {
        let row = session::fetch_one(self, sqlx::query(sql::SNAPSHOT_BACKEND)).await?;
        let pid = session::cell_i64(&row, 0)?;
        let backend_start = session::cell_string(&row, 1)?;
        let pid = i32::try_from(pid).map_err(|_| SourceError::protocol("backend_key"))?;
        if backend_start.is_empty() || backend_start.len() > 64 || backend_start.contains('\0') {
            return Err(SourceError::protocol("backend_key"));
        }
        Ok(BackendKey { pid, backend_start })
    }

    async fn export_snapshot(&mut self) -> Result<String, SourceError> {
        let row = session::fetch_one(self, sqlx::query(sql::EXPORT_SNAPSHOT)).await?;
        let raw = session::cell_string(&row, 0)?;
        if raw.is_empty()
            || raw.len() > 80
            || raw.chars().any(|ch| !(ch.is_ascii_hexdigit() || ch == '-'))
        {
            return Err(SourceError::protocol("snapshot"));
        }
        Ok(raw)
    }

    async fn recheck_settings(&mut self) -> Result<(), SourceError> {
        if self.budget.max_rows() < 4 {
            return Err(SourceError::budget("max_rows"));
        }
        let names = vec![
            "default_transaction_read_only".to_string(),
            "idle_in_transaction_session_timeout".to_string(),
            "lock_timeout".to_string(),
            "statement_timeout".to_string(),
        ];
        let admitted = self.admit_rows(session::Admit::Exact {
            wanted: 4,
            natural: true,
        })?;
        self.arm_rows(admitted.limit);
        let rows = session::fetch_all(self, sqlx::query(sql::BUDGET_SETTINGS).bind(&names)).await?;
        if rows.len() != 4 {
            return Err(SourceError::config("setting_mismatch"));
        }
        let statement_ms = session::duration_ms(self.budget.statement_timeout())?;
        let lock_ms = session::duration_ms(self.budget.lock_timeout())?;
        expect_timeout(&rows, "statement_timeout", &statement_ms)?;
        expect_timeout(&rows, "lock_timeout", &lock_ms)?;
        expect_timeout(&rows, "idle_in_transaction_session_timeout", &statement_ms)?;
        expect_flag(&rows, "default_transaction_read_only", "on")
    }

    async fn recheck_identity(&mut self) -> Result<(), SourceError> {
        let row = session::fetch_one(self, sqlx::query(sql::SNAPSHOT_NODE)).await?;
        let in_recovery = session::cell_bool(&row, 0)?;
        let database = session::cell_string(&row, 2)?;
        let session_user = session::cell_string(&row, 3)?;
        let current_user = session::cell_string(&row, 4)?;
        if in_recovery {
            return Err(SourceError::protocol("in_recovery"));
        }
        if database != self.database_name
            || session_user != self.session_user
            || current_user != self.current_user
            || current_user != session_user
        {
            return Err(SourceError::config("session_user_mismatch"));
        }
        Ok(())
    }

    async fn recheck_privilege(&mut self, schema: &str, table: &str) -> Result<(), SourceError> {
        let admitted = self.admit_rows(session::Admit::Exact {
            wanted: 2,
            natural: true,
        })?;
        self.arm_rows(admitted.limit);
        let rows = session::fetch_all(
            self,
            sqlx::query(sql::SNAPSHOT_RELATION).bind(schema).bind(table),
        )
        .await?;
        if rows.len() != 1 {
            return Err(SourceError::catalog("selected_table_missing"));
        }
        let row = &rows[0];
        let relkind = session::cell_string(row, 1)?;
        if relkind != "r" && relkind != "p" && relkind != "v" {
            return Err(SourceError::catalog("relkind"));
        }
        let rls = session::cell_bool(row, 2)?;
        if rls {
            return Err(SourceError::protocol("rls_enabled"));
        }
        let select = session::cell_opt_bool(row, 4)?;
        let insert = session::cell_opt_bool(row, 5)?;
        let update = session::cell_opt_bool(row, 6)?;
        let delete = session::cell_opt_bool(row, 7)?;
        let truncate = session::cell_opt_bool(row, 8)?;
        if select != Some(true)
            || insert != Some(false)
            || update != Some(false)
            || delete != Some(false)
            || truncate != Some(false)
        {
            return Err(SourceError::privilege("table_privilege"));
        }
        Ok(())
    }

    async fn plan_table(&mut self, request: &WindowRequest<'_>) -> Result<Plan, SourceError> {
        let catalog = self.collect_catalog().await?;
        let Some(table) = catalog.tables.into_iter().find(|table| {
            table.schema == request.binding.schema && table.name == request.binding.table
        }) else {
            return Err(SourceError::catalog("selected_table_missing"));
        };
        let key_attnums: Vec<i32> = table.primary_key.iter().map(|key| key.attnum).collect();
        let graph = self.load_type_graph(&table.columns).await?;
        let mut plans = Vec::with_capacity(table.columns.len());
        for column in &table.columns {
            let key = key_attnums.contains(&column.attnum);
            plans.push(map_column(column, &graph, key));
        }
        guard_plans(&plans, request.policy)?;
        let schema_sql = self.quote_one(&request.binding.schema).await?;
        let table_sql = self.quote_one(&request.binding.table).await?;
        let mut column_sql = Vec::new();
        for plan in &plans {
            if plan.mapping.decision != MapDecision::Lossless {
                continue;
            }
            column_sql.push(self.quote_one(&plan.name).await?);
        }
        let sql = if column_sql.is_empty() {
            compose_presence_scan(&schema_sql, &table_sql)?
        } else {
            compose_scan(&schema_sql, &table_sql, &column_sql)?
        };
        if sql.to_ascii_uppercase().contains("SAVEPOINT") {
            return Err(SourceError::protocol("savepoint"));
        }
        let width = if column_sql.is_empty() {
            1
        } else {
            column_sql.len()
        };
        Ok(Plan {
            sql,
            plans,
            key_attnums,
            width,
        })
    }

    fn arm_portal_bounds(&mut self, width: usize) -> Result<(), SourceError> {
        let cap =
            u32::try_from(self.budget.max_bytes().min(u64::from(u32::MAX))).unwrap_or(u32::MAX);
        let row = cap.max(64);
        let message = row.saturating_add(64).max(MESSAGE_FLOOR);
        let columns = u16::try_from(width).map_err(|_| SourceError::protocol("row_width"))?;
        let bounds = PgIncomingBounds::try_new(message, row, row, columns)
            .map_err(|_| SourceError::config("incoming_bounds"))?;
        self.conn
            .set_incoming_bounds(Some(bounds))
            .map_err(error::map_sqlx)
    }
}

struct BackendKey {
    pid: i32,
    backend_start: String,
}

#[allow(clippy::too_many_arguments)]
async fn pull_portal(
    portal: &mut PgNamedPortal<'_>,
    guard: &mut PgSession,
    request: &WindowRequest<'_>,
    plan: &Plan,
    backend: &BackendKey,
    source_id: &str,
    max_rows: u32,
    max_bytes: u64,
    statement_timeout: Duration,
    used: &mut u64,
    partial: &Path,
) -> Result<WindowReceipt, SourceError> {
    let mut page: Vec<Vec<DecodedCell>> = Vec::new();
    let mut sink = IpcFile::create(partial)?;
    let mut meter = Meter {
        windows: BTreeMap::new(),
        waits: 0,
    };
    let mut rows = 0_u64;
    let cap = u64::from(max_rows);
    let page_rows = usize::try_from(request.page_rows).unwrap_or(usize::MAX);
    loop {
        if request.cancel.load(Ordering::Relaxed) {
            return Err(SourceError::protocol("cancelled"));
        }
        if Instant::now() >= request.deadline {
            return Err(SourceError::budget("deadline"));
        }
        let paced = session::pace_reserve_rows(source_id, 1, request.deadline).await?;
        if paced.waited {
            meter.waits = meter.waits.saturating_add(1);
        }
        let window = paced.hold.window_started();
        let allowance = one_row()?;
        let step = match watch(
            guard,
            request,
            backend,
            statement_timeout,
            Box::pin(portal.pull(allowance)),
        )
        .await
        {
            Ok(Ok(step)) => step,
            Ok(Err(err)) => {
                drop(paced.hold);
                return Err(classify(error::map_sqlx(err), request));
            }
            Err(err) => {
                drop(paced.hold);
                return Err(err);
            }
        };
        let (row, finished) = match step.outcome {
            PgPortalOutcome::Suspended(row) => {
                paced.hold.settle(1);
                if rows + 1 == cap {
                    return Err(SourceError::budget("max_rows"));
                }
                (row, false)
            }
            PgPortalOutcome::Complete { row, .. } => {
                paced.hold.settle(usize::from(row.is_some()));
                (
                    match row {
                        Some(row) => row,
                        None => break,
                    },
                    true,
                )
            }
        };
        if rows + 1 > cap {
            return Err(SourceError::budget("max_rows"));
        }
        let (cells, payload) = decode_row(&plan.plans, &row)?;
        charge(used, max_bytes, payload)?;
        note_window(&mut meter, window);
        page.push(cells);
        rows += 1;
        if page.len() == page_rows {
            flush_page(
                &mut sink,
                request.binding,
                &plan.plans,
                &plan.key_attnums,
                &mut page,
            )?;
        }
        if finished {
            break;
        }
    }
    if !page.is_empty() || rows == 0 {
        flush_page(
            &mut sink,
            request.binding,
            &plan.plans,
            &plan.key_attnums,
            &mut page,
        )?;
    }
    let pages = sink.finish()?;
    Ok(WindowReceipt {
        protocol: WINDOW_PROTOCOL,
        rows,
        payload_bytes: 0,
        pages,
        windows_used: u32::try_from(meter.windows.len()).unwrap_or(u32::MAX),
        max_rows_in_window: meter.windows.values().copied().max().unwrap_or(0),
        rate_waits: meter.waits,
        snapshot: String::new(),
        arrow_sha256: String::new(),
    })
}

fn one_row() -> Result<PgPortalAllowance, SourceError> {
    PgPortalAllowance::positive(1).map_err(|_| SourceError::protocol("portal_allowance"))
}

fn note_window(meter: &mut Meter, window: Instant) {
    let count = meter.windows.entry(window).or_insert(0);
    *count = count.saturating_add(1);
}

fn decode_row(
    plans: &[ColumnPlan],
    row: &sqlx::postgres::PgRow,
) -> Result<(Vec<DecodedCell>, u64), SourceError> {
    let readable: Vec<&ColumnPlan> = plans
        .iter()
        .filter(|plan| plan.mapping.decision == MapDecision::Lossless)
        .collect();
    if readable.is_empty() {
        let raw = row
            .try_get_raw(0)
            .map_err(|_| SourceError::protocol("binary_payload"))?;
        require_binary(raw.format())?;
        return Ok((Vec::new(), 0));
    }
    if row.columns().len() != readable.len() {
        return Err(SourceError::protocol("row_width"));
    }
    let mut cells = Vec::with_capacity(readable.len());
    let mut payload = 0_u64;
    for (index, plan) in readable.iter().enumerate() {
        let raw = row
            .try_get_raw(index)
            .map_err(|_| SourceError::protocol("binary_payload"))?;
        require_binary(raw.format())?;
        let actual = raw
            .type_info()
            .oid()
            .map(|oid| i64::from(oid.0))
            .ok_or_else(|| SourceError::protocol("type_oid_mismatch"))?;
        if !plan.accepts_result_oid(actual) {
            return Err(SourceError::protocol("type_oid_mismatch"));
        }
        if raw.is_null() {
            cells.push(DecodedCell::sql_null());
            continue;
        }
        let bytes = raw
            .as_bytes()
            .map_err(|_| SourceError::protocol("binary_payload"))?;
        payload = payload.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        cells.push(decode_payload(plan, bytes));
    }
    Ok((cells, payload))
}

fn charge(used: &mut u64, max: u64, extra: u64) -> Result<(), SourceError> {
    let next = used
        .checked_add(extra)
        .ok_or_else(|| SourceError::budget("max_bytes"))?;
    if next > max {
        return Err(SourceError::budget("max_bytes"));
    }
    *used = next;
    Ok(())
}

fn flush_page(
    sink: &mut IpcFile,
    binding: &RunBinding,
    plans: &[ColumnPlan],
    key_attnums: &[i32],
    page: &mut Vec<Vec<DecodedCell>>,
) -> Result<(), SourceError> {
    let rows = std::mem::take(page);
    let batch = assemble_batch(binding, plans.to_vec(), key_attnums, rows)?;
    let record = batch.record_batch()?;
    sink.write_batch(&record)
}

struct IpcFile {
    file: Option<File>,
    writer: Option<StreamWriter<File>>,
    pages: u32,
}

impl IpcFile {
    fn create(path: &Path) -> Result<Self, SourceError> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| SourceError::protocol("arrow_ipc"))?;
        Ok(Self {
            file: Some(file),
            writer: None,
            pages: 0,
        })
    }

    fn write_batch(&mut self, record: &arrow_array::RecordBatch) -> Result<(), SourceError> {
        if self.writer.is_none() {
            let file = self
                .file
                .take()
                .ok_or_else(|| SourceError::protocol("arrow_ipc"))?;
            let options = IpcWriteOptions::try_new(8, false, MetadataVersion::V5)
                .map_err(|_| SourceError::protocol("arrow_ipc"))?;
            self.writer = Some(
                StreamWriter::try_new_with_options(file, record.schema().as_ref(), options)
                    .map_err(|_| SourceError::protocol("arrow_ipc"))?,
            );
        }
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| SourceError::protocol("arrow_ipc"))?;
        writer
            .write(record)
            .map_err(|_| SourceError::protocol("arrow_ipc"))?;
        self.pages = self.pages.saturating_add(1);
        Ok(())
    }

    fn finish(mut self) -> Result<u32, SourceError> {
        let mut writer = self
            .writer
            .take()
            .ok_or_else(|| SourceError::protocol("arrow_ipc"))?;
        writer
            .finish()
            .map_err(|_| SourceError::protocol("arrow_ipc"))?;
        let file = writer
            .into_inner()
            .map_err(|_| SourceError::protocol("arrow_ipc"))?;
        file.sync_all()
            .map_err(|_| SourceError::protocol("arrow_ipc"))?;
        Ok(self.pages)
    }
}

async fn watch<T>(
    guard: &mut PgSession,
    request: &WindowRequest<'_>,
    backend: &BackendKey,
    statement_timeout: Duration,
    mut op: Pin<Box<dyn Future<Output = T> + Send + '_>>,
) -> Result<T, SourceError> {
    if request.cancel.load(Ordering::Relaxed) {
        return Err(SourceError::protocol("cancelled"));
    }
    if Instant::now() >= request.deadline {
        return Err(SourceError::budget("deadline"));
    }
    let started = Instant::now();
    let mut drain_until: Option<Instant> = None;
    loop {
        let mut tick = Box::pin(tokio::time::sleep(WATCH_SLICE));
        let watched = std::future::poll_fn(|cx| {
            if let Poll::Ready(value) = op.as_mut().poll(cx) {
                return Poll::Ready(Some(value));
            }
            match tick.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            }
        })
        .await;
        if let Some(value) = watched {
            return Ok(value);
        }
        if drain_until.is_some_and(|until| Instant::now() >= until) {
            return Err(SourceError::protocol("cleanup_unconfirmed"));
        }
        if drain_until.is_none() && interrupted(request, statement_timeout, started) {
            signal_cancel(guard, backend).await?;
            drain_until =
                Some(Instant::now() + Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS));
        }
    }
}

fn interrupted(request: &WindowRequest<'_>, statement_timeout: Duration, started: Instant) -> bool {
    request.cancel.load(Ordering::Relaxed)
        || Instant::now() >= request.deadline
        || started.elapsed() >= statement_timeout
}

async fn signal_cancel(guard: &mut PgSession, backend: &BackendKey) -> Result<(), SourceError> {
    let query = sqlx::query(sql::CANCEL_BACKEND)
        .bind(backend.pid)
        .bind(backend.backend_start.as_str());
    match session::fetch_control(guard, query).await {
        Ok(_) => Ok(()),
        Err(err) if err.fact() == Some("row_not_found") => Ok(()),
        Err(_) => {
            guard.mark_skip_rollback();
            Err(SourceError::protocol("cleanup_unconfirmed"))
        }
    }
}

fn classify(mapped: SourceError, request: &WindowRequest<'_>) -> SourceError {
    if mapped.sqlstate() != Some("57014") {
        return mapped;
    }
    let state = mapped.sqlstate().and_then(SqlState::parse);
    if request.cancel.load(Ordering::Relaxed) {
        return SourceError::new(SourceErrorKind::Protocol, Some("cancelled"), state);
    }
    if Instant::now() >= request.deadline {
        return SourceError::new(SourceErrorKind::Budget, Some("deadline"), state);
    }
    mapped
}

async fn confirm_quiet(guard: &mut PgSession, backend: &BackendKey) -> Result<(), SourceError> {
    let deadline = Instant::now() + Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
    loop {
        match backend_state(guard, backend).await? {
            None => return Ok(()),
            Some(state) if state != "active" => return Ok(()),
            Some(_) if Instant::now() >= deadline => {
                return Err(SourceError::protocol("cleanup_unconfirmed"));
            }
            Some(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
}

async fn backend_state(
    guard: &mut PgSession,
    backend: &BackendKey,
) -> Result<Option<String>, SourceError> {
    let query = sqlx::query(sql::BACKEND_STATE)
        .bind(backend.pid)
        .bind(backend.backend_start.as_str());
    match session::fetch_control(guard, query).await {
        Ok(row) => Ok(Some(session::cell_string(&row, 0)?)),
        Err(err) if err.fact() == Some("row_not_found") => Ok(None),
        Err(err) => Err(err),
    }
}

fn expect_timeout(
    rows: &[sqlx::postgres::PgRow],
    name: &str,
    configured: &str,
) -> Result<(), SourceError> {
    let Some(row) = rows
        .iter()
        .find(|row| session::cell_string(row, 0).ok().as_deref() == Some(name))
    else {
        return Err(SourceError::config("setting_mismatch"));
    };
    let setting = session::cell_string(row, 1)?;
    let unit = session::cell_string(row, 2)?;
    if unit != "ms" || setting != configured {
        return Err(SourceError::config("setting_mismatch"));
    }
    Ok(())
}

fn expect_flag(
    rows: &[sqlx::postgres::PgRow],
    name: &str,
    configured: &str,
) -> Result<(), SourceError> {
    let Some(row) = rows
        .iter()
        .find(|row| session::cell_string(row, 0).ok().as_deref() == Some(name))
    else {
        return Err(SourceError::config("setting_mismatch"));
    };
    let setting = session::cell_string(row, 1)?;
    let unit = session::cell_string(row, 2)?;
    if !unit.is_empty() || setting != configured {
        return Err(SourceError::config("setting_mismatch"));
    }
    Ok(())
}

fn prefer<T>(
    outcome: Result<T, SourceError>,
    cleanup: Result<(), SourceError>,
) -> Result<T, SourceError> {
    match (outcome, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(err), _) if keeps(&err) => Err(err),
        (Err(err), Ok(())) => Err(err),
        (Ok(_), Err(err)) | (Err(_), Err(err)) => Err(err),
    }
}

fn keeps(err: &SourceError) -> bool {
    err.kind() == crate::error::SourceErrorKind::Budget
        || matches!(
            err.fact(),
            Some("cancelled" | "cleanup_unconfirmed" | "setting_mismatch")
        )
}

fn write_snapshot_file(path: &Path, snapshot: &str) -> Result<(), SourceError> {
    if path.as_os_str().is_empty() {
        return Err(SourceError::config("snapshot_file"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|_| SourceError::protocol("snapshot_file"))?;
    std::io::Write::write_all(&mut file, snapshot.as_bytes())
        .map_err(|_| SourceError::protocol("snapshot_file"))?;
    std::io::Write::write_all(&mut file, b"\n")
        .map_err(|_| SourceError::protocol("snapshot_file"))?;
    file.sync_all()
        .map_err(|_| SourceError::protocol("snapshot_file"))?;
    Ok(())
}

fn publish(partial: &Path, final_path: &Path) -> Result<(), SourceError> {
    fs::rename(partial, final_path).map_err(|_| SourceError::protocol("arrow_ipc"))
}

fn hash_file(path: &Path) -> Result<String, SourceError> {
    let bytes = fs::read(path).map_err(|_| SourceError::protocol("arrow_ipc"))?;
    Ok(sha256_hex(&bytes))
}

impl PartialFile {
    fn create(arrow_out: &Path) -> Result<Self, SourceError> {
        let path = partial_path(arrow_out)?;
        if path.exists() {
            fs::remove_file(&path).map_err(|_| SourceError::protocol("arrow_ipc"))?;
        }
        Ok(Self {
            path,
            published: false,
        })
    }

    fn published_ok(mut self) -> Self {
        self.published = true;
        self
    }
}

fn partial_path(arrow_out: &Path) -> Result<PathBuf, SourceError> {
    let name = arrow_out
        .file_name()
        .ok_or_else(|| SourceError::config("arrow_out"))?;
    let mut partial = arrow_out.to_path_buf();
    partial.set_file_name(format!("{}.partial", name.to_string_lossy()));
    if partial == arrow_out {
        return Err(SourceError::config("arrow_out"));
    }
    Ok(partial)
}
