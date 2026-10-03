use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use futures_channel::mpsc::UnboundedSender;
use futures_util::SinkExt;
use log::Level;
use sqlx_core::bytes::{Buf, BytesMut};

use crate::connection::tls::MaybeUpgradeTls;
use crate::error::{Error, IncomingWireError, ReceiveLimitKind};
use crate::message::{
    BackendMessage, BackendMessageFormat, EncodeMessage, FrontendMessage, Notice, Notification,
    ParameterStatus, ReceivedMessage,
};
use crate::net::{self, BufferedSocket, Socket};
use crate::{PgConnectOptions, PgDatabaseError, PgSeverity};

/// Opt-in limits for backend frames read on a [`PgConnection`](crate::PgConnection).
///
/// Unset means the historical buffer-the-frame path. The driver does not invent product caps.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PgIncomingBounds {
    max_message_len: u32,
    max_value_len: u32,
    max_row_payload: u32,
    max_columns: u16,
}

impl PgIncomingBounds {
    /// `max_message_len` is the Int32 length field, which includes itself and excludes the type byte.
    /// `max_value_len` is one DataRow field payload and does not include its 4-byte prefix.
    /// `max_row_payload` counts the natts word, every field prefix, and every field payload.
    ///
    /// `max_message_len` must be at least 4. `max_value_len` must not exceed `max_row_payload`.
    /// `max_columns` may be zero. A row cap below 2 rejects every DataRow that reaches the natts word.
    pub fn try_new(
        max_message_len: u32,
        max_value_len: u32,
        max_row_payload: u32,
        max_columns: u16,
    ) -> Result<Self, Error> {
        if max_message_len < 4 || max_value_len > max_row_payload {
            return Err(Error::InvalidArgument(
                "incoming bounds are inconsistent".to_owned(),
            ));
        }
        Ok(Self {
            max_message_len,
            max_value_len,
            max_row_payload,
            max_columns,
        })
    }

    pub fn max_message_len(self) -> u32 {
        self.max_message_len
    }

    pub fn max_value_len(self) -> u32 {
        self.max_value_len
    }

    pub fn max_row_payload(self) -> u32 {
        self.max_row_payload
    }

    pub fn max_columns(self) -> u16 {
        self.max_columns
    }
}

/// One measured socket read. Counts come from the buffer, not from a bound.
///
/// `reserve_argument` is the `BytesMut::reserve` argument (additional bytes beyond
/// `len`). `reserved_growth` is the change in the sum of the two live view
/// capacities. `view_capacity_before` and `view_capacity_after` are that sum, not
/// a physical resident total and not the sum of retained frames. `assembly_bytes`
/// is bytes copied while joining the windows. An abandoned offset in front of both
/// windows is not included, and in-place growth that keeps the pointer adds nothing.
/// `read_completed` is 0 when the socket read has not returned.
#[derive(Copy, Clone, Debug)]
pub struct PgIncomingAllocEvent {
    pub logical_need: u64,
    pub reserve_argument: u64,
    pub reserved_growth: u64,
    pub read_capacity_before: u64,
    pub available_capacity_before: u64,
    pub available_len_before: u64,
    pub available_capacity_after: u64,
    pub view_capacity_before: u64,
    pub view_capacity_after: u64,
    pub read_len_before: u64,
    pub bytes_returned: u64,
    pub assembly_bytes: u64,
    pub read_completed: u64,
}

/// Incoming buffer high-water for the life of the connection.
///
/// `reserved_additional` is view-capacity growth, not the sum of reserve arguments.
/// `reserve_argument_bytes` is that sum, including no-op calls. `noop_reserve_arguments`
/// is the no-op portion and is not growth. `view_capacity_peak` and
/// `accepted_backing_peak` are sums of live `BytesMut` view capacities. They are
/// not RSS, not `malloc_usable_size`, and not the sum of frames the caller still holds.
/// `trace_bytes` is the diagnostic event log, capped, with `events_dropped` counting
/// rows that were not stored.
#[derive(Clone, Debug)]
pub struct PgIncomingAllocation {
    pub logical_requested_peak: u64,
    pub reserved_additional: u64,
    pub reserve_calls: u64,
    pub noop_reserve_calls: u64,
    pub noop_reserve_arguments: u64,
    pub reserve_argument_bytes: u64,
    pub read_capacity_peak: u64,
    pub available_capacity_peak: u64,
    pub view_capacity_peak: u64,
    pub rounding_peak: u64,
    pub overread_bytes: u64,
    pub reads: u64,
    pub bytes_read: u64,
    pub messages: u64,
    pub accepted_logical_payload: u64,
    pub accepted_backing_peak: u64,
    pub assembly_bytes: u64,
    pub last_logical_need: u64,
    pub trace_bytes: u64,
    pub events_dropped: u64,
    pub events: Vec<PgIncomingAllocEvent>,
}

enum Inspected {
    Need(usize),
    Ready(usize),
    Wire(IncomingWireError, usize),
    Protocol(Error),
}

// the stream is a separate type from the connection to uphold the invariant where an instantiated
// [PgConnection] is a **valid** connection to postgres

// when a new connection is asked for, we work directly on the [PgStream] type until the
// connection is fully established

// in other words, `self` in any PgConnection method is a live connection to postgres that
// is fully prepared to receive queries

pub struct PgStream {
    // A trait object is okay here as the buffering amortizes the overhead of both the dynamic
    // function call as well as the syscall.
    inner: BufferedSocket<Box<dyn Socket>>,

    // buffer of unreceived notification messages from `PUBLISH`
    // this is set when creating a PgListener and only written to if that listener is
    // re-used for query execution in-between receiving messages
    pub(crate) notifications: Option<UnboundedSender<Notification>>,

    pub(crate) parameter_statuses: BTreeMap<String, String>,

    pub(crate) server_version_num: Option<u32>,

    bounds: Option<PgIncomingBounds>,
    fault: Option<IncomingWireError>,
}

impl PgStream {
    pub(super) async fn connect(options: &PgConnectOptions) -> Result<Self, Error> {
        let socket_result = match options.fetch_socket() {
            Some(ref path) => net::connect_uds(path, MaybeUpgradeTls(options)).await?,
            None => net::connect_tcp(&options.host, options.port, MaybeUpgradeTls(options)).await?,
        };

        let socket = socket_result?;

        Ok(Self {
            inner: BufferedSocket::new(socket),
            notifications: None,
            parameter_statuses: BTreeMap::default(),
            server_version_num: None,
            bounds: None,
            fault: None,
        })
    }

    pub(crate) fn set_bounds(&mut self, bounds: Option<PgIncomingBounds>) {
        self.bounds = bounds;
    }

    pub(crate) fn bounds(&self) -> Option<PgIncomingBounds> {
        self.bounds
    }

    pub(crate) fn fault(&self) -> Option<IncomingWireError> {
        self.fault
    }

    pub(crate) fn allocation(&self) -> PgIncomingAllocation {
        let parts = self.inner.incoming_meter_parts();
        let events = self
            .inner
            .incoming_meter_events()
            .into_iter()
            .map(|row| PgIncomingAllocEvent {
                logical_need: row[0],
                reserve_argument: row[1],
                reserved_growth: row[2],
                read_capacity_before: row[3],
                available_capacity_before: row[4],
                available_len_before: row[5],
                available_capacity_after: row[6],
                view_capacity_before: row[7],
                view_capacity_after: row[8],
                read_len_before: row[9],
                bytes_returned: row[10],
                assembly_bytes: row[11],
                read_completed: row[12],
            })
            .collect();
        PgIncomingAllocation {
            logical_requested_peak: parts[0],
            reserved_additional: parts[1],
            reserve_calls: parts[2],
            read_capacity_peak: parts[3],
            available_capacity_peak: parts[4],
            view_capacity_peak: parts[5],
            rounding_peak: parts[6],
            overread_bytes: parts[7],
            reads: parts[8],
            bytes_read: parts[9],
            messages: parts[10],
            accepted_logical_payload: parts[11],
            accepted_backing_peak: parts[12],
            assembly_bytes: parts[13],
            last_logical_need: parts[14],
            trace_bytes: parts[15],
            noop_reserve_calls: parts[16],
            noop_reserve_arguments: parts[17],
            reserve_argument_bytes: parts[18],
            events_dropped: parts[19],
            events,
        }
    }

    #[inline(always)]
    pub(crate) fn write_msg(&mut self, message: impl FrontendMessage) -> Result<(), Error> {
        self.write(EncodeMessage(message))
    }

    pub(crate) async fn send<T>(&mut self, message: T) -> Result<(), Error>
    where
        T: FrontendMessage,
    {
        self.write_msg(message)?;
        self.flush().await?;
        Ok(())
    }

    // Expect a specific type and format
    pub(crate) async fn recv_expect<B: BackendMessage>(&mut self) -> Result<B, Error> {
        self.recv().await?.decode()
    }

    pub(crate) async fn recv_unchecked(&mut self) -> Result<ReceivedMessage, Error> {
        // Cancel-safe: the retained buffer and the fault flag are the parser state.
        // The buffer is split only after `inspect` reports a complete accepted frame,
        // and that split is synchronous. Dropping this future keeps every byte already read.
        if let Some(fault) = self.fault {
            return Err(Error::IncomingWire(fault));
        }

        let bounds = self.bounds;
        // Atomic, not Cell: the read future is Send, and Cell is not Sync.
        let noted = AtomicU64::new(0);
        let result = self
            .inner
            .try_read(|buf| match inspect(buf, bounds) {
                Inspected::Need(needed) => {
                    noted.store(needed as u64, Ordering::Relaxed);
                    Ok(std::ops::ControlFlow::Continue(needed))
                }
                Inspected::Ready(expected) => {
                    noted.store(expected as u64, Ordering::Relaxed);
                    take_message(buf, expected).map(std::ops::ControlFlow::Break)
                }
                Inspected::Wire(err, decision) => {
                    noted.store(decision as u64, Ordering::Relaxed);
                    Err(Error::IncomingWire(err))
                }
                Inspected::Protocol(err) => Err(err),
            })
            .await;

        let decision = noted.load(Ordering::Relaxed);
        if decision > 0 {
            self.note_logical_need(decision);
        }

        match result {
            Ok(message) => {
                self.note_message_accepted(message.contents.len());
                Ok(message)
            }
            Err(err) => {
                if self.fault.is_none() {
                    if let Some(wire) = err.as_incoming_wire() {
                        self.fault = Some(wire);
                    }
                }
                Err(err)
            }
        }
    }

    // Get the next message from the server
    // May wait for more data from the server
    pub(crate) async fn recv(&mut self) -> Result<ReceivedMessage, Error> {
        loop {
            let message = self.recv_unchecked().await?;

            match message.format {
                BackendMessageFormat::ErrorResponse => {
                    // An error returned from the database server.
                    return Err(message.decode::<PgDatabaseError>()?.into());
                }

                BackendMessageFormat::NotificationResponse => {
                    if let Some(buffer) = &mut self.notifications {
                        let notification: Notification = message.decode()?;
                        let _ = buffer.send(notification).await;

                        continue;
                    }
                }

                BackendMessageFormat::ParameterStatus => {
                    // informs the frontend about the current (initial)
                    // setting of backend parameters

                    let ParameterStatus { name, value } = message.decode()?;
                    // TODO: handle `client_encoding`, `DateStyle` change

                    match name.as_str() {
                        "server_version" => {
                            self.server_version_num = parse_server_version(&value);
                        }
                        _ => {
                            self.parameter_statuses.insert(name, value);
                        }
                    }

                    continue;
                }

                BackendMessageFormat::NoticeResponse => {
                    // do we need this to be more configurable?
                    // if you are reading this comment and think so, open an issue

                    let notice: Notice = message.decode()?;

                    let (log_level, tracing_level) = match notice.severity() {
                        PgSeverity::Fatal | PgSeverity::Panic | PgSeverity::Error => {
                            (Level::Error, tracing::Level::ERROR)
                        }
                        PgSeverity::Warning => (Level::Warn, tracing::Level::WARN),
                        PgSeverity::Notice => (Level::Info, tracing::Level::INFO),
                        PgSeverity::Debug => (Level::Debug, tracing::Level::DEBUG),
                        PgSeverity::Info | PgSeverity::Log => (Level::Trace, tracing::Level::TRACE),
                    };

                    let log_is_enabled = log::log_enabled!(
                        target: "sqlx::postgres::notice",
                        log_level
                    ) || sqlx_core::private_tracing_dynamic_enabled!(
                        target: "sqlx::postgres::notice",
                        tracing_level
                    );
                    if log_is_enabled {
                        sqlx_core::private_tracing_dynamic_event!(
                            target: "sqlx::postgres::notice",
                            tracing_level,
                            message = notice.message()
                        );
                    }

                    continue;
                }

                _ => {}
            }

            return Ok(message);
        }
    }
}

impl Deref for PgStream {
    type Target = BufferedSocket<Box<dyn Socket>>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for PgStream {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

fn inspect(buf: &[u8], bounds: Option<PgIncomingBounds>) -> Inspected {
    if buf.len() < 5 {
        return Inspected::Need(5);
    }

    let format = buf[0];
    let message_len = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
    if message_len < 4 {
        return Inspected::Wire(IncomingWireError::ShortLength, 5);
    }

    // The type byte plus the length field must have a finite end. On a 64-bit
    // usize, `u32::MAX + 1` does not overflow; the `u32` check is the one that fires.
    let Some(frame_u32) = message_len.checked_add(1) else {
        return Inspected::Wire(IncomingWireError::Overflow, 5);
    };
    let Some(expected) = (message_len as usize).checked_add(1) else {
        return Inspected::Wire(IncomingWireError::Overflow, 5);
    };
    if expected != frame_u32 as usize {
        return Inspected::Wire(IncomingWireError::Overflow, 5);
    }

    if let Some(bounds) = bounds {
        if message_len > bounds.max_message_len {
            return Inspected::Wire(
                IncomingWireError::Limit(ReceiveLimitKind::Message),
                5,
            );
        }
    }

    let format_kind = match BackendMessageFormat::try_from_u8(format) {
        Ok(kind) => kind,
        Err(err) => {
            return if bounds.is_some() {
                Inspected::Wire(IncomingWireError::BadType, 5)
            } else {
                Inspected::Protocol(err)
            };
        }
    };

    if bounds.is_none() || !matches!(format_kind, BackendMessageFormat::DataRow) {
        if buf.len() < expected {
            return Inspected::Need(expected);
        }
        return Inspected::Ready(expected);
    }

    inspect_data_row(buf, expected, bounds.expect("bounds checked above"))
}

fn inspect_data_row(buf: &[u8], expected: usize, bounds: PgIncomingBounds) -> Inspected {
    if expected < 7 {
        return Inspected::Wire(IncomingWireError::Truncated, 5);
    }
    if buf.len() < 7 {
        return Inspected::Need(7);
    }

    let natts = u16::from_be_bytes([buf[5], buf[6]]);
    if natts > bounds.max_columns {
        return Inspected::Wire(IncomingWireError::Limit(ReceiveLimitKind::Columns), 7);
    }

    let Some(prefix) = (natts as u32).checked_mul(4).and_then(|words| words.checked_add(2)) else {
        return Inspected::Wire(IncomingWireError::Overflow, 7);
    };
    if prefix > bounds.max_row_payload {
        return Inspected::Wire(IncomingWireError::Limit(ReceiveLimitKind::Row), 7);
    }
    let Some(prefixes_end) = 5usize.checked_add(prefix as usize) else {
        return Inspected::Wire(IncomingWireError::Overflow, 7);
    };
    if prefixes_end > expected {
        return Inspected::Wire(IncomingWireError::Truncated, 7);
    }

    let mut cursor = 7usize;
    let mut row_sum = prefix;
    for _ in 0..natts {
        let Some(prefix_end) = cursor.checked_add(4) else {
            return Inspected::Wire(IncomingWireError::Overflow, cursor);
        };
        if prefix_end > expected {
            return Inspected::Wire(IncomingWireError::Truncated, cursor);
        }
        if buf.len() < prefix_end {
            return Inspected::Need(prefix_end);
        }

        let flen = i32::from_be_bytes([
            buf[cursor],
            buf[cursor + 1],
            buf[cursor + 2],
            buf[cursor + 3],
        ]);
        match u32::try_from(flen) {
            Err(_) if flen == -1 => {
                cursor = prefix_end;
            }
            Err(_) => {
                return Inspected::Wire(IncomingWireError::NegativeLength, prefix_end);
            }
            Ok(flen_u) => {
                if flen_u > bounds.max_value_len {
                    return Inspected::Wire(
                        IncomingWireError::Limit(ReceiveLimitKind::Value),
                        prefix_end,
                    );
                }
                let Some(next_sum) = row_sum.checked_add(flen_u) else {
                    return Inspected::Wire(IncomingWireError::Overflow, prefix_end);
                };
                if next_sum > bounds.max_row_payload {
                    return Inspected::Wire(
                        IncomingWireError::Limit(ReceiveLimitKind::Row),
                        prefix_end,
                    );
                }
                let Ok(flen_usize) = usize::try_from(flen_u) else {
                    return Inspected::Wire(IncomingWireError::Overflow, prefix_end);
                };
                let Some(body_end) = prefix_end.checked_add(flen_usize) else {
                    return Inspected::Wire(IncomingWireError::Overflow, prefix_end);
                };
                if body_end > expected {
                    return Inspected::Wire(IncomingWireError::Truncated, prefix_end);
                }
                if buf.len() < body_end {
                    return Inspected::Need(body_end);
                }
                row_sum = next_sum;
                cursor = body_end;
            }
        }
    }

    if cursor != expected {
        return Inspected::Wire(IncomingWireError::Remaining, cursor);
    }
    if buf.len() < expected {
        return Inspected::Need(expected);
    }
    Inspected::Ready(expected)
}

fn take_message(buf: &mut BytesMut, expected: usize) -> Result<ReceivedMessage, Error> {
    if expected < 5 || buf.len() < expected {
        return Err(err_protocol!(
            "buffered frame shorter than the inspected length"
        ));
    }
    let format = BackendMessageFormat::try_from_u8(buf[0])?;
    let message_len = expected - 1;
    buf.advance(1);
    let mut contents = buf.split_to(message_len).freeze();
    contents.advance(4);
    Ok(ReceivedMessage { format, contents })
}

// reference:
// https://github.com/postgres/postgres/blob/6feebcb6b44631c3dc435e971bd80c2dd218a5ab/src/interfaces/libpq/fe-exec.c#L1030-L1065
fn parse_server_version(s: &str) -> Option<u32> {
    let mut parts = Vec::<u32>::with_capacity(3);

    let mut from = 0;
    let mut chs = s.char_indices().peekable();
    while let Some((i, ch)) = chs.next() {
        match ch {
            '.' => {
                if let Ok(num) = u32::from_str(&s[from..i]) {
                    parts.push(num);
                    from = i + 1;
                } else {
                    break;
                }
            }
            _ if ch.is_ascii_digit() => {
                if chs.peek().is_none() {
                    if let Ok(num) = u32::from_str(&s[from..]) {
                        parts.push(num);
                    }
                    break;
                }
            }
            _ => {
                if let Ok(num) = u32::from_str(&s[from..i]) {
                    parts.push(num);
                }
                break;
            }
        };
    }

    let version_num = match parts.as_slice() {
        [major, minor, rev] => (100 * major + minor) * 100 + rev,
        [major, minor] if *major >= 10 => 100 * 100 * major + minor,
        [major, minor] => (100 * major + minor) * 100,
        [major] => 100 * 100 * major,
        _ => return None,
    };

    Some(version_num)
}

#[cfg(test)]
mod tests {
    use super::parse_server_version;

    #[test]
    fn test_parse_server_version_num() {
        // old style
        assert_eq!(parse_server_version("9.6.1"), Some(90601));
        // new style
        assert_eq!(parse_server_version("10.1"), Some(100001));
        // old style without minor version
        assert_eq!(parse_server_version("9.6devel"), Some(90600));
        // new style without minor version, e.g.  */
        assert_eq!(parse_server_version("10devel"), Some(100000));
        assert_eq!(parse_server_version("13devel87"), Some(130000));
        // unknown
        assert_eq!(parse_server_version("unknown"), None);
    }
}
