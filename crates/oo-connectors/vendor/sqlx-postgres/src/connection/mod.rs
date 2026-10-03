use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};
use std::future::Future;
use std::sync::Arc;

use crate::HashMap;

use crate::common::StatementCache;
use crate::error::Error;
use crate::ext::ustr::UStr;
use crate::io::{PortalId, StatementId};
use crate::message::{
    BackendMessageFormat, Close, Query, ReadyForQuery, ReceivedMessage, Terminate,
    TransactionStatus,
};
use crate::statement::PgStatementMetadata;
use crate::transaction::Transaction;
use crate::types::Oid;
use crate::{PgConnectOptions, PgTypeInfo, Postgres};

pub(crate) use sqlx_core::connection::*;
use sqlx_core::sql_str::SqlSafeStr;

pub use self::portal::{
    PgNamedPortal, PgPortalAccounting, PgPortalAllowance, PgPortalClose, PgPortalCommandTag,
    PgPortalOutcome, PgPortalStep, EXECUTE_LIMIT,
};
pub use self::stream::{
    PgIncomingAllocEvent, PgIncomingAllocation, PgIncomingBounds, PgStream,
};
use sqlx_core::bytes::Bytes;

#[cfg(feature = "offline")]
mod describe;
mod establish;
mod executor;
mod portal;
mod resolve;
mod sasl;
mod stream;
mod tls;

/// One backend frame, with the original type byte and the body after the length word.
#[derive(Debug)]
pub struct PgBackendFrame {
    format_byte: u8,
    contents: Bytes,
}

impl PgBackendFrame {
    pub fn format_byte(&self) -> u8 {
        self.format_byte
    }

    pub fn contents(&self) -> &Bytes {
        &self.contents
    }
}

fn format_wire_byte(format: &BackendMessageFormat) -> u8 {
    match format {
        BackendMessageFormat::Authentication => b'R',
        BackendMessageFormat::BackendKeyData => b'K',
        BackendMessageFormat::BindComplete => b'2',
        BackendMessageFormat::CloseComplete => b'3',
        BackendMessageFormat::CommandComplete => b'C',
        BackendMessageFormat::CopyData => b'd',
        BackendMessageFormat::CopyDone => b'c',
        BackendMessageFormat::CopyInResponse => b'G',
        BackendMessageFormat::CopyOutResponse => b'H',
        BackendMessageFormat::DataRow => b'D',
        BackendMessageFormat::EmptyQueryResponse => b'I',
        BackendMessageFormat::ErrorResponse => b'E',
        BackendMessageFormat::NoData => b'n',
        BackendMessageFormat::NoticeResponse => b'N',
        BackendMessageFormat::NotificationResponse => b'A',
        BackendMessageFormat::ParameterDescription => b't',
        BackendMessageFormat::ParameterStatus => b'S',
        BackendMessageFormat::ParseComplete => b'1',
        BackendMessageFormat::PortalSuspended => b's',
        BackendMessageFormat::ReadyForQuery => b'Z',
        BackendMessageFormat::RowDescription => b'T',
    }
}

/// A connection to a PostgreSQL database.
///
/// See [`PgConnectOptions`] for connection URL reference.
pub struct PgConnection {
    pub(crate) inner: Box<PgConnectionInner>,
}

/// Exclusive named-portal state. Ordinary queries require [`PortalLease::Clear`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PortalLease {
    Clear,
    Busy,
    Poisoned,
}

pub struct PgConnectionInner {
    // underlying TCP or UDS stream,
    // wrapped in a potentially TLS stream,
    // wrapped in a buffered stream
    pub(crate) stream: PgStream,

    // process id of this backend
    // used to send cancel requests
    #[allow(dead_code)]
    process_id: u32,

    // secret key of this backend
    // used to send cancel requests
    #[allow(dead_code)]
    secret_key: u32,

    // sequence of statement IDs for use in preparing statements
    // in PostgreSQL, the statement is prepared to a user-supplied identifier
    next_statement_id: StatementId,

    // separate from statement ids; named portals do not reuse the statement counter
    next_portal_id: PortalId,

    // Clear: ordinary executor. Busy: a named portal holds the connection.
    // Poisoned: the connection must not be reused.
    pub(crate) portal_lease: PortalLease,

    // cache statement by query string to the id and columns
    cache_statement: StatementCache<(StatementId, Arc<PgStatementMetadata>)>,

    // cache user-defined types by id <-> info
    cache_type_info: HashMap<Oid, PgTypeInfo>,
    cache_type_oid: HashMap<UStr, Oid>,
    cache_elem_type_to_array: HashMap<Oid, Oid>,
    cache_table_data: HashMap<Oid, TableData>,

    // number of ReadyForQuery messages that we are currently expecting
    pub(crate) pending_ready_for_query_count: usize,

    // current transaction status
    transaction_status: TransactionStatus,
    pub(crate) transaction_depth: usize,

    log_settings: LogSettings,
}

pub(crate) struct TableData {
    table_name: Arc<str>,
    /// Attribute number -> name.
    columns: BTreeMap<i16, Arc<str>>,
}

impl PgConnection {
    /// the version number of the server in `libpq` format
    pub fn server_version_num(&self) -> Option<u32> {
        self.inner.stream.server_version_num
    }

    /// Replace the opt-in receive bounds. A poisoned connection stays poisoned.
    pub fn set_incoming_bounds(&mut self, bounds: Option<PgIncomingBounds>) -> Result<(), Error> {
        self.inner.stream.set_bounds(bounds);
        Ok(())
    }

    pub fn incoming_bounds(&self) -> Option<PgIncomingBounds> {
        self.inner.stream.bounds()
    }

    /// View capacities, reserve arguments, growth, and copy counts on the socket buffer.
    /// This is not the configured cap and it is not a resident-set size.
    pub fn incoming_allocation(&self) -> PgIncomingAllocation {
        self.inner.stream.allocation()
    }

    /// False after a receive-bound or length fault. Hard close is still available.
    pub fn incoming_synchronized(&self) -> bool {
        self.inner.stream.fault().is_none()
    }

    /// Read one backend frame without decoding it or swallowing notices.
    pub async fn recv_backend(&mut self) -> Result<PgBackendFrame, Error> {
        let message = self.inner.stream.recv_unchecked().await?;
        Ok(PgBackendFrame {
            format_byte: format_wire_byte(&message.format),
            contents: message.contents,
        })
    }

    // will return when the connection is ready for another query
    pub(crate) async fn wait_until_ready(&mut self) -> Result<(), Error> {
        if self.inner.portal_lease == PortalLease::Poisoned {
            return Err(Error::Protocol(
                "connection is unsynchronized after a named portal".into(),
            ));
        }

        if let Some(fault) = self.inner.stream.fault() {
            return Err(Error::IncomingWire(fault));
        }

        if !self.inner.stream.write_buffer_mut().is_empty() {
            self.inner.stream.flush().await?;
        }

        while self.inner.pending_ready_for_query_count > 0 {
            let message = self.inner.stream.recv().await?;

            if let BackendMessageFormat::ReadyForQuery = message.format {
                self.handle_ready_for_query(message)?;
            }
        }

        Ok(())
    }

    async fn recv_ready_for_query(&mut self) -> Result<(), Error> {
        let r: ReadyForQuery = self.inner.stream.recv_expect().await?;

        self.inner.pending_ready_for_query_count -= 1;
        self.inner.transaction_status = r.transaction_status;

        Ok(())
    }

    #[inline(always)]
    fn handle_ready_for_query(&mut self, message: ReceivedMessage) -> Result<(), Error> {
        self.inner.pending_ready_for_query_count = self
            .inner
            .pending_ready_for_query_count
            .checked_sub(1)
            .ok_or_else(|| err_protocol!("received more ReadyForQuery messages than expected"))?;

        self.inner.transaction_status = message.decode::<ReadyForQuery>()?.transaction_status;

        Ok(())
    }

    /// Queue a simple query (not prepared) to execute the next time this connection is used.
    ///
    /// Used for rolling back transactions and releasing advisory locks.
    #[inline(always)]
    pub(crate) fn queue_simple_query(&mut self, query: &str) -> Result<(), Error> {
        match self.inner.portal_lease {
            // Drop of a transaction must not panic or write on an unsynchronized socket.
            PortalLease::Poisoned => Ok(()),
            PortalLease::Busy => Err(Error::Protocol(
                "named portal holds this connection".into(),
            )),
            PortalLease::Clear => {
                self.inner.stream.write_msg(Query(query))?;
                self.inner.pending_ready_for_query_count += 1;
                Ok(())
            }
        }
    }

    pub(crate) fn refuse_portal_lease(&self) -> Result<(), Error> {
        match self.inner.portal_lease {
            PortalLease::Clear => Ok(()),
            PortalLease::Busy => Err(Error::Protocol(
                "named portal holds this connection".into(),
            )),
            PortalLease::Poisoned => Err(Error::Protocol(
                "connection is unsynchronized after a named portal".into(),
            )),
        }
    }

    pub(crate) fn in_transaction(&self) -> bool {
        match self.inner.transaction_status {
            TransactionStatus::Transaction => true,
            TransactionStatus::Error | TransactionStatus::Idle => false,
        }
    }
}

impl Debug for PgConnection {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgConnection").finish()
    }
}

impl Connection for PgConnection {
    type Database = Postgres;

    type Options = PgConnectOptions;

    async fn close(mut self) -> Result<(), Error> {
        // The normal, graceful termination procedure is that the frontend sends a Terminate
        // message and immediately closes the connection.

        // On receipt of this message, the backend closes the
        // connection and terminates.
        self.inner.stream.send(Terminate).await?;
        self.inner.stream.shutdown().await?;

        Ok(())
    }

    async fn close_hard(mut self) -> Result<(), Error> {
        self.inner.stream.shutdown().await?;

        Ok(())
    }

    async fn ping(&mut self) -> Result<(), Error> {
        // Users were complaining about this showing up in query statistics on the server.
        // By sending a comment we avoid an error if the connection was in the middle of a rowset
        // self.execute("/* SQLx ping */").map_ok(|_| ()).boxed()

        // The simplest call-and-response that's possible.
        self.refuse_portal_lease()?;
        self.write_sync();
        self.wait_until_ready().await
    }

    fn begin(
        &mut self,
    ) -> impl Future<Output = Result<Transaction<'_, Self::Database>, Error>> + Send + '_ {
        Transaction::begin(self, None)
    }

    fn begin_with(
        &mut self,
        statement: impl SqlSafeStr,
    ) -> impl Future<Output = Result<Transaction<'_, Self::Database>, Error>> + Send + '_
    where
        Self: Sized,
    {
        Transaction::begin(self, Some(statement.into_sql_str()))
    }

    fn cached_statements_size(&self) -> usize {
        self.inner.cache_statement.len()
    }

    async fn clear_cached_statements(&mut self) -> Result<(), Error> {
        self.inner.cache_type_oid.clear();

        let mut cleared = 0_usize;

        self.wait_until_ready().await?;

        while let Some((id, _)) = self.inner.cache_statement.remove_lru() {
            self.inner.stream.write_msg(Close::Statement(id))?;
            cleared += 1;
        }

        if cleared > 0 {
            self.write_sync();
            self.inner.stream.flush().await?;

            self.wait_for_close_complete(cleared).await?;
            self.recv_ready_for_query().await?;
        }

        Ok(())
    }

    fn shrink_buffers(&mut self) {
        self.inner.stream.shrink_buffers();
    }

    #[doc(hidden)]
    fn flush(&mut self) -> impl Future<Output = Result<(), Error>> + Send + '_ {
        self.wait_until_ready()
    }

    #[doc(hidden)]
    fn should_flush(&self) -> bool {
        !self.inner.stream.write_buffer().is_empty()
    }
}

// Implement `AsMut<Self>` so that `PgConnection` can be wrapped in
// a `PgAdvisoryLockGuard`.
//
// See: https://github.com/launchbadge/sqlx/issues/2520
impl AsMut<PgConnection> for PgConnection {
    fn as_mut(&mut self) -> &mut PgConnection {
        self
    }
}
