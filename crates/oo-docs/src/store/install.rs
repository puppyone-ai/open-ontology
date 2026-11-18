//! Checksummed installer for `_oo_docs` (10 §3.1 rules 1-4).
//!
//! The four SQL files live in `crates/oo-pipeline/migrations/` and are
//! compiled in with `include_str!`; SHA-256 is computed from those bytes. One
//! transaction takes `pg_advisory_xact_lock(6413202611)`, locks
//! `_oo_docs.migration_lock`, compares every stored row with its compiled
//! digest (`OO001` on a difference, before anything runs), then applies the
//! missing files in order and inserts their version rows. A file whose
//! predecessor is absent raises `OO002`. An exact match runs nothing and does
//! not touch `applied_at`.
//!
//! Fresh install needs a superuser (it creates the roles). An upgrade may use
//! a member of `oo_docs_owner`. The caller passes one idle connection; this
//! module opens no pool and reads no credential.

use std::fmt;

use sha2::{Digest, Sha256};
use sqlx::Connection;
use sqlx::postgres::PgConnection;

/// Migration executor lock. Not a data fence.
pub const MIGRATION_LOCK_KEY: i64 = 6_413_202_611;

/// One compiled migration.
#[derive(Clone, Copy, Debug)]
pub struct Migration {
    /// `_oo_docs.schema_migrations.version`.
    pub version: &'static str,
    /// File name under `crates/oo-pipeline/migrations/`.
    pub file: &'static str,
    sql: &'static str,
}

impl Migration {
    /// Lowercase hex SHA-256 of the compiled file bytes.
    pub fn sha256(&self) -> String {
        sha256_hex(self.sql.as_bytes())
    }
}

/// `docs-bootstrap/1`.
pub const VERSION_BOOTSTRAP: &str = "docs-bootstrap/1";
/// `docs-sources-jobs/1`.
pub const VERSION_SOURCES_JOBS: &str = "docs-sources-jobs/1";
/// `docs-index/1`.
pub const VERSION_INDEX: &str = "docs-index/1";
/// `docs-packs-shares/1`.
pub const VERSION_PACKS_SHARES: &str = "docs-packs-shares/1";
/// `docs-llm/1`.
pub const VERSION_LLM: &str = "docs-llm/1";
/// `docs-consistency/1`.
pub const VERSION_CONSISTENCY: &str = "docs-consistency/1";
/// `docs-suggest/1`.
pub const VERSION_SUGGEST: &str = "docs-suggest/1";
/// `docs-share-consistency/1`.
pub const VERSION_SHARE_CONSISTENCY: &str = "docs-share-consistency/1";
/// `docs-parse-media/1`.
pub const VERSION_PARSE_MEDIA: &str = "docs-parse-media/1";
/// `docs-cloud-audit/1`.
pub const VERSION_CLOUD_AUDIT: &str = "docs-cloud-audit/1";

const MIGRATIONS: [Migration; 10] = [
    Migration {
        version: VERSION_BOOTSTRAP,
        file: "0137_docs_bootstrap.sql",
        sql: include_str!("../../../oo-pipeline/migrations/0137_docs_bootstrap.sql"),
    },
    Migration {
        version: VERSION_SOURCES_JOBS,
        file: "0138_docs_sources_jobs.sql",
        sql: include_str!("../../../oo-pipeline/migrations/0138_docs_sources_jobs.sql"),
    },
    Migration {
        version: VERSION_INDEX,
        file: "0139_docs_index.sql",
        sql: include_str!("../../../oo-pipeline/migrations/0139_docs_index.sql"),
    },
    Migration {
        version: VERSION_PACKS_SHARES,
        file: "0140_docs_packs_shares.sql",
        sql: include_str!("../../../oo-pipeline/migrations/0140_docs_packs_shares.sql"),
    },
    Migration {
        version: VERSION_LLM,
        file: "0141_docs_llm.sql",
        sql: include_str!("../../../oo-pipeline/migrations/0141_docs_llm.sql"),
    },
    Migration {
        version: VERSION_CONSISTENCY,
        file: "0142_docs_consistency.sql",
        sql: include_str!("../../../oo-pipeline/migrations/0142_docs_consistency.sql"),
    },
    Migration {
        version: VERSION_SUGGEST,
        file: "0143_docs_suggest.sql",
        sql: include_str!("../../../oo-pipeline/migrations/0143_docs_suggest.sql"),
    },
    Migration {
        version: VERSION_SHARE_CONSISTENCY,
        file: "0145_docs_share_consistency.sql",
        sql: include_str!("../../../oo-pipeline/migrations/0145_docs_share_consistency.sql"),
    },
    Migration {
        version: VERSION_PARSE_MEDIA,
        file: "0146_docs_parse_media.sql",
        sql: include_str!("../../../oo-pipeline/migrations/0146_docs_parse_media.sql"),
    },
    Migration {
        version: VERSION_CLOUD_AUDIT,
        file: "0148_docs_cloud_audit.sql",
        sql: include_str!("../../../oo-pipeline/migrations/0148_docs_cloud_audit.sql"),
    },
];

/// `docs-vectors/1`, the optional pgvector table (10 11.3).
pub const VERSION_VECTORS: &str = "docs-vectors/1";

/// Optional migration: applied only when the server ships the `vector` extension and the session
/// is an administrator. It is never in [`SCHEMA_MIN`]; readiness checks its checksum when stored.
pub const VECTORS: Migration = Migration {
    version: VERSION_VECTORS,
    file: "0144_docs_vectors.sql",
    sql: include_str!("../../../oo-pipeline/migrations/0144_docs_vectors.sql"),
};

/// Versions that must all exist (`SCHEMA_MIN`, 3.1 rule 4).
pub const SCHEMA_MIN: [&str; 10] = [
    VERSION_BOOTSTRAP,
    VERSION_SOURCES_JOBS,
    VERSION_INDEX,
    VERSION_PACKS_SHARES,
    VERSION_LLM,
    VERSION_CONSISTENCY,
    VERSION_SUGGEST,
    VERSION_SHARE_CONSISTENCY,
    VERSION_PARSE_MEDIA,
    VERSION_CLOUD_AUDIT,
];

/// Migrations compiled into this binary, in install order.
pub fn migrations() -> &'static [Migration] {
    &MIGRATIONS
}

/// Lowercase hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        hex.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    hex
}

/// What one successful [`install`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    /// These versions were executed and recorded, in order.
    Applied(Vec<&'static str>),
    /// Every stored checksum already matched. No script ran.
    Replay,
}

/// Install failure. `OO001` and `OO002` are the migration SQLSTATEs.
#[derive(Debug)]
pub enum InstallError {
    /// A stored checksum differs from the compiled file. Nothing ran.
    ChecksumMismatch {
        /// Stored version.
        version: String,
        /// Stored checksum.
        stored: String,
    },
    /// A version is stored but an earlier one is not.
    MissingPrerequisite {
        /// The version that is absent.
        missing: &'static str,
    },
    /// The session cannot create roles or assume `oo_docs_owner`.
    NotAdministrator,
    /// The catalog is not one this installer may touch.
    Refused(String),
    /// PostgreSQL rejected a statement; the transaction rolled back.
    Database {
        /// SQLSTATE, empty when the driver has none.
        sqlstate: String,
        /// Server or driver message.
        message: String,
    },
}

impl InstallError {
    /// `OO001`, `OO002`, or the server SQLSTATE.
    pub fn sqlstate(&self) -> Option<&str> {
        match self {
            Self::ChecksumMismatch { .. } => Some("OO001"),
            Self::MissingPrerequisite { .. } => Some("OO002"),
            Self::Database { sqlstate, .. } if !sqlstate.is_empty() => Some(sqlstate),
            _ => None,
        }
    }
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChecksumMismatch { version, stored } => {
                write!(f, "migration checksum mismatch for {version}: {stored}")
            }
            Self::MissingPrerequisite { missing } => {
                write!(f, "missing prerequisite migration {missing}")
            }
            Self::NotAdministrator => f.write_str("docs install requires an administrator"),
            Self::Refused(message) => f.write_str(message),
            Self::Database { sqlstate, message } => {
                write!(f, "docs install failed sqlstate={sqlstate} {message}")
            }
        }
    }
}

impl std::error::Error for InstallError {}

fn db(err: sqlx::Error) -> InstallError {
    let d = err.as_database_error();
    let sqlstate = d
        .and_then(|d| d.code().map(|c| c.into_owned()))
        .unwrap_or_default();
    let message = d.map_or_else(|| err.to_string(), |d| d.message().to_string());
    InstallError::Database { sqlstate, message }
}

async fn exec(conn: &mut PgConnection, sql: &'static str) -> Result<(), InstallError> {
    sqlx::raw_sql(sql)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(db)
}

#[derive(Debug)]
struct Stored {
    version: String,
    sha256: String,
    compat: String,
}

async fn read_rows(conn: &mut PgConnection) -> Result<Vec<Stored>, InstallError> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT version, sha256, compat FROM _oo_docs.schema_migrations ORDER BY applied_at, version",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db)?;
    Ok(rows
        .into_iter()
        .map(|(version, sha256, compat)| Stored {
            version,
            sha256,
            compat,
        })
        .collect())
}

async fn table_exists(conn: &mut PgConnection) -> Result<bool, InstallError> {
    sqlx::query_scalar("SELECT pg_catalog.to_regclass('_oo_docs.schema_migrations') IS NOT NULL")
        .fetch_one(&mut *conn)
        .await
        .map_err(db)
}

async fn is_superuser(conn: &mut PgConnection) -> Result<bool, InstallError> {
    sqlx::query_scalar("SELECT rolsuper FROM pg_catalog.pg_roles WHERE rolname = current_user")
        .fetch_one(&mut *conn)
        .await
        .map_err(db)
}

async fn is_owner_member(conn: &mut PgConnection) -> Result<bool, InstallError> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_docs_owner') \
         AND pg_catalog.pg_has_role(current_user, 'oo_docs_owner', 'MEMBER')",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(db)
}

fn check_rows(rows: &[Stored]) -> Result<(), InstallError> {
    for migration in MIGRATIONS.iter().chain(std::iter::once(&VECTORS)) {
        if let Some(row) = rows
            .iter()
            .find(|r| r.version == migration.version && r.sha256 != migration.sha256())
        {
            return Err(InstallError::ChecksumMismatch {
                version: row.version.clone(),
                stored: row.sha256.clone(),
            });
        }
    }
    if let Some(row) = rows.iter().find(|r| {
        r.compat == "breaking"
            && !MIGRATIONS.iter().any(|m| m.version == r.version)
            && r.version != VERSION_VECTORS
    }) {
        return Err(InstallError::Refused(format!(
            "database has unknown breaking migration {}",
            row.version
        )));
    }
    Ok(())
}

/// Install or replay `_oo_docs`.
///
/// `conn` must be idle. See the module docs for the order of checks. A
/// failure rolls everything back, so readiness is never true after one.
pub async fn install(conn: &mut PgConnection) -> Result<InstallOutcome, InstallError> {
    let superuser = is_superuser(conn).await?;
    if !superuser && !is_owner_member(conn).await? {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(db)?;
    match run(&mut tx, superuser).await {
        Ok(outcome) => {
            tx.commit().await.map_err(db)?;
            Ok(outcome)
        }
        Err(err) => {
            let _ = tx.rollback().await;
            Err(err)
        }
    }
}

async fn run(conn: &mut PgConnection, superuser: bool) -> Result<InstallOutcome, InstallError> {
    exec(conn, "SET LOCAL lock_timeout = '15s'").await?;
    exec(conn, "SET LOCAL statement_timeout = '60s'").await?;
    exec(conn, "SELECT pg_catalog.pg_advisory_xact_lock(6413202611)").await?;

    let fresh = !table_exists(conn).await?;
    if fresh && !superuser {
        return Err(InstallError::NotAdministrator);
    }
    let rows = if fresh {
        Vec::new()
    } else {
        exec(
            conn,
            "SELECT 1 FROM _oo_docs.migration_lock WHERE singleton FOR UPDATE",
        )
        .await?;
        read_rows(conn).await?
    };
    check_rows(&rows)?;

    // A stored version needs every earlier one.
    let mut gap: Option<&'static str> = None;
    for migration in &MIGRATIONS {
        let present = rows.iter().any(|r| r.version == migration.version);
        match (present, gap) {
            (false, None) => gap = Some(migration.version),
            (true, Some(missing)) => return Err(InstallError::MissingPrerequisite { missing }),
            _ => {}
        }
    }

    let mut applied = Vec::new();
    for migration in &MIGRATIONS {
        if rows.iter().any(|r| r.version == migration.version) {
            continue;
        }
        if migration.version == VERSION_BOOTSTRAP {
            sqlx::raw_sql(migration.sql)
                .execute(&mut *conn)
                .await
                .map_err(db)?;
        } else {
            // Superuser or a member of the owner role.
            exec(conn, "SET LOCAL ROLE oo_docs_owner").await?;
            sqlx::raw_sql(migration.sql)
                .execute(&mut *conn)
                .await
                .map_err(db)?;
        }
        exec(conn, "RESET ROLE").await?;
        exec(conn, "SET LOCAL ROLE oo_docs_owner").await?;
        sqlx::query("INSERT INTO _oo_docs.schema_migrations (version, sha256) VALUES ($1, $2)")
            .bind(migration.version)
            .bind(migration.sha256())
            .execute(&mut *conn)
            .await
            .map_err(db)?;
        exec(conn, "RESET ROLE").await?;
        applied.push(migration.version);
    }
    if !rows.iter().any(|r| r.version == VERSION_VECTORS)
        && vectors_available(conn, superuser).await?
    {
        // The administrator creates the extension; the owner creates the table.
        exec(
            conn,
            "CREATE EXTENSION IF NOT EXISTS vector WITH SCHEMA public",
        )
        .await?;
        exec(conn, "SET LOCAL ROLE oo_docs_owner").await?;
        sqlx::raw_sql(VECTORS.sql)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
        sqlx::query("INSERT INTO _oo_docs.schema_migrations (version, sha256) VALUES ($1, $2)")
            .bind(VECTORS.version)
            .bind(VECTORS.sha256())
            .execute(&mut *conn)
            .await
            .map_err(db)?;
        exec(conn, "RESET ROLE").await?;
        applied.push(VECTORS.version);
    }
    if applied.is_empty() {
        Ok(InstallOutcome::Replay)
    } else {
        Ok(InstallOutcome::Applied(applied))
    }
}

async fn vectors_available(conn: &mut PgConnection, superuser: bool) -> Result<bool, InstallError> {
    let present: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_extension WHERE extname = 'vector')",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(db)?;
    if present {
        return Ok(true);
    }
    if !superuser {
        return Ok(false);
    }
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_available_extensions WHERE name = 'vector')",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(db)
}

/// Readiness proven against the compiled digests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedSchema {
    /// Versions present with the compiled checksum.
    pub known: Vec<&'static str>,
    /// Versions this binary does not know, all `additive` (usable by an
    /// older engine after a rollback, 3.1 rule 4).
    pub unknown_additive: Vec<String>,
}

/// Why the database is not a verified `_oo_docs`.
#[derive(Debug)]
pub enum ReadinessError {
    /// `_oo_docs.schema_migrations` is absent.
    NotInstalled,
    /// A required version row is missing.
    Incomplete {
        /// First missing version.
        missing: &'static str,
    },
    /// A stored checksum differs (`OO001`).
    ChecksumMismatch {
        /// Stored version.
        version: String,
        /// Stored checksum.
        stored: String,
    },
    /// An unknown version is marked `breaking` (`DOCS_ENGINE_SCHEMA_MISMATCH`).
    SchemaMismatch {
        /// The unknown breaking version.
        version: String,
    },
    /// The query failed.
    Database(String),
}

impl fmt::Display for ReadinessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInstalled => f.write_str("docs schema is not installed"),
            Self::Incomplete { missing } => write!(f, "docs schema is missing {missing}"),
            Self::ChecksumMismatch { version, stored } => {
                write!(f, "migration checksum mismatch for {version}: {stored}")
            }
            Self::SchemaMismatch { version } => {
                write!(f, "DOCS_ENGINE_SCHEMA_MISMATCH: unknown breaking {version}")
            }
            Self::Database(message) => write!(f, "docs readiness failed: {message}"),
        }
    }
}

impl std::error::Error for ReadinessError {}

/// Compare stored rows with the compiled digests. Read only: it does not
/// install, take the migration lock, or run a script. The runtime role can
/// call it.
pub async fn verify_readiness(conn: &mut PgConnection) -> Result<VerifiedSchema, ReadinessError> {
    let wrap = |e: InstallError| ReadinessError::Database(e.to_string());
    if !table_exists(conn).await.map_err(wrap)? {
        return Err(ReadinessError::NotInstalled);
    }
    let rows = read_rows(conn).await.map_err(wrap)?;
    let mut known = Vec::new();
    for migration in &MIGRATIONS {
        match rows.iter().find(|r| r.version == migration.version) {
            None => {
                return Err(ReadinessError::Incomplete {
                    missing: migration.version,
                });
            }
            Some(row) if row.sha256 != migration.sha256() => {
                return Err(ReadinessError::ChecksumMismatch {
                    version: row.version.clone(),
                    stored: row.sha256.clone(),
                });
            }
            Some(_) => known.push(migration.version),
        }
    }
    if let Some(row) = rows
        .iter()
        .find(|r| r.version == VERSION_VECTORS && r.sha256 != VECTORS.sha256())
    {
        return Err(ReadinessError::ChecksumMismatch {
            version: row.version.clone(),
            stored: row.sha256.clone(),
        });
    }
    let mut unknown_additive = Vec::new();
    for row in rows.iter().filter(|r| {
        !MIGRATIONS.iter().any(|m| m.version == r.version) && r.version != VERSION_VECTORS
    }) {
        if row.compat == "breaking" {
            return Err(ReadinessError::SchemaMismatch {
                version: row.version.clone(),
            });
        }
        unknown_additive.push(row.version.clone());
    }
    Ok(VerifiedSchema {
        known,
        unknown_additive,
    })
}
