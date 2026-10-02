//! One read-only PostgreSQL session. The connection is not pooled and is not
//! printed: the driver options retain the password.

use std::fmt;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Row};

use crate::config::{PgSourceConfig, TableSelection};
use crate::error::{SourceError, map_connect, map_sqlx};
use crate::model::{
    AccessReport, BlockReason, CdcPrerequisite, DbaItem, Fact, VersionFact, WriteDetermination,
    admit_version, connector_capabilities, tls_report,
};
use crate::sql;

/// Configured connector. `connect` opens a fresh session every time.
pub struct PgConnector {
    config: PgSourceConfig,
}

impl PgConnector {
    /// Store a validated configuration.
    pub fn new(config: PgSourceConfig) -> Self {
        Self { config }
    }

    /// Open one TLS session and read the server version.
    ///
    /// An untested version still returns a session. `inspect_access` then
    /// blocks registration. A failed version query fails closed.
    pub async fn connect(&self) -> Result<PgSession, SourceError> {
        let options = PgConnectOptions::from_explicit()
            .host(self.config.endpoint().host())
            .port(self.config.endpoint().port())
            .username(self.config.credentials().username())
            .password(self.config.credentials().password())
            .database(self.config.endpoint().database())
            .application_name(sql::APPLICATION_NAME)
            .extra_float_digits(2i8)
            .ssl_mode(PgSslMode::VerifyFull)
            .ssl_root_cert_from_pem(self.config.tls().ca_pem().to_vec());
        if options.get_options().is_some() {
            return Err(SourceError::config("startup_options"));
        }
        let options = options.disable_statement_logging();
        let connect_timeout = self.config.budget().connect_timeout();
        let connect = options.connect();
        let conn = match tokio::time::timeout(connect_timeout, connect).await {
            Ok(Ok(conn)) => conn,
            Ok(Err(err)) => return Err(map_connect(err)),
            Err(_) => return Err(SourceError::budget("connect_timeout")),
        };
        let mut session = PgSession {
            conn,
            source_id: self.config.source_id().as_str().to_string(),
            secret_ref: self.config.secret_ref().as_str().to_string(),
            selection: self.config.selection().clone(),
            budget: self.config.budget(),
            version_num: 0,
            version_raw: String::new(),
            major: 0,
            minor: 0,
            admission: "unknown",
            poisoned: false,
            in_transaction: false,
            identity_aligned: false,
            startup_clean: false,
            negotiated_tls: String::new(),
            session_user: String::new(),
            current_user: String::new(),
            database_name: String::new(),
        };
        session.read_identity().await?;
        session.apply_guards().await?;
        session.read_version().await?;
        Ok(session)
    }
}

impl fmt::Debug for PgConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgConnector")
            .field("source_id", self.config.source_id())
            .field("secret_ref", self.config.secret_ref())
            .finish()
    }
}

/// A connected source. Dropping it closes the socket. There is no `COMMIT`.
pub struct PgSession {
    pub(crate) conn: PgConnection,
    pub(crate) source_id: String,
    pub(crate) secret_ref: String,
    pub(crate) selection: TableSelection,
    pub(crate) budget: crate::config::PgBudget,
    pub(crate) version_num: i64,
    pub(crate) version_raw: String,
    pub(crate) major: i64,
    pub(crate) minor: i64,
    pub(crate) admission: &'static str,
    pub(crate) poisoned: bool,
    pub(crate) in_transaction: bool,
    pub(crate) identity_aligned: bool,
    pub(crate) startup_clean: bool,
    pub(crate) negotiated_tls: String,
    pub(crate) session_user: String,
    pub(crate) current_user: String,
    pub(crate) database_name: String,
}

impl fmt::Debug for PgSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgSession")
            .field("source_id", &self.source_id)
            .field("secret_ref", &self.secret_ref)
            .field("admission", &self.admission)
            .field("connected", &true)
            .finish()
    }
}

impl PgSession {
    /// Read-only access inspection.
    pub async fn inspect_access(&mut self) -> Result<AccessReport, SourceError> {
        self.ensure_open()?;
        if !self.identity_aligned || !self.startup_clean {
            return Ok(self.session_block_report());
        }
        if self.admission != "tested" {
            return Ok(self.version_block_report());
        }
        self.begin().await?;
        let result = self.collect_access().await;
        self.finish(result).await
    }

    /// Authoritative snapshot of the selected tables.
    pub async fn catalog(&mut self) -> Result<crate::model::SourceCatalog, SourceError> {
        self.ensure_open()?;
        if !self.identity_aligned {
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
        self.begin().await?;
        let result = self.collect_catalog().await;
        self.finish(result).await
    }

    pub(crate) fn ensure_open(&self) -> Result<(), SourceError> {
        if self.poisoned {
            Err(SourceError::connect("session_poisoned"))
        } else {
            Ok(())
        }
    }

    async fn read_identity(&mut self) -> Result<(), SourceError> {
        let allow = [
            "DateStyle",
            "client_encoding",
            "TimeZone",
            "extra_float_digits",
            "application_name",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        let budget = self.budget.statement_timeout();
        let result = tokio::time::timeout(
            budget,
            sqlx::query(sql::IDENTITY)
                .bind(&allow)
                .fetch_one(&mut self.conn),
        )
        .await;
        let row = match result {
            Ok(Ok(row)) => row,
            Ok(Err(err)) => return Err(map_sqlx(err)),
            Err(_) => {
                self.poisoned = true;
                return Err(SourceError::budget("statement_timeout"));
            }
        };
        let session_user = cell_string(&row, 0)?;
        let current_user = cell_string(&row, 1)?;
        let ssl = cell_opt_bool(&row, 2)?;
        let version = cell_opt_string(&row, 3)?;
        let extra = cell_string(&row, 4)?;
        let database = cell_string(&row, 5)?;
        let negotiated = match classify_server_tls(ssl, version)? {
            ServerTls::Observed(version) => version,
            ServerTls::Unknown => "unknown".to_string(),
        };
        if session_user.is_empty()
            || current_user.is_empty()
            || session_user.len() > 256
            || current_user.len() > 256
            || database.is_empty()
        {
            return Err(SourceError::catalog("identity"));
        }
        self.identity_aligned = session_user == current_user;
        self.startup_clean = extra.is_empty();
        self.negotiated_tls = negotiated;
        self.session_user = session_user;
        self.current_user = current_user;
        self.database_name = database;
        Ok(())
    }

    pub(crate) fn tls(&self) -> crate::model::TlsReport {
        tls_report(&self.negotiated_tls)
    }

    async fn apply_guards(&mut self) -> Result<(), SourceError> {
        let statement_ms = duration_ms(self.budget.statement_timeout())?;
        let lock_ms = duration_ms(self.budget.lock_timeout())?;
        let budget = self.budget.statement_timeout();
        let result = tokio::time::timeout(
            budget,
            sqlx::query(sql::SESSION_GUARDS)
                .bind(statement_ms)
                .bind(lock_ms)
                .execute(&mut self.conn),
        )
        .await;
        match result {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(err)) => Err(map_sqlx(err)),
            Err(_) => {
                self.poisoned = true;
                Err(SourceError::budget("statement_timeout"))
            }
        }
    }

    async fn read_version(&mut self) -> Result<(), SourceError> {
        let budget = self.budget.statement_timeout();
        let result =
            tokio::time::timeout(budget, sqlx::query(sql::VERSION).fetch_one(&mut self.conn)).await;
        let row = match result {
            Ok(Ok(row)) => row,
            Ok(Err(err)) => return Err(map_sqlx(err)),
            Err(_) => {
                self.poisoned = true;
                return Err(SourceError::budget("statement_timeout"));
            }
        };
        let num: i64 = row
            .try_get(0)
            .map_err(|_| SourceError::version("version_unknown"))?;
        let raw: String = row
            .try_get(1)
            .map_err(|_| SourceError::version("version_unknown"))?;
        if num <= 0 || raw.is_empty() || raw.len() > 128 || raw.contains('\0') {
            return Err(SourceError::version("version_unknown"));
        }
        self.version_num = num;
        self.major = num / 10_000;
        self.minor = num % 10_000;
        self.version_raw = raw;
        self.admission = admit_version(self.major, self.minor, &self.version_raw);
        Ok(())
    }

    pub(crate) async fn begin(&mut self) -> Result<(), SourceError> {
        self.ensure_open()?;
        if self.in_transaction {
            return Err(SourceError::protocol("nested_transaction"));
        }
        let budget = self.budget.statement_timeout();
        let result =
            tokio::time::timeout(budget, sqlx::query(sql::BEGIN).execute(&mut self.conn)).await;
        match result {
            Ok(Ok(_)) => {
                self.in_transaction = true;
                Ok(())
            }
            Ok(Err(err)) => Err(map_sqlx(err)),
            Err(_) => {
                self.poisoned = true;
                Err(SourceError::budget("statement_timeout"))
            }
        }
    }

    pub(crate) async fn finish<T>(&mut self, result: Result<T, SourceError>) -> Result<T, SourceError> {
        let rolled = self.rollback().await;
        match (result, rolled) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(err), _) => Err(err),
            (Ok(_), Err(err)) => Err(err),
        }
    }

    async fn rollback(&mut self) -> Result<(), SourceError> {
        if !self.in_transaction {
            return Ok(());
        }
        let budget = self.budget.statement_timeout();
        let result =
            tokio::time::timeout(budget, sqlx::query(sql::ROLLBACK).execute(&mut self.conn)).await;
        self.in_transaction = false;
        match result {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(err)) => {
                self.poisoned = true;
                Err(map_sqlx(err))
            }
            Err(_) => {
                self.poisoned = true;
                Err(SourceError::budget("statement_timeout"))
            }
        }
    }

    pub(crate) fn version_fact(&self) -> VersionFact {
        VersionFact {
            server_version_num: self.version_num,
            server_version: self.version_raw.clone(),
            major: self.major,
            minor: self.minor,
            declared_minimum_major: crate::DECLARED_MIN_MAJOR,
            tested_versions: crate::TESTED_VERSIONS
                .iter()
                .map(|item| (*item).to_string())
                .collect(),
            admission: self.admission.to_string(),
        }
    }

    fn version_block_report(&self) -> AccessReport {
        let code = match self.admission {
            "below_minimum" => "version_below_minimum",
            "untested" => "version_untested",
            _ => "version_unknown",
        };
        AccessReport {
            protocol: crate::PROTOCOL.to_string(),
            core_contract: crate::CORE_BOUNDARY.to_string(),
            source_id: self.source_id.clone(),
            secret_ref: self.secret_ref.clone(),
            tls: self.tls(),
            version: self.version_fact(),
            registration_allowed: false,
            block_reasons: vec![BlockReason::new(code, code)],
            principal: Fact::not_collected("not_collected_untested_version"),
            in_recovery: Fact::not_collected("not_collected_untested_version"),
            role: Fact::not_collected("not_collected_untested_version"),
            memberships: Vec::new(),
            write: WriteDetermination {
                state: "not_collected".to_string(),
                findings: Vec::new(),
            },
            security_definers: Vec::new(),
            rls: Vec::new(),
            publications: Fact::not_collected("not_collected_untested_version"),
            replication_slots: Fact::not_collected("not_collected_untested_version"),
            cdc: CdcPrerequisite {
                wal_level: Fact::not_collected("not_collected_untested_version"),
                replication_attribute: Fact::not_collected("not_collected_untested_version"),
                publication_for_selected: Fact::not_collected("not_collected_untested_version"),
                implemented: false,
            },
            capabilities: connector_capabilities(),
            dba_checklist: vec![DbaItem {
                id: "version".to_string(),
                level: "info".to_string(),
                risk:
                    "This server version is outside the tested set. No source object was created."
                        .to_string(),
                rollback: "No source change was made.".to_string(),
                statement: String::new(),
            }],
        }
    }

    fn session_block_report(&self) -> AccessReport {
        let mut blocks = Vec::new();
        if !self.identity_aligned {
            blocks.push(BlockReason::new("role_mismatch", "session_user"));
        }
        if !self.startup_clean {
            blocks.push(BlockReason::new("startup_option", "startup_options"));
        }
        AccessReport {
            protocol: crate::PROTOCOL.to_string(),
            core_contract: crate::CORE_BOUNDARY.to_string(),
            source_id: self.source_id.clone(),
            secret_ref: self.secret_ref.clone(),
            tls: self.tls(),
            version: self.version_fact(),
            registration_allowed: false,
            block_reasons: blocks,
            principal: crate::model::Fact::known(crate::model::Principal {
                current_user: self.current_user.clone(),
                session_user: self.session_user.clone(),
                database: self.database_name.clone(),
            }),
            in_recovery: crate::model::Fact::not_collected("not_collected_session_rejected"),
            role: crate::model::Fact::not_collected("not_collected_session_rejected"),
            memberships: Vec::new(),
            write: WriteDetermination {
                state: "unknown".to_string(),
                findings: Vec::new(),
            },
            security_definers: Vec::new(),
            rls: Vec::new(),
            publications: crate::model::Fact::not_collected("not_collected_session_rejected"),
            replication_slots: crate::model::Fact::not_collected("not_collected_session_rejected"),
            cdc: CdcPrerequisite {
                wal_level: crate::model::Fact::not_collected("not_collected_session_rejected"),
                replication_attribute: crate::model::Fact::not_collected(
                    "not_collected_session_rejected",
                ),
                publication_for_selected: crate::model::Fact::not_collected(
                    "not_collected_session_rejected",
                ),
                implemented: false,
            },
            capabilities: connector_capabilities(),
            dba_checklist: Vec::new(),
        }
    }
}

/// Server `pg_stat_ssl` fact for this backend.
#[derive(Debug)]
enum ServerTls {
    /// `ssl = true` and a `TLSv1.` version string.
    Observed(String),
    /// No row. Not plaintext, and not a negotiated version.
    Unknown,
}

/// A null `pg_stat_ssl` row is unknown. `ssl = false` is plaintext.
fn classify_server_tls(
    ssl: Option<bool>,
    version: Option<String>,
) -> Result<ServerTls, SourceError> {
    match ssl {
        Some(false) => Err(SourceError::tls("tls_not_negotiated")),
        None => {
            if version.is_some() {
                Err(SourceError::tls("tls_version_unknown"))
            } else {
                Ok(ServerTls::Unknown)
            }
        }
        Some(true) => {
            let Some(version) = version else {
                return Err(SourceError::tls("tls_version_unknown"));
            };
            if !version.starts_with("TLSv1.") || version.len() > 32 || version.contains('\0') {
                return Err(SourceError::tls("tls_version_unknown"));
            }
            Ok(ServerTls::Observed(version))
        }
    }
}

pub(crate) fn duration_ms(value: Duration) -> Result<String, SourceError> {
    let ms = value.as_millis();
    if ms == 0 || ms > 120_000 {
        return Err(SourceError::config("timeout"));
    }
    Ok(ms.to_string())
}

pub(crate) fn within_rows(len: usize, max_rows: u32) -> Result<(), SourceError> {
    if len > usize::try_from(max_rows).unwrap_or(usize::MAX) {
        Err(SourceError::budget("max_rows"))
    } else {
        Ok(())
    }
}

pub(crate) async fn fetch_all(
    session: &mut PgSession,
    query: sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>,
) -> Result<Vec<sqlx::postgres::PgRow>, SourceError> {
    let budget = session.budget.statement_timeout();
    let result = tokio::time::timeout(budget, query.fetch_all(&mut session.conn)).await;
    match result {
        Ok(Ok(rows)) => {
            within_rows(rows.len(), session.budget.max_rows())?;
            Ok(rows)
        }
        Ok(Err(err)) => Err(map_sqlx(err)),
        Err(_) => {
            session.poisoned = true;
            Err(SourceError::budget("statement_timeout"))
        }
    }
}

pub(crate) async fn fetch_one(
    session: &mut PgSession,
    query: sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>,
) -> Result<sqlx::postgres::PgRow, SourceError> {
    let budget = session.budget.statement_timeout();
    let result = tokio::time::timeout(budget, query.fetch_one(&mut session.conn)).await;
    match result {
        Ok(Ok(row)) => Ok(row),
        Ok(Err(err)) => Err(map_sqlx(err)),
        Err(_) => {
            session.poisoned = true;
            Err(SourceError::budget("statement_timeout"))
        }
    }
}

pub(crate) fn cell_string(
    row: &sqlx::postgres::PgRow,
    index: usize,
) -> Result<String, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

pub(crate) fn cell_opt_string(
    row: &sqlx::postgres::PgRow,
    index: usize,
) -> Result<Option<String>, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

pub(crate) fn cell_i64(row: &sqlx::postgres::PgRow, index: usize) -> Result<i64, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

pub(crate) fn cell_opt_i64(
    row: &sqlx::postgres::PgRow,
    index: usize,
) -> Result<Option<i64>, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

pub(crate) fn cell_i32(row: &sqlx::postgres::PgRow, index: usize) -> Result<i32, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

pub(crate) fn cell_bool(row: &sqlx::postgres::PgRow, index: usize) -> Result<bool, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

pub(crate) fn cell_opt_bool(
    row: &sqlx::postgres::PgRow,
    index: usize,
) -> Result<Option<bool>, SourceError> {
    row.try_get(index)
        .map_err(|_| SourceError::catalog("decode"))
}

#[cfg(test)]
mod tests {
    use super::{ServerTls, classify_server_tls};
    use crate::error::SourceErrorKind;

    #[test]
    fn missing_ssl_row_is_unknown() {
        assert!(matches!(
            classify_server_tls(None, None).unwrap(),
            ServerTls::Unknown
        ));
    }

    #[test]
    fn false_ssl_is_plaintext() {
        let err = classify_server_tls(Some(false), None).unwrap_err();
        assert_eq!(err.kind(), SourceErrorKind::Tls);
        assert_eq!(err.fact(), Some("tls_not_negotiated"));
    }

    #[test]
    fn true_ssl_keeps_only_a_tls_version() {
        let err = classify_server_tls(Some(true), None).unwrap_err();
        assert_eq!(err.fact(), Some("tls_version_unknown"));
        match classify_server_tls(Some(true), Some("TLSv1.3".to_string())).unwrap() {
            ServerTls::Observed(version) => assert_eq!(version, "TLSv1.3"),
            ServerTls::Unknown => panic!("missing version was accepted"),
        }
        let bad = classify_server_tls(Some(true), Some("ssl".to_string())).unwrap_err();
        assert_eq!(bad.fact(), Some("tls_version_unknown"));
    }

    #[test]
    fn version_without_ssl_is_not_success() {
        let err = classify_server_tls(None, Some("TLSv1.3".to_string())).unwrap_err();
        assert_eq!(err.fact(), Some("tls_version_unknown"));
    }
}
