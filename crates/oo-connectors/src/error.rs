//! Closed source errors. Display and Debug carry a kind, a static fact, and a
//! SQLSTATE. They never carry driver text, DSNs, or row contents.

use std::fmt;

/// Classification of a source failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceErrorKind {
    /// The caller config is rejected before a socket is opened.
    Config,
    /// The TCP or protocol session could not be established.
    Connect,
    /// The server rejected the credentials.
    Auth,
    /// TLS trust or hostname verification failed.
    Tls,
    /// A catalog privilege probe could not be completed.
    Privilege,
    /// The server version is missing, below the floor, or untested.
    Version,
    /// A time, row, or byte budget was exceeded.
    Budget,
    /// The catalog snapshot could not be built.
    Catalog,
    /// Two source names map to one stable identifier.
    Identifier,
    /// The driver returned a condition this slice does not describe further.
    Protocol,
}

/// Five-character SQLSTATE. Invalid codes are dropped, not echoed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SqlState([u8; 5]);

impl SqlState {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let bytes = raw.as_bytes();
        if bytes.len() != 5 || !bytes.iter().all(|b| b.is_ascii_alphanumeric()) {
            return None;
        }
        let mut code = [0; 5];
        code.copy_from_slice(bytes);
        Some(Self(code))
    }

    /// The five ASCII characters.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("?????")
    }
}

impl fmt::Debug for SqlState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for SqlState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Failure returned by connect, inspect_access, and catalog.
#[derive(Clone, PartialEq, Eq)]
pub struct SourceError {
    kind: SourceErrorKind,
    fact: Option<&'static str>,
    sqlstate: Option<SqlState>,
}

impl SourceError {
    pub(crate) fn new(
        kind: SourceErrorKind,
        fact: Option<&'static str>,
        sqlstate: Option<SqlState>,
    ) -> Self {
        Self {
            kind,
            fact,
            sqlstate,
        }
    }

    pub(crate) fn config(fact: &'static str) -> Self {
        Self::new(SourceErrorKind::Config, Some(fact), None)
    }

    pub(crate) fn connect(fact: &'static str) -> Self {
        Self::new(SourceErrorKind::Connect, Some(fact), None)
    }

    pub(crate) fn tls(fact: &'static str) -> Self {
        Self::new(SourceErrorKind::Tls, Some(fact), None)
    }

    pub(crate) fn budget(fact: &'static str) -> Self {
        Self::new(SourceErrorKind::Budget, Some(fact), None)
    }

    pub(crate) fn catalog(fact: &'static str) -> Self {
        Self::new(SourceErrorKind::Catalog, Some(fact), None)
    }

    pub(crate) fn identifier(fact: &'static str) -> Self {
        Self::new(SourceErrorKind::Identifier, Some(fact), None)
    }

    pub(crate) fn version(fact: &'static str) -> Self {
        Self::new(SourceErrorKind::Version, Some(fact), None)
    }

    pub(crate) fn protocol(fact: &'static str) -> Self {
        Self::new(SourceErrorKind::Protocol, Some(fact), None)
    }

    /// Error class.
    pub fn kind(&self) -> SourceErrorKind {
        self.kind
    }

    /// Static fact code, when one applies.
    pub fn fact(&self) -> Option<&'static str> {
        self.fact
    }

    /// SQLSTATE, when the server supplied a five-character code.
    pub fn sqlstate(&self) -> Option<&str> {
        self.sqlstate.as_ref().map(SqlState::as_str)
    }
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "source error: {}", kind_name(self.kind))?;
        if let Some(fact) = self.fact {
            write!(f, " fact={fact}")?;
        }
        if let Some(state) = &self.sqlstate {
            write!(f, " sqlstate={state}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceError")
            .field("kind", &self.kind)
            .field("fact", &self.fact)
            .field("sqlstate", &self.sqlstate)
            .finish()
    }
}

impl std::error::Error for SourceError {}

pub(crate) fn kind_name(kind: SourceErrorKind) -> &'static str {
    match kind {
        SourceErrorKind::Config => "config",
        SourceErrorKind::Connect => "connect",
        SourceErrorKind::Auth => "auth",
        SourceErrorKind::Tls => "tls",
        SourceErrorKind::Privilege => "privilege",
        SourceErrorKind::Version => "version",
        SourceErrorKind::Budget => "budget",
        SourceErrorKind::Catalog => "catalog",
        SourceErrorKind::Identifier => "identifier",
        SourceErrorKind::Protocol => "protocol",
    }
}

/// Map a driver error by variant and SQLSTATE. The driver message is dropped.
pub(crate) fn map_sqlx(err: sqlx::Error) -> SourceError {
    map_driver(err, false)
}

/// Connect-time mapping. rustls reports certificate and hostname failures as
/// `io::ErrorKind::InvalidData`, which SQLx then wraps as [`sqlx::Error::Io`].
pub(crate) fn map_connect(err: sqlx::Error) -> SourceError {
    map_driver(err, true)
}

fn map_driver(err: sqlx::Error, connect: bool) -> SourceError {
    if let Some(db) = err.as_database_error() {
        let sqlstate = db.code().as_deref().and_then(SqlState::parse);
        let (kind, fact) = match sqlstate.as_ref().map(SqlState::as_str) {
            Some("28P01" | "28000") => (SourceErrorKind::Auth, Some("authentication_failed")),
            Some("42501") => (SourceErrorKind::Privilege, Some("insufficient_privilege")),
            Some("3D000") => (SourceErrorKind::Connect, Some("database")),
            Some(state) if state.starts_with("08") => {
                (SourceErrorKind::Connect, Some("connection"))
            }
            Some("57014") => (SourceErrorKind::Budget, Some("statement_timeout")),
            Some("55P03") => (SourceErrorKind::Budget, Some("lock_timeout")),
            _ => (SourceErrorKind::Protocol, Some("database")),
        };
        return SourceError::new(kind, fact, sqlstate);
    }
    match err {
        sqlx::Error::Tls(_) => SourceError::tls("tls_handshake"),
        sqlx::Error::Io(err) if connect && io_is_tls(&err) => SourceError::tls("tls_handshake"),
        sqlx::Error::Io(_) => SourceError::connect("io"),
        sqlx::Error::PoolTimedOut => SourceError::budget("connect_timeout"),
        sqlx::Error::RowNotFound => SourceError::catalog("row_not_found"),
        sqlx::Error::Configuration(_) => SourceError::config("driver_configuration"),
        sqlx::Error::Protocol(_) => SourceError::protocol("protocol"),
        _ => SourceError::protocol("driver"),
    }
}

fn io_is_tls(err: &std::io::Error) -> bool {
    if err.kind() == std::io::ErrorKind::InvalidData {
        return true;
    }
    let mut current = err.get_ref().map(|item| item as &dyn std::error::Error);
    while let Some(item) = current {
        if std::any::type_name_of_val(item).contains("rustls") {
            return true;
        }
        current = item.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_closed_static_text() {
        let err = SourceError::new(
            SourceErrorKind::Auth,
            Some("authentication_failed"),
            SqlState::parse("28P01"),
        );
        let text = format!("{err} {err:?}");
        assert_eq!(
            err.to_string(),
            "source error: auth fact=authentication_failed sqlstate=28P01"
        );
        assert!(!text.contains("secret"));
        assert!(!text.contains("password"));
        assert!(SqlState::parse("28P0").is_none());
        assert!(SqlState::parse("28P0!").is_none());
    }

    #[test]
    fn connect_io_invalid_data_is_tls_and_drops_the_driver_text() {
        let marker = "closed-driver-text";
        let io = std::io::Error::new(std::io::ErrorKind::InvalidData, marker);
        let mapped = map_connect(sqlx::Error::Io(io));
        assert_eq!(mapped.kind(), SourceErrorKind::Tls);
        assert!(!format!("{mapped} {mapped:?}").contains(marker));
        let refused = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, marker);
        let mapped = map_connect(sqlx::Error::Io(refused));
        assert_eq!(mapped.kind(), SourceErrorKind::Connect);
        assert!(!format!("{mapped} {mapped:?}").contains(marker));
    }
}
