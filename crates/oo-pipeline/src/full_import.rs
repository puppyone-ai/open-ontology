//! Unpublished PostgreSQL full import.
//!
//! One dedicated target connection holds the shard advisory lock. Each batch
//! commits shadow rows, masked quality, and the existing apply receipt
//! together. `awaiting_publish` is returned only after the owning reader
//! yields `RunComplete`. Schema installation does not return that summary.

use std::fmt;

use sha2::{Digest, Sha256};
use sqlx::Connection;
use sqlx::postgres::PgConnection;

use oo_connectors::types::{ColumnMapping, ColumnPlan, TYPE_CONVERSION_PROTOCOL};
use oo_connectors::{
    SnapshotBinding, SnapshotOwner, SnapshotRequest, SourceColumn, SourceError, StreamBudget,
    StreamItem,
};
use oo_core::TYPE_POLICY_V1;
use oo_core::canonical::{ARBITRARY_DECIMAL_SCALE, LogicalType};

use crate::copy_encoder::{
    ArrowPhysical, CellPresence, CopyBudgets, CopyColumn, ExactDecimal, MapDecision, TypedBatch,
};
use crate::copy_sink::{self, SinkError};
use crate::physical_mapping::{
    Level, MappingRequest, PlanError, plan_physical_mapping, quote_physical_name,
};
use crate::target_installer::{sha256_hex, verify_full_import_readiness};

/// Protocol name for this entry.
pub const PROTOCOL: &str = "full-import/v1";

/// Preimage for the receipt hook digest. There is no hook payload.
pub const HOOK_PREIMAGE: &[u8] = b"full-import/v1 hook none";

const APPROVED_OIDS: &[i64] = &[
    16, 17, 20, 21, 23, 25, 114, 700, 701, 1042, 1043, 1082, 1083, 1114, 1184, 1700, 2950,
];

/// Lowercase hex SHA-256 of [`HOOK_PREIMAGE`].
pub fn hook_digest() -> String {
    sha256_hex(HOOK_PREIMAGE)
}

/// What the caller asks the entry to read and where to fence it.
pub struct ImportRequest {
    /// Closed shard token.
    pub shard: String,
    /// Owner token, a UUID string.
    pub owner: String,
    /// Source snapshot. Generation zero is rejected by the reader.
    pub snapshot: SnapshotRequest,
    /// Reader pull caps. The queue depth stays one.
    pub stream: StreamBudget,
    /// Encoder caps for one batch.
    pub copy: CopyBudgets,
}

/// Success state. Installation of the schema is not this state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState {
    /// The reader finished every selected table and the seal matched.
    AwaitingPublish,
}

/// One stored column in a shadow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnSummary {
    /// Source column name.
    pub name: String,
    /// Source `attnum`.
    pub attnum: i32,
    /// Stored physical name.
    pub physical_name: String,
    /// Oid used for the shadow type. Domains use the base oid.
    pub type_oid: i64,
}

/// Counts for one selected table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableSummary {
    /// Source schema name.
    pub schema: String,
    /// Source table name.
    pub name: String,
    /// Stored physical table name.
    pub physical_name: String,
    /// Rows copied.
    pub rows: u64,
    /// Batches committed.
    pub batches: u64,
    /// Encoder row chunks. The header and trailer are not included.
    pub row_chunks: u64,
    /// SHA-256 of the canonical row hashes, in pull order.
    pub canonical_digest: String,
    /// Columns in `attnum` order.
    pub columns: Vec<ColumnSummary>,
}

/// Result of a finished import. Callers cannot build [`RunState::AwaitingPublish`]
/// except through [`import_unpublished`] or [`ImportSession::finish`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunSummary {
    /// Database run id.
    pub run_id: String,
    /// Always [`RunState::AwaitingPublish`] on this value.
    pub state: RunState,
    /// Source id from the reader binding.
    pub source_id: String,
    /// Source snapshot generation.
    pub source_generation: u64,
    /// Target fence generation that sealed the run.
    pub target_generation: i64,
    /// Selected tables.
    pub tables: Vec<TableSummary>,
}

/// One step of a pulled import.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplyStep {
    /// One batch committed.
    Applied {
        /// Tuples in the batch.
        rows: u64,
        /// Encoder row chunks.
        row_chunks: u32,
        /// Receipt batch id.
        batch_id: String,
    },
    /// The receipt already had this payload. No second copy was started.
    Replay {
        /// Receipt batch id.
        batch_id: String,
    },
    /// The table scan ended and its coverage row was stored.
    TableDone {
        /// Source schema.
        schema: String,
        /// Source table.
        table: String,
        /// Rows copied for the table.
        rows: u64,
    },
    /// Every selected table reached end of scan.
    Exhausted,
}

/// Receipt identity returned when the commit response is not treated as success.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmbiguousCommit {
    /// Source token.
    pub source_id: String,
    /// Physical table name stored as `table_generation`.
    pub physical_name: String,
    /// Receipt batch id.
    pub batch_id: String,
    /// Payload digest.
    pub payload_digest: String,
    /// Applied position text.
    pub applied_pos: String,
    /// Run id.
    pub run_id: String,
    /// Config digest.
    pub config_digest: String,
    /// Type-policy digest.
    pub type_digest: String,
    /// Hook digest.
    pub hook_digest: String,
    /// Schema digest.
    pub schema_digest: String,
    /// Shard.
    pub shard: String,
    /// Owner token.
    pub owner: String,
    /// Target generation that wrote the batch.
    pub target_generation: i64,
}

/// What a later connection can prove about a receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    /// The stored payload digest matches.
    Replay,
    /// No receipt is stored.
    Absent,
    /// The batch id is stored with a different payload.
    Conflict,
}

/// Import failure. A refusal is not [`RunSummary`].
#[derive(Debug)]
pub enum ImportError {
    /// `0005` readiness is not proven.
    NotReady,
    /// A selected column is outside the approved COPY oid set.
    Unsupported {
        /// Source type name.
        type_name: String,
        /// Catalog oid.
        oid: i64,
    },
    /// Masked input was stored and the run was retired. Nothing was copied.
    Refused {
        /// Static reason token.
        reason: String,
    },
    /// Receipt identity conflict. The data, quality, and receipt transaction aborted.
    Conflict,
    /// The run bindings do not match.
    Binding(String),
    /// The source reader failed.
    Source(String),
    /// The target rejected a statement.
    Database {
        /// SQLSTATE, or empty when the driver has none.
        sqlstate: String,
        /// Server or driver message.
        message: String,
    },
}

impl ImportError {
    /// SQLSTATE when the target reported one.
    pub fn sqlstate(&self) -> Option<&str> {
        match self {
            Self::Conflict => Some("OO010"),
            Self::Database { sqlstate, .. } if !sqlstate.is_empty() => Some(sqlstate.as_str()),
            Self::Database { .. }
            | Self::NotReady
            | Self::Unsupported { .. }
            | Self::Refused { .. }
            | Self::Binding(_)
            | Self::Source(_) => None,
        }
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotReady => formatter.write_str("full import schema is not ready"),
            Self::Unsupported { type_name, oid } => {
                write!(formatter, "unsupported copy type {type_name} oid {oid}")
            }
            Self::Refused { reason } => write!(formatter, "full import refused {reason}"),
            Self::Conflict => formatter.write_str("apply receipt identity conflict"),
            Self::Binding(message) | Self::Source(message) => formatter.write_str(message),
            Self::Database { sqlstate, message } => {
                write!(
                    formatter,
                    "full import failed sqlstate={sqlstate} {message}"
                )
            }
        }
    }
}

impl std::error::Error for ImportError {}

/// An open target transaction that has not been committed.
pub struct Uncommitted<'a> {
    tx: sqlx::Transaction<'a, sqlx::Postgres>,
    memory: BatchMemory,
    identity: AmbiguousCommit,
    backend_pid: i64,
}

impl Uncommitted<'_> {
    /// Backend that holds the open transaction.
    pub fn backend_pid(&self) -> i64 {
        self.backend_pid
    }

    /// Identity that a later connection can look up.
    pub fn identity(&self) -> &AmbiguousCommit {
        &self.identity
    }

    /// Roll the batch back. Shadow rows, quality, and the receipt go together.
    pub async fn rollback(self) -> Result<(), ImportError> {
        self.tx.rollback().await.map_err(database_error)
    }

    /// Commit and return the identity without treating that response as run completion.
    pub async fn commit_ambiguous(self) -> Result<AmbiguousCommit, ImportError> {
        let identity = self.identity.clone();
        self.tx.commit().await.map_err(database_error)?;
        Ok(identity)
    }
}

/// Open fenced import. The caller keeps the only writer connection for this shard.
pub struct ImportSession {
    shard: String,
    owner: String,
    target_generation: i64,
    source_id: String,
    source_generation: i64,
    config_digest: String,
    type_digest: String,
    input_digest: String,
    schema_digest: String,
    hook_digest: String,
    tables: Vec<TableCursor>,
    run_id: String,
    reader: Option<SnapshotOwner>,
    cancel: oo_connectors::RunCancel,
    stream_budget: StreamBudget,
    copy_budget: CopyBudgets,
    stream_open: bool,
    saw_exhausted: bool,
    source_backend: i64,
}

/// Read one fresh source cut into an unpublished target generation.
pub async fn import_unpublished(
    conn: &mut PgConnection,
    request: ImportRequest,
) -> Result<RunSummary, ImportError> {
    let mut session = ImportSession::open(&mut *conn, request).await?;
    loop {
        match session.pull_and_apply(&mut *conn).await? {
            ApplyStep::Exhausted => break,
            ApplyStep::Applied { .. } | ApplyStep::Replay { .. } | ApplyStep::TableDone { .. } => {}
        }
    }
    session.finish(&mut *conn).await
}

impl ImportSession {
    /// Adopt the shard, open the source snapshot, and create the unpublished run.
    pub async fn open(
        conn: &mut PgConnection,
        request: ImportRequest,
    ) -> Result<Self, ImportError> {
        verify_full_import_readiness(&mut *conn)
            .await
            .map_err(|_| ImportError::NotReady)?;
        let owner = resolved_owner(&mut *conn, request.owner).await?;
        let target_generation = adopt_generation(&mut *conn, &request.shard, &owner).await?;
        sqlx::query("SELECT _oo.full_import_bind_session($1, $2::uuid, $3)")
            .bind(&request.shard)
            .bind(&owner)
            .bind(target_generation)
            .execute(&mut *conn)
            .await
            .map_err(database_error)?;
        let reader = SnapshotOwner::open(request.snapshot)
            .await
            .map_err(source_error)?;
        let cancel = reader.run_cancel();
        let binding = reader.binding().clone();
        if let Err(error) = approved_catalog(&binding) {
            let _ = reader.abort().await;
            return Err(error);
        }
        let source_generation = i64::try_from(binding.generation)
            .map_err(|_| ImportError::Binding("source generation".to_string()))?;
        let config_digest = hex_bytes(&binding.config_identity);
        let type_digest = hex_bytes(&binding.type_policy_identity);
        let input_digest = hex_bytes(&binding.input_digest);
        let schema_digest = binding.schema_hash.clone();
        let hook = hook_digest();
        let tables =
            register_tables(&mut *conn, &binding, source_generation, &config_digest).await?;
        let run_id = match open_run(
            &mut *conn,
            &request.shard,
            &owner,
            target_generation,
            &binding,
            source_generation,
            &config_digest,
            &type_digest,
            &input_digest,
            &schema_digest,
            &tables,
        )
        .await
        {
            Ok(run_id) => run_id,
            Err(error) => {
                let _ = reader.abort().await;
                return Err(error);
            }
        };
        if let Err(error) = prepare_shadows(
            &mut *conn,
            &request.shard,
            &owner,
            target_generation,
            &run_id,
            &binding.source_id,
            &config_digest,
            source_generation,
            &tables,
        )
        .await
        {
            let _ = retire_run(
                &mut *conn,
                &request.shard,
                &owner,
                target_generation,
                &run_id,
            )
            .await;
            let _ = reader.abort().await;
            return Err(error);
        }
        Ok(Self {
            shard: request.shard,
            owner,
            target_generation,
            source_id: binding.source_id,
            source_generation,
            config_digest,
            type_digest,
            input_digest,
            schema_digest,
            hook_digest: hook,
            tables,
            run_id,
            reader: Some(reader),
            cancel,
            stream_budget: request.stream,
            copy_budget: request.copy,
            stream_open: false,
            saw_exhausted: false,
            source_backend: binding.backend.pid,
        })
    }

    /// Source backend pid captured at snapshot open.
    pub fn source_backend_pid(&self) -> i64 {
        self.source_backend
    }

    /// Target backend generation held by this connection.
    pub fn target_generation(&self) -> i64 {
        self.target_generation
    }

    /// Run id.
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Shadow names and columns known so far. Digests are empty until a table ends.
    pub fn tables(&self) -> Vec<TableSummary> {
        self.tables.iter().map(TableCursor::summary).collect()
    }

    /// Cancel handle for the source pull.
    pub fn cancel_handle(&self) -> oo_connectors::RunCancel {
        self.cancel.clone()
    }

    /// Pull the next source item and commit it, or record that the scan is done.
    pub async fn pull_and_apply(
        &mut self,
        conn: &mut PgConnection,
    ) -> Result<ApplyStep, ImportError> {
        match self.hold_next(conn).await? {
            Held::Ready(step) => Ok(step),
            Held::Open(pending) => {
                let memory = pending.memory.clone();
                let step = memory.step.clone();
                pending.tx.commit().await.map_err(database_error)?;
                self.remember(&memory);
                Ok(step)
            }
        }
    }

    /// Copy the next batch and leave the target transaction open.
    pub async fn stage_next_uncommitted<'c>(
        &mut self,
        conn: &'c mut PgConnection,
    ) -> Result<Uncommitted<'c>, ImportError> {
        match self.hold_next(conn).await? {
            Held::Open(pending) => Ok(*pending),
            Held::Ready(_) => Err(ImportError::Binding("next item is not a batch".to_string())),
        }
    }

    /// Ask the source to stop, release it, and retire the unpublished run.
    pub async fn cancel(&mut self, conn: &mut PgConnection) -> Result<(), ImportError> {
        self.cancel.request();
        if let Some(reader) = self.reader.take() {
            let _ = reader.cancel().await;
        }
        self.retire_current(conn).await
    }

    /// Retire the open run. Shadows stay, without a seal and without a grant.
    pub async fn retire(&mut self, conn: &mut PgConnection) -> Result<(), ImportError> {
        if let Some(reader) = self.reader.take() {
            let _ = reader.abort().await;
        }
        self.retire_current(conn).await
    }

    /// Release the shard lock held by this connection.
    pub async fn fail_fence(&mut self, conn: &mut PgConnection) -> Result<(), ImportError> {
        sqlx::query("SELECT _oo.fail_target_generation($1, $2::uuid, $3)")
            .bind(&self.shard)
            .bind(&self.owner)
            .bind(self.target_generation)
            .execute(&mut *conn)
            .await
            .map_err(database_error)?;
        Ok(())
    }

    /// Seal only when this owner returned [`oo_connectors::RunComplete`] and the bindings match.
    pub async fn finish(mut self, conn: &mut PgConnection) -> Result<RunSummary, ImportError> {
        let Some(reader) = self.reader.take() else {
            return Err(ImportError::Binding("reader is closed".to_string()));
        };
        if !self.saw_exhausted {
            let _ = reader.cancel().await;
            let _ = self.retire_current(conn).await;
            return Err(ImportError::Binding("reader is not exhausted".to_string()));
        }
        let complete = match reader.finish_complete().await {
            Ok(complete) => complete,
            Err(error) => {
                let _ = self.retire_current(conn).await;
                return Err(source_error(error));
            }
        };
        if complete.source_id != self.source_id
            || u64::try_from(self.source_generation).ok() != Some(complete.generation)
            || hex_bytes(&complete.config_identity) != self.config_digest
            || hex_bytes(&complete.type_policy_identity) != self.type_digest
            || hex_bytes(&complete.input_digest) != self.input_digest
            || complete.schema_hash != self.schema_digest
        {
            let _ = self.retire_current(conn).await;
            return Err(ImportError::Binding(
                "run complete binding mismatch".to_string(),
            ));
        }
        let selected_schema: Vec<String> = self
            .tables
            .iter()
            .map(|table| table.schema_ident.clone())
            .collect();
        let selected_table: Vec<String> = self
            .tables
            .iter()
            .map(|table| table.table_ident.clone())
            .collect();
        let sealed: String = sqlx::query_scalar(
            "SELECT _oo.full_import_seal_run($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::text[])",
        )
        .bind(&self.shard)
        .bind(&self.owner)
        .bind(self.target_generation)
        .bind(&self.run_id)
        .bind(&self.source_id)
        .bind(self.source_generation)
        .bind(&self.config_digest)
        .bind(&self.type_digest)
        .bind(&self.input_digest)
        .bind(&self.schema_digest)
        .bind(&selected_schema)
        .bind(&selected_table)
        .fetch_one(&mut *conn)
        .await
        .map_err(database_error)?;
        if sealed != "awaiting_publish" {
            return Err(ImportError::Binding(
                "seal did not await publish".to_string(),
            ));
        }
        Ok(RunSummary {
            run_id: self.run_id,
            state: RunState::AwaitingPublish,
            source_id: self.source_id,
            source_generation: u64::try_from(self.source_generation).unwrap_or(complete.generation),
            target_generation: self.target_generation,
            tables: self.tables.iter().map(TableCursor::summary).collect(),
        })
    }

    async fn hold_next<'c>(&mut self, conn: &'c mut PgConnection) -> Result<Held<'c>, ImportError> {
        self.ensure_stream(conn).await?;
        let pulled = {
            let reader = self
                .reader
                .as_mut()
                .ok_or_else(|| ImportError::Binding("reader is closed".to_string()))?;
            reader.next_batch().await
        };
        let item = match pulled {
            Ok(item) => item,
            Err(error) => {
                let mapped = source_error(error);
                let _ = self.fail_reader(conn).await;
                return Err(mapped);
            }
        };
        match item {
            StreamItem::Batch(batch) => self.hold_batch(conn, &batch.typed).await,
            StreamItem::TableEof(eof) => {
                self.note_eof(conn, &eof.schema, &eof.table, eof.rows_emitted)
                    .await?;
                Ok(Held::Ready(ApplyStep::TableDone {
                    schema: eof.schema,
                    table: eof.table,
                    rows: eof.rows_emitted,
                }))
            }
            StreamItem::Exhausted(_) => {
                self.saw_exhausted = true;
                Ok(Held::Ready(ApplyStep::Exhausted))
            }
        }
    }

    async fn ensure_stream(&mut self, conn: &mut PgConnection) -> Result<(), ImportError> {
        if self.stream_open {
            return Ok(());
        }
        let budget = self.stream_budget;
        let opened = {
            let reader = self
                .reader
                .as_mut()
                .ok_or_else(|| ImportError::Binding("reader is closed".to_string()))?;
            reader.open_stream(budget).await
        };
        match opened {
            Ok(()) => {
                self.stream_open = true;
                Ok(())
            }
            Err(error) => {
                let mapped = source_error(error);
                let _ = self.fail_reader(conn).await;
                Err(mapped)
            }
        }
    }

    async fn hold_batch<'c>(
        &mut self,
        conn: &'c mut PgConnection,
        batch: &TypedBatch,
    ) -> Result<Held<'c>, ImportError> {
        if let Some(reason) = refusal_reason(batch) {
            self.record_refusal(conn, batch, &reason).await?;
            let _ = self.fail_reader(conn).await;
            return Err(ImportError::Refused { reason });
        }
        let cursor = self
            .tables
            .iter()
            .find(|table| table.schema == batch.schema && table.name == batch.table)
            .cloned()
            .ok_or_else(|| ImportError::Binding("batch table is not selected".to_string()))?;
        let positions = positions_for(cursor.next_pos, batch.rows.len())?;
        let columns = copy_columns(&cursor);
        let statement = copy_statement(&cursor)?;
        let batch_id = format!("t{}.b{}", cursor.index, cursor.batches);
        let rejects = reject_rows(batch);
        let quality = quality_bytes(
            &batch_id,
            batch.rows.len() as u64,
            0,
            &self.input_digest,
            &rejects,
        );
        let hashed = copy_sink::hash_batch(batch, &columns, &positions, self.copy_budget)
            .map_err(sink_error)?;
        let mut payload_hasher = hashed.hasher.clone();
        payload_hasher.update(&quality);
        let payload_digest = hex_bytes(payload_hasher.finalize().as_ref());
        let applied_pos = if batch.rows.is_empty() {
            "0".to_string()
        } else {
            positions[positions.len() - 1].to_plain_string()
        };
        let mut tx = conn.begin().await.map_err(database_error)?;
        let decision = stage_decision(
            &mut tx,
            self,
            &cursor.physical,
            &batch_id,
            &payload_digest,
            &applied_pos,
        )
        .await?;
        if decision == "replay" {
            tx.rollback().await.map_err(database_error)?;
            return Ok(Held::Ready(ApplyStep::Replay { batch_id }));
        }
        let sent = copy_sink::send_batch(
            &mut tx,
            &statement,
            batch,
            &columns,
            &positions,
            self.copy_budget,
        )
        .await
        .map_err(sink_error)?;
        if sent.hasher.finalize() != hashed.hasher.finalize() {
            let _ = tx.rollback().await;
            return Err(ImportError::Binding(
                "copy bytes changed between passes".to_string(),
            ));
        }
        let wrote: String = sqlx::query_scalar(
            "SELECT _oo.full_import_write_batch($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15::numeric, $16, $17, $18::text[], $19::bigint[], $20::integer[], $21::bigint[], $22::text[])",
        )
        .bind(&self.shard)
        .bind(&self.owner)
        .bind(self.target_generation)
        .bind(&self.run_id)
        .bind(i32::try_from(cursor.index).unwrap_or(i32::MAX))
        .bind(&self.source_id)
        .bind(&cursor.physical)
        .bind(&batch_id)
        .bind(&payload_digest)
        .bind(&self.config_digest)
        .bind(&self.type_digest)
        .bind(&self.hook_digest)
        .bind(&self.schema_digest)
        .bind(&self.input_digest)
        .bind(&applied_pos)
        .bind(i64::try_from(batch.rows.len()).unwrap_or(i64::MAX))
        .bind(0_i64)
        .bind(Vec::<String>::new())
        .bind(Vec::<i64>::new())
        .bind(Vec::<i32>::new())
        .bind(Vec::<i64>::new())
        .bind(Vec::<String>::new())
        .fetch_one(&mut *tx)
        .await
        .map_err(database_error)?;
        if wrote == "replay" {
            tx.rollback().await.map_err(database_error)?;
            return Ok(Held::Ready(ApplyStep::Replay { batch_id }));
        }
        let backend_pid: i32 = sqlx::query_scalar("SELECT pg_catalog.pg_backend_pid()")
            .fetch_one(&mut *tx)
            .await
            .map_err(database_error)?;
        let identity = AmbiguousCommit {
            source_id: self.source_id.clone(),
            physical_name: cursor.physical.clone(),
            batch_id: batch_id.clone(),
            payload_digest,
            applied_pos,
            run_id: self.run_id.clone(),
            config_digest: self.config_digest.clone(),
            type_digest: self.type_digest.clone(),
            hook_digest: self.hook_digest.clone(),
            schema_digest: self.schema_digest.clone(),
            shard: self.shard.clone(),
            owner: self.owner.clone(),
            target_generation: self.target_generation,
        };
        let step = ApplyStep::Applied {
            rows: sent.rows,
            row_chunks: sent.row_chunks,
            batch_id,
        };
        Ok(Held::Open(Box::new(Uncommitted {
            tx,
            memory: BatchMemory {
                index: cursor.index,
                rows: sent.rows,
                row_chunks: u64::from(sent.row_chunks),
                hashes: batch.rows.iter().map(|row| row.canonical_hash).collect(),
                step,
            },
            identity,
            backend_pid: i64::from(backend_pid),
        })))
    }

    async fn note_eof(
        &mut self,
        conn: &mut PgConnection,
        schema: &str,
        table: &str,
        rows: u64,
    ) -> Result<(), ImportError> {
        let index = self
            .tables
            .iter()
            .position(|cursor| cursor.schema == schema && cursor.name == table)
            .ok_or_else(|| ImportError::Binding("table end is not selected".to_string()))?;
        if self.tables[index].rows != rows {
            let _ = self.fail_reader(conn).await;
            return Err(ImportError::Binding(
                "table row coverage mismatch".to_string(),
            ));
        }
        if rows == 0 && self.tables[index].batches == 0 {
            let empty = empty_batch(
                &self.tables[index],
                &self.source_id,
                self.source_generation,
                &self.input_digest,
            )?;
            match self.hold_batch(conn, &empty).await? {
                Held::Open(pending) => {
                    let memory = pending.memory.clone();
                    pending.tx.commit().await.map_err(database_error)?;
                    self.remember(&memory);
                }
                Held::Ready(ApplyStep::Replay { .. }) => {}
                Held::Ready(_) => {
                    return Err(ImportError::Binding("empty table did not copy".to_string()));
                }
            }
        }
        let digest = hex_bytes(self.tables[index].hasher.clone().finalize().as_ref());
        let rows_applied = i64::try_from(self.tables[index].rows).unwrap_or(i64::MAX);
        sqlx::query(
            "SELECT _oo.full_import_note_table($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8)",
        )
        .bind(&self.shard)
        .bind(&self.owner)
        .bind(self.target_generation)
        .bind(&self.run_id)
        .bind(i32::try_from(index).unwrap_or(i32::MAX))
        .bind(rows_applied)
        .bind(&digest)
        .bind(&self.source_id)
        .execute(&mut *conn)
        .await
        .map_err(database_error)?;
        self.tables[index].exhausted = true;
        self.tables[index].canonical = digest;
        Ok(())
    }

    async fn record_refusal(
        &mut self,
        conn: &mut PgConnection,
        batch: &TypedBatch,
        reason: &str,
    ) -> Result<(), ImportError> {
        let physical = self
            .tables
            .iter()
            .find(|table| table.schema == batch.schema && table.name == batch.table)
            .map(|table| table.physical.clone())
            .ok_or_else(|| ImportError::Binding("refused table is not selected".to_string()))?;
        let rows = reject_rows(batch);
        let (reasons, lens, attnums, indexes, keys) = if rows.is_empty() {
            (
                vec![reason.to_string()],
                vec![0_i64],
                vec![0_i32],
                vec![0_i64],
                vec![String::new()],
            )
        } else {
            (
                rows.iter().map(|row| row.reason.clone()).collect(),
                rows.iter().map(|row| row.value_len).collect(),
                rows.iter().map(|row| row.attnum).collect(),
                rows.iter().map(|row| row.row_index).collect(),
                rows.iter().map(|row| row.key_digest.clone()).collect(),
            )
        };
        let batch_id = format!("t0.refuse.{}", self.run_id);
        let payload = sha256_hex(
            quality_bytes(&batch_id, 0, rows.len() as u64, &self.input_digest, &[]).as_slice(),
        );
        let mut tx = conn.begin().await.map_err(database_error)?;
        sqlx::query(
            "SELECT _oo.full_import_record_refusal($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9::text[], $10::bigint[], $11::integer[], $12::bigint[], $13::text[])",
        )
        .bind(&self.shard)
        .bind(&self.owner)
        .bind(self.target_generation)
        .bind(&self.run_id)
        .bind(&physical)
        .bind(&batch_id)
        .bind(&self.input_digest)
        .bind(&payload)
        .bind(&reasons)
        .bind(&lens)
        .bind(&attnums)
        .bind(&indexes)
        .bind(&keys)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
        sqlx::query("SELECT _oo.full_import_retire_run($1, $2::uuid, $3, $4::uuid)")
            .bind(&self.shard)
            .bind(&self.owner)
            .bind(self.target_generation)
            .bind(&self.run_id)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        tx.commit().await.map_err(database_error)?;
        Ok(())
    }

    async fn fail_reader(&mut self, conn: &mut PgConnection) -> Result<(), ImportError> {
        if let Some(reader) = self.reader.take() {
            let _ = reader.cancel().await;
        }
        self.retire_current(conn).await
    }

    async fn retire_current(&mut self, conn: &mut PgConnection) -> Result<(), ImportError> {
        retire_run(
            conn,
            &self.shard,
            &self.owner,
            self.target_generation,
            &self.run_id,
        )
        .await
    }

    fn remember(&mut self, memory: &BatchMemory) {
        let Some(table) = self.tables.get_mut(memory.index) else {
            return;
        };
        table.rows = table.rows.saturating_add(memory.rows);
        table.batches = table.batches.saturating_add(1);
        table.row_chunks = table.row_chunks.saturating_add(memory.row_chunks);
        table.next_pos = table.next_pos.saturating_add(memory.rows);
        for hash in &memory.hashes {
            table.hasher.update(hash);
        }
    }
}

async fn resolved_owner(conn: &mut PgConnection, owner: String) -> Result<String, ImportError> {
    if !owner.is_empty() {
        return Ok(owner);
    }
    sqlx::query_scalar("SELECT pg_catalog.gen_random_uuid()::text")
        .fetch_one(&mut *conn)
        .await
        .map_err(database_error)
}

/// Adopt one shard. A second backend that cannot take the lock returns the server error.
pub async fn adopt_generation(
    conn: &mut PgConnection,
    shard: &str,
    owner: &str,
) -> Result<i64, ImportError> {
    sqlx::query_scalar("SELECT _oo.adopt_target_generation($1, $2::uuid)")
        .bind(shard)
        .bind(owner)
        .fetch_one(&mut *conn)
        .await
        .map_err(database_error)
}

/// Look up a committed receipt. This does not write.
pub async fn recover(
    conn: &mut PgConnection,
    source: &str,
    physical: &str,
    batch_id: &str,
    payload_digest: &str,
) -> Result<Recovery, ImportError> {
    let result: Result<String, ImportError> =
        sqlx::query_scalar("SELECT _oo.full_import_recover($1, $2, $3, $4)")
            .bind(source)
            .bind(physical)
            .bind(batch_id)
            .bind(payload_digest)
            .fetch_one(&mut *conn)
            .await
            .map_err(database_error);
    match result {
        Ok(value) if value == "replay" => Ok(Recovery::Replay),
        Ok(value) if value == "absent" => Ok(Recovery::Absent),
        Ok(value) => Err(ImportError::Binding(format!("recover token {value}"))),
        Err(ImportError::Conflict) => Ok(Recovery::Conflict),
        Err(error) => Err(error),
    }
}

/// Bind a connection that already adopted `generation` and replay one receipt.
///
/// Replay does not start COPY.
pub async fn replay_batch(
    conn: &mut PgConnection,
    generation: i64,
    commit: &AmbiguousCommit,
) -> Result<ApplyStep, ImportError> {
    sqlx::query("SELECT _oo.full_import_bind_session($1, $2::uuid, $3)")
        .bind(&commit.shard)
        .bind(&commit.owner)
        .bind(generation)
        .execute(&mut *conn)
        .await
        .map_err(database_error)?;
    let mut tx = conn.begin().await.map_err(database_error)?;
    let decision: String = sqlx::query_scalar(
        "SELECT _oo.full_import_stage_decision($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12::numeric)",
    )
    .bind(&commit.shard)
    .bind(&commit.owner)
    .bind(generation)
    .bind(&commit.source_id)
    .bind(&commit.physical_name)
    .bind(&commit.batch_id)
    .bind(&commit.payload_digest)
    .bind(&commit.config_digest)
    .bind(&commit.type_digest)
    .bind(&commit.hook_digest)
    .bind(&commit.schema_digest)
    .bind(&commit.applied_pos)
    .fetch_one(&mut *tx)
    .await
    .map_err(database_error)?;
    tx.rollback().await.map_err(database_error)?;
    if decision != "replay" {
        return Err(ImportError::Binding(
            "replay was not the stored receipt".to_string(),
        ));
    }
    Ok(ApplyStep::Replay {
        batch_id: commit.batch_id.clone(),
    })
}

/// Insert one shadow row and a quality row, then present a conflicting receipt.
///
/// `OO010` aborts that transaction.
pub async fn abort_conflicting(
    conn: &mut PgConnection,
    generation: i64,
    commit: &AmbiguousCommit,
    payload_digest: &str,
) -> Result<(), ImportError> {
    sqlx::query("SELECT _oo.full_import_bind_session($1, $2::uuid, $3)")
        .bind(&commit.shard)
        .bind(&commit.owner)
        .bind(generation)
        .execute(&mut *conn)
        .await
        .map_err(database_error)?;
    let mut tx = conn.begin().await.map_err(database_error)?;
    let result = sqlx::query(
        "SELECT _oo.full_import_probe_conflict($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12::numeric)",
    )
    .bind(&commit.shard)
    .bind(&commit.owner)
    .bind(generation)
    .bind(&commit.source_id)
    .bind(&commit.physical_name)
    .bind(&commit.batch_id)
    .bind(payload_digest)
    .bind(&commit.config_digest)
    .bind(&commit.type_digest)
    .bind(&commit.hook_digest)
    .bind(&commit.schema_digest)
    .bind(&commit.applied_pos)
    .execute(&mut *tx)
    .await
    .map_err(database_error);
    match result {
        Err(ImportError::Conflict) => {
            let _ = tx.rollback().await;
            Ok(())
        }
        Ok(_) => {
            let _ = tx.rollback().await;
            Err(ImportError::Binding("conflict did not abort".to_string()))
        }
        Err(error) => {
            let _ = tx.rollback().await;
            Err(error)
        }
    }
}

enum Held<'a> {
    Ready(ApplyStep),
    Open(Box<Uncommitted<'a>>),
}

#[derive(Clone)]
struct BatchMemory {
    index: usize,
    rows: u64,
    row_chunks: u64,
    hashes: Vec<[u8; 32]>,
    step: ApplyStep,
}

#[derive(Clone)]
struct TableCursor {
    index: usize,
    schema: String,
    name: String,
    schema_ident: String,
    table_ident: String,
    physical: String,
    columns: Vec<ShadowColumn>,
    next_pos: u64,
    batches: u64,
    rows: u64,
    row_chunks: u64,
    hasher: Sha256,
    exhausted: bool,
    canonical: String,
}

#[derive(Clone)]
struct ShadowColumn {
    name: String,
    attnum: i32,
    ident: String,
    physical: String,
    oid: i64,
    typmod: i32,
    not_null: bool,
}

struct RejectByte {
    reason: String,
    value_len: i64,
    attnum: i32,
    row_index: i64,
    key_digest: String,
}

impl TableCursor {
    fn summary(&self) -> TableSummary {
        TableSummary {
            schema: self.schema.clone(),
            name: self.name.clone(),
            physical_name: self.physical.clone(),
            rows: self.rows,
            batches: self.batches,
            row_chunks: self.row_chunks,
            canonical_digest: self.canonical.clone(),
            columns: self
                .columns
                .iter()
                .map(|column| ColumnSummary {
                    name: column.name.clone(),
                    attnum: column.attnum,
                    physical_name: column.physical.clone(),
                    type_oid: column.oid,
                })
                .collect(),
        }
    }
}

fn approved_catalog(binding: &SnapshotBinding) -> Result<(), ImportError> {
    for table in &binding.catalog.tables {
        for column in &table.columns {
            approved_column(column)?;
        }
    }
    Ok(())
}

/// One approved physical column. `typmod` is the stored `atttypmod`.
/// This function does not invent a typmod from the column name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApprovedColumn {
    pub oid: i64,
    pub typmod: i32,
}

/// Approve `columns` as `(oid, atttypmod)` pairs.
/// A domain must be passed as its base oid. An unknown oid refuses the whole list.
pub fn approved_columns(columns: &[(i64, i32)]) -> Result<Vec<ApprovedColumn>, &'static str> {
    columns
        .iter()
        .map(|(oid, typmod)| approved_column_typmod(*oid, *typmod))
        .collect()
}

/// Approve one base oid and return its physical typmod unchanged.
pub fn approved_column_typmod(oid: i64, typmod: i32) -> Result<ApprovedColumn, &'static str> {
    if APPROVED_OIDS.contains(&oid) {
        Ok(ApprovedColumn { oid, typmod })
    } else {
        Err("unsupported type")
    }
}

fn approved_column(column: &SourceColumn) -> Result<i64, ImportError> {
    if column.typtype != "b" && column.typtype != "d" {
        return Err(unsupported(column));
    }
    let oid = if column.typtype == "d" {
        column.base_type_oid.unwrap_or(column.type_oid)
    } else {
        column.type_oid
    };
    approved_column_typmod(oid, column.typmod)
        .map(|approved| approved.oid)
        .map_err(|_| unsupported(column))
}

fn unsupported(column: &SourceColumn) -> ImportError {
    ImportError::Unsupported {
        type_name: column.type_name.clone(),
        oid: column.type_oid,
    }
}

async fn register_tables(
    conn: &mut PgConnection,
    binding: &SnapshotBinding,
    source_generation: i64,
    config_digest: &str,
) -> Result<Vec<TableCursor>, ImportError> {
    let mut tables = Vec::new();
    for (index, table) in binding.catalog.tables.iter().enumerate() {
        register_one(
            conn,
            &binding.source_id,
            config_digest,
            source_generation,
            Level::Namespace,
            &table.schema_ident,
            "",
            "",
        )
        .await?;
        let physical = register_one(
            conn,
            &binding.source_id,
            config_digest,
            source_generation,
            Level::Table,
            &table.schema_ident,
            &table.name_ident,
            "",
        )
        .await?;
        let mut columns = Vec::new();
        for column in &table.columns {
            let oid = approved_column(column)?;
            let column_physical = register_one(
                conn,
                &binding.source_id,
                config_digest,
                source_generation,
                Level::Column,
                &table.schema_ident,
                &table.name_ident,
                &column.name_ident,
            )
            .await?;
            columns.push(ShadowColumn {
                name: column.name.clone(),
                attnum: column.attnum,
                ident: column.name_ident.clone(),
                physical: column_physical,
                oid,
                typmod: column.typmod,
                not_null: column.not_null,
            });
        }
        tables.push(TableCursor {
            index,
            schema: table.schema.clone(),
            name: table.name.clone(),
            schema_ident: table.schema_ident.clone(),
            table_ident: table.name_ident.clone(),
            physical,
            columns,
            next_pos: 0,
            batches: 0,
            rows: 0,
            row_chunks: 0,
            hasher: Sha256::new(),
            exhausted: false,
            canonical: String::new(),
        });
    }
    Ok(tables)
}

#[allow(clippy::too_many_arguments)]
async fn register_one(
    conn: &mut PgConnection,
    source: &str,
    config_digest: &str,
    source_generation: i64,
    level: Level,
    namespace: &str,
    table: &str,
    column: &str,
) -> Result<String, ImportError> {
    let request = MappingRequest {
        source: source.to_string(),
        config_digest: config_digest.to_string(),
        run_generation: source_generation,
        level,
        logical_namespace: namespace.to_string(),
        logical_table: table.to_string(),
        logical_column: column.to_string(),
    };
    let plan = plan_physical_mapping(&request).map_err(plan_error)?;
    sqlx::query("SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(source)
        .bind(config_digest)
        .bind(source_generation)
        .bind(level.as_str())
        .bind(namespace)
        .bind(table)
        .bind(column)
        .bind(plan.identity_digest.as_slice())
        .execute(&mut *conn)
        .await
        .map_err(database_error)?;
    let stored: String =
        sqlx::query_scalar("SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)")
            .bind(source)
            .bind(config_digest)
            .bind(source_generation)
            .bind(level.as_str())
            .bind(namespace)
            .bind(table)
            .bind(column)
            .fetch_one(&mut *conn)
            .await
            .map_err(database_error)?;
    if stored != plan.physical_name {
        return Err(ImportError::Binding(
            "stored physical name mismatch".to_string(),
        ));
    }
    Ok(stored)
}

#[allow(clippy::too_many_arguments)]
async fn open_run(
    conn: &mut PgConnection,
    shard: &str,
    owner: &str,
    target_generation: i64,
    binding: &SnapshotBinding,
    source_generation: i64,
    config_digest: &str,
    type_digest: &str,
    input_digest: &str,
    schema_digest: &str,
    tables: &[TableCursor],
) -> Result<String, ImportError> {
    let schemas: Vec<String> = tables
        .iter()
        .map(|table| table.schema_ident.clone())
        .collect();
    let names: Vec<String> = tables
        .iter()
        .map(|table| table.table_ident.clone())
        .collect();
    let physical: Vec<String> = tables.iter().map(|table| table.physical.clone()).collect();
    let mut tx = conn.begin().await.map_err(database_error)?;
    let run_id: String = sqlx::query_scalar(
        "SELECT _oo.full_import_open_run($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
    )
    .bind(shard)
    .bind(owner)
    .bind(target_generation)
    .bind(&binding.source_id)
    .bind(source_generation)
    .bind(config_digest)
    .bind(type_digest)
    .bind(input_digest)
    .bind(schema_digest)
    .bind(&schemas)
    .bind(&names)
    .bind(&physical)
    .fetch_one(&mut *tx)
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    Ok(run_id)
}

#[allow(clippy::too_many_arguments)]
async fn prepare_shadows(
    conn: &mut PgConnection,
    shard: &str,
    owner: &str,
    target_generation: i64,
    run_id: &str,
    source: &str,
    config_digest: &str,
    source_generation: i64,
    tables: &[TableCursor],
) -> Result<(), ImportError> {
    let mut tx = conn.begin().await.map_err(database_error)?;
    for table in tables {
        let idents: Vec<String> = table
            .columns
            .iter()
            .map(|column| column.ident.clone())
            .collect();
        let oids: Vec<i64> = table.columns.iter().map(|column| column.oid).collect();
        let typmods: Vec<i32> = table.columns.iter().map(|column| column.typmod).collect();
        let required: Vec<bool> = table.columns.iter().map(|column| column.not_null).collect();
        let physical: String = sqlx::query_scalar(
            "SELECT _oo.full_import_prepare_shadow($1, $2::uuid, $3, $4::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
        )
        .bind(shard)
        .bind(owner)
        .bind(target_generation)
        .bind(run_id)
        .bind(i32::try_from(table.index).unwrap_or(i32::MAX))
        .bind(source)
        .bind(config_digest)
        .bind(source_generation)
        .bind(&table.schema_ident)
        .bind(&table.table_ident)
        .bind(&idents)
        .bind(&oids)
        .bind(&typmods)
        .bind(&required)
        .fetch_one(&mut *tx)
        .await
        .map_err(database_error)?;
        if physical != table.physical {
            return Err(ImportError::Binding("shadow name mismatch".to_string()));
        }
    }
    tx.commit().await.map_err(database_error)?;
    Ok(())
}

async fn retire_run(
    conn: &mut PgConnection,
    shard: &str,
    owner: &str,
    generation: i64,
    run_id: &str,
) -> Result<(), ImportError> {
    sqlx::query("SELECT _oo.full_import_retire_run($1, $2::uuid, $3, $4::uuid)")
        .bind(shard)
        .bind(owner)
        .bind(generation)
        .bind(run_id)
        .execute(&mut *conn)
        .await
        .map_err(database_error)?;
    Ok(())
}

async fn stage_decision(
    conn: &mut PgConnection,
    session: &ImportSession,
    physical: &str,
    batch_id: &str,
    payload_digest: &str,
    applied_pos: &str,
) -> Result<String, ImportError> {
    sqlx::query_scalar(
        "SELECT _oo.full_import_stage_decision($1, $2::uuid, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12::numeric)",
    )
    .bind(&session.shard)
    .bind(&session.owner)
    .bind(session.target_generation)
    .bind(&session.source_id)
    .bind(physical)
    .bind(batch_id)
    .bind(payload_digest)
    .bind(&session.config_digest)
    .bind(&session.type_digest)
    .bind(&session.hook_digest)
    .bind(&session.schema_digest)
    .bind(applied_pos)
    .fetch_one(&mut *conn)
    .await
    .map_err(database_error)
}

fn copy_columns(table: &TableCursor) -> Vec<CopyColumn> {
    let mut columns = Vec::with_capacity(table.columns.len() + 1);
    columns.push(CopyColumn::SystemPosition);
    for column in &table.columns {
        columns.push(CopyColumn::User {
            attnum: column.attnum,
        });
    }
    columns
}

fn copy_statement(table: &TableCursor) -> Result<String, ImportError> {
    let quoted = quote_physical_name(&table.physical).map_err(plan_error)?;
    let mut list = String::from("\"_oo_pos\"");
    for column in &table.columns {
        list.push_str(", ");
        list.push_str(&quote_physical_name(&column.physical).map_err(plan_error)?);
    }
    Ok(format!(
        "COPY _oo.{quoted} ({list}) FROM STDIN WITH (FORMAT binary)"
    ))
}

fn positions_for(start: u64, count: usize) -> Result<Vec<ExactDecimal>, ImportError> {
    let mut positions = Vec::with_capacity(count);
    for offset in 0..count {
        let index = start.saturating_add(offset as u64);
        positions.push(
            ExactDecimal::new(false, index.to_string(), 0)
                .map_err(|_| ImportError::Binding("position".to_string()))?,
        );
    }
    Ok(positions)
}

fn refusal_reason(batch: &TypedBatch) -> Option<String> {
    if let Some(reject) = batch.rejects.first() {
        return Some(reject.reason.to_string());
    }
    for plan in &batch.plans {
        if let MapDecision::Unsupported(reason) = plan.mapping.decision {
            return Some(reason.to_string());
        }
    }
    for row in &batch.rows {
        for cell in &row.cells {
            if cell.presence == CellPresence::Unavailable {
                return Some(
                    cell.reject
                        .map(|reject| reject.reason.to_string())
                        .unwrap_or_else(|| "unavailable".to_string()),
                );
            }
        }
    }
    None
}

fn reject_rows(batch: &TypedBatch) -> Vec<RejectByte> {
    batch
        .rejects
        .iter()
        .map(|reject| RejectByte {
            reason: reject.reason.to_string(),
            value_len: i64::try_from(reject.value_len).unwrap_or(i64::MAX),
            attnum: reject.attnum,
            row_index: i64::try_from(reject.row_index).unwrap_or(i64::MAX),
            key_digest: reject.key_digest.clone().unwrap_or_default(),
        })
        .collect()
}

/// One masked reject line in the full-import quality preimage.
///
/// `key_digest` is empty when the source did not capture one. The line does
/// not contain the rejected value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualityReject {
    /// Closed reason token.
    pub reason: String,
    /// Wire length. Not the value.
    pub value_len: i64,
    /// Source attribute number.
    pub attnum: i32,
    /// Zero-based row index inside the batch.
    pub row_index: i64,
    /// Key digest, or empty when absent.
    pub key_digest: String,
}

/// Bytes mixed into a payload digest after the COPY payload.
///
/// The first line is `full-import/v1 quality`. A batch with no reject lines
/// and a rejected count of zero is the historical preimage. A later batch
/// records one `reject` line per masked loss. The value itself is not a field.
pub fn quality_preimage(
    batch_id: &str,
    accepted: u64,
    rejected: u64,
    input_digest: &str,
    rejects: &[QualityReject],
) -> Vec<u8> {
    let mut text = format!(
        "full-import/v1 quality\nbatch\t{batch_id}\t{accepted}\t{rejected}\t{input_digest}\n"
    );
    for reject in rejects {
        text.push_str(&format!(
            "reject\t{}\t{}\t{}\t{}\t{}\t{input_digest}\n",
            reject.reason, reject.value_len, reject.attnum, reject.row_index, reject.key_digest
        ));
    }
    text.into_bytes()
}

fn quality_bytes(
    batch_id: &str,
    accepted: u64,
    rejected: u64,
    input_digest: &str,
    rejects: &[RejectByte],
) -> Vec<u8> {
    let owned: Vec<QualityReject> = rejects
        .iter()
        .map(|reject| QualityReject {
            reason: reject.reason.clone(),
            value_len: reject.value_len,
            attnum: reject.attnum,
            row_index: reject.row_index,
            key_digest: reject.key_digest.clone(),
        })
        .collect();
    quality_preimage(batch_id, accepted, rejected, input_digest, &owned)
}

fn empty_batch(
    table: &TableCursor,
    source_id: &str,
    source_generation: i64,
    input_digest: &str,
) -> Result<TypedBatch, ImportError> {
    let mut plans = Vec::new();
    for column in &table.columns {
        plans.push(synthetic_plan(column)?);
    }
    let mut digest = [0_u8; 32];
    for (index, byte) in hex_decode(input_digest)
        .ok_or_else(|| ImportError::Binding("input digest".to_string()))?
        .into_iter()
        .take(32)
        .enumerate()
    {
        digest[index] = byte;
    }
    Ok(TypedBatch {
        protocol: TYPE_CONVERSION_PROTOCOL,
        type_policy_version: TYPE_POLICY_V1,
        source_id: source_id.to_string(),
        schema: table.schema.clone(),
        table: table.name.clone(),
        run_generation: u64::try_from(source_generation).unwrap_or(0),
        input_digest: digest,
        plans,
        fields: Vec::new(),
        rows: Vec::new(),
        arrow: Vec::new(),
        rejects: Vec::new(),
    })
}

fn synthetic_plan(column: &ShadowColumn) -> Result<ColumnPlan, ImportError> {
    let (logical, arrow, precision) = match column.oid {
        16 => (LogicalType::Bool, ArrowPhysical::Boolean, None),
        21 => (LogicalType::Int, ArrowPhysical::Int16, None),
        23 => (LogicalType::Int, ArrowPhysical::Int32, None),
        20 => (LogicalType::Int, ArrowPhysical::Int64, None),
        700 | 701 => (LogicalType::Float64, ArrowPhysical::Float64, None),
        25 | 114 => (LogicalType::Text, ArrowPhysical::Utf8, None),
        1042 | 1043 => {
            let precision = if column.typmod < 0 {
                None
            } else {
                Some(column.typmod - 4)
            };
            (LogicalType::Text, ArrowPhysical::Utf8, precision)
        }
        17 | 2950 => (LogicalType::Bytes, ArrowPhysical::Binary, None),
        1082 => (LogicalType::Date, ArrowPhysical::Date32, None),
        1083 => (LogicalType::Time, ArrowPhysical::Time64Micros, None),
        1114 => (
            LogicalType::TimestampNaive,
            ArrowPhysical::TimestampMicrosNaive,
            None,
        ),
        1184 => (
            LogicalType::TimestampTz,
            ArrowPhysical::TimestampMicrosUtc,
            None,
        ),
        1700 if column.typmod < 0 => (
            LogicalType::Decimal {
                precision: 0,
                scale: ARBITRARY_DECIMAL_SCALE,
            },
            ArrowPhysical::DecimalUtf8 {
                precision: None,
                scale: None,
            },
            None,
        ),
        1700 => {
            let precision_i = ((column.typmod - 4) >> 16) & 65535;
            let scale_i = (column.typmod - 4) & 65535;
            (
                LogicalType::Decimal {
                    precision: u16::try_from(precision_i).unwrap_or(0),
                    scale: i16::try_from(scale_i).unwrap_or(0),
                },
                ArrowPhysical::DecimalUtf8 {
                    precision: Some(precision_i),
                    scale: Some(scale_i),
                },
                Some(precision_i),
            )
        }
        _ => {
            return Err(ImportError::Unsupported {
                type_name: column.name.clone(),
                oid: column.oid,
            });
        }
    };
    Ok(ColumnPlan {
        mapping: ColumnMapping {
            protocol: TYPE_CONVERSION_PROTOCOL,
            source_type: column.name.clone(),
            arrow_type: column.name.clone(),
            pg_type: column.name.clone(),
            precision,
            scale: None,
            catalog_v1_numeric_scale: None,
            numeric_typmod_version: None,
            timezone: None,
            nullable: !column.not_null,
            type_policy_version: TYPE_POLICY_V1,
            decision: MapDecision::Lossless,
            metadata: Vec::new(),
        },
        attnum: column.attnum,
        name: column.name.clone(),
        oid: column.oid,
        decode_oid: column.oid,
        logical: Some(logical),
        arrow: Some(arrow),
        element: None,
        enum_labels: Vec::new(),
        key: false,
    })
}

fn plan_error(error: PlanError) -> ImportError {
    ImportError::Binding(error.to_string())
}

fn source_error(error: SourceError) -> ImportError {
    ImportError::Source(error.to_string())
}

fn sink_error(error: SinkError) -> ImportError {
    match error {
        SinkError::Encode(error) => ImportError::Refused {
            reason: error.code.to_string(),
        },
        SinkError::Driver(detail) => ImportError::Database {
            sqlstate: String::new(),
            message: detail.to_string(),
        },
    }
}

fn database_error(error: sqlx::Error) -> ImportError {
    let sqlstate = error
        .as_database_error()
        .and_then(|db| db.code().map(|code| code.into_owned()))
        .unwrap_or_default();
    let message = error
        .as_database_error()
        .map(|db| db.message().to_string())
        .unwrap_or_else(|| error.to_string());
    if sqlstate == "OO010" {
        return ImportError::Conflict;
    }
    ImportError::Database { sqlstate, message }
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let high = hex_nibble(bytes[index])?;
        let low = hex_nibble(bytes[index + 1])?;
        out.push((high << 4) | low);
        index += 2;
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_columns_keep_the_physical_typmod() {
        let numeric = approved_column_typmod(1700, 2_490_376).expect("numeric");
        assert_eq!(numeric.typmod, 2_490_376);
        let uuid = approved_column_typmod(2950, -1).expect("uuid");
        assert_eq!(uuid.typmod, -1);
        let rows = approved_columns(&[(1700, 2_490_376), (2950, -1)]).expect("rows");
        assert_eq!(rows[0].typmod, 2_490_376);
        assert_eq!(rows[1].oid, 2950);
        assert!(approved_column_typmod(99999, -1).is_err());
        assert!(approved_columns(&[(2950, -1), (1700, -1), (1, -1)]).is_err());
    }

    #[test]
    fn hook_digest_is_sixty_four_hex_and_snapshot_finished_is_not_used() {
        let digest = hook_digest();
        assert_eq!(digest.len(), 64);
        assert!(
            digest
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        let source = include_str!("full_import.rs");
        let banned = ["Snapshot", "Finished"].concat();
        assert!(!source.contains(&banned));
        assert!(source.contains("finish_complete"));
    }

    #[test]
    fn quality_preimage_keeps_a_zero_reject_line_and_records_a_loss() {
        let clear = quality_preimage("t0.b0", 5, 0, "abc", &[]);
        assert_eq!(clear, b"full-import/v1 quality\nbatch\tt0.b0\t5\t0\tabc\n");
        let loss = quality_preimage(
            "t0.b0",
            5,
            1,
            "abc",
            &[QualityReject {
                reason: "zero_date".to_string(),
                value_len: 10,
                attnum: 12,
                row_index: 1,
                key_digest: String::new(),
            }],
        );
        let text = String::from_utf8(loss).expect("utf8");
        assert!(text.starts_with("full-import/v1 quality\nbatch\tt0.b0\t5\t1\tabc\n"));
        assert!(text.contains("reject\tzero_date\t10\t12\t1\t\tabc\n"));
        assert!(!text.contains("raw"));
    }
}
