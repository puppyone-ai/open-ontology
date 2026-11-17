//! Postgres `_oo_docs` access (10 §3). Board WS-BE5.
//!
//! `install` owns the schema and its checksummed migrations. `jobs` and
//! `events` wrap the `_oo_docs` SECURITY DEFINER functions; the runtime role
//! has no direct DML on the job and event tables.

use std::fmt;

pub mod events;
pub mod install;
pub mod jobs;

/// Database failure with the SQLSTATE kept apart from the message.
#[derive(Debug)]
pub enum StoreError {
    /// PostgreSQL or the driver rejected a statement.
    Database {
        /// SQLSTATE, empty when the driver has none.
        sqlstate: String,
        /// Server or driver message.
        message: String,
    },
    /// A row did not have the shape the function contract promises.
    Protocol(String),
}

impl StoreError {
    /// SQLSTATE when the failure came from the server (`OO010` input fault,
    /// `OO030` insert-only table, `40001` serialization, ...).
    pub fn sqlstate(&self) -> Option<&str> {
        match self {
            Self::Database { sqlstate, .. } if !sqlstate.is_empty() => Some(sqlstate),
            _ => None,
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database { sqlstate, message } => {
                write!(f, "docs store failed sqlstate={sqlstate} {message}")
            }
            Self::Protocol(message) => write!(f, "docs store protocol error: {message}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<sqlx::Error> for StoreError {
    fn from(err: sqlx::Error) -> Self {
        let db = err.as_database_error();
        Self::Database {
            sqlstate: db
                .and_then(|d| d.code().map(|c| c.into_owned()))
                .unwrap_or_default(),
            message: db.map_or_else(|| err.to_string(), |d| d.message().to_string()),
        }
    }
}
