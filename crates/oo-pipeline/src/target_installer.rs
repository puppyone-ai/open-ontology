//! Install the frozen target schema on one caller-owned connection.
//!
//! The scripts are `0000`, `0001`, `0003`, and `0004`. SHA-256 is computed
//! from those compiled bytes. `0002` is not executed. A fresh database
//! commits the schema and its version rows in one transaction. A valid v1
//! database goes straight to `0003` and then `0004`, with no v2 row. A valid
//! v2 database takes the same scripts and keeps the stored v1 and v2 rows.
//!
//! This is not the Loader. It does not copy rows, write receipts, create a
//! publication, or admit a source reader.

use std::fmt;

use sha2::{Digest, Sha256};
use sqlx::postgres::PgConnection;
use sqlx::Connection;

/// Migration executor lock. Not a shard fence.
pub const MIGRATION_LOCK_KEY: i64 = 6_404_202_605;

/// Bootstrap version stored for `0000_bootstrap_roles.sql`.
pub const VERSION_BOOTSTRAP: &str = "target-bootstrap/1";

/// Fence version stored for `0001_target_fence_expand.sql`.
pub const VERSION_FENCE: &str = "target-fence-receipts/1";

/// Historical v2 version. The installer does not execute that file.
pub const VERSION_SEGMENT_V2: &str = "target-fence-receipts/2";

/// Preflight version stored for `0003_target_segment_preflight_expand.sql`.
pub const VERSION_PREFLIGHT: &str = "target-fence-receipts/3";

/// Mapping version stored for `0004_physical_mapping.sql`.
pub const VERSION_MAPPING: &str = "physical-mapping/v1";

/// Full-import version stored for `0005_full_import_expand.sql`.
///
/// `install` does not apply this version. [`install_full_import`] does.
pub const VERSION_FULL_IMPORT: &str = "full-import/v1";

/// Frozen SHA-256 of `0002_target_segment_null_expand.sql`.
///
/// A stored v2 row must match this digest. The file is not compiled into the
/// install path and is not sent to the server.
pub const HISTORICAL_V2_SHA256: &str =
    "59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512";

const FILE_BOOTSTRAP: &str = "0000_bootstrap_roles.sql";
const FILE_FENCE: &str = "0001_target_fence_expand.sql";
const FILE_SEGMENT_V2: &str = "0002_target_segment_null_expand.sql";
const FILE_PREFLIGHT: &str = "0003_target_segment_preflight_expand.sql";
const FILE_MAPPING: &str = "0004_physical_mapping.sql";
const FILE_FULL_IMPORT: &str = "0005_full_import_expand.sql";

const SCRIPT_BOOTSTRAP: &str = include_str!("../migrations/0000_bootstrap_roles.sql");
const SCRIPT_FENCE: &str = include_str!("../migrations/0001_target_fence_expand.sql");
const SCRIPT_PREFLIGHT: &str = include_str!("../migrations/0003_target_segment_preflight_expand.sql");
const SCRIPT_MAPPING: &str = include_str!("../migrations/0004_physical_mapping.sql");
const SCRIPT_FULL_IMPORT: &str = include_str!("../migrations/0005_full_import_expand.sql");

const LOCK_SQL: &str = "SELECT pg_catalog.pg_advisory_xact_lock(6404202605)";
const ROW_LOCK_SQL: &str = "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE";
const RAISE_OO001: &str = "\
DO $oo001$\n\
BEGIN\n\
  RAISE EXCEPTION 'migration checksum mismatch' USING ERRCODE = 'OO001';\n\
END\n\
$oo001$;";

const ABSENT_MESSAGE: &str = "target-fence-receipts/1 segment relation is absent";

/// SHA-256 of the four scripts compiled into this crate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledDigests {
    bootstrap: String,
    fence: String,
    preflight: String,
    mapping: String,
}

impl CompiledDigests {
    /// Digest of `0000_bootstrap_roles.sql`.
    pub fn bootstrap(&self) -> &str {
        &self.bootstrap
    }

    /// Digest of `0001_target_fence_expand.sql`.
    pub fn fence(&self) -> &str {
        &self.fence
    }

    /// Digest of `0003_target_segment_preflight_expand.sql`.
    pub fn preflight(&self) -> &str {
        &self.preflight
    }

    /// Digest of `0004_physical_mapping.sql`.
    pub fn mapping(&self) -> &str {
        &self.mapping
    }
}

/// Digests of the embedded scripts. The caller does not supply these.
pub fn compiled_digests() -> CompiledDigests {
    CompiledDigests {
        bootstrap: sha256_hex(SCRIPT_BOOTSTRAP.as_bytes()),
        fence: sha256_hex(SCRIPT_FENCE.as_bytes()),
        preflight: sha256_hex(SCRIPT_PREFLIGHT.as_bytes()),
        mapping: sha256_hex(SCRIPT_MAPPING.as_bytes()),
    }
}

/// SHA-256 of `0005_full_import_expand.sql`. This is not part of [`compiled_digests`].
pub fn compiled_full_import_digest() -> String {
    sha256_hex(SCRIPT_FULL_IMPORT.as_bytes())
}

/// Lowercase hex SHA-256. This is the migration checksum function.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(hex_digit(byte >> 4));
        hex.push(hex_digit(byte & 0x0f));
    }
    hex
}

fn hex_digit(nibble: u8) -> char {
    char::from(b"0123456789abcdef"[usize::from(nibble)])
}

/// What one successful `install` call did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    /// Empty catalog. All four production markers were inserted together.
    Bootstrapped,
    /// Valid v1, no v2 row. `0003` then `0004`. No v2 marker was inserted.
    UpgradedFromV1,
    /// Valid v2 row kept. `0003` then `0004`. Historical rows were not rewritten.
    UpgradedFromV2,
    /// Valid v3 already stored. `0004` only.
    InstalledMapping,
    /// Stored production checksums already match. No script ran.
    Replay,
}

impl fmt::Display for InstallOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Bootstrapped => "bootstrapped",
            Self::UpgradedFromV1 => "upgraded_from_v1",
            Self::UpgradedFromV2 => "upgraded_from_v2",
            Self::InstalledMapping => "installed_mapping",
            Self::Replay => "replay",
        })
    }
}

/// Install failure. `OO001` and `OO002` are the migration sqlstates.
#[derive(Debug)]
pub enum InstallError {
    /// The session cannot create roles or assume `oo_target_owner`.
    NotAdministrator,
    /// A stored checksum differs. The row was not updated.
    ChecksumMismatch {
        /// `schema_migrations.version`.
        version: String,
        /// Stored checksum.
        stored: String,
    },
    /// `_oo.apply_receipts` or `apply_receipts_segment_check` is absent.
    MissingRelation {
        /// Server message. The fresh-catalog path does not use this error.
        message: String,
    },
    /// The catalog is neither empty nor a forward install. Nothing was written.
    Refused {
        /// Why this catalog was left unchanged.
        message: String,
    },
    /// PostgreSQL rejected a statement. The install transaction rolled back.
    Database {
        /// SQLSTATE, or empty when the driver has none.
        sqlstate: String,
        /// Server or driver message.
        message: String,
    },
}

impl InstallError {
    /// `OO001`, `OO002`, or the driver sqlstate.
    pub fn sqlstate(&self) -> Option<&str> {
        match self {
            Self::ChecksumMismatch { .. } => Some("OO001"),
            Self::MissingRelation { .. } => Some("OO002"),
            Self::Database { sqlstate, .. } if !sqlstate.is_empty() => Some(sqlstate.as_str()),
            Self::Database { .. } | Self::NotAdministrator | Self::Refused { .. } => None,
        }
    }

    /// Version whose stored checksum did not match.
    pub fn version(&self) -> Option<&str> {
        match self {
            Self::ChecksumMismatch { version, .. } => Some(version.as_str()),
            _ => None,
        }
    }
}

impl fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAdministrator => {
                formatter.write_str("target install requires the administrator connection")
            }
            Self::ChecksumMismatch { version, stored } => {
                write!(formatter, "migration checksum mismatch for {version}: {stored}")
            }
            Self::MissingRelation { message } => formatter.write_str(message),
            Self::Refused { message } => formatter.write_str(message),
            Self::Database { sqlstate, message } => {
                write!(formatter, "target install failed sqlstate={sqlstate} {message}")
            }
        }
    }
}

impl std::error::Error for InstallError {}

/// Readiness proven against the compiled digests.
///
/// Callers cannot build this value. [`verify_readiness`] returns it only
/// when the stored production rows match [`compiled_digests`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedReadiness {
    digests: CompiledDigests,
    retains_historical_v2: bool,
}

impl VerifiedReadiness {
    /// Compiled digests that matched the stored rows.
    pub fn digests(&self) -> &CompiledDigests {
        &self.digests
    }

    /// A historical v2 row is present and matches [`HISTORICAL_V2_SHA256`].
    pub fn retains_historical_v2(&self) -> bool {
        self.retains_historical_v2
    }
}

/// Why the connection is not a verified target.
#[derive(Debug)]
pub enum ReadinessError {
    /// `_oo.schema_migrations` is absent.
    NotInstalled,
    /// A production row is missing. This is not a verified target.
    Incomplete,
    /// A stored checksum differs from the compiled or historical digest.
    ChecksumMismatch {
        /// `schema_migrations.version`.
        version: String,
        /// Stored checksum.
        stored: String,
    },
    /// The readiness query failed.
    Database {
        /// SQLSTATE, or empty when the driver has none.
        sqlstate: String,
        /// Server or driver message.
        message: String,
    },
}

impl ReadinessError {
    /// Stable kind. None of these values is readiness.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NotInstalled => "not_installed",
            Self::Incomplete => "incomplete",
            Self::ChecksumMismatch { .. } => "checksum_mismatch",
            Self::Database { .. } => "database",
        }
    }

    /// `OO001` when a stored checksum differs.
    pub fn sqlstate(&self) -> Option<&str> {
        match self {
            Self::ChecksumMismatch { .. } => Some("OO001"),
            Self::Database { sqlstate, .. } if !sqlstate.is_empty() => Some(sqlstate.as_str()),
            Self::Database { .. } | Self::NotInstalled | Self::Incomplete => None,
        }
    }
}

impl fmt::Display for ReadinessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInstalled => formatter.write_str("target schema is not installed"),
            Self::Incomplete => formatter.write_str("target schema is not ready"),
            Self::ChecksumMismatch { version, stored } => {
                write!(formatter, "migration checksum mismatch for {version}: {stored}")
            }
            Self::Database { sqlstate, message } => {
                write!(formatter, "target readiness failed sqlstate={sqlstate} {message}")
            }
        }
    }
}

impl std::error::Error for ReadinessError {}

/// What one successful [`install_full_import`] call did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullImportInstallOutcome {
    /// `0005` ran and its marker was inserted. `0000` through `0004` were not rewritten.
    Installed,
    /// The stored `full-import/v1` row already matches. No script ran.
    Replay,
}

impl fmt::Display for FullImportInstallOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Installed => "installed",
            Self::Replay => "replay",
        })
    }
}

/// `0005` readiness. Schema setup is not a finished import.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedFullImport {
    digest: String,
}

impl VerifiedFullImport {
    /// Compiled digest of `0005_full_import_expand.sql`.
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

/// Install or replay the frozen target schema.
///
/// `conn` must be idle. Bootstrap needs a superuser. An upgrade needs a
/// superuser or a member of `oo_target_owner`. The function opens one
/// transaction, takes [`MIGRATION_LOCK_KEY`], locks `_oo.migration_lock`
/// when that row exists, and commits the scripts and version rows together.
/// A mismatch or a missing relation rolls back.
pub async fn install(conn: &mut PgConnection) -> Result<InstallOutcome, InstallError> {
    let digests = compiled_digests();
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }

    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = exec_static(&mut tx, "SET LOCAL lock_timeout = '15s'").await {
        return fail(tx, err).await;
    }
    if let Err(err) = exec_static(&mut tx, "SET LOCAL statement_timeout = '60s'").await {
        return fail(tx, err).await;
    }
    let assumed = if principal.superuser {
        Ok(())
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await
    };
    if let Err(err) = assumed {
        return fail(tx, err).await;
    }
    if let Err(err) = exec_static(&mut tx, LOCK_SQL).await {
        return fail(tx, err).await;
    }

    let catalog = match read_catalog(&mut tx).await {
        Ok(catalog) => catalog,
        Err(err) => return fail(tx, err).await,
    };
    if !principal_can_install(&principal, &catalog) {
        return fail(tx, InstallError::NotAdministrator).await;
    }
    let row_locked = if catalog.migration_lock {
        exec_static(&mut tx, ROW_LOCK_SQL).await
    } else {
        Ok(())
    };
    if let Err(err) = row_locked {
        return fail(tx, err).await;
    }
    let rows = if catalog.migrations {
        match read_rows(&mut tx).await {
            Ok(rows) => rows,
            Err(err) => return fail(tx, err).await,
        }
    } else {
        Vec::new()
    };
    if let Some(hit) = checksum_mismatch(&rows, &digests) {
        let raised = exec_static(&mut tx, RAISE_OO001).await;
        let _ = tx.rollback().await;
        return match raised {
            Err(InstallError::Database { sqlstate, .. }) if sqlstate == "OO001" => {
                Err(InstallError::ChecksumMismatch {
                    version: hit.version,
                    stored: hit.stored,
                })
            }
            Err(err) => Err(err),
            Ok(()) => Err(InstallError::Refused {
                message: "checksum mismatch did not raise OO001".to_string(),
            }),
        };
    }

    let route = match route_for(&catalog, &rows, &digests) {
        Ok(route) => route,
        Err(err) => return fail(tx, err).await,
    };
    let outcome = match apply_route(&mut tx, route, &digests).await {
        Ok(outcome) => outcome,
        Err(err) => return fail(tx, err).await,
    };
    tx.commit().await.map_err(database_error)?;
    Ok(outcome)
}

/// Compare stored rows with the compiled digests.
///
/// This does not bootstrap, `SET ROLE`, take the migration lock, or execute
/// a migration script. A caller hash is not an argument.
pub async fn verify_readiness(
    conn: &mut PgConnection,
) -> Result<VerifiedReadiness, ReadinessError> {
    let mut tx = conn.begin().await.map_err(readiness_database)?;
    if let Err(err) = exec_static(&mut tx, "SET TRANSACTION READ ONLY").await {
        let _ = tx.rollback().await;
        return Err(readiness_from_install(err));
    }
    let catalog = match read_catalog(&mut tx).await {
        Ok(catalog) => catalog,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(readiness_from_install(err));
        }
    };
    if !catalog.migrations {
        let _ = tx.rollback().await;
        return Err(ReadinessError::NotInstalled);
    }
    let rows = match read_rows(&mut tx).await {
        Ok(rows) => rows,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(readiness_from_install(err));
        }
    };
    tx.commit().await.map_err(readiness_database)?;

    let digests = compiled_digests();
    if let Some(hit) = checksum_mismatch(&rows, &digests) {
        return Err(ReadinessError::ChecksumMismatch {
            version: hit.version,
            stored: hit.stored,
        });
    }
    if !production_exact(&rows, &digests) {
        return Err(ReadinessError::Incomplete);
    }
    let retains_historical_v2 = match find_row(&rows, VERSION_SEGMENT_V2) {
        Some(row) => row_exact(
            row,
            VERSION_SEGMENT_V2,
            HISTORICAL_V2_SHA256,
            FILE_SEGMENT_V2,
        ),
        None => false,
    };
    Ok(VerifiedReadiness {
        digests,
        retains_historical_v2,
    })
}

/// Install `0005` after the production rows are exact.
///
/// This does not call [`install`] and does not change `0000` through `0004`.
/// A stored `full-import/v1` checksum that differs raises `OO001` and does not
/// update the row. An exact row is a replay.
pub async fn install_full_import(
    conn: &mut PgConnection,
) -> Result<FullImportInstallOutcome, InstallError> {
    let digest = compiled_full_import_digest();
    let digests = compiled_digests();
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        return rollback_full(tx, err).await;
    }
    let catalog = match read_catalog(&mut tx).await {
        Ok(catalog) => catalog,
        Err(err) => return rollback_full(tx, err).await,
    };
    let rows = if catalog.migrations {
        match read_rows(&mut tx).await {
            Ok(rows) => rows,
            Err(err) => return rollback_full(tx, err).await,
        }
    } else {
        Vec::new()
    };
    if let Some(hit) = checksum_mismatch(&rows, &digests) {
        return mismatch(tx, hit).await;
    }
    if let Some(row) = find_row(&rows, VERSION_FULL_IMPORT) {
        if !row_exact(row, VERSION_FULL_IMPORT, &digest, FILE_FULL_IMPORT) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_FULL_IMPORT.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await;
        }
        if let Err(err) = grant_caller_backend_visibility(&mut tx, principal.superuser).await {
            return rollback_full(tx, err).await;
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(FullImportInstallOutcome::Replay);
    }
    if !production_exact(&rows, &digests) {
        return rollback_full(
            tx,
            InstallError::Refused {
                message: "full import requires the production target schema".to_string(),
            },
        )
        .await;
    }
    if let Err(err) = grant_caller_backend_visibility(&mut tx, principal.superuser).await {
        return rollback_full(tx, err).await;
    }
    if let Err(err) = exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await {
        return rollback_full(tx, err).await;
    }
    if let Err(err) = exec_script(&mut tx, SCRIPT_FULL_IMPORT).await {
        return rollback_full(tx, err).await;
    }
    if let Err(err) = insert_markers(
        &mut tx,
        &[marker(VERSION_FULL_IMPORT, &digest, FILE_FULL_IMPORT)],
    )
    .await
    {
        return rollback_full(tx, err).await;
    }
    tx.commit().await.map_err(database_error)?;
    Ok(FullImportInstallOutcome::Installed)
}

/// Compare the production rows and the `full-import/v1` row with the compiled files.
///
/// This does not install, `SET ROLE`, or take the migration lock.
pub async fn verify_full_import_readiness(
    conn: &mut PgConnection,
) -> Result<VerifiedFullImport, ReadinessError> {
    let mut tx = conn.begin().await.map_err(readiness_database)?;
    if let Err(err) = exec_static(&mut tx, "SET TRANSACTION READ ONLY").await {
        let _ = tx.rollback().await;
        return Err(readiness_from_install(err));
    }
    let catalog = match read_catalog(&mut tx).await {
        Ok(catalog) => catalog,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(readiness_from_install(err));
        }
    };
    if !catalog.migrations {
        let _ = tx.rollback().await;
        return Err(ReadinessError::NotInstalled);
    }
    let rows = match read_rows(&mut tx).await {
        Ok(rows) => rows,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(readiness_from_install(err));
        }
    };
    tx.commit().await.map_err(readiness_database)?;
    let digests = compiled_digests();
    if let Some(hit) = checksum_mismatch(&rows, &digests) {
        return Err(ReadinessError::ChecksumMismatch {
            version: hit.version,
            stored: hit.stored,
        });
    }
    if !production_exact(&rows, &digests) {
        return Err(ReadinessError::Incomplete);
    }
    let digest = compiled_full_import_digest();
    match find_row(&rows, VERSION_FULL_IMPORT) {
        Some(row) if row_exact(row, VERSION_FULL_IMPORT, &digest, FILE_FULL_IMPORT) => {
            Ok(VerifiedFullImport { digest })
        }
        Some(row) => Err(ReadinessError::ChecksumMismatch {
            version: row.version.clone(),
            stored: row.checksum.clone(),
        }),
        None => Err(ReadinessError::Incomplete),
    }
}

async fn prepare_locked(
    conn: &mut sqlx::PgConnection,
    principal: &Principal,
) -> Result<(), InstallError> {
    exec_static(conn, "SET LOCAL lock_timeout = '15s'").await?;
    exec_static(conn, "SET LOCAL statement_timeout = '60s'").await?;
    if !principal.superuser {
        exec_static(conn, "SET LOCAL ROLE oo_target_owner").await?;
    }
    exec_static(conn, LOCK_SQL).await?;
    let catalog = read_catalog(conn).await?;
    if catalog.migration_lock {
        exec_static(conn, ROW_LOCK_SQL).await?;
    }
    Ok(())
}

async fn grant_caller_backend_visibility(
    conn: &mut PgConnection,
    superuser: bool,
) -> Result<(), InstallError> {
    if superuser {
        return exec_static(
            conn,
            "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE",
        )
        .await;
    }
    let visible: bool = sqlx::query_scalar("SELECT pg_catalog.pg_has_role('pg_read_all_stats', 'member')")
        .fetch_one(&mut *conn)
        .await
        .map_err(database_error)?;
    if visible {
        Ok(())
    } else {
        Err(InstallError::Refused {
            message: "oo_target_owner cannot read the caller backend identity".to_string(),
        })
    }
}

async fn rollback_full(
    tx: sqlx::Transaction<'_, sqlx::Postgres>,
    err: InstallError,
) -> Result<FullImportInstallOutcome, InstallError> {
    let _ = tx.rollback().await;
    Err(err)
}

async fn mismatch(
    mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
    hit: Mismatch,
) -> Result<FullImportInstallOutcome, InstallError> {
    let raised = exec_static(&mut tx, RAISE_OO001).await;
    let _ = tx.rollback().await;
    match raised {
        Err(InstallError::Database { sqlstate, .. }) if sqlstate == "OO001" => {
            Err(InstallError::ChecksumMismatch {
                version: hit.version,
                stored: hit.stored,
            })
        }
        Err(err) => Err(err),
        Ok(()) => Err(InstallError::Refused {
            message: "checksum mismatch did not raise OO001".to_string(),
        }),
    }
}

#[derive(Clone, Copy)]
struct Principal {
    superuser: bool,
    owner_member: bool,
}

struct Catalog {
    schema: bool,
    migrations: bool,
    migration_lock: bool,
    receipts: bool,
    constraint: bool,
    owner_role: bool,
    runtime_role: bool,
}

#[derive(Clone)]
struct StoredRow {
    version: String,
    checksum: String,
    phase: String,
    filename: String,
}

struct Mismatch {
    version: String,
    stored: String,
}

#[derive(Clone, Copy)]
enum Route {
    Bootstrap,
    UpgradeFromV1,
    UpgradeFromV2,
    InstallMapping,
    Replay,
    AbsentRelation,
}

fn principal_can_install(principal: &Principal, catalog: &Catalog) -> bool {
    if fresh_catalog(catalog) {
        return principal.superuser;
    }
    principal.superuser || principal.owner_member
}

fn fresh_catalog(catalog: &Catalog) -> bool {
    !catalog.schema
        && !catalog.migrations
        && !catalog.migration_lock
        && !catalog.receipts
        && !catalog.constraint
        && !catalog.owner_role
        && !catalog.runtime_role
}

fn route_for(
    catalog: &Catalog,
    rows: &[StoredRow],
    digests: &CompiledDigests,
) -> Result<Route, InstallError> {
    if fresh_catalog(catalog) && rows.is_empty() {
        return Ok(Route::Bootstrap);
    }
    if !catalog.receipts || !catalog.constraint {
        return Ok(Route::AbsentRelation);
    }
    if production_exact(rows, digests) && historical_v2_acceptable(rows) {
        return Ok(Route::Replay);
    }
    if v1_base(rows, digests)
        && find_row(rows, VERSION_PREFLIGHT).is_none()
        && find_row(rows, VERSION_MAPPING).is_none()
    {
        return if find_row(rows, VERSION_SEGMENT_V2).is_none() {
            Ok(Route::UpgradeFromV1)
        } else {
            Ok(Route::UpgradeFromV2)
        };
    }
    if v1_base(rows, digests)
        && matches!(
            find_row(rows, VERSION_PREFLIGHT),
            Some(row) if row_exact(row, VERSION_PREFLIGHT, digests.preflight(), FILE_PREFLIGHT)
        )
        && find_row(rows, VERSION_MAPPING).is_none()
    {
        return Ok(Route::InstallMapping);
    }
    Err(InstallError::Refused {
        message: "target catalog is not a fresh install or a forward migration".to_string(),
    })
}

fn v1_base(rows: &[StoredRow], digests: &CompiledDigests) -> bool {
    matches!(find_row(rows, VERSION_BOOTSTRAP), Some(row) if row_exact(row, VERSION_BOOTSTRAP, digests.bootstrap(), FILE_BOOTSTRAP))
        && matches!(find_row(rows, VERSION_FENCE), Some(row) if row_exact(row, VERSION_FENCE, digests.fence(), FILE_FENCE))
        && historical_v2_acceptable(rows)
}

fn production_exact(rows: &[StoredRow], digests: &CompiledDigests) -> bool {
    matches!(find_row(rows, VERSION_BOOTSTRAP), Some(row) if row_exact(row, VERSION_BOOTSTRAP, digests.bootstrap(), FILE_BOOTSTRAP))
        && matches!(find_row(rows, VERSION_FENCE), Some(row) if row_exact(row, VERSION_FENCE, digests.fence(), FILE_FENCE))
        && matches!(find_row(rows, VERSION_PREFLIGHT), Some(row) if row_exact(row, VERSION_PREFLIGHT, digests.preflight(), FILE_PREFLIGHT))
        && matches!(find_row(rows, VERSION_MAPPING), Some(row) if row_exact(row, VERSION_MAPPING, digests.mapping(), FILE_MAPPING))
}

fn historical_v2_acceptable(rows: &[StoredRow]) -> bool {
    match find_row(rows, VERSION_SEGMENT_V2) {
        None => true,
        Some(row) => row_exact(row, VERSION_SEGMENT_V2, HISTORICAL_V2_SHA256, FILE_SEGMENT_V2),
    }
}

fn row_exact(row: &StoredRow, version: &str, checksum: &str, filename: &str) -> bool {
    row.version == version
        && row.checksum == checksum
        && row.phase == "expand"
        && row.filename == filename
}

fn checksum_mismatch(rows: &[StoredRow], digests: &CompiledDigests) -> Option<Mismatch> {
    for row in rows {
        let expected = match row.version.as_str() {
            VERSION_BOOTSTRAP => Some(digests.bootstrap()),
            VERSION_FENCE => Some(digests.fence()),
            VERSION_SEGMENT_V2 => Some(HISTORICAL_V2_SHA256),
            VERSION_PREFLIGHT => Some(digests.preflight()),
            VERSION_MAPPING => Some(digests.mapping()),
            _ => None,
        };
        if let Some(expected) = expected
            && row.checksum != expected
        {
            return Some(Mismatch {
                version: row.version.clone(),
                stored: row.checksum.clone(),
            });
        }
    }
    None
}

fn find_row<'a>(rows: &'a [StoredRow], version: &str) -> Option<&'a StoredRow> {
    rows.iter().find(|row| row.version == version)
}

async fn apply_route(
    conn: &mut PgConnection,
    route: Route,
    digests: &CompiledDigests,
) -> Result<InstallOutcome, InstallError> {
    match route {
        Route::Replay => Ok(InstallOutcome::Replay),
        Route::Bootstrap => {
            exec_script(conn, SCRIPT_BOOTSTRAP).await?;
            exec_static(conn, "SET LOCAL ROLE oo_target_owner").await?;
            exec_script(conn, SCRIPT_FENCE).await?;
            exec_static(conn, ROW_LOCK_SQL).await?;
            exec_script(conn, SCRIPT_PREFLIGHT).await?;
            exec_script(conn, SCRIPT_MAPPING).await?;
            insert_markers(
                conn,
                &[
                    marker(VERSION_BOOTSTRAP, digests.bootstrap(), FILE_BOOTSTRAP),
                    marker(VERSION_FENCE, digests.fence(), FILE_FENCE),
                    marker(VERSION_PREFLIGHT, digests.preflight(), FILE_PREFLIGHT),
                    marker(VERSION_MAPPING, digests.mapping(), FILE_MAPPING),
                ],
            )
            .await?;
            Ok(InstallOutcome::Bootstrapped)
        }
        Route::UpgradeFromV1 | Route::UpgradeFromV2 => {
            exec_static(conn, "SET LOCAL ROLE oo_target_owner").await?;
            exec_script(conn, SCRIPT_PREFLIGHT).await?;
            exec_script(conn, SCRIPT_MAPPING).await?;
            insert_markers(
                conn,
                &[
                    marker(VERSION_PREFLIGHT, digests.preflight(), FILE_PREFLIGHT),
                    marker(VERSION_MAPPING, digests.mapping(), FILE_MAPPING),
                ],
            )
            .await?;
            Ok(if matches!(route, Route::UpgradeFromV1) {
                InstallOutcome::UpgradedFromV1
            } else {
                InstallOutcome::UpgradedFromV2
            })
        }
        Route::InstallMapping => {
            exec_static(conn, "SET LOCAL ROLE oo_target_owner").await?;
            exec_script(conn, SCRIPT_MAPPING).await?;
            insert_markers(
                conn,
                &[marker(VERSION_MAPPING, digests.mapping(), FILE_MAPPING)],
            )
            .await?;
            Ok(InstallOutcome::InstalledMapping)
        }
        Route::AbsentRelation => {
            exec_static(conn, "SET LOCAL ROLE oo_target_owner").await?;
            match exec_script(conn, SCRIPT_PREFLIGHT).await {
                Ok(()) => Err(InstallError::Refused {
                    message: "preflight returned while the segment relation was absent".to_string(),
                }),
                Err(InstallError::Database { sqlstate, message })
                    if sqlstate == "OO002" && message.contains(ABSENT_MESSAGE) =>
                {
                    Err(InstallError::MissingRelation { message })
                }
                Err(err) => Err(err),
            }
        }
    }
}

struct Marker {
    version: &'static str,
    checksum: String,
    filename: &'static str,
}

fn marker(version: &'static str, checksum: &str, filename: &'static str) -> Marker {
    Marker {
        version,
        checksum: checksum.to_string(),
        filename,
    }
}

async fn insert_markers(conn: &mut PgConnection, markers: &[Marker]) -> Result<(), InstallError> {
    for marker in markers {
        if !registration_closed(marker) {
            return Err(InstallError::Refused {
                message: "migration registration bytes are not the compiled scripts".to_string(),
            });
        }
    }
    let mut values = String::new();
    for (index, marker) in markers.iter().enumerate() {
        if index > 0 {
            values.push_str(",\n");
        }
        values.push_str("  ('");
        values.push_str(marker.version);
        values.push_str("', '");
        values.push_str(&marker.checksum);
        values.push_str("', 'expand', '");
        values.push_str(marker.filename);
        values.push_str("')");
    }
    let sql = format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values}"
    );
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(database_error)
}

fn registration_closed(marker: &Marker) -> bool {
    token_closed(marker.version)
        && marker.checksum.len() == 64
        && marker.checksum.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        && token_closed(marker.filename)
        && marker.checksum != HISTORICAL_V2_SHA256
        && marker.version != VERSION_SEGMENT_V2
}

fn token_closed(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '.' | '-'))
}

async fn fail(
    tx: sqlx::Transaction<'_, sqlx::Postgres>,
    err: InstallError,
) -> Result<InstallOutcome, InstallError> {
    let _ = tx.rollback().await;
    Err(err)
}

async fn read_principal(conn: &mut PgConnection) -> Result<Principal, InstallError> {
    let superuser: bool = sqlx::query_scalar(
        "SELECT rolsuper FROM pg_catalog.pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(database_error)?;
    let owner_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_target_owner')",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(database_error)?;
    let owner_member = if owner_exists {
        sqlx::query_scalar(
            "SELECT pg_catalog.pg_has_role(current_user, 'oo_target_owner', 'MEMBER')",
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(database_error)?
    } else {
        false
    };
    Ok(Principal {
        superuser,
        owner_member,
    })
}

async fn read_catalog(conn: &mut PgConnection) -> Result<Catalog, InstallError> {
    let schema = scalar_bool(
        conn,
        "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_namespace WHERE nspname = '_oo')",
    )
    .await?;
    let migrations = scalar_bool(
        conn,
        "SELECT pg_catalog.to_regclass('_oo.schema_migrations') IS NOT NULL",
    )
    .await?;
    let migration_lock = scalar_bool(
        conn,
        "SELECT pg_catalog.to_regclass('_oo.migration_lock') IS NOT NULL",
    )
    .await?;
    let receipts = scalar_bool(
        conn,
        "SELECT pg_catalog.to_regclass('_oo.apply_receipts') IS NOT NULL",
    )
    .await?;
    let constraint = scalar_bool(
        conn,
        "SELECT EXISTS (\
            SELECT 1 \
            FROM pg_catalog.pg_constraint AS constraint_row \
            JOIN pg_catalog.pg_class AS relation ON relation.oid = constraint_row.conrelid \
            JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
            WHERE namespace.nspname = '_oo' \
              AND relation.relname = 'apply_receipts' \
              AND constraint_row.conname = 'apply_receipts_segment_check'\
        )",
    )
    .await?;
    let owner_role = scalar_bool(
        conn,
        "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_target_owner')",
    )
    .await?;
    let runtime_role = scalar_bool(
        conn,
        "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_pipeline_runtime')",
    )
    .await?;
    Ok(Catalog {
        schema,
        migrations,
        migration_lock,
        receipts,
        constraint,
        owner_role,
        runtime_role,
    })
}

async fn scalar_bool(conn: &mut PgConnection, sql: &'static str) -> Result<bool, InstallError> {
    sqlx::query_scalar(sql)
        .fetch_one(&mut *conn)
        .await
        .map_err(database_error)
}

async fn read_rows(conn: &mut PgConnection) -> Result<Vec<StoredRow>, InstallError> {
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT version, checksum, phase, filename \
         FROM _oo.schema_migrations \
         ORDER BY version",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(database_error)?;
    Ok(rows
        .into_iter()
        .map(|(version, checksum, phase, filename)| StoredRow {
            version,
            checksum,
            phase,
            filename,
        })
        .collect())
}

async fn exec_static(conn: &mut PgConnection, sql: &'static str) -> Result<(), InstallError> {
    sqlx::raw_sql(sql)
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(database_error)
}

async fn exec_script(conn: &mut PgConnection, sql: &'static str) -> Result<(), InstallError> {
    exec_static(conn, sql).await
}

fn database_error(err: sqlx::Error) -> InstallError {
    let sqlstate = err
        .as_database_error()
        .and_then(|db| db.code().map(|code| code.into_owned()))
        .unwrap_or_default();
    let message = err
        .as_database_error()
        .map(|db| db.message().to_string())
        .unwrap_or_else(|| err.to_string());
    InstallError::Database { sqlstate, message }
}

fn readiness_database(err: sqlx::Error) -> ReadinessError {
    readiness_from_install(database_error(err))
}

fn readiness_from_install(err: InstallError) -> ReadinessError {
    match err {
        InstallError::Database { sqlstate, message } => ReadinessError::Database { sqlstate, message },
        InstallError::ChecksumMismatch { version, stored } => {
            ReadinessError::ChecksumMismatch { version, stored }
        }
        InstallError::NotAdministrator => ReadinessError::Database {
            sqlstate: String::new(),
            message: "target install requires the administrator connection".to_string(),
        },
        InstallError::MissingRelation { message } | InstallError::Refused { message } => {
            ReadinessError::Database {
                sqlstate: String::new(),
                message,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_digests_match_the_frozen_files() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let digests = compiled_digests();
        assert_eq!(
            digests.bootstrap(),
            "37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208"
        );
        assert_eq!(
            digests.fence(),
            "97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9"
        );
        assert_eq!(
            digests.preflight(),
            "d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3"
        );
        assert_eq!(
            digests.mapping(),
            "844de6f645f8f1ebf29d05118ae25d4669d8dc870eed1fa7697d4b98456eaf95"
        );
        assert_eq!(MIGRATION_LOCK_KEY, 6_404_202_605);
        assert!(LOCK_SQL.contains("6404202605"));
        assert!(!SCRIPT_PREFLIGHT.contains("::regclass"));
        assert_eq!(SCRIPT_PREFLIGHT.matches(ABSENT_MESSAGE).count(), 3);
        assert!(!SCRIPT_BOOTSTRAP.contains("GRANT oo_target_owner TO oo_pipeline_runtime"));
        assert!(!SCRIPT_MAPPING.contains("COPY "));
    }

    #[test]
    fn full_import_digest_matches_the_contract_file() {
        let digest = compiled_full_import_digest();
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_ne!(digest, compiled_digests().mapping());
        assert!(!SCRIPT_FULL_IMPORT.contains("COPY "));
        let contract = include_str!("../../../contracts/pipeline/full-import-v1.json");
        assert!(contract.contains(&digest));
        assert!(!contract.contains("37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208"));
    }

    #[test]
    fn routes_do_not_execute_v2_and_do_not_repair() {
        let digests = compiled_digests();
        let empty = Catalog {
            schema: false,
            migrations: false,
            migration_lock: false,
            receipts: false,
            constraint: false,
            owner_role: false,
            runtime_role: false,
        };
        assert!(matches!(
            route_for(&empty, &[], &digests),
            Ok(Route::Bootstrap)
        ));
        let present = Catalog {
            schema: true,
            migrations: true,
            migration_lock: true,
            receipts: true,
            constraint: true,
            owner_role: true,
            runtime_role: true,
        };
        let v1 = vec![
            stored(VERSION_BOOTSTRAP, digests.bootstrap(), FILE_BOOTSTRAP),
            stored(VERSION_FENCE, digests.fence(), FILE_FENCE),
        ];
        assert!(matches!(
            route_for(&present, &v1, &digests),
            Ok(Route::UpgradeFromV1)
        ));
        let mut v2 = v1.clone();
        v2.push(stored(VERSION_SEGMENT_V2, HISTORICAL_V2_SHA256, FILE_SEGMENT_V2));
        assert!(matches!(
            route_for(&present, &v2, &digests),
            Ok(Route::UpgradeFromV2)
        ));
        let mut ready = v2.clone();
        ready.push(stored(VERSION_PREFLIGHT, digests.preflight(), FILE_PREFLIGHT));
        ready.push(stored(VERSION_MAPPING, digests.mapping(), FILE_MAPPING));
        assert!(matches!(
            route_for(&present, &ready, &digests),
            Ok(Route::Replay)
        ));
        let mut direct = v1.clone();
        direct.push(stored(VERSION_PREFLIGHT, digests.preflight(), FILE_PREFLIGHT));
        direct.push(stored(VERSION_MAPPING, digests.mapping(), FILE_MAPPING));
        assert!(matches!(
            route_for(&present, &direct, &digests),
            Ok(Route::Replay)
        ));
        let dropped = Catalog {
            constraint: false,
            ..present
        };
        assert!(matches!(
            route_for(&dropped, &v1, &digests),
            Ok(Route::AbsentRelation)
        ));
        let lie = vec![
            stored(VERSION_BOOTSTRAP, digests.bootstrap(), FILE_BOOTSTRAP),
            stored(VERSION_FENCE, "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", FILE_FENCE),
        ];
        let hit = checksum_mismatch(&lie, &digests).expect("mismatch");
        assert_eq!(hit.version, VERSION_FENCE);
        assert!(checksum_mismatch(&v1, &digests).is_none());
        assert!(!fresh_catalog(&present));
        assert!(!principal_can_install(
            &Principal {
                superuser: false,
                owner_member: true,
            },
            &empty
        ));
        assert!(principal_can_install(
            &Principal {
                superuser: true,
                owner_member: false,
            },
            &empty
        ));
    }

    fn stored(version: &str, checksum: &str, filename: &str) -> StoredRow {
        StoredRow {
            version: version.to_string(),
            checksum: checksum.to_string(),
            phase: "expand".to_string(),
            filename: filename.to_string(),
        }
    }
}
