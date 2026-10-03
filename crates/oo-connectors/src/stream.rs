//! Pull-driven full-snapshot Arrow stream. Protocol `oo.source.pgsql.stream/v1`.
//!
//! `next_batch` polls the only drive. That drive writes `Execute` while the
//! caller is waiting. There is no background reader and no unbounded queue.
//! One owner, one repeatable-read transaction, every selected table.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};
use std::time::{Duration, Instant};

use sqlx::postgres::{
    PgArguments, PgIncomingBounds, PgNamedPortal, PgPortalAllowance, PgPortalOutcome,
};
use sqlx::{Row, ValueRef};

use crate::error::{self, SourceError};
use crate::session::{self, PgSession};
use crate::snapshot::{self, RecheckCut, StreamGuard};
use crate::sql;
use crate::types::{
    assemble_batch, arrow_buffer_preimage, decode_payload, require_binary, ColumnPlan,
    DecodedCell, MapDecision, RunBinding, TypedBatch,
};

/// Stream protocol. This is not the snapshot protocol and not the whole reader.
pub const STREAM_PROTOCOL: &str = "oo.source.pgsql.stream/v1";

const MAX_BATCH_ROWS: u32 = 65_536;
const MESSAGE_FLOOR: u32 = 8_192;

/// Caps for one pull. `queue_batches` is 1. Config identity does not include these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamBudget {
    batch_rows: u32,
    batch_bytes: u64,
    max_value_bytes: u32,
    max_row_bytes: u32,
}

impl StreamBudget {
    /// `batch_rows` is at most 65536. `max_row_bytes` is at least 2 and at least
    /// `max_value_bytes`. The queue depth is fixed at one batch.
    pub fn try_new(
        batch_rows: u32,
        batch_bytes: u64,
        max_value_bytes: u32,
        max_row_bytes: u32,
    ) -> Result<Self, SourceError> {
        if batch_rows == 0 || batch_rows > MAX_BATCH_ROWS {
            return Err(SourceError::config("batch_rows"));
        }
        if batch_bytes == 0 {
            return Err(SourceError::config("batch_bytes"));
        }
        if max_value_bytes == 0 {
            return Err(SourceError::config("max_value"));
        }
        if max_row_bytes < 2 || max_row_bytes < max_value_bytes {
            return Err(SourceError::config("max_row"));
        }
        Ok(Self {
            batch_rows,
            batch_bytes,
            max_value_bytes,
            max_row_bytes,
        })
    }

    pub fn batch_rows(self) -> u32 {
        self.batch_rows
    }

    pub fn batch_bytes(self) -> u64 {
        self.batch_bytes
    }

    pub fn max_value_bytes(self) -> u32 {
        self.max_value_bytes
    }

    pub fn max_row_bytes(self) -> u32 {
        self.max_row_bytes
    }

    pub fn queue_batches(self) -> u32 {
        1
    }
}

/// Counts from this pull. Wire fields are connection-life `BytesMut` view peaks
/// sampled while the named portal is not holding the connection. They are not
/// RSS, not `malloc_usable_size`, and not backend heap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamAccounting {
    pub executes: u64,
    pub reserved_rows: u64,
    pub consumed_rows: u64,
    pub decoded_payload_bytes: u64,
    pub arrow_preimage_bytes: u64,
    pub arrow_assembly_bytes: u64,
    pub retained_batches: u64,
    pub retained_rows: u64,
    pub wire_view_capacity_peak: u64,
    pub wire_accepted_logical_payload: u64,
    pub wire_assembly_bytes: u64,
}

/// One demanded Arrow batch. `resumable` is false. A key digest on a row is the
/// existing canonical digest, not a resume cursor.
#[allow(exported_private_dependencies)]
pub struct StreamBatch {
    pub typed: TypedBatch,
    pub record_batch: arrow_array::RecordBatch,
    pub accounting: StreamAccounting,
    pub resumable: bool,
}

/// The scan of one table has no further row. This is not run exhaustion and
/// not [`crate::SnapshotFinished`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableEof {
    pub schema: String,
    pub table: String,
    pub rows_emitted: u64,
    pub columns: u32,
    pub zero_column: bool,
    pub resumable_key: bool,
    pub accounting: StreamAccounting,
}

/// One answer from [`crate::SnapshotOwner::next_batch`].
#[allow(exported_private_dependencies)]
pub enum StreamItem {
    Batch(Box<StreamBatch>),
    TableEof(TableEof),
    /// Every selected table has reached [`TableEof`]. The owner is still open.
    Exhausted(StreamAccounting),
}

pub(crate) struct StreamTablePlan {
    pub schema: String,
    pub name: String,
    pub sql: String,
    pub plans: Vec<ColumnPlan>,
    pub key_attnums: Vec<i32>,
    pub zero_column: bool,
    pub presence: bool,
    pub result_columns: usize,
    pub arrow_columns: u32,
    pub cut: RecheckCut,
    /// Inclusive lower bound. Empty means unbounded. An unkeyed scan stays empty.
    pub lower: Vec<snapshot::ProbeValue>,
    /// Exclusive upper bound. Empty means unbounded.
    pub upper: Vec<snapshot::ProbeValue>,
    /// Oid for each declared key column. Empty on an unkeyed scan.
    pub key_oids: Vec<i64>,
}

pub(crate) struct StreamPlan {
    pub source_id: String,
    pub generation: u64,
    pub input_digest: [u8; 32],
    pub deadline: Instant,
    pub statement_timeout: Duration,
    pub max_bytes: u64,
    pub batch_rows: u32,
    pub batch_bytes: u64,
    pub max_value_bytes: u32,
    pub max_row_bytes: u32,
    pub guard: StreamGuard,
    pub tables: Vec<StreamTablePlan>,
}

struct Acc {
    executes: u64,
    reserved_rows: u64,
    consumed_rows: u64,
    decoded_payload_bytes: u64,
    arrow_preimage_bytes: u64,
    arrow_assembly_bytes: u64,
    wire_view_capacity_peak: u64,
    wire_accepted_logical_payload: u64,
    wire_assembly_bytes: u64,
}

impl Acc {
    fn new() -> Self {
        Self {
            executes: 0,
            reserved_rows: 0,
            consumed_rows: 0,
            decoded_payload_bytes: 0,
            arrow_preimage_bytes: 0,
            arrow_assembly_bytes: 0,
            wire_view_capacity_peak: 0,
            wire_accepted_logical_payload: 0,
            wire_assembly_bytes: 0,
        }
    }

    fn view(&self, retained_rows: u64) -> StreamAccounting {
        StreamAccounting {
            executes: self.executes,
            reserved_rows: self.reserved_rows,
            consumed_rows: self.consumed_rows,
            decoded_payload_bytes: self.decoded_payload_bytes,
            arrow_preimage_bytes: self.arrow_preimage_bytes,
            arrow_assembly_bytes: self.arrow_assembly_bytes,
            retained_batches: 0,
            retained_rows,
            wire_view_capacity_peak: self.wire_view_capacity_peak,
            wire_accepted_logical_payload: self.wire_accepted_logical_payload,
            wire_assembly_bytes: self.wire_assembly_bytes,
        }
    }
}

struct Held {
    cells: Vec<DecodedCell>,
    payload: u64,
}

enum GateState {
    Idle,
    Demand(u64),
    Ready { id: u64, item: StreamItem },
    Stop,
}

struct GateInner {
    state: GateState,
    demand_waker: Option<Waker>,
    drive_waker: Option<Waker>,
}

struct Gate {
    inner: Mutex<GateInner>,
}

impl Gate {
    fn new() -> Self {
        Self {
            inner: Mutex::new(GateInner {
                state: GateState::Idle,
                demand_waker: None,
                drive_waker: None,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, GateInner> {
        self.inner.lock().unwrap_or_else(|err| err.into_inner())
    }

    fn demand(&self, id: u64) {
        let mut guard = self.lock();
        if matches!(guard.state, GateState::Stop) {
            return;
        }
        guard.state = GateState::Demand(id);
        if let Some(waker) = guard.drive_waker.take() {
            waker.wake();
        }
    }

    fn stop(&self) {
        let mut guard = self.lock();
        guard.state = GateState::Stop;
        if let Some(waker) = guard.drive_waker.take() {
            waker.wake();
        }
    }

    fn register_waiter(&self, waker: Waker) {
        self.lock().demand_waker = Some(waker);
    }

    fn take_ready(&self, id: u64) -> Option<StreamItem> {
        let mut guard = self.lock();
        match &guard.state {
            GateState::Ready { id: ready, .. } if *ready == id => {
                let GateState::Ready { item, .. } =
                    std::mem::replace(&mut guard.state, GateState::Idle)
                else {
                    return None;
                };
                if let Some(waker) = guard.drive_waker.take() {
                    waker.wake();
                }
                Some(item)
            }
            _ => None,
        }
    }
}

enum Signal {
    Stop,
    Demand(u64),
    Delivered,
}

async fn wait_demand(gate: &Gate) -> Signal {
    std::future::poll_fn(|cx| {
        let mut guard = gate.lock();
        if matches!(guard.state, GateState::Stop) {
            return Poll::Ready(Signal::Stop);
        }
        if let GateState::Demand(id) = guard.state {
            guard.state = GateState::Idle;
            return Poll::Ready(Signal::Demand(id));
        }
        guard.drive_waker = Some(cx.waker().clone());
        Poll::Pending
    })
    .await
}

async fn emit_item(gate: &Gate, id: u64, item: StreamItem) -> Signal {
    let mut pending = Some(item);
    std::future::poll_fn(|cx| {
        let mut guard = gate.lock();
        if matches!(guard.state, GateState::Stop) {
            return Poll::Ready(Signal::Stop);
        }
        if let Some(item) = pending.take() {
            guard.state = GateState::Ready { id, item };
            if let Some(waker) = guard.demand_waker.take() {
                waker.wake();
            }
        }
        if let GateState::Ready { id: ready, .. } = &guard.state
            && *ready == id
        {
            guard.drive_waker = Some(cx.waker().clone());
            return Poll::Pending;
        }
        Poll::Ready(Signal::Delivered)
    })
    .await
}

pub(crate) struct DriveEnd {
    pub data: PgSession,
    pub guard: PgSession,
    pub error: Option<SourceError>,
}

pub(crate) enum Next {
    Item(StreamItem),
    Ended(Option<Box<DriveEnd>>),
}

enum Outcome {
    Item(StreamItem),
    End(Box<DriveEnd>),
}

pub(crate) struct StreamSlot {
    gate: Arc<Gate>,
    drive: Option<Pin<Box<dyn Future<Output = DriveEnd> + Send>>>,
    next_id: u64,
}

impl StreamSlot {
    pub(crate) fn start(data: PgSession, guard: PgSession, plan: StreamPlan) -> Self {
        let gate = Arc::new(Gate::new());
        let gate_run = Arc::clone(&gate);
        Self {
            gate,
            drive: Some(Box::pin(run(data, guard, plan, gate_run))),
            next_id: 1,
        }
    }

    pub(crate) async fn next_item(&mut self) -> Next {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.gate.demand(id);
        let Some(mut drive) = self.drive.take() else {
            return Next::Ended(None);
        };
        let gate = Arc::clone(&self.gate);
        let outcome = std::future::poll_fn(|cx| {
            gate.register_waiter(cx.waker().clone());
            if let Some(item) = gate.take_ready(id) {
                return Poll::Ready(Outcome::Item(item));
            }
            match Pin::new(&mut drive).poll(cx) {
                Poll::Ready(end) => Poll::Ready(Outcome::End(Box::new(end))),
                Poll::Pending => {
                    if let Some(item) = gate.take_ready(id) {
                        Poll::Ready(Outcome::Item(item))
                    } else {
                        Poll::Pending
                    }
                }
            }
        })
        .await;
        match outcome {
            Outcome::Item(item) => {
                self.drive = Some(drive);
                Next::Item(item)
            }
            Outcome::End(end) => Next::Ended(Some(end)),
        }
    }

    pub(crate) async fn stop(mut self) -> DriveEnd {
        self.gate.stop();
        self.drive
            .take()
            .expect("stream drive")
            .await
    }
}

async fn run(
    mut data: PgSession,
    mut guard: PgSession,
    plan: StreamPlan,
    gate: Arc<Gate>,
) -> DriveEnd {
    let mut acc = Acc::new();
    sample(&data, &mut acc);
    let error = scan(&mut data, &mut guard, &plan, &gate, &mut acc)
        .await
        .err();
    DriveEnd { data, guard, error }
}

struct Drive<'a> {
    table: &'a StreamTablePlan,
    plan: &'a StreamPlan,
    gate: &'a Gate,
    acc: &'a mut Acc,
    held: &'a mut Option<Held>,
    rows_emitted: &'a mut u64,
}

enum Hold {
    Stop {
        closed: Result<(), SourceError>,
    },
    Fail {
        err: SourceError,
        closed: Result<(), SourceError>,
    },
    Done {
        closed: Result<(), SourceError>,
        id: u64,
        rows: Option<Vec<Vec<DecodedCell>>>,
        pending: Option<Held>,
    },
}

enum Tail {
    Eof,
    Row(Held),
}

#[allow(clippy::cognitive_complexity)]
async fn scan(
    data: &mut PgSession,
    guard: &mut PgSession,
    plan: &StreamPlan,
    gate: &Gate,
    acc: &mut Acc,
) -> Result<(), SourceError> {
    for table in &plan.tables {
        let mut held: Option<Held> = None;
        let mut rows_emitted = 0_u64;
        let mut tail: Option<Tail> = None;
        loop {
            if let Some(pending_tail) = tail.take() {
                match wait_demand(gate).await {
                    Signal::Stop => return Ok(()),
                    Signal::Delivered => {
                        tail = Some(pending_tail);
                    }
                    Signal::Demand(id) => match pending_tail {
                        Tail::Eof => {
                            let item = table_eof(table, rows_emitted, acc, false);
                            if matches!(emit_item(gate, id, item).await, Signal::Stop) {
                                return Ok(());
                            }
                            break;
                        }
                        Tail::Row(ready) => {
                            if ready.payload > plan.batch_bytes
                                || acc.decoded_payload_bytes + ready.payload > plan.max_bytes
                            {
                                return Err(SourceError::budget("max_bytes"));
                            }
                            acc.decoded_payload_bytes += ready.payload;
                            rows_emitted += 1;
                            let item = make_batch(table, plan, vec![ready.cells], acc, false)?;
                            if matches!(emit_item(gate, id, item).await, Signal::Stop) {
                                return Ok(());
                            }
                            tail = Some(Tail::Eof);
                        }
                    },
                }
                continue;
            }
            match wait_demand(gate).await {
                Signal::Stop => return Ok(()),
                Signal::Delivered => {}
                Signal::Demand(id) => {
                    ensure_deadline(plan.deadline)?;
                    snapshot::recheck_stream(data, &table.cut).await?;
                    snapshot::guard_still_current(guard, &plan.guard).await?;
                    data.execute_control(sql::SAVEPOINT_STREAM).await?;
                    if let Err(err) = install_bounds(data, plan, table.result_columns) {
                        return prefer(err, rollback_only(data).await);
                    }
                    let arguments = match portal_arguments(table) {
                        Ok(arguments) => arguments,
                        Err(err) => return prefer(err, rollback_only(data).await),
                    };
                    let mut open_err: Option<SourceError> = None;
                    let mut exit_slot = None;
                    {
                        match data.conn.open_named_portal(&table.sql, arguments).await {
                            Ok(portal) => {
                                exit_slot = Some(
                                    hold_portal(
                                        portal,
                                        id,
                                        Drive {
                                            table,
                                            plan,
                                            gate,
                                            acc,
                                            held: &mut held,
                                            rows_emitted: &mut rows_emitted,
                                        },
                                    )
                                    .await,
                                );
                            }
                            Err(err) => open_err = Some(error::map_sqlx(err)),
                        }
                    }
                    if let Some(mapped) = open_err {
                        if data.conn.named_portal_poisoned()
                            || !data.conn.incoming_synchronized()
                        {
                            data.mark_skip_rollback();
                            return Err(mapped);
                        }
                        return prefer(mapped, rollback_only(data).await);
                    }
                    let exit = exit_slot.ok_or_else(|| SourceError::protocol("stream_ended"))?;
                    match exit {
                        Hold::Stop { closed } => {
                            settle_after(data, closed, true, acc).await?;
                            return Ok(());
                        }
                        Hold::Fail { err, closed } => {
                            return prefer(err, settle_after(data, closed, true, acc).await);
                        }
                        Hold::Done {
                            closed,
                            id,
                            rows,
                            pending,
                        } => {
                            settle_after(data, closed, true, acc).await?;
                            if let Some(rows) = rows {
                                let item =
                                    make_batch(table, plan, rows, acc, pending.is_some())?;
                                if matches!(emit_item(gate, id, item).await, Signal::Stop) {
                                    return Ok(());
                                }
                                tail = Some(match pending {
                                    Some(ready) => Tail::Row(ready),
                                    None => Tail::Eof,
                                });
                            } else {
                                let item = table_eof(table, rows_emitted, acc, false);
                                if matches!(emit_item(gate, id, item).await, Signal::Stop) {
                                    return Ok(());
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    loop {
        match wait_demand(gate).await {
            Signal::Stop => return Ok(()),
            Signal::Demand(id) => {
                let item = StreamItem::Exhausted(acc.view(0));
                if matches!(emit_item(gate, id, item).await, Signal::Stop) {
                    return Ok(());
                }
            }
            Signal::Delivered => {}
        }
    }
}

async fn hold_portal(
    mut portal: PgNamedPortal<'_>,
    first_id: u64,
    mut drive: Drive<'_>,
) -> Hold {
    let mut queued = Some(first_id);
    loop {
        let id = if let Some(id) = queued.take() {
            id
        } else {
            match wait_demand(drive.gate).await {
                Signal::Stop => {
                    let closed = close_owned(portal).await;
                    return Hold::Stop { closed };
                }
                Signal::Demand(id) => id,
                Signal::Delivered => continue,
            }
        };
        let filled = fill_batch(&mut portal, &mut drive).await;
        match filled {
            Err(err) => {
                let closed = close_owned(portal).await;
                return Hold::Fail { err, closed };
            }
            Ok(Filled::Eof) => {
                let closed = close_owned(portal).await;
                return Hold::Done {
                    closed,
                    id,
                    rows: None,
                    pending: None,
                };
            }
            Ok(Filled::Batch { rows, eof: false }) => {
                let table = drive.table;
                let plan = drive.plan;
                let retained = drive.held.is_some();
                let item = match make_batch(table, plan, rows, drive.acc, retained) {
                    Ok(item) => item,
                    Err(err) => {
                        let closed = close_owned(portal).await;
                        return Hold::Fail { err, closed };
                    }
                };
                if matches!(emit_item(drive.gate, id, item).await, Signal::Stop) {
                    let closed = close_owned(portal).await;
                    return Hold::Stop { closed };
                }
            }
            Ok(Filled::Batch { rows, eof: true }) => {
                let pending = drive.held.take();
                let closed = close_owned(portal).await;
                return Hold::Done {
                    closed,
                    id,
                    rows: Some(rows),
                    pending,
                };
            }
        }
    }
}

enum Filled {
    Batch { rows: Vec<Vec<DecodedCell>>, eof: bool },
    Eof,
}

async fn fill_batch(
    portal: &mut PgNamedPortal<'_>,
    drive: &mut Drive<'_>,
) -> Result<Filled, SourceError> {
    let mut rows: Vec<Vec<DecodedCell>> = Vec::new();
    let mut wire = 0_u64;
    let ready = drive.held.take();
    if let Some(ready) = ready {
        place_row(&mut rows, &mut wire, drive, ready.cells, ready.payload)?;
    }
    let mut eof = false;
    let batch_rows = drive.plan.batch_rows;
    while (rows.len() as u32) < batch_rows && !eof && drive.held.is_none() {
        if Instant::now() >= drive.plan.deadline {
            if rows.is_empty() {
                return Err(SourceError::budget("deadline"));
            }
            break;
        }
        let plan = drive.plan;
        let table = drive.table;
        match pull_one(portal, plan, drive.acc).await? {
            Pulled::Row(row) => {
                let (cells, payload) = decode_scanned(table, plan, &row)?;
                if matches!(
                    place_row(&mut rows, &mut wire, drive, cells, payload)?,
                    Placed::Held
                ) {
                    break;
                }
            }
            Pulled::Eof(row) => {
                eof = true;
                if let Some(row) = row {
                    let (cells, payload) = decode_scanned(table, plan, &row)?;
                    place_row(&mut rows, &mut wire, drive, cells, payload)?;
                }
            }
        }
    }
    if rows.is_empty() {
        if eof {
            return Ok(Filled::Eof);
        }
        return Err(SourceError::protocol("stream_ended"));
    }
    Ok(Filled::Batch { rows, eof })
}

enum Placed {
    Took,
    Held,
}

fn place_row(
    rows: &mut Vec<Vec<DecodedCell>>,
    wire: &mut u64,
    drive: &mut Drive<'_>,
    cells: Vec<DecodedCell>,
    payload: u64,
) -> Result<Placed, SourceError> {
    if rows.is_empty()
        && (payload > drive.plan.batch_bytes
            || drive.acc.decoded_payload_bytes + payload > drive.plan.max_bytes)
    {
        return Err(SourceError::budget("max_bytes"));
    }
    if !rows.is_empty()
        && (*wire + payload > drive.plan.batch_bytes
            || drive.acc.decoded_payload_bytes + payload > drive.plan.max_bytes)
    {
        *drive.held = Some(Held { cells, payload });
        return Ok(Placed::Held);
    }
    *wire += payload;
    drive.acc.decoded_payload_bytes += payload;
    rows.push(cells);
    *drive.rows_emitted += 1;
    Ok(Placed::Took)
}

enum Pulled {
    Row(sqlx::postgres::PgRow),
    Eof(Option<sqlx::postgres::PgRow>),
}

async fn pull_one(
    portal: &mut PgNamedPortal<'_>,
    plan: &StreamPlan,
    acc: &mut Acc,
) -> Result<Pulled, SourceError> {
    let hold = session::try_reserve_rows(&plan.source_id, 1)?;
    acc.reserved_rows += 1;
    acc.executes += 1;
    let allowance = PgPortalAllowance::positive(1)
        .map_err(|_| SourceError::protocol("portal_allowance"))?;
    match tokio::time::timeout(plan.statement_timeout, portal.pull(allowance)).await {
        Ok(Ok(step)) => {
            let consumed = step.accounting.consumed_rows;
            hold.settle(usize::try_from(consumed).unwrap_or(0));
            acc.consumed_rows += u64::from(consumed);
            match step.outcome {
                PgPortalOutcome::Suspended(row) => Ok(Pulled::Row(row)),
                PgPortalOutcome::Complete { row, .. } => Ok(Pulled::Eof(row)),
            }
        }
        Ok(Err(err)) => Err(error::map_sqlx(err)),
        Err(_elapsed) => Err(SourceError::budget("statement_timeout")),
    }
}

fn decode_scanned(
    table: &StreamTablePlan,
    plan: &StreamPlan,
    row: &sqlx::postgres::PgRow,
) -> Result<(Vec<DecodedCell>, u64), SourceError> {
    if table.presence {
        if row.columns().len() != 1 {
            return Err(SourceError::protocol("row_width"));
        }
        let raw = row
            .try_get_raw(0)
            .map_err(|_| SourceError::protocol("binary_payload"))?;
        require_binary(raw.format())?;
        if raw.is_null() {
            return Err(SourceError::protocol("binary_payload"));
        }
        let bytes = raw
            .as_bytes()
            .map_err(|_| SourceError::protocol("binary_payload"))?;
        if bytes.len() as u64 > u64::from(plan.max_value_bytes) {
            return Err(SourceError::budget("max_value"));
        }
        return Ok((Vec::new(), bytes.len() as u64));
    }
    let readable: Vec<&ColumnPlan> = table
        .plans
        .iter()
        .filter(|plan| plan.mapping.decision == MapDecision::Lossless)
        .collect();
    if row.columns().len() != readable.len() {
        return Err(SourceError::protocol("row_width"));
    }
    let mut cells = Vec::with_capacity(readable.len());
    let mut payload = 0_u64;
    for (index, column) in readable.iter().enumerate() {
        let raw = row
            .try_get_raw(index)
            .map_err(|_| SourceError::protocol("binary_payload"))?;
        require_binary(raw.format())?;
        let actual = raw
            .type_info()
            .oid()
            .map(|oid| i64::from(oid.0))
            .ok_or_else(|| SourceError::protocol("type_oid_mismatch"))?;
        if !column.accepts_result_oid(actual) {
            return Err(SourceError::protocol("type_oid_mismatch"));
        }
        if raw.is_null() {
            cells.push(DecodedCell::sql_null());
            continue;
        }
        let bytes = raw
            .as_bytes()
            .map_err(|_| SourceError::protocol("binary_payload"))?;
        if bytes.len() as u64 > u64::from(plan.max_value_bytes) {
            return Err(SourceError::budget("max_value"));
        }
        payload += bytes.len() as u64;
        cells.push(decode_payload(column, bytes));
    }
    Ok((cells, payload))
}

fn make_batch(
    table: &StreamTablePlan,
    plan: &StreamPlan,
    rows: Vec<Vec<DecodedCell>>,
    acc: &mut Acc,
    retained_rows: bool,
) -> Result<StreamItem, SourceError> {
    let binding = RunBinding {
        source_id: plan.source_id.clone(),
        schema: table.schema.clone(),
        table: table.name.clone(),
        run_generation: plan.generation,
        input_digest: plan.input_digest,
    };
    let typed = assemble_batch(&binding, table.plans.clone(), &table.key_attnums, rows)?;
    let record_batch = typed.record_batch()?;
    acc.arrow_preimage_bytes += arrow_buffer_preimage(&typed.arrow).len() as u64;
    acc.arrow_assembly_bytes += record_batch.get_array_memory_size() as u64;
    Ok(StreamItem::Batch(Box::new(StreamBatch {
        typed,
        record_batch,
        accounting: acc.view(u64::from(retained_rows)),
        resumable: false,
    })))
}

fn table_eof(
    table: &StreamTablePlan,
    rows_emitted: u64,
    acc: &Acc,
    retained_rows: bool,
) -> StreamItem {
    StreamItem::TableEof(TableEof {
        schema: table.schema.clone(),
        table: table.name.clone(),
        rows_emitted,
        columns: table.arrow_columns,
        zero_column: table.zero_column,
        resumable_key: false,
        accounting: acc.view(u64::from(retained_rows)),
    })
}

async fn close_owned(portal: PgNamedPortal<'_>) -> Result<(), SourceError> {
    if portal.connection_poisoned() {
        drop(portal);
        return Err(SourceError::protocol("portal_poison"));
    }
    portal.close().await.map_err(error::map_sqlx)?;
    Ok(())
}

async fn settle_after(
    data: &mut PgSession,
    closed: Result<(), SourceError>,
    rollback: bool,
    acc: &mut Acc,
) -> Result<(), SourceError> {
    sample(data, acc);
    let poisoned = closed.as_ref().ok().is_none()
        && closed.as_ref().err().and_then(|err| err.fact()) == Some("portal_poison");
    if poisoned || !data.conn.incoming_synchronized() || data.conn.named_portal_poisoned() {
        data.mark_skip_rollback();
        return match closed {
            Err(err) if err.fact() == Some("portal_poison") => Ok(()),
            Err(err) => Err(err),
            Ok(()) => Ok(()),
        };
    }
    closed?;
    data.conn
        .set_incoming_bounds(None)
        .map_err(error::map_sqlx)?;
    if rollback {
        data.execute_control(sql::ROLLBACK_STREAM).await?;
        data.execute_control(sql::RELEASE_STREAM).await?;
    }
    Ok(())
}

async fn rollback_only(data: &mut PgSession) -> Result<(), SourceError> {
    if !data.conn.incoming_synchronized() || data.conn.named_portal_poisoned() {
        data.mark_skip_rollback();
        return Ok(());
    }
    data.conn
        .set_incoming_bounds(None)
        .map_err(error::map_sqlx)?;
    data.execute_control(sql::ROLLBACK_STREAM).await?;
    data.execute_control(sql::RELEASE_STREAM).await?;
    Ok(())
}

fn prefer(primary: SourceError, settled: Result<(), SourceError>) -> Result<(), SourceError> {
    if primary.kind() == error::SourceErrorKind::Budget {
        return Err(primary);
    }
    match settled {
        Ok(()) => Err(primary),
        Err(err) => Err(err),
    }
}

fn portal_arguments(table: &StreamTablePlan) -> Result<PgArguments, SourceError> {
    if table.lower.is_empty() && table.upper.is_empty() {
        return Ok(PgArguments::default());
    }
    if (!table.lower.is_empty() && table.lower.len() != table.key_oids.len())
        || (!table.upper.is_empty() && table.upper.len() != table.key_oids.len())
    {
        return Err(SourceError::protocol("key_plan"));
    }
    let mut args = PgArguments::default();
    for (value, oid) in table.lower.iter().zip(table.key_oids.iter().copied()) {
        snapshot::push_probe(&mut args, value, oid)?;
    }
    for (value, oid) in table.upper.iter().zip(table.key_oids.iter().copied()) {
        snapshot::push_probe(&mut args, value, oid)?;
    }
    Ok(args)
}

fn install_bounds(
    data: &mut PgSession,
    plan: &StreamPlan,
    columns: usize,
) -> Result<(), SourceError> {
    let message = plan
        .max_row_bytes
        .checked_add(4)
        .ok_or_else(|| SourceError::config("incoming_bounds"))?
        .max(MESSAGE_FLOOR);
    let columns = u16::try_from(columns).map_err(|_| SourceError::protocol("row_width"))?;
    let bounds = PgIncomingBounds::try_new(
        message,
        plan.max_value_bytes,
        plan.max_row_bytes,
        columns,
    )
    .map_err(|_| SourceError::config("incoming_bounds"))?;
    data.conn
        .set_incoming_bounds(Some(bounds))
        .map_err(error::map_sqlx)
}

fn sample(data: &PgSession, acc: &mut Acc) {
    let alloc = data.conn.incoming_allocation();
    acc.wire_view_capacity_peak = alloc.view_capacity_peak;
    acc.wire_accepted_logical_payload = alloc.accepted_logical_payload;
    acc.wire_assembly_bytes = alloc.assembly_bytes;
}

fn ensure_deadline(deadline: Instant) -> Result<(), SourceError> {
    if Instant::now() >= deadline {
        Err(SourceError::budget("deadline"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::StreamBudget;

    #[test]
    fn budget_bounds_are_closed() {
        assert_eq!(
            StreamBudget::try_new(0, 1, 1, 2).unwrap_err().fact(),
            Some("batch_rows")
        );
        assert_eq!(
            StreamBudget::try_new(65_537, 1, 1, 2).unwrap_err().fact(),
            Some("batch_rows")
        );
        assert_eq!(
            StreamBudget::try_new(1, 0, 1, 2).unwrap_err().fact(),
            Some("batch_bytes")
        );
        assert_eq!(
            StreamBudget::try_new(1, 1, 0, 2).unwrap_err().fact(),
            Some("max_value")
        );
        assert_eq!(
            StreamBudget::try_new(1, 1, 4, 3).unwrap_err().fact(),
            Some("max_row")
        );
        let budget = StreamBudget::try_new(2, 64, 4, 10).unwrap();
        assert_eq!(budget.queue_batches(), 1);
        assert_eq!(budget.batch_rows(), 2);
        assert_eq!(budget.max_value_bytes(), 4);
        assert_eq!(budget.max_row_bytes(), 10);
    }
}
