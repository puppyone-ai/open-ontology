//! Read-only portal adapter. EXPLAIN is prefixed here, only around guard-restored SQL.

use oo_core::ExactDecimal;
use oo_gateway::{PolicyError, TlsEndpoint, connect_require_tls};
use postgres_types::ToSql;
use tokio::task::JoinHandle;
use tokio_postgres::{Client, Error as PgError, Transaction};

use crate::numeric::{PgNumeric, int_to_decimal};
use crate::probe::Kind;
use crate::report::sha256_hex;

const EXPLAIN_PREFIX: &str = "EXPLAIN (FORMAT TEXT) ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bind {
    Text(String),
    Int2(i16),
    Int4(i32),
    Int8(i64),
}

impl Bind {
    fn as_tosql(&self) -> &(dyn ToSql + Sync) {
        match self {
            Self::Text(value) => value,
            Self::Int2(value) => value,
            Self::Int4(value) => value,
            Self::Int8(value) => value,
        }
    }

    pub fn from_kind(kind: Kind, text: &str) -> Result<Self, ()> {
        match kind {
            Kind::Text => Ok(Self::Text(text.to_owned())),
            Kind::Int2 => Ok(Self::Int2(i16::try_from(text.parse::<i64>().map_err(|_| ())?).map_err(|_| ())?)),
            Kind::Int4 => Ok(Self::Int4(i32::try_from(text.parse::<i64>().map_err(|_| ())?).map_err(|_| ())?)),
            Kind::Int8 => Ok(Self::Int8(text.parse::<i64>().map_err(|_| ())?)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    Null,
    Decimal(ExactDecimal),
    Shape,
    Truncated,
}

pub struct Session {
    client: Option<Client>,
    driver: JoinHandle<()>,
    pub database_used: bool,
    dead: bool,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.client.take();
        self.driver.abort();
    }
}

pub async fn connect(endpoint: &TlsEndpoint, password: &str, ca_pem: &[u8]) -> Result<Session, &'static str> {
    let (client, connection) = connect_require_tls(endpoint, password, ca_pem)
        .await
        .map_err(policy_reason)?;
    let driver = tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(Session {
        client: Some(client),
        driver,
        database_used: false,
        dead: false,
    })
}

fn policy_reason(error: PolicyError) -> &'static str {
    match error {
        PolicyError::CertificateRejected => "certificate",
        _ => "connect",
    }
}

#[derive(Debug)]
pub enum StepError {
    Block(&'static str),
    Uncertain,
}

impl Session {
    pub fn dead(&self) -> bool {
        self.dead
    }

    pub async fn value(&mut self, sql: &str, binds: &[Bind], timeout_ms: u32, max_bytes: usize, role: &str) -> Result<Cell, StepError> {
        let rows = self.portal(sql, binds, 2, timeout_ms, role).await?;
        Ok(decode_value(&rows, max_bytes))
    }

    pub async fn count(
        &mut self,
        sql: &str,
        binds: &[Bind],
        fetch: i32,
        timeout_ms: u32,
        max_bytes: usize,
        role: &str,
    ) -> Result<Fetched, StepError> {
        let rows = self.portal(sql, binds, fetch, timeout_ms, role).await?;
        let mut bytes = 0_usize;
        for _row in &rows {
            bytes = bytes.saturating_add(1);
            if bytes > max_bytes {
                return Ok(Fetched::Truncated);
            }
        }
        Ok(Fetched::Rows(rows.len() as u64))
    }

    pub async fn explain(
        &mut self,
        restored_select: &str,
        binds: &[Bind],
        timeout_ms: u32,
        max_bytes: usize,
        role: &str,
    ) -> Result<ExplainOut, StepError> {
        let sql = format!("{EXPLAIN_PREFIX}{restored_select}");
        let rows = self.portal(&sql, binds, 32, timeout_ms, role).await?;
        if rows.len() == 32 {
            return Ok(ExplainOut::Truncated);
        }
        let mut text = String::new();
        for row in &rows {
            if row.columns().len() != 1 {
                return Ok(ExplainOut::Shape);
            }
            let line: Option<String> = match row.try_get(0) {
                Ok(value) => value,
                Err(_) => return Ok(ExplainOut::Shape),
            };
            let Some(line) = line else {
                return Ok(ExplainOut::Shape);
            };
            if text.len().saturating_add(line.len()).saturating_add(1) > max_bytes {
                return Ok(ExplainOut::Truncated);
            }
            text.push_str(&line);
            text.push('\n');
        }
        if rows.is_empty() {
            return Ok(ExplainOut::Shape);
        }
        Ok(ExplainOut::Ready {
            rows: rows.len() as u32,
            text_sha256: sha256_hex(text.as_bytes()),
        })
    }

    async fn portal(
        &mut self,
        sql: &str,
        binds: &[Bind],
        max_rows: i32,
        timeout_ms: u32,
        role: &str,
    ) -> Result<Vec<tokio_postgres::Row>, StepError> {
        if self.dead || max_rows <= 0 {
            self.dead = true;
            return Err(StepError::Uncertain);
        }
        let outcome = if let Some(client) = self.client.as_mut() {
            let begun = client.transaction().await;
            match begun {
                Ok(transaction) => match run_portal(&transaction, sql, binds, max_rows, timeout_ms, role).await {
                    Ok(rows) => match transaction.commit().await {
                        Ok(()) => PortalOutcome::Ready(rows),
                        Err(error) => PortalOutcome::CommitFailed(classify(error)),
                    },
                    Err(error) => match transaction.rollback().await {
                        Ok(()) => PortalOutcome::Failed(classify(error)),
                        Err(_) => PortalOutcome::Uncertain,
                    },
                },
                Err(error) => PortalOutcome::BeginFailed(classify(error)),
            }
        } else {
            PortalOutcome::NoClient
        };
        let outcome = if let PortalOutcome::CommitFailed(error) = outcome {
            let params: [&(dyn ToSql + Sync); 0] = [];
            let confirmed = if let Some(client) = self.client.as_mut() {
                client.execute("ROLLBACK", &params).await.is_ok()
            } else {
                false
            };
            if confirmed {
                PortalOutcome::Failed(error)
            } else {
                PortalOutcome::Uncertain
            }
        } else {
            outcome
        };
        match outcome {
            PortalOutcome::Ready(rows) => {
                self.database_used = true;
                Ok(rows)
            }
            PortalOutcome::Failed(error) => {
                self.database_used = true;
                Err(error)
            }
            PortalOutcome::Uncertain => {
                self.database_used = true;
                self.dead = true;
                self.client.take();
                Err(StepError::Uncertain)
            }
            PortalOutcome::BeginFailed(error) => {
                self.dead = true;
                self.client.take();
                Err(error)
            }
            PortalOutcome::NoClient => {
                self.dead = true;
                Err(StepError::Uncertain)
            }
            PortalOutcome::CommitFailed(_) => {
                self.database_used = true;
                self.dead = true;
                self.client.take();
                Err(StepError::Uncertain)
            }
        }
    }
}

enum PortalOutcome {
    Ready(Vec<tokio_postgres::Row>),
    Failed(StepError),
    CommitFailed(StepError),
    Uncertain,
    BeginFailed(StepError),
    NoClient,
}

async fn run_portal(
    transaction: &Transaction<'_>,
    sql: &str,
    binds: &[Bind],
    max_rows: i32,
    timeout_ms: u32,
    role: &str,
) -> Result<Vec<tokio_postgres::Row>, PgError> {
    let params: [&(dyn ToSql + Sync); 0] = [];
    let timeout_sql = format!("SET LOCAL statement_timeout = '{timeout_ms}ms'");
    transaction.execute(&timeout_sql, &params).await?;
    transaction.execute("SET LOCAL transaction_read_only = on", &params).await?;
    let role_sql = format!("SET ROLE {role}");
    transaction.execute(&role_sql, &params).await?;
    let statement = transaction.prepare(sql).await?;
    let params: Vec<&(dyn ToSql + Sync)> = binds.iter().map(Bind::as_tosql).collect();
    let portal = transaction.bind(&statement, &params).await?;
    transaction.query_portal(&portal, max_rows).await
}

fn classify(error: PgError) -> StepError {
    match error.code().map(|code| code.code()) {
        Some("57014") => StepError::Block("timeout"),
        _ => StepError::Block("query"),
    }
}

pub enum Fetched {
    Rows(u64),
    Truncated,
}

pub enum ExplainOut {
    Ready { rows: u32, text_sha256: String },
    Truncated,
    Shape,
}

fn decode_value(rows: &[tokio_postgres::Row], max_bytes: usize) -> Cell {
    if rows.len() != 1 {
        return Cell::Shape;
    }
    let row = &rows[0];
    if row.columns().len() != 1 {
        return Cell::Shape;
    }
    match row.columns()[0].type_().oid() {
        21 => match row.try_get::<_, Option<i16>>(0) {
            Ok(Some(value)) => sized(int_to_decimal(i64::from(value)), max_bytes),
            Ok(None) => Cell::Null,
            Err(_) => Cell::Shape,
        },
        23 => match row.try_get::<_, Option<i32>>(0) {
            Ok(Some(value)) => sized(int_to_decimal(i64::from(value)), max_bytes),
            Ok(None) => Cell::Null,
            Err(_) => Cell::Shape,
        },
        20 => match row.try_get::<_, Option<i64>>(0) {
            Ok(Some(value)) => sized(int_to_decimal(value), max_bytes),
            Ok(None) => Cell::Null,
            Err(_) => Cell::Shape,
        },
        1700 => match row.try_get::<_, Option<PgNumeric>>(0) {
            Ok(Some(value)) => sized(value.decimal().clone(), max_bytes),
            Ok(None) => Cell::Null,
            Err(_) => Cell::Shape,
        },
        _ => Cell::Shape,
    }
}

fn sized(decimal: ExactDecimal, max_bytes: usize) -> Cell {
    if decimal.to_plain_string().len() > max_bytes {
        Cell::Truncated
    } else {
        Cell::Decimal(decimal)
    }
}
