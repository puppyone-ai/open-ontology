//! Pull-driven named portal on an already open transaction.
//!
//! Each pull reserves one row and then writes `Execute` with limit 1.
//! The response is read until `PortalSuspended`, `CommandComplete`,
//! `EmptyQueryResponse`, or an error before another `Execute` is written.
//! `Flush` keeps the portal. `Sync` is used for close and for error recovery.

use std::sync::Arc;

use sqlx_core::arguments::Arguments;
use sqlx_core::bytes::Bytes;

use crate::error::Error;
use crate::io::{PortalId, StatementId};
use crate::message::{
    BackendMessageFormat, Bind, Close, CommandComplete, DataRow, Execute, Flush,
};
use crate::statement::PgStatementMetadata;
use crate::PgArguments;
use crate::{PgConnection, PgRow, PgValueFormat};

use super::executor::prepare;
use super::PortalLease;
use crate::message::TransactionStatus;

/// Wire `Execute` row limit for this API. The protocol uses zero for unlimited.
pub const EXECUTE_LIMIT: u32 = 1;

const _: () = assert!(EXECUTE_LIMIT == 1);

const TAG_PREFIX: usize = 24;

/// Caller grant. This API reserves [`EXECUTE_LIMIT`] rows from it and leaves the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PgPortalAllowance {
    rows: std::num::NonZeroU32,
}

impl PgPortalAllowance {
    pub fn positive(rows: u32) -> Result<Self, Error> {
        let rows = std::num::NonZeroU32::new(rows).ok_or_else(|| {
            Error::Protocol("named portal row allowance must be positive".into())
        })?;
        Ok(Self { rows })
    }

    pub fn rows(self) -> u32 {
        self.rows.get()
    }
}

/// Rows reserved for one `Execute` and `DataRow` frames accepted into a step.
///
/// `reserved_rows` is recorded before the request is written. `consumed_rows`
/// counts rows delivered in a successful step. A discarded error result stays at
/// zero. This is not a source-wide limiter and not a finished-scan token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PgPortalAccounting {
    pub execute_limit: u32,
    pub reserved_rows: u32,
    pub consumed_rows: u32,
}

/// Bounded copy of a `CommandComplete` tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PgPortalCommandTag {
    rows_affected: u64,
    prefix: [u8; TAG_PREFIX],
    prefix_len: u8,
    truncated: bool,
}

impl PgPortalCommandTag {
    pub fn rows_affected(self) -> u64 {
        self.rows_affected
    }

    pub fn prefix(&self) -> &[u8] {
        &self.prefix[..usize::from(self.prefix_len)]
    }

    pub fn truncated(self) -> bool {
        self.truncated
    }

    fn from_contents(contents: &Bytes, rows_affected: u64) -> Self {
        let raw: &[u8] = contents;
        let trimmed = raw.strip_suffix(&[0]).unwrap_or(raw);
        let mut prefix = [0u8; TAG_PREFIX];
        let n = trimmed.len().min(TAG_PREFIX);
        prefix[..n].copy_from_slice(&trimmed[..n]);
        Self {
            rows_affected,
            prefix,
            prefix_len: u8::try_from(n).expect("tag prefix fits in one byte"),
            truncated: trimmed.len() > TAG_PREFIX,
        }
    }
}

/// One drained `Execute`. A row by itself is neither suspension nor completion.
pub enum PgPortalOutcome {
    /// `DataRow` followed by `PortalSuspended`. The portal and transaction stay open.
    Suspended(PgRow),
    /// `CommandComplete` or `EmptyQueryResponse`, after the owned portal and
    /// statement were closed and `ReadyForQuery` was observed. The caller
    /// transaction is still open.
    Complete {
        row: Option<PgRow>,
        command_tag: PgPortalCommandTag,
    },
}

pub struct PgPortalStep {
    pub outcome: PgPortalOutcome,
    pub accounting: PgPortalAccounting,
}

/// Explicit close of a portal that had not completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PgPortalClose {
    pub close_completes: u8,
    pub ready: bool,
    pub transaction_open: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Ready,
    Suspended,
    Closed,
    Poisoned,
}

/// Named portal bound inside the caller's transaction.
///
/// The portal holds the connection borrow. Dropping it before close or
/// completion poisons the connection. Drop does not claim the backend is gone.
pub struct PgNamedPortal<'c> {
    conn: &'c mut PgConnection,
    portal: PortalId,
    statement: StatementId,
    metadata: Arc<PgStatementMetadata>,
    phase: Phase,
    last_pull: Option<PgPortalAccounting>,
}

impl Drop for PgNamedPortal<'_> {
    fn drop(&mut self) {
        if self.phase != Phase::Closed {
            self.conn.inner.portal_lease = PortalLease::Poisoned;
            self.phase = Phase::Poisoned;
        }
    }
}

impl PgNamedPortal<'_> {
    pub fn last_pull(&self) -> Option<PgPortalAccounting> {
        self.last_pull
    }

    pub fn connection_poisoned(&self) -> bool {
        self.conn.inner.portal_lease == PortalLease::Poisoned || self.phase == Phase::Poisoned
    }

    pub async fn pull(&mut self, allowance: PgPortalAllowance) -> Result<PgPortalStep, Error> {
        if self.connection_poisoned() || self.conn.inner.stream.fault().is_some() {
            self.phase = Phase::Poisoned;
            self.conn.inner.portal_lease = PortalLease::Poisoned;
            return Err(Error::Protocol(
                "connection is unsynchronized after a named portal".into(),
            ));
        }
        if self.phase == Phase::Closed {
            return Err(Error::Protocol("named portal is closed".into()));
        }
        let mut flight = Flight {
            portal: self,
            armed: true,
        };
        let result = flight.pull(allowance).await;
        flight.armed = false;
        result
    }

    pub async fn close(mut self) -> Result<PgPortalClose, Error> {
        if self.phase == Phase::Closed {
            return Ok(PgPortalClose {
                close_completes: 0,
                ready: true,
                transaction_open: matches!(
                    self.conn.inner.transaction_status,
                    TransactionStatus::Transaction
                ),
            });
        }
        if self.connection_poisoned() {
            self.phase = Phase::Poisoned;
            self.conn.inner.portal_lease = PortalLease::Poisoned;
            return Err(Error::Protocol(
                "connection is unsynchronized after a named portal".into(),
            ));
        }
        let mut flight = Flight {
            portal: &mut self,
            armed: true,
        };
        let result = flight.close_owned().await;
        flight.armed = false;
        drop(flight);
        result?;
        self.phase = Phase::Closed;
        self.conn.inner.portal_lease = PortalLease::Clear;
        Ok(PgPortalClose {
            close_completes: 2,
            ready: true,
            transaction_open: true,
        })
    }
}

struct Flight<'a, 'c> {
    portal: &'a mut PgNamedPortal<'c>,
    armed: bool,
}

impl Drop for Flight<'_, '_> {
    fn drop(&mut self) {
        if self.armed && self.portal.phase != Phase::Closed {
            self.portal.phase = Phase::Poisoned;
            self.portal.conn.inner.portal_lease = PortalLease::Poisoned;
        }
    }
}

impl Flight<'_, '_> {
    async fn pull(&mut self, allowance: PgPortalAllowance) -> Result<PgPortalStep, Error> {
        if allowance.rows() < EXECUTE_LIMIT {
            return Err(Error::Protocol(
                "named portal row allowance must cover the execute limit".into(),
            ));
        }
        let accounting = PgPortalAccounting {
            execute_limit: EXECUTE_LIMIT,
            reserved_rows: EXECUTE_LIMIT,
            consumed_rows: 0,
        };
        self.portal.last_pull = Some(accounting);

        if let Err(err) = self.write_execute() {
            self.poison();
            return Err(err);
        }
        if let Err(err) = self.portal.conn.inner.stream.flush().await {
            self.poison();
            return Err(Error::Io(err));
        }

        match self.drain_execute().await {
            Ok(Drained::Suspended(row)) => {
                self.portal.phase = Phase::Suspended;
                let accounting = PgPortalAccounting {
                    consumed_rows: 1,
                    ..accounting
                };
                self.portal.last_pull = Some(accounting);
                Ok(PgPortalStep {
                    outcome: PgPortalOutcome::Suspended(row),
                    accounting,
                })
            }
            Ok(Drained::Complete { row, tag }) => {
                let consumed = u32::from(row.is_some());
                let accounting = PgPortalAccounting {
                    consumed_rows: consumed,
                    ..accounting
                };
                self.portal.last_pull = Some(accounting);
                if let Err(err) = self.close_owned().await {
                    if let Some(last) = self.portal.last_pull.as_mut() {
                        last.consumed_rows = 0;
                    }
                    return Err(err);
                }
                self.portal.phase = Phase::Closed;
                self.portal.conn.inner.portal_lease = PortalLease::Clear;
                Ok(PgPortalStep {
                    outcome: PgPortalOutcome::Complete {
                        row,
                        command_tag: tag,
                    },
                    accounting,
                })
            }
            Err(err) => Err(err),
        }
    }

    fn write_execute(&mut self) -> Result<(), Error> {
        let portal = self.portal.portal;
        self.portal.conn.inner.stream.write_msg(Execute {
            portal,
            limit: EXECUTE_LIMIT,
        })?;
        self.portal.conn.inner.stream.write_msg(Flush)?;
        Ok(())
    }

    fn poison(&mut self) {
        self.portal.phase = Phase::Poisoned;
        self.portal.conn.inner.portal_lease = PortalLease::Poisoned;
    }

    async fn drain_execute(&mut self) -> Result<Drained, Error> {
        let mut row: Option<PgRow> = None;
        let mut rows: u32 = 0;
        loop {
            let message = match self.portal.conn.inner.stream.recv().await {
                Ok(message) => message,
                Err(err) => {
                    if let Some(last) = self.portal.last_pull.as_mut() {
                        last.consumed_rows = 0;
                    }
                    return Err(self.recover_error(err).await);
                }
            };
            match message.format {
                BackendMessageFormat::DataRow => {
                    let data = match message.decode::<DataRow>() {
                        Ok(data) => data,
                        Err(err) => {
                            self.poison();
                            return Err(err);
                        }
                    };
                    rows = rows.saturating_add(1);
                    if rows == 1 {
                        row = Some(PgRow {
                            data,
                            format: PgValueFormat::Binary,
                            metadata: Arc::clone(&self.portal.metadata),
                        });
                    }
                }
                BackendMessageFormat::PortalSuspended => {
                    if rows != 1 {
                        self.poison();
                        return Err(Error::Protocol(
                            "named portal suspension did not match the one reserved row".into(),
                        ));
                    }
                    let Some(row) = row else {
                        self.poison();
                        return Err(Error::Protocol(
                            "named portal suspension missing the row".into(),
                        ));
                    };
                    return Ok(Drained::Suspended(row));
                }
                BackendMessageFormat::CommandComplete => {
                    if rows > 1 {
                        self.poison();
                        return Err(Error::Protocol(
                            "named portal execute returned more rows than reserved".into(),
                        ));
                    }
                    let contents = message.contents.clone();
                    let complete = match message.decode::<CommandComplete>() {
                        Ok(complete) => complete,
                        Err(err) => {
                            self.poison();
                            return Err(err);
                        }
                    };
                    return Ok(Drained::Complete {
                        row,
                        tag: PgPortalCommandTag::from_contents(&contents, complete.rows_affected()),
                    });
                }
                BackendMessageFormat::EmptyQueryResponse => {
                    if rows != 0 {
                        self.poison();
                        return Err(Error::Protocol(
                            "named portal empty query carried a row".into(),
                        ));
                    }
                    return Ok(Drained::Complete {
                        row: None,
                        tag: PgPortalCommandTag::from_contents(&Bytes::from_static(b"EMPTY"), 0),
                    });
                }
                _ => {
                    self.poison();
                    return Err(Error::Protocol(
                        "named portal execute saw an unexpected message".into(),
                    ));
                }
            }
        }
    }

    async fn recover_error(&mut self, err: Error) -> Error {
        let synced = finish_extended_error(self.portal.conn, err).await;
        if self.portal.conn.inner.portal_lease == PortalLease::Clear {
            self.portal.phase = Phase::Closed;
        } else {
            self.portal.phase = Phase::Poisoned;
        }
        synced
    }

    async fn close_owned(&mut self) -> Result<(), Error> {
        let outcome = self.close_owned_inner().await;
        if outcome.is_err() {
            self.poison();
        }
        outcome
    }

    async fn close_owned_inner(&mut self) -> Result<(), Error> {
        if self.portal.conn.inner.pending_ready_for_query_count != 0 {
            return Err(Error::Protocol(
                "named portal close while a ready response is pending".into(),
            ));
        }
        let portal = self.portal.portal;
        let statement = self.portal.statement;
        self.portal
            .conn
            .inner
            .stream
            .write_msg(Close::Portal(portal))?;
        self.portal
            .conn
            .inner
            .stream
            .write_msg(Close::Statement(statement))?;
        self.portal.conn.write_sync();
        self.portal.conn.inner.stream.flush().await?;
        expect_close_complete(self.portal.conn).await?;
        expect_close_complete(self.portal.conn).await?;
        self.portal.conn.recv_ready_for_query().await?;
        if self.portal.conn.inner.pending_ready_for_query_count != 0
            || !matches!(
                self.portal.conn.inner.transaction_status,
                TransactionStatus::Transaction
            )
        {
            return Err(Error::Protocol(
                "named portal close lost the caller transaction".into(),
            ));
        }
        Ok(())
    }
}

enum Drained {
    Suspended(PgRow),
    Complete {
        row: Option<PgRow>,
        tag: PgPortalCommandTag,
    },
}

async fn expect_close_complete(conn: &mut PgConnection) -> Result<(), Error> {
    let message = conn.inner.stream.recv().await?;
    if message.format == BackendMessageFormat::CloseComplete {
        Ok(())
    } else {
        Err(Error::Protocol(
            "named portal close expected CloseComplete".into(),
        ))
    }
}

/// After `ErrorResponse`, one `Sync` and `ReadyForQuery` in status `E` leaves
/// rollback available. Any other shape poisons the connection.
async fn finish_extended_error(conn: &mut PgConnection, err: Error) -> Error {
    if !matches!(err, Error::Database(_))
        || conn.inner.pending_ready_for_query_count != 0
        || conn.inner.stream.fault().is_some()
    {
        conn.inner.portal_lease = PortalLease::Poisoned;
        return err;
    }
    conn.write_sync();
    if conn.inner.stream.flush().await.is_err() {
        conn.inner.portal_lease = PortalLease::Poisoned;
        return err;
    }
    match conn.recv_ready_for_query().await {
        Ok(())
            if matches!(conn.inner.transaction_status, TransactionStatus::Error)
                && conn.inner.pending_ready_for_query_count == 0 =>
        {
            conn.inner.portal_lease = PortalLease::Clear;
            err
        }
        _ => {
            conn.inner.portal_lease = PortalLease::Poisoned;
            err
        }
    }
}

struct OpenGuard<'a> {
    conn: Option<&'a mut PgConnection>,
    armed: bool,
}

impl Drop for OpenGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            if let Some(conn) = self.conn.as_deref_mut() {
                conn.inner.portal_lease = PortalLease::Poisoned;
            }
        }
    }
}

impl<'a> OpenGuard<'a> {
    async fn open(
        mut self,
        sql: &str,
        mut arguments: PgArguments,
    ) -> Result<PgNamedPortal<'a>, Error> {
        self.armed = true;

        let prepared = {
            let conn = self.conn.as_deref_mut().expect("connection");
            prepare(conn, sql, &arguments.types, None, true, false).await
        };
        let (statement, metadata) = match prepared {
            Ok(prepared) => prepared,
            Err(err) => return Err(self.abandon_prepare(err).await),
        };

        let portal = match {
            let conn = self.conn.as_deref_mut().expect("connection");
            take_portal_id(conn)
        } {
            Ok(portal) => portal,
            Err(err) => return Err(self.fail_after_prepare(statement, err).await),
        };

        if let Err(err) = {
            let conn = self.conn.as_deref_mut().expect("connection");
            arguments.apply_patches(conn, &metadata.parameters).await
        } {
            return Err(self.fail_after_prepare(statement, err).await);
        }

        let num_params = match u16::try_from(arguments.len()) {
            Ok(num_params) => num_params,
            Err(_) => {
                return Err(self
                    .fail_after_prepare(
                        statement,
                        Error::Protocol("named portal has too many arguments".into()),
                    )
                    .await);
            }
        };

        let bound = {
            let conn = self.conn.as_deref_mut().expect("connection");
            bind_portal(conn, portal, statement, num_params, &arguments).await
        };
        if let Err(err) = bound {
            return Err(self.fail_bind(statement, err).await);
        }

        self.armed = false;
        let conn = self.conn.take().expect("connection");
        conn.inner.portal_lease = PortalLease::Busy;
        Ok(PgNamedPortal {
            conn,
            portal,
            statement,
            metadata,
            phase: Phase::Ready,
            last_pull: None,
        })
    }

    async fn abandon_prepare(&mut self, err: Error) -> Error {
        let Some(conn) = self.conn.as_deref_mut() else {
            return err;
        };
        let ready = conn.wait_until_ready().await.is_ok();
        let known = ready
            && conn.inner.pending_ready_for_query_count == 0
            && conn.inner.stream.fault().is_none()
            && matches!(
                conn.inner.transaction_status,
                TransactionStatus::Transaction | TransactionStatus::Error
            );
        if known {
            self.armed = false;
        } else {
            conn.inner.portal_lease = PortalLease::Poisoned;
        }
        err
    }

    async fn fail_after_prepare(&mut self, statement: StatementId, err: Error) -> Error {
        let Some(conn) = self.conn.as_deref_mut() else {
            return err;
        };
        let ready = conn.wait_until_ready().await.is_ok();
        let known = ready
            && conn.inner.pending_ready_for_query_count == 0
            && conn.inner.stream.fault().is_none();
        if !known {
            conn.inner.portal_lease = PortalLease::Poisoned;
            return err;
        }
        match conn.inner.transaction_status {
            TransactionStatus::Transaction => {
                if release_statement(conn, statement).await.is_err() {
                    conn.inner.portal_lease = PortalLease::Poisoned;
                    return err;
                }
                self.armed = false;
            }
            TransactionStatus::Error => {
                self.armed = false;
            }
            TransactionStatus::Idle => {
                conn.inner.portal_lease = PortalLease::Poisoned;
            }
        }
        err
    }

    async fn fail_bind(&mut self, statement: StatementId, err: Error) -> Error {
        let Some(conn) = self.conn.as_deref_mut() else {
            return err;
        };
        let err = finish_extended_error(conn, err).await;
        if conn.inner.portal_lease != PortalLease::Clear {
            return err;
        }
        if matches!(conn.inner.transaction_status, TransactionStatus::Transaction)
            && conn.inner.stream.fault().is_none()
            && conn.inner.pending_ready_for_query_count == 0
            && release_statement(conn, statement).await.is_err()
        {
            conn.inner.portal_lease = PortalLease::Poisoned;
            return err;
        }
        if conn.inner.portal_lease == PortalLease::Clear {
            self.armed = false;
        }
        err
    }
}

fn take_portal_id(conn: &mut PgConnection) -> Result<PortalId, Error> {
    let id = conn.inner.next_portal_id;
    let Some(next) = id.checked_next() else {
        return Err(Error::Protocol(
            "named portal identifiers exhausted".into(),
        ));
    };
    conn.inner.next_portal_id = next;
    Ok(id)
}

async fn release_statement(conn: &mut PgConnection, statement: StatementId) -> Result<(), Error> {
    if conn.inner.pending_ready_for_query_count != 0 || conn.inner.stream.fault().is_some() {
        return Err(Error::Protocol(
            "named portal could not release its statement".into(),
        ));
    }
    conn.inner.stream.write_msg(Close::Statement(statement))?;
    conn.write_sync();
    conn.inner.stream.flush().await?;
    expect_close_complete(conn).await?;
    conn.recv_ready_for_query().await?;
    if conn.inner.pending_ready_for_query_count != 0
        || !matches!(
            conn.inner.transaction_status,
            TransactionStatus::Transaction
        )
    {
        return Err(Error::Protocol(
            "named portal statement close lost the caller transaction".into(),
        ));
    }
    Ok(())
}

async fn bind_portal(
    conn: &mut PgConnection,
    portal: PortalId,
    statement: StatementId,
    num_params: u16,
    arguments: &PgArguments,
) -> Result<(), Error> {
    conn.inner.stream.write_msg(Bind {
        portal,
        statement,
        formats: &[PgValueFormat::Binary],
        num_params,
        params: &arguments.buffer,
        result_formats: &[PgValueFormat::Binary],
    })?;
    conn.inner.stream.write_msg(Flush)?;
    conn.inner.stream.flush().await?;
    let message = conn.inner.stream.recv().await?;
    if message.format == BackendMessageFormat::BindComplete {
        Ok(())
    } else {
        Err(Error::Protocol(
            "named portal bind expected BindComplete".into(),
        ))
    }
}

impl PgConnection {
    pub fn named_portal_poisoned(&self) -> bool {
        self.inner.portal_lease == PortalLease::Poisoned
    }

    /// Bind `sql` as a named portal on this connection's open transaction.
    ///
    /// Admission is `transaction_depth >= 1` and ready-for-query status `T`.
    /// The returned portal keeps that exclusive borrow until it is closed,
    /// completed, or dropped.
    pub async fn open_named_portal<'a>(
        &'a mut self,
        sql: &str,
        arguments: PgArguments,
    ) -> Result<PgNamedPortal<'a>, Error> {
        self.admit_open(&arguments)?;
        OpenGuard {
            conn: Some(self),
            armed: false,
        }
        .open(sql, arguments)
        .await
    }

    fn admit_open(&self, arguments: &PgArguments) -> Result<(), Error> {
        if self.inner.portal_lease == PortalLease::Poisoned || self.inner.stream.fault().is_some()
        {
            return Err(Error::Protocol(
                "connection is unsynchronized after a named portal".into(),
            ));
        }
        if self.inner.portal_lease != PortalLease::Clear {
            return Err(Error::Protocol(
                "named portal holds this connection".into(),
            ));
        }
        if self.inner.transaction_depth == 0
            || !matches!(
                self.inner.transaction_status,
                TransactionStatus::Transaction
            )
        {
            return Err(Error::Protocol(
                "named portal requires an open transaction".into(),
            ));
        }
        if self.inner.next_statement_id.checked_next().is_none() {
            return Err(Error::Protocol(
                "named statement identifiers exhausted".into(),
            ));
        }
        if self.inner.next_portal_id.checked_next().is_none() {
            return Err(Error::Protocol(
                "named portal identifiers exhausted".into(),
            ));
        }
        if u16::try_from(arguments.len()).is_err() {
            return Err(Error::Protocol(
                "named portal has too many arguments".into(),
            ));
        }
        Ok(())
    }
}
