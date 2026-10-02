//! Caller-supplied source configuration. The secret value is zeroized and is
//! not part of Debug. The secret reference is not the value.

use std::fmt;
use std::time::Duration;

use zeroize::Zeroizing;

use crate::error::SourceError;

const MAX_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_ROWS: u32 = 100_000;
const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// Trusted source name. Lowercase, and stable in audit records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceId(String);

impl SourceId {
    /// Accept `^[a-z][a-z0-9_]{0,62}$`.
    pub fn new(raw: impl AsRef<str>) -> Result<Self, SourceError> {
        let raw = raw.as_ref();
        let mut chars = raw.chars();
        let Some(first) = chars.next() else {
            return Err(SourceError::config("source_id"));
        };
        if !first.is_ascii_lowercase()
            || !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            || raw.len() > 63
        {
            return Err(SourceError::config("source_id"));
        }
        Ok(Self(raw.to_string()))
    }

    /// The validated identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `secret://` reference. This is the name of the secret, not its value.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretRef(String);

impl SecretRef {
    /// Accept a short `secret://` reference without whitespace.
    pub fn new(raw: impl AsRef<str>) -> Result<Self, SourceError> {
        let raw = raw.as_ref();
        let Some(rest) = raw.strip_prefix("secret://") else {
            return Err(SourceError::config("secret_ref"));
        };
        if rest.is_empty()
            || rest.len() > 200
            || !rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
        {
            return Err(SourceError::config("secret_ref"));
        }
        Ok(Self(raw.to_string()))
    }

    /// The reference string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// TCP endpoint. Unix sockets and DSNs are rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PgEndpoint {
    host: String,
    port: u16,
    database: String,
}

impl PgEndpoint {
    /// Validate host, port, and database name before a connection is built.
    pub fn new(
        host: impl AsRef<str>,
        port: u16,
        database: impl AsRef<str>,
    ) -> Result<Self, SourceError> {
        let host = host.as_ref();
        let database = database.as_ref();
        if !valid_host(host) || port == 0 || !valid_simple_name(database) {
            return Err(SourceError::config("endpoint"));
        }
        Ok(Self {
            host: host.to_string(),
            port,
            database: database.to_string(),
        })
    }

    /// DNS name passed to hostname verification.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// TCP port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Database name.
    pub fn database(&self) -> &str {
        &self.database
    }
}

/// Username plus a zeroizing password. There is no password getter.
#[derive(Clone)]
pub struct PgCredentials {
    username: String,
    password: Zeroizing<String>,
}

impl PgCredentials {
    /// Reject an empty password. The caller resolves `secret://` before this.
    pub fn new(
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Result<Self, SourceError> {
        let username = username.into();
        let password = password.into();
        if !valid_simple_name(&username)
            || password.is_empty()
            || password.contains('\0')
            || password.len() > 1024
        {
            return Err(SourceError::config("credentials"));
        }
        Ok(Self {
            username,
            password: Zeroizing::new(password),
        })
    }

    /// Login name. This is not the password.
    pub fn username(&self) -> &str {
        &self.username
    }

    pub(crate) fn password(&self) -> &str {
        &self.password
    }
}

impl fmt::Debug for PgCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgCredentials")
            .field("username", &self.username)
            .field("password", &"[redacted]")
            .finish()
    }
}

/// PEM certificate authority material supplied by the caller.
#[derive(Clone)]
pub struct PgTlsConfig {
    ca_pem: Zeroizing<Vec<u8>>,
}

impl PgTlsConfig {
    /// Require a certificate bundle. A private key is not a trust anchor.
    pub fn from_ca_pem(pem: impl AsRef<[u8]>) -> Result<Self, SourceError> {
        let pem = pem.as_ref();
        if pem.is_empty() || pem.len() > 1024 * 1024 {
            return Err(SourceError::config("ca_pem"));
        }
        let text = String::from_utf8_lossy(pem);
        if text.contains("PRIVATE KEY")
            || !text.contains("-----BEGIN CERTIFICATE-----")
            || !text.contains("-----END CERTIFICATE-----")
        {
            return Err(SourceError::config("ca_pem"));
        }
        Ok(Self {
            ca_pem: Zeroizing::new(pem.to_vec()),
        })
    }

    pub(crate) fn ca_pem(&self) -> &[u8] {
        &self.ca_pem
    }
}

impl fmt::Debug for PgTlsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgTlsConfig")
            .field("mode", &"verify-full")
            .field("ca_pem", &"[redacted]")
            .finish()
    }
}

/// Time and size budgets for one inspection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PgBudget {
    connect_timeout: Duration,
    statement_timeout: Duration,
    lock_timeout: Duration,
    max_rows: u32,
    max_bytes: u64,
}

impl PgBudget {
    /// Timeouts must be at most 120 seconds. Row and byte caps are fixed.
    pub fn new(
        connect_timeout: Duration,
        statement_timeout: Duration,
        lock_timeout: Duration,
        max_rows: u32,
        max_bytes: u64,
    ) -> Result<Self, SourceError> {
        if !valid_timeout(connect_timeout)
            || !valid_timeout(statement_timeout)
            || !valid_timeout(lock_timeout)
            || max_rows == 0
            || max_rows > MAX_ROWS
            || max_bytes == 0
            || max_bytes > MAX_BYTES
        {
            return Err(SourceError::config("budget"));
        }
        Ok(Self {
            connect_timeout,
            statement_timeout,
            lock_timeout,
            max_rows,
            max_bytes,
        })
    }

    /// Client timeout around the driver connect future.
    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    /// Server `statement_timeout` and the client timeout around each query.
    pub fn statement_timeout(&self) -> Duration {
        self.statement_timeout
    }

    /// Server `lock_timeout`.
    pub fn lock_timeout(&self) -> Duration {
        self.lock_timeout
    }

    /// Maximum catalog rows accepted from one result set.
    pub fn max_rows(&self) -> u32 {
        self.max_rows
    }

    /// Maximum UTF-8 bytes retained from names and evidence hashes.
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }
}

/// One selected table. Names are catalog text, not SQL fragments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedTable {
    schema: String,
    table: String,
}

impl SelectedTable {
    /// Reject NUL and names longer than 63 bytes.
    pub fn new(schema: impl Into<String>, table: impl Into<String>) -> Result<Self, SourceError> {
        let schema = schema.into();
        let table = table.into();
        if !valid_catalog_name(&schema) || !valid_catalog_name(&table) {
            return Err(SourceError::config("selection"));
        }
        Ok(Self { schema, table })
    }

    /// Schema name as stored in `pg_namespace.nspname`.
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// Table name as stored in `pg_class.relname`.
    pub fn table(&self) -> &str {
        &self.table
    }
}

/// Tables the snapshot is allowed to describe. Empty is a legal no-data scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableSelection {
    tables: Vec<SelectedTable>,
}

impl TableSelection {
    /// Reject duplicate `(schema, table)` pairs.
    pub fn new(tables: Vec<SelectedTable>) -> Result<Self, SourceError> {
        let mut seen = Vec::<(String, String)>::new();
        for table in &tables {
            let key = (table.schema.clone(), table.table.clone());
            if seen.iter().any(|item| item == &key) {
                return Err(SourceError::config("selection_duplicate"));
            }
            seen.push(key);
        }
        Ok(Self { tables })
    }

    /// Selected tables in caller order.
    pub fn tables(&self) -> &[SelectedTable] {
        &self.tables
    }
}

/// Complete input to [`crate::PgConnector::new`].
#[derive(Clone)]
pub struct PgSourceConfig {
    source_id: SourceId,
    endpoint: PgEndpoint,
    credentials: PgCredentials,
    secret_ref: SecretRef,
    tls: PgTlsConfig,
    budget: PgBudget,
    selection: TableSelection,
}

impl PgSourceConfig {
    /// Validate the selection against the row budget.
    pub fn try_new(
        source_id: SourceId,
        endpoint: PgEndpoint,
        credentials: PgCredentials,
        secret_ref: SecretRef,
        tls: PgTlsConfig,
        budget: PgBudget,
        selection: TableSelection,
    ) -> Result<Self, SourceError> {
        if selection.tables.len() > usize::try_from(budget.max_rows).unwrap_or(usize::MAX) {
            return Err(SourceError::config("selection_budget"));
        }
        Ok(Self {
            source_id,
            endpoint,
            credentials,
            secret_ref,
            tls,
            budget,
            selection,
        })
    }

    /// Trusted source id.
    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    /// Endpoint the caller already chose.
    pub fn endpoint(&self) -> &PgEndpoint {
        &self.endpoint
    }

    /// Resolved credentials. The password is not returned by a getter.
    pub fn credentials(&self) -> &PgCredentials {
        &self.credentials
    }

    /// Secret reference recorded on the access report.
    pub fn secret_ref(&self) -> &SecretRef {
        &self.secret_ref
    }

    /// Caller CA bundle.
    pub fn tls(&self) -> &PgTlsConfig {
        &self.tls
    }

    /// Budgets applied to the session.
    pub fn budget(&self) -> PgBudget {
        self.budget
    }

    /// Selected tables.
    pub fn selection(&self) -> &TableSelection {
        &self.selection
    }
}

impl fmt::Debug for PgSourceConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgSourceConfig")
            .field("source_id", &self.source_id)
            .field("secret_ref", &self.secret_ref)
            .field("database", &self.endpoint.database)
            .field("credentials", &self.credentials)
            .field("tls", &self.tls)
            .field("budget", &self.budget)
            .finish()
    }
}

fn valid_timeout(value: Duration) -> bool {
    !value.is_zero() && value <= MAX_TIMEOUT
}

fn valid_host(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 || host.starts_with('/') || host.starts_with('.') {
        return false;
    }
    if host.contains("://")
        || host.contains('@')
        || host.contains('/')
        || host.contains('?')
        || host.contains('\\')
        || host.contains('#')
        || host.contains(':')
        || host.chars().any(|c| c.is_whitespace() || c == '\0')
    {
        return false;
    }
    host.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

fn valid_simple_name(name: &str) -> bool {
    (1..=63).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn valid_catalog_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 63 && !name.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pem() -> &'static [u8] {
        b"-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n"
    }

    #[test]
    fn rejects_dsn_socket_empty_password_and_prints_no_secret() {
        assert!(PgEndpoint::new("/tmp/.s.PGSQL.5432", 5432, "db").is_err());
        assert!(PgEndpoint::new("postgres://db.example/app", 5432, "db").is_err());
        assert!(PgEndpoint::new("db.example", 5432, "app").is_ok());
        assert!(PgCredentials::new("reader", "").is_err());
        let creds = PgCredentials::new("reader", "super-secret-value").unwrap();
        let rendered = format!("{creds:?}");
        assert!(!rendered.contains("super-secret-value"));
        assert!(rendered.contains("[redacted]"));
        assert!(PgTlsConfig::from_ca_pem(b"-----BEGIN PRIVATE KEY-----\n").is_err());
        assert!(PgTlsConfig::from_ca_pem(pem()).is_ok());
        assert!(SourceId::new("Source").is_err());
        assert!(SecretRef::new("env:password").is_err());
        let budget = PgBudget::new(
            Duration::from_secs(121),
            Duration::from_secs(1),
            Duration::from_secs(1),
            10,
            10,
        );
        assert!(budget.is_err());
    }
}
