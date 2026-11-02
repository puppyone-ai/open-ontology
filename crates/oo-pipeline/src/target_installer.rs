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
use sqlx::Connection;
use sqlx::postgres::PgConnection;

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

/// Sealed-import catalog version stored for `0006_sealed_import_catalog.sql`.
///
/// `install` and [`install_full_import`] do not apply this version.
/// [`install_sealed_catalog`] does. The read command does not install it.
pub const VERSION_SEALED_CATALOG: &str = "sealed-import-catalog/v1";

/// Native sealed-catalog version stored for `0008_native_sealed_catalog.sql`.
///
/// [`install_sealed_catalog`] does not apply this version.
/// [`install_native_sealed_catalog`] does, and only after `0006` is exact.
pub const VERSION_NATIVE_CATALOG: &str = "native-sealed-catalog/v1";

/// jsonb document type SQL stored for `0009_full_import_jsonb.sql`.
///
/// [`install_full_import`] does not apply this version. [`install_jsonb_document`]
/// does, and only after `full-import/v1` is exact. oid 114 stays `json`.
pub const VERSION_FULL_IMPORT_JSONB: &str = "full-import-jsonb/v1";

/// Native update version stored for `0010_native_update_expand.sql`.
///
/// [`install_jsonb_document`] does not apply this version. [`install_native_update`]
/// does, and only after `full-import/v1` is exact. It does not rewrite `0000` through `0009`.
pub const VERSION_NATIVE_UPDATE: &str = "native-update/v1";
pub const VERSION_NATIVE_PARTITION: &str = "native-partition/v1";
pub const VERSION_NATIVE_PARTITION_REBIND: &str = "native-partition-rebind/v1";
pub const VERSION_NATIVE_INTERVAL: &str = "native-interval/v1";
pub const VERSION_NATIVE_OLAP_PARTITION: &str = "native-olap-partition/v1";
pub const VERSION_NATIVE_OLAP_GUARD: &str = "native-olap-guard-precedence/v1";
pub const VERSION_NATIVE_OLAP_GUARD_SIGNATURE: &str = "native-olap-guard-signature/v1";
pub const VERSION_NATIVE_PG_TRANSACTION_GROUP: &str = "native-pg-transaction-group/v1";
pub const VERSION_NATIVE_EMPTY_CATALOG: &str = "native-empty-sealed-catalog/v1";
pub const VERSION_NATIVE_PG_GROUP_RECEIPT: &str = "native-pg-group-receipt/v1";
pub const VERSION_NATIVE_PG_GROUP_PRIOR: &str = "native-pg-group-prior/v1";
pub const VERSION_NATIVE_PG_GROUP_GENERATION: &str = "native-pg-group-generation/v1";
pub const VERSION_NATIVE_SOURCE_PROGRESS_READ: &str = "native-source-progress-read/v1";
pub const VERSION_NATIVE_PARTITION_OBSERVATION_READ: &str = "native-partition-observation-read/v1";
pub const VERSION_NATIVE_SQLSERVER_CT: &str = "native-sqlserver-ct/v1";
pub const VERSION_NATIVE_GTID_TRANSACTION: &str = "native-gtid-transaction/v1";
pub const VERSION_NATIVE_C1_SOURCE_EPOCH: &str = "native-c1-source-epoch/v1";
pub const VERSION_NATIVE_C1_SOURCE_EPOCH_RUNTIME: &str = "native-c1-source-epoch-runtime/v1";
pub const VERSION_NATIVE_PUBLICATION: &str = "native-publication/v1";
pub const VERSION_NATIVE_C1_BASELINE_STAGE: &str = "native-c1-baseline-stage/v1";
pub const VERSION_NATIVE_C1_GTID_STAGE_IDENTITY: &str = "native-c1-gtid-stage-identity/v1";
pub const VERSION_NATIVE_C1_GTID_MEASURED_IDENTITY: &str = "native-c1-gtid-measured-identity/v1";
pub const VERSION_NATIVE_PG_SINGLE_TRANSACTION: &str = "native-pg-single-transaction/v1";

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
const FILE_SEALED_CATALOG: &str = "0006_sealed_import_catalog.sql";
const FILE_NATIVE_CATALOG: &str = "0008_native_sealed_catalog.sql";
const FILE_FULL_IMPORT_JSONB: &str = "0009_full_import_jsonb.sql";
const FILE_NATIVE_UPDATE: &str = "0010_native_update_expand.sql";
const FILE_NATIVE_PARTITION: &str = "0011_native_partition.sql";
const FILE_NATIVE_PARTITION_REBIND: &str = "0012_native_partition_rebind.sql";
const FILE_NATIVE_INTERVAL: &str = "0013_native_interval.sql";
const FILE_NATIVE_OLAP_PARTITION: &str = "0014_native_olap_partition.sql";
const FILE_NATIVE_OLAP_GUARD: &str = "0015_native_olap_guard_precedence.sql";
const FILE_NATIVE_OLAP_GUARD_SIGNATURE: &str = "0016_native_olap_guard_signature.sql";
const FILE_NATIVE_PG_TRANSACTION_GROUP: &str = "0017_native_pg_transaction_group.sql";
const FILE_NATIVE_EMPTY_CATALOG: &str = "0018_native_empty_sealed_catalog.sql";
const FILE_NATIVE_PG_GROUP_RECEIPT: &str = "0019_native_pg_group_receipt.sql";
const FILE_NATIVE_PG_GROUP_PRIOR: &str = "0020_native_pg_group_prior.sql";
const FILE_NATIVE_PG_GROUP_GENERATION: &str = "0021_native_pg_group_generation.sql";
const FILE_NATIVE_SQLSERVER_CT: &str = "0023_native_sqlserver_ct.sql";
const FILE_NATIVE_GTID_TRANSACTION: &str = "0024_native_gtid_transaction.sql";
const FILE_NATIVE_C1_SOURCE_EPOCH: &str = "0025_native_c1_source_epoch.sql";
const FILE_NATIVE_C1_SOURCE_EPOCH_RUNTIME: &str = "0026_native_c1_runtime_privileges.sql";
const FILE_NATIVE_PUBLICATION: &str = "0027_native_publication.sql";
const FILE_NATIVE_C1_BASELINE_STAGE: &str = "0028_native_c1_baseline_stage.sql";
const FILE_NATIVE_C1_GTID_STAGE_IDENTITY: &str = "0029_native_c1_gtid_stage_identity.sql";
const FILE_NATIVE_C1_GTID_MEASURED_IDENTITY: &str = "0030_native_c1_gtid_measured_identity.sql";
const FILE_NATIVE_PG_SINGLE_TRANSACTION: &str = "0022_native_pg_single_transaction.sql";
const FILE_JOBS_MIRROR_REBIND: &str = "0005_mirror_rebind_expand.sql";
const FILE_JOBS_MIRROR_RETRY: &str = "0006_mirror_retry_run_at_expand.sql";
const FILE_JOBS_MIRROR_SYNC: &str = "0007_verified_mirror_sync_expand.sql";
const FILE_JOBS_MIRROR_READ: &str = "0008_native_mirror_read_grants_expand.sql";
const FILE_JOBS_RECEIPT_READ: &str = "0010_native_receipt_read_grants_expand.sql";
const FILE_JOBS_CLAIM_SELECTED: &str = "0009_claim_selected_expand.sql";

const SCRIPT_BOOTSTRAP: &str = include_str!("../migrations/0000_bootstrap_roles.sql");
const SCRIPT_FENCE: &str = include_str!("../migrations/0001_target_fence_expand.sql");
const SCRIPT_PREFLIGHT: &str =
    include_str!("../migrations/0003_target_segment_preflight_expand.sql");
const SCRIPT_MAPPING: &str = include_str!("../migrations/0004_physical_mapping.sql");
const SCRIPT_FULL_IMPORT: &str = include_str!("../migrations/0005_full_import_expand.sql");
const SCRIPT_SEALED_CATALOG: &str = include_str!("../migrations/0006_sealed_import_catalog.sql");
const SCRIPT_NATIVE_CATALOG: &str = include_str!("../migrations/0008_native_sealed_catalog.sql");
const SCRIPT_FULL_IMPORT_JSONB: &str = include_str!("../migrations/0009_full_import_jsonb.sql");
const SCRIPT_NATIVE_UPDATE: &str = include_str!("../migrations/0010_native_update_expand.sql");
const SCRIPT_NATIVE_PARTITION: &str = include_str!("../migrations/0011_native_partition.sql");
const SCRIPT_NATIVE_PARTITION_REBIND: &str =
    include_str!("../migrations/0012_native_partition_rebind.sql");
const SCRIPT_NATIVE_INTERVAL: &str = include_str!("../migrations/0013_native_interval.sql");
const SCRIPT_NATIVE_OLAP_PARTITION: &str =
    include_str!("../migrations/0014_native_olap_partition.sql");
const SCRIPT_NATIVE_OLAP_GUARD: &str =
    include_str!("../migrations/0015_native_olap_guard_precedence.sql");
const SCRIPT_NATIVE_OLAP_GUARD_SIGNATURE: &str =
    include_str!("../migrations/0016_native_olap_guard_signature.sql");
const SCRIPT_NATIVE_PG_TRANSACTION_GROUP: &str =
    include_str!("../migrations/0017_native_pg_transaction_group.sql");
const SCRIPT_NATIVE_EMPTY_CATALOG: &str =
    include_str!("../migrations/0018_native_empty_sealed_catalog.sql");
const SCRIPT_NATIVE_PG_GROUP_RECEIPT: &str =
    include_str!("../migrations/0019_native_pg_group_receipt.sql");
const SCRIPT_NATIVE_PG_GROUP_PRIOR: &str =
    include_str!("../migrations/0020_native_pg_group_prior.sql");
const SCRIPT_NATIVE_PG_GROUP_GENERATION: &str =
    include_str!("../migrations/0021_native_pg_group_generation.sql");
const SCRIPT_NATIVE_SQLSERVER_CT: &str = include_str!("../migrations/0023_native_sqlserver_ct.sql");
const SCRIPT_NATIVE_GTID_TRANSACTION: &str =
    include_str!("../migrations/0024_native_gtid_transaction.sql");
const SCRIPT_NATIVE_C1_SOURCE_EPOCH: &str =
    include_str!("../migrations/0025_native_c1_source_epoch.sql");
const SCRIPT_NATIVE_C1_SOURCE_EPOCH_RUNTIME: &str =
    include_str!("../migrations/0026_native_c1_runtime_privileges.sql");
const SCRIPT_NATIVE_PUBLICATION: &str = include_str!("../migrations/0027_native_publication.sql");
const SCRIPT_NATIVE_C1_BASELINE_STAGE: &str =
    include_str!("../migrations/0028_native_c1_baseline_stage.sql");
const SCRIPT_NATIVE_C1_GTID_STAGE_IDENTITY: &str =
    include_str!("../migrations/0029_native_c1_gtid_stage_identity.sql");
const SCRIPT_NATIVE_C1_GTID_MEASURED_IDENTITY: &str =
    include_str!("../migrations/0030_native_c1_gtid_measured_identity.sql");
const SCRIPT_NATIVE_PG_SINGLE_TRANSACTION: &str =
    include_str!("../migrations/0022_native_pg_single_transaction.sql");
const SCRIPT_JOBS_MIRROR_REBIND: &str =
    include_str!("../../oo-jobs/migrations/0005_mirror_rebind_expand.sql");
const SCRIPT_JOBS_MIRROR_RETRY: &str =
    include_str!("../../oo-jobs/migrations/0006_mirror_retry_run_at_expand.sql");
const SCRIPT_JOBS_MIRROR_SYNC: &str =
    include_str!("../../oo-jobs/migrations/0007_verified_mirror_sync_expand.sql");
const SCRIPT_JOBS_MIRROR_READ: &str =
    include_str!("../../oo-jobs/migrations/0008_native_mirror_read_grants_expand.sql");
const SCRIPT_JOBS_RECEIPT_READ: &str =
    include_str!("../../oo-jobs/migrations/0010_native_receipt_read_grants_expand.sql");
const SCRIPT_JOBS_CLAIM_SELECTED: &str =
    include_str!("../../oo-jobs/migrations/0009_claim_selected_expand.sql");

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

/// SHA-256 of `0006_sealed_import_catalog.sql`. This is not part of [`compiled_digests`].
pub fn compiled_sealed_catalog_digest() -> String {
    sha256_hex(SCRIPT_SEALED_CATALOG.as_bytes())
}

/// SHA-256 of `0008_native_sealed_catalog.sql`. This is not part of [`compiled_digests`].
pub fn compiled_native_catalog_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_CATALOG.as_bytes())
}

/// SHA-256 of `0009_full_import_jsonb.sql`. This is not part of [`compiled_digests`].
pub fn compiled_jsonb_document_digest() -> String {
    sha256_hex(SCRIPT_FULL_IMPORT_JSONB.as_bytes())
}

/// SHA-256 of `0010_native_update_expand.sql`. This is not part of [`compiled_digests`].
pub fn compiled_native_update_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_UPDATE.as_bytes())
}

pub fn compiled_native_partition_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_PARTITION.as_bytes())
}

pub fn compiled_native_partition_rebind_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_PARTITION_REBIND.as_bytes())
}

pub fn compiled_native_interval_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_INTERVAL.as_bytes())
}

pub fn compiled_native_olap_partition_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_OLAP_PARTITION.as_bytes())
}

pub fn compiled_native_olap_guard_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_OLAP_GUARD.as_bytes())
}

pub fn compiled_native_olap_guard_signature_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_OLAP_GUARD_SIGNATURE.as_bytes())
}

pub fn compiled_native_pg_transaction_group_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_PG_TRANSACTION_GROUP.as_bytes())
}

pub fn compiled_native_empty_catalog_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_EMPTY_CATALOG.as_bytes())
}

pub fn compiled_native_pg_group_receipt_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_PG_GROUP_RECEIPT.as_bytes())
}

pub fn compiled_native_pg_group_prior_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_PG_GROUP_PRIOR.as_bytes())
}

pub fn compiled_native_pg_group_generation_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_PG_GROUP_GENERATION.as_bytes())
}

pub fn compiled_native_sqlserver_ct_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_SQLSERVER_CT.as_bytes())
}

pub fn compiled_native_gtid_transaction_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_GTID_TRANSACTION.as_bytes())
}
pub fn compiled_native_c1_source_epoch_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C1_SOURCE_EPOCH.as_bytes())
}
pub fn compiled_native_c1_source_epoch_runtime_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C1_SOURCE_EPOCH_RUNTIME.as_bytes())
}
pub fn compiled_native_publication_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_PUBLICATION.as_bytes())
}
pub fn compiled_native_c1_baseline_stage_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C1_BASELINE_STAGE.as_bytes())
}
pub fn compiled_native_c1_gtid_stage_identity_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C1_GTID_STAGE_IDENTITY.as_bytes())
}
pub fn compiled_native_c1_gtid_measured_identity_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C1_GTID_MEASURED_IDENTITY.as_bytes())
}

pub fn compiled_native_pg_single_transaction_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_PG_SINGLE_TRANSACTION.as_bytes())
}

pub fn compiled_jobs_mirror_rebind_digest() -> String {
    sha256_hex(SCRIPT_JOBS_MIRROR_READ.as_bytes())
}

pub fn compiled_jobs_receipt_read_digest() -> String {
    sha256_hex(SCRIPT_JOBS_RECEIPT_READ.as_bytes())
}

pub fn compiled_jobs_claim_selected_digest() -> String {
    sha256_hex(SCRIPT_JOBS_CLAIM_SELECTED.as_bytes())
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
                write!(
                    formatter,
                    "migration checksum mismatch for {version}: {stored}"
                )
            }
            Self::MissingRelation { message } => formatter.write_str(message),
            Self::Refused { message } => formatter.write_str(message),
            Self::Database { sqlstate, message } => {
                write!(
                    formatter,
                    "target install failed sqlstate={sqlstate} {message}"
                )
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
                write!(
                    formatter,
                    "migration checksum mismatch for {version}: {stored}"
                )
            }
            Self::Database { sqlstate, message } => {
                write!(
                    formatter,
                    "target readiness failed sqlstate={sqlstate} {message}"
                )
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

/// What one successful [`install_jsonb_document`] call did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonbDocumentInstallOutcome {
    /// `0009` replaced `full_import_type_sql` and its marker was inserted.
    Installed,
    /// The stored `full-import-jsonb/v1` row already matches. No script ran.
    Replay,
}

/// What one successful [`install_native_update`] call did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeUpdateInstallOutcome {
    /// `0010` created the update tables and its marker was inserted.
    Installed,
    /// The stored `native-update/v1` row already matches. No script ran.
    Replay,
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

/// What one successful [`install_sealed_catalog`] call did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealedCatalogInstallOutcome {
    /// `0006` was applied and its version row was inserted.
    Installed,
    /// The stored `sealed-import-catalog/v1` row already matches.
    Replay,
}

impl fmt::Display for SealedCatalogInstallOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Installed => "installed",
            Self::Replay => "replay",
        })
    }
}

/// What one successful [`install_native_sealed_catalog`] call did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeCatalogInstallOutcome {
    /// `0008` was applied and its version row was inserted.
    Installed,
    /// The stored `native-sealed-catalog/v1` row already matches.
    Replay,
}

impl fmt::Display for NativeCatalogInstallOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Installed => "installed",
            Self::Replay => "replay",
        })
    }
}

/// `0006` readiness. This is not a publication and not a source catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedSealedCatalog {
    digest: String,
}

impl VerifiedSealedCatalog {
    /// Compiled digest of `0006_sealed_import_catalog.sql`.
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

/// `0008` readiness. This is not a publication and not a source catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedNativeCatalog {
    digest: String,
}

impl VerifiedNativeCatalog {
    /// Compiled digest of `0008_native_sealed_catalog.sql`.
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

/// Install `0009` after `full-import/v1` is exact.
///
/// This does not call [`install`] or [`install_full_import`] and does not
/// change `0000` through `0008`. A stored checksum that differs raises `OO001`
/// and does not update the row. An exact row is a replay. The script replaces
/// `_oo.full_import_type_sql` so oid 3802 typmod -1 returns `jsonb` and oid 114
/// stays `json`. It does not copy rows.
pub async fn install_jsonb_document(
    conn: &mut PgConnection,
) -> Result<JsonbDocumentInstallOutcome, InstallError> {
    let digest = compiled_jsonb_document_digest();
    let full_digest = compiled_full_import_digest();
    let digests = compiled_digests();
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        return rollback_jsonb(tx, err).await;
    }
    let catalog = match read_catalog(&mut tx).await {
        Ok(catalog) => catalog,
        Err(err) => return rollback_jsonb(tx, err).await,
    };
    let rows = if catalog.migrations {
        match read_rows(&mut tx).await {
            Ok(rows) => rows,
            Err(err) => return rollback_jsonb(tx, err).await,
        }
    } else {
        Vec::new()
    };
    if let Some(hit) = checksum_mismatch(&rows, &digests) {
        return mismatch_jsonb(tx, hit).await;
    }
    if !production_exact(&rows, &digests) {
        return rollback_jsonb(
            tx,
            InstallError::Refused {
                message: "jsonb document import requires the production target schema".to_string(),
            },
        )
        .await;
    }
    match find_row(&rows, VERSION_FULL_IMPORT) {
        Some(row) if row_exact(row, VERSION_FULL_IMPORT, &full_digest, FILE_FULL_IMPORT) => {}
        Some(row) => {
            return mismatch_jsonb(
                tx,
                Mismatch {
                    version: VERSION_FULL_IMPORT.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await;
        }
        None => {
            return rollback_jsonb(
                tx,
                InstallError::Refused {
                    message: "jsonb document import requires the full import schema".to_string(),
                },
            )
            .await;
        }
    }
    if let Some(row) = find_row(&rows, VERSION_FULL_IMPORT_JSONB) {
        if !row_exact(
            row,
            VERSION_FULL_IMPORT_JSONB,
            &digest,
            FILE_FULL_IMPORT_JSONB,
        ) {
            return mismatch_jsonb(
                tx,
                Mismatch {
                    version: VERSION_FULL_IMPORT_JSONB.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await;
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(JsonbDocumentInstallOutcome::Replay);
    }
    if let Err(err) = exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await {
        return rollback_jsonb(tx, err).await;
    }
    if let Err(err) = exec_script(&mut tx, SCRIPT_FULL_IMPORT_JSONB).await {
        return rollback_jsonb(tx, err).await;
    }
    if let Err(err) = insert_markers(
        &mut tx,
        &[marker(
            VERSION_FULL_IMPORT_JSONB,
            &digest,
            FILE_FULL_IMPORT_JSONB,
        )],
    )
    .await
    {
        return rollback_jsonb(tx, err).await;
    }
    tx.commit().await.map_err(database_error)?;
    Ok(JsonbDocumentInstallOutcome::Installed)
}

/// Install `0010` after `full-import/v1` is exact.
///
/// This does not call [`install`] or [`install_full_import`] and does not
/// change `0000` through `0009`. A stored checksum that differs raises `OO001`
/// and does not update the row. An exact row is a replay. The script does not
/// copy rows and does not create a publication.
pub async fn install_native_update(
    conn: &mut PgConnection,
) -> Result<NativeUpdateInstallOutcome, InstallError> {
    let digest = compiled_native_update_digest();
    let full_digest = compiled_full_import_digest();
    let digests = compiled_digests();
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        return rollback_native_update(tx, err).await;
    }
    let catalog = match read_catalog(&mut tx).await {
        Ok(catalog) => catalog,
        Err(err) => return rollback_native_update(tx, err).await,
    };
    let rows = if catalog.migrations {
        match read_rows(&mut tx).await {
            Ok(rows) => rows,
            Err(err) => return rollback_native_update(tx, err).await,
        }
    } else {
        Vec::new()
    };
    if let Some(hit) = checksum_mismatch(&rows, &digests) {
        return mismatch_native_update(tx, hit).await;
    }
    if !production_exact(&rows, &digests) {
        return rollback_native_update(
            tx,
            InstallError::Refused {
                message: "native update requires the production target schema".to_string(),
            },
        )
        .await;
    }
    match find_row(&rows, VERSION_FULL_IMPORT) {
        Some(row) if row_exact(row, VERSION_FULL_IMPORT, &full_digest, FILE_FULL_IMPORT) => {}
        Some(row) => {
            return mismatch_native_update(
                tx,
                Mismatch {
                    version: VERSION_FULL_IMPORT.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await;
        }
        None => {
            return rollback_native_update(
                tx,
                InstallError::Refused {
                    message: "native update requires the full import schema".to_string(),
                },
            )
            .await;
        }
    }
    if let Some(row) = find_row(&rows, VERSION_NATIVE_UPDATE) {
        if !row_exact(row, VERSION_NATIVE_UPDATE, &digest, FILE_NATIVE_UPDATE) {
            return mismatch_native_update(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_UPDATE.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await;
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(NativeUpdateInstallOutcome::Replay);
    }
    if let Err(err) = exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await {
        return rollback_native_update(tx, err).await;
    }
    if let Err(err) = exec_script(&mut tx, SCRIPT_NATIVE_UPDATE).await {
        return rollback_native_update(tx, err).await;
    }
    if let Err(err) = insert_markers(
        &mut tx,
        &[marker(VERSION_NATIVE_UPDATE, &digest, FILE_NATIVE_UPDATE)],
    )
    .await
    {
        return rollback_native_update(tx, err).await;
    }
    tx.commit().await.map_err(database_error)?;
    Ok(NativeUpdateInstallOutcome::Installed)
}

/// Install the C4 partition state extension after the exact native update migration.
pub async fn install_native_partition(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = match read_rows(&mut tx).await {
        Ok(rows) => rows,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };
    let prior = compiled_native_update_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_UPDATE), Some(row) if row_exact(row, VERSION_NATIVE_UPDATE, &prior, FILE_NATIVE_UPDATE))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "native partition requires exact native update schema".to_string(),
        });
    }
    let digest = compiled_native_partition_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_PARTITION) {
        if !row_exact(
            row,
            VERSION_NATIVE_PARTITION,
            &digest,
            FILE_NATIVE_PARTITION,
        ) {
            match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_PARTITION.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => return Err(err),
                Ok(_) => {
                    return Err(InstallError::Refused {
                        message: "checksum mismatch did not raise OO001".to_string(),
                    });
                }
            }
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_PARTITION).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_PARTITION,
                &digest,
                FILE_NATIVE_PARTITION,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_partition_rebind(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = match read_rows(&mut tx).await {
        Ok(rows) => rows,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };
    let prior = compiled_native_partition_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_PARTITION), Some(row) if row_exact(row, VERSION_NATIVE_PARTITION, &prior, FILE_NATIVE_PARTITION))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "partition rebind requires exact partition schema".to_string(),
        });
    }
    let digest = compiled_native_partition_rebind_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_PARTITION_REBIND) {
        if !row_exact(
            row,
            VERSION_NATIVE_PARTITION_REBIND,
            &digest,
            FILE_NATIVE_PARTITION_REBIND,
        ) {
            match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_PARTITION_REBIND.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => return Err(err),
                Ok(_) => {
                    return Err(InstallError::Refused {
                        message: "checksum mismatch did not raise OO001".to_string(),
                    });
                }
            }
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "GRANT USAGE ON SCHEMA jobs TO oo_target_owner").await?;
        exec_static(&mut tx, "GRANT SELECT ON jobs.tasks TO oo_target_owner").await?;
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_PARTITION_REBIND).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_PARTITION_REBIND,
                &digest,
                FILE_NATIVE_PARTITION_REBIND,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Extend the installed full-import functions with exact OID 1186 interval.
pub async fn install_native_interval(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    for (version, digest, file) in [
        (
            VERSION_FULL_IMPORT_JSONB,
            compiled_jsonb_document_digest(),
            FILE_FULL_IMPORT_JSONB,
        ),
        (
            VERSION_NATIVE_PARTITION_REBIND,
            compiled_native_partition_rebind_digest(),
            FILE_NATIVE_PARTITION_REBIND,
        ),
    ] {
        if !matches!(find_row(&rows, version), Some(row) if row_exact(row, version, &digest, file))
        {
            let _ = tx.rollback().await;
            return Err(InstallError::Refused {
                message: "native interval requires exact prior schema".to_string(),
            });
        }
    }
    let digest = compiled_native_interval_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_INTERVAL) {
        if !row_exact(row, VERSION_NATIVE_INTERVAL, &digest, FILE_NATIVE_INTERVAL) {
            return match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_INTERVAL.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => Err(err),
                Ok(_) => Err(InstallError::Refused {
                    message: "checksum mismatch did not raise OO001".to_string(),
                }),
            };
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_INTERVAL).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_INTERVAL,
                &digest,
                FILE_NATIVE_INTERVAL,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Add source-bound OLAP partition routing without changing historical scripts.
pub async fn install_native_olap_partition(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    for (version, digest, file) in [
        (
            VERSION_NATIVE_PARTITION,
            compiled_native_partition_digest(),
            FILE_NATIVE_PARTITION,
        ),
        (
            VERSION_NATIVE_PARTITION_REBIND,
            compiled_native_partition_rebind_digest(),
            FILE_NATIVE_PARTITION_REBIND,
        ),
        (
            VERSION_NATIVE_INTERVAL,
            compiled_native_interval_digest(),
            FILE_NATIVE_INTERVAL,
        ),
    ] {
        if !matches!(find_row(&rows, version), Some(row) if row_exact(row, version, &digest, file))
        {
            let _ = tx.rollback().await;
            return Err(InstallError::Refused {
                message: "native OLAP partition requires exact prior schema".to_string(),
            });
        }
    }
    let digest = compiled_native_olap_partition_digest();
    let installed = find_row(&rows, VERSION_NATIVE_OLAP_PARTITION).is_some();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_OLAP_PARTITION)
        && !row_exact(
            row,
            VERSION_NATIVE_OLAP_PARTITION,
            &digest,
            FILE_NATIVE_OLAP_PARTITION,
        )
    {
        return match mismatch(
            tx,
            Mismatch {
                version: VERSION_NATIVE_OLAP_PARTITION.to_string(),
                stored: row.checksum.clone(),
            },
        )
        .await
        {
            Err(err) => Err(err),
            Ok(_) => Err(InstallError::Refused {
                message: "checksum mismatch did not raise OO001".to_string(),
            }),
        };
    }
    let guard_digest = compiled_native_olap_guard_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_OLAP_GUARD)
        && (!installed
            || !row_exact(
                row,
                VERSION_NATIVE_OLAP_GUARD,
                &guard_digest,
                FILE_NATIVE_OLAP_GUARD,
            ))
    {
        return match mismatch(
            tx,
            Mismatch {
                version: VERSION_NATIVE_OLAP_GUARD.to_string(),
                stored: row.checksum.clone(),
            },
        )
        .await
        {
            Err(err) => Err(err),
            Ok(_) => Err(InstallError::Refused {
                message: "checksum mismatch did not raise OO001".to_string(),
            }),
        };
    }
    let signature_digest = compiled_native_olap_guard_signature_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_OLAP_GUARD_SIGNATURE) {
        if !installed
            || !matches!(find_row(&rows, VERSION_NATIVE_OLAP_GUARD), Some(prior) if row_exact(prior, VERSION_NATIVE_OLAP_GUARD, &guard_digest, FILE_NATIVE_OLAP_GUARD))
            || !row_exact(
                row,
                VERSION_NATIVE_OLAP_GUARD_SIGNATURE,
                &signature_digest,
                FILE_NATIVE_OLAP_GUARD_SIGNATURE,
            )
        {
            return match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_OLAP_GUARD_SIGNATURE.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => Err(err),
                Ok(_) => Err(InstallError::Refused {
                    message: "checksum mismatch did not raise OO001".to_string(),
                }),
            };
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        if !installed {
            exec_script(&mut tx, SCRIPT_NATIVE_OLAP_PARTITION).await?;
            insert_markers(
                &mut tx,
                &[marker(
                    VERSION_NATIVE_OLAP_PARTITION,
                    &digest,
                    FILE_NATIVE_OLAP_PARTITION,
                )],
            )
            .await?;
        }
        if find_row(&rows, VERSION_NATIVE_OLAP_GUARD).is_none() {
            exec_script(&mut tx, SCRIPT_NATIVE_OLAP_GUARD).await?;
            insert_markers(
                &mut tx,
                &[marker(
                    VERSION_NATIVE_OLAP_GUARD,
                    &guard_digest,
                    FILE_NATIVE_OLAP_GUARD,
                )],
            )
            .await?;
        }
        exec_script(&mut tx, SCRIPT_NATIVE_OLAP_GUARD_SIGNATURE).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_OLAP_GUARD_SIGNATURE,
                &signature_digest,
                FILE_NATIVE_OLAP_GUARD_SIGNATURE,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Install the atomic PostgreSQL C1 group record after the exact OLAP guard.
pub async fn install_native_pg_transaction_group(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_olap_guard_signature_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_OLAP_GUARD_SIGNATURE), Some(row)
        if row_exact(row, VERSION_NATIVE_OLAP_GUARD_SIGNATURE, &prior, FILE_NATIVE_OLAP_GUARD_SIGNATURE))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "native transaction group requires exact prior schema".to_string(),
        });
    }
    let digest = compiled_native_pg_transaction_group_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_PG_TRANSACTION_GROUP) {
        if !row_exact(
            row,
            VERSION_NATIVE_PG_TRANSACTION_GROUP,
            &digest,
            FILE_NATIVE_PG_TRANSACTION_GROUP,
        ) {
            return match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_PG_TRANSACTION_GROUP.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => Err(err),
                Ok(_) => Err(InstallError::Refused {
                    message: "checksum mismatch did not raise OO001".to_string(),
                }),
            };
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_PG_TRANSACTION_GROUP).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_PG_TRANSACTION_GROUP,
                &digest,
                FILE_NATIVE_PG_TRANSACTION_GROUP,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Keep an exhausted zero-row native table eligible for sealed catalog export.
pub async fn install_native_empty_catalog(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_pg_transaction_group_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_PG_TRANSACTION_GROUP), Some(row)
        if row_exact(row, VERSION_NATIVE_PG_TRANSACTION_GROUP, &prior, FILE_NATIVE_PG_TRANSACTION_GROUP))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "empty native catalog requires exact prior schema".to_string(),
        });
    }
    let digest = compiled_native_empty_catalog_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_EMPTY_CATALOG) {
        if !row_exact(
            row,
            VERSION_NATIVE_EMPTY_CATALOG,
            &digest,
            FILE_NATIVE_EMPTY_CATALOG,
        ) {
            return match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_EMPTY_CATALOG.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => Err(err),
                Ok(_) => Err(InstallError::Refused {
                    message: "checksum mismatch did not raise OO001".to_string(),
                }),
            };
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_EMPTY_CATALOG).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_EMPTY_CATALOG,
                &digest,
                FILE_NATIVE_EMPTY_CATALOG,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Attest group completeness against actual per-table storage receipt fields.
pub async fn install_native_pg_group_receipt(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_empty_catalog_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_EMPTY_CATALOG), Some(row)
        if row_exact(row, VERSION_NATIVE_EMPTY_CATALOG, &prior, FILE_NATIVE_EMPTY_CATALOG))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "group receipt requires exact prior schema".to_string(),
        });
    }
    let digest = compiled_native_pg_group_receipt_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_PG_GROUP_RECEIPT) {
        if !row_exact(
            row,
            VERSION_NATIVE_PG_GROUP_RECEIPT,
            &digest,
            FILE_NATIVE_PG_GROUP_RECEIPT,
        ) {
            return match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_PG_GROUP_RECEIPT.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => Err(err),
                Ok(_) => Err(InstallError::Refused {
                    message: "checksum mismatch did not raise OO001".to_string(),
                }),
            };
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_PG_GROUP_RECEIPT).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_PG_GROUP_RECEIPT,
                &digest,
                FILE_NATIVE_PG_GROUP_RECEIPT,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Owner-only accessor for the prior PostgreSQL source generation.
pub async fn install_native_pg_group_prior(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_pg_group_receipt_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_PG_GROUP_RECEIPT), Some(row)
        if row_exact(row, VERSION_NATIVE_PG_GROUP_RECEIPT, &prior, FILE_NATIVE_PG_GROUP_RECEIPT))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "group prior requires exact prior schema".to_string(),
        });
    }
    let digest = compiled_native_pg_group_prior_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_PG_GROUP_PRIOR) {
        if !row_exact(
            row,
            VERSION_NATIVE_PG_GROUP_PRIOR,
            &digest,
            FILE_NATIVE_PG_GROUP_PRIOR,
        ) {
            return match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_PG_GROUP_PRIOR.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => Err(err),
                Ok(_) => Err(InstallError::Refused {
                    message: "checksum mismatch did not raise OO001".to_string(),
                }),
            };
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_PG_GROUP_PRIOR).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_PG_GROUP_PRIOR,
                &digest,
                FILE_NATIVE_PG_GROUP_PRIOR,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Seal the whole-source generation independently of per-table generations.
pub async fn install_native_pg_group_generation(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_pg_group_prior_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_PG_GROUP_PRIOR), Some(row)
        if row_exact(row, VERSION_NATIVE_PG_GROUP_PRIOR, &prior, FILE_NATIVE_PG_GROUP_PRIOR))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "group generation requires exact prior schema".to_string(),
        });
    }
    let digest = compiled_native_pg_group_generation_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_PG_GROUP_GENERATION) {
        if !row_exact(
            row,
            VERSION_NATIVE_PG_GROUP_GENERATION,
            &digest,
            FILE_NATIVE_PG_GROUP_GENERATION,
        ) {
            return match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_PG_GROUP_GENERATION.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => Err(err),
                Ok(_) => Err(InstallError::Refused {
                    message: "checksum mismatch did not raise OO001".to_string(),
                }),
            };
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_PG_GROUP_GENERATION).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_PG_GROUP_GENERATION,
                &digest,
                FILE_NATIVE_PG_GROUP_GENERATION,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Accept an independently sealed complete transaction with one affected table.
pub async fn install_native_pg_single_transaction(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_pg_group_generation_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_PG_GROUP_GENERATION), Some(row)
        if row_exact(row, VERSION_NATIVE_PG_GROUP_GENERATION, &prior, FILE_NATIVE_PG_GROUP_GENERATION))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "single transaction requires exact prior schema".to_string(),
        });
    }
    let digest = compiled_native_pg_single_transaction_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_PG_SINGLE_TRANSACTION) {
        if !row_exact(
            row,
            VERSION_NATIVE_PG_SINGLE_TRANSACTION,
            &digest,
            FILE_NATIVE_PG_SINGLE_TRANSACTION,
        ) {
            return match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_PG_SINGLE_TRANSACTION.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => Err(err),
                Ok(_) => Err(InstallError::Refused {
                    message: "checksum mismatch did not raise OO001".to_string(),
                }),
            };
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_PG_SINGLE_TRANSACTION).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_PG_SINGLE_TRANSACTION,
                &digest,
                FILE_NATIVE_PG_SINGLE_TRANSACTION,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Fresh installs create base catalog functions before their empty-stream successor.
/// A legacy target missing base markers must not downgrade the already installed successor.
pub async fn install_catalog_before_empty(conn: &mut PgConnection) -> Result<(), InstallError> {
    // Keep the predecessor/successor decision stable across the installers' transactions.
    let mut tx = conn.begin().await.map_err(database_error)?;
    let principal = read_principal(&mut tx).await?;
    prepare_locked(&mut tx, &principal).await?;
    let result = async {
        let rows = read_rows(&mut tx).await?;
        if find_row(&rows, VERSION_NATIVE_EMPTY_CATALOG).is_some()
            && !matches!(find_row(&rows, VERSION_NATIVE_CATALOG), Some(row)
                if row_exact(row, VERSION_NATIVE_CATALOG, &compiled_native_catalog_digest(), FILE_NATIVE_CATALOG)) {
            return Err(InstallError::Refused { message: "empty catalog successor lacks exact base catalog; refusing downgrade".to_string() });
        }
        install_sealed_catalog(&mut tx).await?;
        install_native_sealed_catalog(&mut tx).await?;
        Ok(())
    }.await;
    match result {
        Ok(()) => tx.commit().await.map_err(database_error),
        Err(error) => {
            let _ = tx.rollback().await;
            Err(error)
        }
    }
}

/// Install SQLServer-only CT anchor paths; prior migrations stay immutable.
pub async fn install_native_sqlserver_ct(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_pg_single_transaction_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_PG_SINGLE_TRANSACTION), Some(row)
        if row_exact(row, VERSION_NATIVE_PG_SINGLE_TRANSACTION, &prior, FILE_NATIVE_PG_SINGLE_TRANSACTION))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "single transaction requires exact prior schema".to_string(),
        });
    }
    let digest = compiled_native_sqlserver_ct_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_SQLSERVER_CT) {
        if !row_exact(
            row,
            VERSION_NATIVE_SQLSERVER_CT,
            &digest,
            FILE_NATIVE_SQLSERVER_CT,
        ) {
            return match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_SQLSERVER_CT.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => Err(err),
                Ok(_) => Err(InstallError::Refused {
                    message: "checksum mismatch did not raise OO001".to_string(),
                }),
            };
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_SQLSERVER_CT).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_SQLSERVER_CT,
                &digest,
                FILE_NATIVE_SQLSERVER_CT,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Install the GTID 5-tuple after the SQL Server CT marker. Prior hashes stay fixed.
pub async fn install_native_gtid_transaction(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_sqlserver_ct_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_SQLSERVER_CT), Some(row)
        if row_exact(row, VERSION_NATIVE_SQLSERVER_CT, &prior, FILE_NATIVE_SQLSERVER_CT))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "gtid transaction requires exact sqlserver ct schema".to_string(),
        });
    }
    let digest = compiled_native_gtid_transaction_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_GTID_TRANSACTION) {
        if !row_exact(
            row,
            VERSION_NATIVE_GTID_TRANSACTION,
            &digest,
            FILE_NATIVE_GTID_TRANSACTION,
        ) {
            return match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_GTID_TRANSACTION.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => Err(err),
                Ok(_) => Err(InstallError::Refused {
                    message: "checksum mismatch did not raise OO001".to_string(),
                }),
            };
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_GTID_TRANSACTION).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_GTID_TRANSACTION,
                &digest,
                FILE_NATIVE_GTID_TRANSACTION,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Add the MongoDB C1 source epoch quality seal while retaining the frozen
/// 23-argument quality-seal call used by PostgreSQL.
pub async fn install_native_c1_source_epoch(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_gtid_transaction_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_GTID_TRANSACTION), Some(row)
        if row_exact(row, VERSION_NATIVE_GTID_TRANSACTION, &prior, FILE_NATIVE_GTID_TRANSACTION))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "source epoch requires exact gtid transaction schema".to_string(),
        });
    }
    let digest = compiled_native_c1_source_epoch_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C1_SOURCE_EPOCH) {
        if !row_exact(
            row,
            VERSION_NATIVE_C1_SOURCE_EPOCH,
            &digest,
            FILE_NATIVE_C1_SOURCE_EPOCH,
        ) {
            return match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C1_SOURCE_EPOCH.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => Err(err),
                Ok(_) => Err(InstallError::Refused {
                    message: "checksum mismatch did not raise OO001".to_string(),
                }),
            };
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C1_SOURCE_EPOCH).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C1_SOURCE_EPOCH,
                &digest,
                FILE_NATIVE_C1_SOURCE_EPOCH,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Grant the runtime access to the immutable Mongo source-epoch seal overload.
pub async fn install_native_c1_source_epoch_runtime(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_c1_source_epoch_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_C1_SOURCE_EPOCH), Some(row)
        if row_exact(row, VERSION_NATIVE_C1_SOURCE_EPOCH, &prior, FILE_NATIVE_C1_SOURCE_EPOCH))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "source epoch runtime grant requires exact installed source epoch migration"
                .to_string(),
        });
    }
    let digest = compiled_native_c1_source_epoch_runtime_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C1_SOURCE_EPOCH_RUNTIME) {
        if !row_exact(
            row,
            VERSION_NATIVE_C1_SOURCE_EPOCH_RUNTIME,
            &digest,
            FILE_NATIVE_C1_SOURCE_EPOCH_RUNTIME,
        ) {
            return match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C1_SOURCE_EPOCH_RUNTIME.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => Err(err),
                Ok(_) => Err(InstallError::Refused {
                    message: "checksum mismatch did not raise OO001".to_string(),
                }),
            };
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C1_SOURCE_EPOCH_RUNTIME).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C1_SOURCE_EPOCH_RUNTIME,
                &digest,
                FILE_NATIVE_C1_SOURCE_EPOCH_RUNTIME,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Install the fenced native full-import publication successor after source-epoch runtime grants.
pub async fn install_native_publication(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_c1_source_epoch_runtime_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_C1_SOURCE_EPOCH_RUNTIME), Some(row)
        if row_exact(row, VERSION_NATIVE_C1_SOURCE_EPOCH_RUNTIME, &prior, FILE_NATIVE_C1_SOURCE_EPOCH_RUNTIME))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "native publication requires exact source-epoch runtime migration".to_string(),
        });
    }
    let digest = compiled_native_publication_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_PUBLICATION) {
        if !row_exact(
            row,
            VERSION_NATIVE_PUBLICATION,
            &digest,
            FILE_NATIVE_PUBLICATION,
        ) {
            return match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_PUBLICATION.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => Err(err),
                Ok(_) => Err(InstallError::Refused {
                    message: "checksum mismatch did not raise OO001".to_string(),
                }),
            };
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_PUBLICATION).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_PUBLICATION,
                &digest,
                FILE_NATIVE_PUBLICATION,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Install the append-only CDC-first baseline staging contract after the
/// immutable source-epoch runtime migration. The staging rows stay private.
pub async fn install_native_c1_baseline_stage(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_c1_source_epoch_runtime_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_C1_SOURCE_EPOCH_RUNTIME), Some(row)
        if row_exact(row, VERSION_NATIVE_C1_SOURCE_EPOCH_RUNTIME, &prior, FILE_NATIVE_C1_SOURCE_EPOCH_RUNTIME))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "C1 baseline stage requires exact source-epoch runtime schema".to_string(),
        });
    }
    let digest = compiled_native_c1_baseline_stage_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C1_BASELINE_STAGE) {
        if !row_exact(
            row,
            VERSION_NATIVE_C1_BASELINE_STAGE,
            &digest,
            FILE_NATIVE_C1_BASELINE_STAGE,
        ) {
            return match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C1_BASELINE_STAGE.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => Err(err),
                Ok(_) => Err(InstallError::Refused {
                    message: "C1 baseline stage checksum mismatch did not raise OO001".to_string(),
                }),
            };
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C1_BASELINE_STAGE).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C1_BASELINE_STAGE,
                &digest,
                FILE_NATIVE_C1_BASELINE_STAGE,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Bind C1 bootstrap positions to the measured MySQL UUID and contiguous GTIDs.
pub async fn install_native_c1_gtid_stage_identity(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let expected = compiled_native_c1_baseline_stage_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_C1_BASELINE_STAGE), Some(row)
        if row_exact(row, VERSION_NATIVE_C1_BASELINE_STAGE, &expected, FILE_NATIVE_C1_BASELINE_STAGE))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "C1 GTID stage identity requires exact 0028".to_string(),
        });
    }
    let digest = compiled_native_c1_gtid_stage_identity_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C1_GTID_STAGE_IDENTITY) {
        if !row_exact(
            row,
            VERSION_NATIVE_C1_GTID_STAGE_IDENTITY,
            &digest,
            FILE_NATIVE_C1_GTID_STAGE_IDENTITY,
        ) {
            return match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C1_GTID_STAGE_IDENTITY.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => Err(err),
                Ok(_) => Err(InstallError::Refused {
                    message: "C1 GTID stage identity checksum mismatch did not raise OO001"
                        .to_string(),
                }),
            };
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C1_GTID_STAGE_IDENTITY).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C1_GTID_STAGE_IDENTITY,
                &digest,
                FILE_NATIVE_C1_GTID_STAGE_IDENTITY,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Correct the GTID stage identity format to the registered `mysql.<uuid>` contract.
pub async fn install_native_c1_gtid_measured_identity(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    for (version, digest, filename) in [
        (
            VERSION_NATIVE_C1_BASELINE_STAGE,
            compiled_native_c1_baseline_stage_digest(),
            FILE_NATIVE_C1_BASELINE_STAGE,
        ),
        (
            VERSION_NATIVE_C1_GTID_STAGE_IDENTITY,
            compiled_native_c1_gtid_stage_identity_digest(),
            FILE_NATIVE_C1_GTID_STAGE_IDENTITY,
        ),
    ] {
        if !matches!(find_row(&rows, version), Some(row) if row_exact(row, version, &digest, filename))
        {
            let _ = tx.rollback().await;
            return Err(InstallError::Refused {
                message: format!("C1 GTID measured identity requires exact {version}"),
            });
        }
    }
    let digest = compiled_native_c1_gtid_measured_identity_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C1_GTID_MEASURED_IDENTITY) {
        if !row_exact(
            row,
            VERSION_NATIVE_C1_GTID_MEASURED_IDENTITY,
            &digest,
            FILE_NATIVE_C1_GTID_MEASURED_IDENTITY,
        ) {
            return match mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C1_GTID_MEASURED_IDENTITY.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            {
                Err(err) => Err(err),
                Ok(_) => Err(InstallError::Refused {
                    message: "C1 GTID measured identity checksum mismatch did not raise OO001"
                        .to_string(),
                }),
            };
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C1_GTID_MEASURED_IDENTITY).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C1_GTID_MEASURED_IDENTITY,
                &digest,
                FILE_NATIVE_C1_GTID_MEASURED_IDENTITY,
            )],
        )
        .await?;
        Ok::<(), InstallError>(())
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Install the typed CDC-first bridge after the immutable measured-UUID stage.
pub async fn install_native_c1_baseline_typed_bridge(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_c1_gtid_measured_identity_digest();
    if !matches!(find_row(&rows,VERSION_NATIVE_C1_GTID_MEASURED_IDENTITY),Some(row)
        if row_exact(row,VERSION_NATIVE_C1_GTID_MEASURED_IDENTITY,&prior,FILE_NATIVE_C1_GTID_MEASURED_IDENTITY))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "C1 typed bridge requires exact 0030".to_string(),
        });
    }
    let digest = compiled_native_c1_baseline_typed_bridge_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C1_BASELINE_TYPED_BRIDGE) {
        if !row_exact(
            row,
            VERSION_NATIVE_C1_BASELINE_TYPED_BRIDGE,
            &digest,
            FILE_NATIVE_C1_BASELINE_TYPED_BRIDGE,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C1_BASELINE_TYPED_BRIDGE.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C1_BASELINE_TYPED_BRIDGE).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C1_BASELINE_TYPED_BRIDGE,
                &digest,
                FILE_NATIVE_C1_BASELINE_TYPED_BRIDGE,
            )],
        )
        .await
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Install same-task C1 baseline retry adoption after the immutable typed bridge.
pub async fn install_native_c1_baseline_lease_rebind(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_c1_baseline_typed_bridge_digest();
    if !matches!(find_row(&rows,VERSION_NATIVE_C1_BASELINE_TYPED_BRIDGE),Some(row)
        if row_exact(row,VERSION_NATIVE_C1_BASELINE_TYPED_BRIDGE,&prior,FILE_NATIVE_C1_BASELINE_TYPED_BRIDGE))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "C1 baseline retry requires exact 0031".to_string(),
        });
    }
    let digest = compiled_native_c1_baseline_lease_rebind_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C1_BASELINE_LEASE_REBIND) {
        if !row_exact(
            row,
            VERSION_NATIVE_C1_BASELINE_LEASE_REBIND,
            &digest,
            FILE_NATIVE_C1_BASELINE_LEASE_REBIND,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C1_BASELINE_LEASE_REBIND.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C1_BASELINE_LEASE_REBIND).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C1_BASELINE_LEASE_REBIND,
                &digest,
                FILE_NATIVE_C1_BASELINE_LEASE_REBIND,
            )],
        )
        .await
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Preserve the existing MySQL nullable zero-date quality policy in the baseline binder.
pub async fn install_native_c1_baseline_date_quality(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_c1_baseline_lease_rebind_digest();
    if !matches!(find_row(&rows,VERSION_NATIVE_C1_BASELINE_LEASE_REBIND),Some(row)
        if row_exact(row,VERSION_NATIVE_C1_BASELINE_LEASE_REBIND,&prior,FILE_NATIVE_C1_BASELINE_LEASE_REBIND))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "C1 baseline date quality requires exact 0032".to_string(),
        });
    }
    let digest = compiled_native_c1_baseline_date_quality_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C1_BASELINE_DATE_QUALITY) {
        if !row_exact(
            row,
            VERSION_NATIVE_C1_BASELINE_DATE_QUALITY,
            &digest,
            FILE_NATIVE_C1_BASELINE_DATE_QUALITY,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C1_BASELINE_DATE_QUALITY.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C1_BASELINE_DATE_QUALITY).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C1_BASELINE_DATE_QUALITY,
                &digest,
                FILE_NATIVE_C1_BASELINE_DATE_QUALITY,
            )],
        )
        .await
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Keep full baseline verification linear in the number of source rows.
pub async fn install_native_c1_baseline_key_join(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_c1_baseline_date_quality_digest();
    if !matches!(find_row(&rows,VERSION_NATIVE_C1_BASELINE_DATE_QUALITY),Some(row)
        if row_exact(row,VERSION_NATIVE_C1_BASELINE_DATE_QUALITY,&prior,FILE_NATIVE_C1_BASELINE_DATE_QUALITY))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "C1 baseline key join requires exact 0033".to_string(),
        });
    }
    let digest = compiled_native_c1_baseline_key_join_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C1_BASELINE_KEY_JOIN) {
        if !row_exact(
            row,
            VERSION_NATIVE_C1_BASELINE_KEY_JOIN,
            &digest,
            FILE_NATIVE_C1_BASELINE_KEY_JOIN,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C1_BASELINE_KEY_JOIN.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C1_BASELINE_KEY_JOIN).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C1_BASELINE_KEY_JOIN,
                &digest,
                FILE_NATIVE_C1_BASELINE_KEY_JOIN,
            )],
        )
        .await
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Reuse the exact ACKed source mirror for subsequent binlog cuts.
pub async fn install_native_c1_ack_seed(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_c1_baseline_key_join_digest();
    if !matches!(find_row(&rows,VERSION_NATIVE_C1_BASELINE_KEY_JOIN),Some(row)
        if row_exact(row,VERSION_NATIVE_C1_BASELINE_KEY_JOIN,&prior,FILE_NATIVE_C1_BASELINE_KEY_JOIN))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "C1 ACK seed requires exact 0034".to_string(),
        });
    }
    let digest = compiled_native_c1_ack_seed_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C1_ACK_SEED) {
        if !row_exact(
            row,
            VERSION_NATIVE_C1_ACK_SEED,
            &digest,
            FILE_NATIVE_C1_ACK_SEED,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C1_ACK_SEED.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C1_ACK_SEED).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C1_ACK_SEED,
                &digest,
                FILE_NATIVE_C1_ACK_SEED,
            )],
        )
        .await
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Publish an ACKed multi-run C1 cut through the existing release transaction.
pub async fn install_native_c1_collection_publication(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    for (version, digest, file) in [
        (
            VERSION_NATIVE_PUBLICATION,
            compiled_native_publication_digest(),
            FILE_NATIVE_PUBLICATION,
        ),
        (
            VERSION_NATIVE_C1_ACK_SEED,
            compiled_native_c1_ack_seed_digest(),
            FILE_NATIVE_C1_ACK_SEED,
        ),
    ] {
        if !matches!(find_row(&rows,version),Some(row) if row_exact(row,version,&digest,file)) {
            let _ = tx.rollback().await;
            return Err(InstallError::Refused {
                message: "C1 publication requires exact 0027 and 0035".into(),
            });
        }
    }
    let digest = compiled_native_c1_collection_publication_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C1_COLLECTION_PUBLICATION) {
        if !row_exact(
            row,
            VERSION_NATIVE_C1_COLLECTION_PUBLICATION,
            &digest,
            FILE_NATIVE_C1_COLLECTION_PUBLICATION,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C1_COLLECTION_PUBLICATION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C1_COLLECTION_PUBLICATION).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C1_COLLECTION_PUBLICATION,
                &digest,
                FILE_NATIVE_C1_COLLECTION_PUBLICATION,
            )],
        )
        .await
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Publish a complete current per-table C4/C5 collection through the existing release transaction.
pub async fn install_native_per_table_publication(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    for (version, digest, file) in [(
        VERSION_NATIVE_C1_COLLECTION_PUBLICATION,
        compiled_native_c1_collection_publication_digest(),
        FILE_NATIVE_C1_COLLECTION_PUBLICATION,
    )] {
        if !matches!(find_row(&rows,version),Some(row) if row_exact(row,version,&digest,file)) {
            let _ = tx.rollback().await;
            return Err(InstallError::Refused {
                message: "Per-table publication requires exact 0036".into(),
            });
        }
    }
    let digest = compiled_native_per_table_publication_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_PER_TABLE_PUBLICATION) {
        if !row_exact(
            row,
            VERSION_NATIVE_PER_TABLE_PUBLICATION,
            &digest,
            FILE_NATIVE_PER_TABLE_PUBLICATION,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_PER_TABLE_PUBLICATION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_PER_TABLE_PUBLICATION).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_PER_TABLE_PUBLICATION,
                &digest,
                FILE_NATIVE_PER_TABLE_PUBLICATION,
            )],
        )
        .await
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Allow the scheduled runtime to confirm only the source identity and global
/// generation before producing its next capture.
pub async fn install_native_source_progress_read(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_per_table_publication_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_PER_TABLE_PUBLICATION), Some(row)
        if row_exact(row, VERSION_NATIVE_PER_TABLE_PUBLICATION, &prior, FILE_NATIVE_PER_TABLE_PUBLICATION))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "source progress read requires exact 0039".into(),
        });
    }
    let digest = compiled_native_source_progress_read_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_SOURCE_PROGRESS_READ) {
        if !row_exact(
            row,
            VERSION_NATIVE_SOURCE_PROGRESS_READ,
            &digest,
            FILE_NATIVE_SOURCE_PROGRESS_READ,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_SOURCE_PROGRESS_READ.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_SOURCE_PROGRESS_READ).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_SOURCE_PROGRESS_READ,
                &digest,
                FILE_NATIVE_SOURCE_PROGRESS_READ,
            )],
        )
        .await
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Allow the scheduled runtime to verify only the two C4 sidecar columns used
/// when it accepts a quiet partition cycle.
pub async fn install_native_partition_observation_read(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_source_progress_read_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_SOURCE_PROGRESS_READ), Some(row)
        if row_exact(row, VERSION_NATIVE_SOURCE_PROGRESS_READ, &prior, FILE_NATIVE_SOURCE_PROGRESS_READ))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "partition observation read requires exact 0040".into(),
        });
    }
    let digest = compiled_native_partition_observation_read_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_PARTITION_OBSERVATION_READ) {
        if !row_exact(
            row,
            VERSION_NATIVE_PARTITION_OBSERVATION_READ,
            &digest,
            FILE_NATIVE_PARTITION_OBSERVATION_READ,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_PARTITION_OBSERVATION_READ.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_PARTITION_OBSERVATION_READ).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_PARTITION_OBSERVATION_READ,
                &digest,
                FILE_NATIVE_PARTITION_OBSERVATION_READ,
            )],
        )
        .await
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Install the chunked-typed/v2 C1 baseline objects after the exact 0041 install.
pub async fn install_native_c1_chunked_baseline(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_partition_observation_read_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_PARTITION_OBSERVATION_READ), Some(row)
        if row_exact(row, VERSION_NATIVE_PARTITION_OBSERVATION_READ, &prior, FILE_NATIVE_PARTITION_OBSERVATION_READ))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "C1 chunked baseline requires exact 0041".into(),
        });
    }
    let digest = compiled_native_c1_chunked_baseline_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C1_CHUNKED_BASELINE) {
        if !row_exact(
            row,
            VERSION_NATIVE_C1_CHUNKED_BASELINE,
            &digest,
            FILE_NATIVE_C1_CHUNKED_BASELINE,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C1_CHUNKED_BASELINE.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C1_CHUNKED_BASELINE).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C1_CHUNKED_BASELINE,
                &digest,
                FILE_NATIVE_C1_CHUNKED_BASELINE,
            )],
        )
        .await
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Additive typed-key staging; deliberately not enabled by legacy installers.
pub async fn install_native_c1_typed_keys(conn: &mut PgConnection) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-typed-keys/v3";
    const FILE: &str = "0076_native_c1_typed_keys.sql";
    const SCRIPT: &str = include_str!("../migrations/0076_native_c1_typed_keys.sql");
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    if !matches!(find_row(&rows, VERSION_NATIVE_C1_CHUNKED_BASELINE), Some(row)
        if row_exact(row, VERSION_NATIVE_C1_CHUNKED_BASELINE,
            &compiled_native_c1_chunked_baseline_digest(), FILE_NATIVE_C1_CHUNKED_BASELINE))
    {
        return Err(InstallError::Refused {
            message: "typed C1 requires exact 0042".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_typed_bridge(conn: &mut PgConnection) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-typed-bridge/v3";
    const FILE: &str = "0077_native_c1_typed_bridge.sql";
    const SCRIPT: &str = include_str!("../migrations/0077_native_c1_typed_bridge.sql");
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    if !matches!(find_row(&rows,"native-c1-typed-keys/v3"),Some(row) if row_exact(row,
        "native-c1-typed-keys/v3",&sha256_hex(include_str!("../migrations/0076_native_c1_typed_keys.sql").as_bytes()),
        "0076_native_c1_typed_keys.sql"))
    {
        return Err(InstallError::Refused {
            message: "typed bridge requires exact 0076".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_typed_commit_time(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-typed-commit-time/v3";
    const FILE: &str = "0078_native_c1_typed_commit_time.sql";
    const SCRIPT: &str = include_str!("../migrations/0078_native_c1_typed_commit_time.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    if !matches!(find_row(&rows,"native-c1-typed-bridge/v3"),Some(row) if row_exact(row,
        "native-c1-typed-bridge/v3",&sha256_hex(include_str!("../migrations/0077_native_c1_typed_bridge.sql").as_bytes()),
        "0077_native_c1_typed_bridge.sql"))
    {
        return Err(InstallError::Refused {
            message: "typed commit time requires exact 0077".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_source_identity(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-source-identity/v1";
    const FILE: &str = "0079_native_c1_source_identity.sql";
    const SCRIPT: &str = include_str!("../migrations/0079_native_c1_source_identity.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    if !matches!(find_row(&rows,"native-c1-typed-commit-time/v3"),Some(row) if row_exact(row,
        "native-c1-typed-commit-time/v3",&sha256_hex(include_str!("../migrations/0078_native_c1_typed_commit_time.sql").as_bytes()),
        "0078_native_c1_typed_commit_time.sql"))
    {
        return Err(InstallError::Refused {
            message: "source identity adapter requires exact 0078".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_buffer_checkpoint(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-buffer-checkpoint/v1";
    const FILE: &str = "0080_native_c1_buffer_checkpoint.sql";
    const SCRIPT: &str = include_str!("../migrations/0080_native_c1_buffer_checkpoint.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    if !matches!(find_row(&rows,"native-c1-source-identity/v1"),Some(row) if row_exact(row,
        "native-c1-source-identity/v1",&sha256_hex(include_str!("../migrations/0079_native_c1_source_identity.sql").as_bytes()),
        "0079_native_c1_source_identity.sql"))
    {
        return Err(InstallError::Refused {
            message: "buffer checkpoint requires exact 0079".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_buffer_context(conn: &mut PgConnection) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-buffer-context/v1";
    const FILE: &str = "0081_native_c1_buffer_context.sql";
    const SCRIPT: &str = include_str!("../migrations/0081_native_c1_buffer_context.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    if !matches!(find_row(&rows,"native-c1-buffer-checkpoint/v1"),Some(row) if row_exact(row,
        "native-c1-buffer-checkpoint/v1",&sha256_hex(include_str!("../migrations/0080_native_c1_buffer_checkpoint.sql").as_bytes()),
        "0080_native_c1_buffer_checkpoint.sql"))
    {
        return Err(InstallError::Refused {
            message: "buffer context requires exact 0080".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_oracle_tail(conn: &mut PgConnection) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-oracle-tail/v1";
    const FILE: &str = "0082_native_c1_oracle_tail.sql";
    const SCRIPT: &str = include_str!("../migrations/0082_native_c1_oracle_tail.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    if !matches!(find_row(&rows,"native-c1-buffer-context/v1"),Some(row) if row_exact(row,
        "native-c1-buffer-context/v1",&sha256_hex(include_str!("../migrations/0081_native_c1_buffer_context.sql").as_bytes()),
        "0081_native_c1_buffer_context.sql"))
    {
        return Err(InstallError::Refused {
            message: "Oracle tail requires exact 0081".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_export_binding(conn: &mut PgConnection) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-export-binding/v1";
    const FILE: &str = "0083_native_c1_export_binding.sql";
    const SCRIPT: &str = include_str!("../migrations/0083_native_c1_export_binding.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    if !matches!(find_row(&rows,"native-c1-oracle-tail/v1"),Some(row) if row_exact(row,
        "native-c1-oracle-tail/v1",&sha256_hex(include_str!("../migrations/0082_native_c1_oracle_tail.sql").as_bytes()),
        "0082_native_c1_oracle_tail.sql"))
    {
        return Err(InstallError::Refused {
            message: "export binding requires exact 0082".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_typed_export(conn: &mut PgConnection) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-typed-export/v1";
    const FILE: &str = "0095_native_c1_typed_export.sql";
    const SCRIPT: &str = include_str!("../migrations/0095_native_c1_typed_export.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    for (version, file, script) in [
        (
            "native-c1-export-binding/v1",
            "0083_native_c1_export_binding.sql",
            include_str!("../migrations/0083_native_c1_export_binding.sql"),
        ),
        (
            "native-c1-mysql-delta/v1",
            "0088_native_c1_mysql_delta.sql",
            include_str!("../migrations/0088_native_c1_mysql_delta.sql"),
        ),
    ] {
        if !matches!(find_row(&rows,version),Some(row) if row_exact(row,version,&sha256_hex(script.as_bytes()),file))
        {
            return Err(InstallError::Refused {
                message: format!("Typed parts export requires exact {file}"),
            });
        }
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_oracle_fill_resume(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-oracle-fill-resume/v1";
    const FILE: &str = "0097_native_c1_oracle_fill_resume.sql";
    const SCRIPT: &str = include_str!("../migrations/0097_native_c1_oracle_fill_resume.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    if !matches!(find_row(&rows,"native-c1-oracle-continuous/v1"),Some(row) if row_exact(row,
        "native-c1-oracle-continuous/v1",&sha256_hex(include_str!("../migrations/0087_native_c1_oracle_continuous.sql").as_bytes()),
        "0087_native_c1_oracle_continuous.sql"))
    {
        return Err(InstallError::Refused {
            message: "Oracle fill resume requires exact 0087".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_oracle_mixed_resume(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-oracle-mixed-resume/v1";
    const FILE: &str = "0099_native_c1_oracle_mixed_resume.sql";
    const SCRIPT: &str = include_str!("../migrations/0099_native_c1_oracle_mixed_resume.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    if !matches!(find_row(&rows,"native-c1-oracle-fill-resume/v1"),Some(row) if row_exact(row,
        "native-c1-oracle-fill-resume/v1",&sha256_hex(include_str!("../migrations/0097_native_c1_oracle_fill_resume.sql").as_bytes()),
        "0097_native_c1_oracle_fill_resume.sql"))
    {
        return Err(InstallError::Refused {
            message: "Oracle mixed resume requires exact 0097".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_mysql_initial_resume(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-mysql-initial-resume/v1";
    const FILE: &str = "0098_native_c1_mysql_initial_resume.sql";
    const SCRIPT: &str = include_str!("../migrations/0098_native_c1_mysql_initial_resume.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    for (version, file, script) in [
        (
            "native-c1-typed-export/v1",
            "0095_native_c1_typed_export.sql",
            include_str!("../migrations/0095_native_c1_typed_export.sql"),
        ),
        (
            "native-c1-oracle-fill-resume/v1",
            "0097_native_c1_oracle_fill_resume.sql",
            include_str!("../migrations/0097_native_c1_oracle_fill_resume.sql"),
        ),
    ] {
        if !matches!(find_row(&rows,version),Some(row) if row_exact(row,version,&sha256_hex(script.as_bytes()),file))
        {
            return Err(InstallError::Refused {
                message: format!("MySQL initial resume requires exact {file}"),
            });
        }
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_mysql_connection_revision(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const VERSION: &str = "native-mysql-connection-revision/v1";
    const FILE: &str = "0100_native_mysql_connection_revision.sql";
    const SCRIPT: &str = include_str!("../migrations/0100_native_mysql_connection_revision.sql");
    const FIX_VERSION: &str = "native-mysql-connection-expression/v1";
    const FIX_FILE: &str = "0101_native_mysql_connection_expression.sql";
    const FIX_SCRIPT: &str =
        include_str!("../migrations/0101_native_mysql_connection_expression.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    for (version, file, script) in [
        (
            "native-c1-mysql-delta/v1",
            "0088_native_c1_mysql_delta.sql",
            include_str!("../migrations/0088_native_c1_mysql_delta.sql"),
        ),
        (
            "native-c1-oracle-mixed-resume/v1",
            "0099_native_c1_oracle_mixed_resume.sql",
            include_str!("../migrations/0099_native_c1_oracle_mixed_resume.sql"),
        ),
    ] {
        if !matches!(find_row(&rows,version),Some(row) if row_exact(row,version,&sha256_hex(script.as_bytes()),file))
        {
            return Err(InstallError::Refused {
                message: format!("Connection revision requires exact {file}"),
            });
        }
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    let fix_digest = sha256_hex(FIX_SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, FIX_VERSION) {
        if !row_exact(row, FIX_VERSION, &fix_digest, FIX_FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: FIX_VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, FIX_SCRIPT).await?;
        insert_markers(&mut tx, &[marker(FIX_VERSION, &fix_digest, FIX_FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_oracle_initial_rebind(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const VERSION: &str = "native-oracle-initial-rebind/v1";
    const FILE: &str = "0102_native_oracle_initial_rebind.sql";
    const SCRIPT: &str = include_str!("../migrations/0102_native_oracle_initial_rebind.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    for (version, file, script) in [
        (
            "native-c1-source-identity/v1",
            "0079_native_c1_source_identity.sql",
            include_str!("../migrations/0079_native_c1_source_identity.sql"),
        ),
        (
            "native-mysql-connection-expression/v1",
            "0101_native_mysql_connection_expression.sql",
            include_str!("../migrations/0101_native_mysql_connection_expression.sql"),
        ),
    ] {
        if !matches!(find_row(&rows,version),Some(row) if row_exact(row,version,&sha256_hex(script.as_bytes()),file))
        {
            return Err(InstallError::Refused {
                message: format!("Oracle initial rebind requires exact {file}"),
            });
        }
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_C1_COMPLETE_SCOPE: &str = "native-c1-complete-scope/v1";
const FILE_NATIVE_C1_COMPLETE_SCOPE: &str = "0109_native_c1_complete_scope.sql";
const SCRIPT_NATIVE_C1_COMPLETE_SCOPE: &str =
    include_str!("../migrations/0109_native_c1_complete_scope.sql");

pub fn compiled_native_c1_complete_scope_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C1_COMPLETE_SCOPE.as_bytes())
}

pub async fn install_native_c1_complete_scope(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    for (version, file, script) in [
        (
            "native-c1-oracle-continuous/v1",
            "0087_native_c1_oracle_continuous.sql",
            include_str!("../migrations/0087_native_c1_oracle_continuous.sql"),
        ),
        (
            "native-c1-source-names/v1",
            "0084_native_c1_source_names.sql",
            include_str!("../migrations/0084_native_c1_source_names.sql"),
        ),
        (
            "native-library-mysql-receipt/v1",
            "0092_native_library_mysql_receipt.sql",
            include_str!("../migrations/0092_native_library_mysql_receipt.sql"),
        ),
        (
            "native-c1-oracle-fill-resume/v1",
            "0097_native_c1_oracle_fill_resume.sql",
            include_str!("../migrations/0097_native_c1_oracle_fill_resume.sql"),
        ),
        (
            "native-c1-oracle-mixed-resume/v1",
            "0099_native_c1_oracle_mixed_resume.sql",
            include_str!("../migrations/0099_native_c1_oracle_mixed_resume.sql"),
        ),
    ] {
        if !matches!(find_row(&rows, version), Some(row) if row_exact(row, version, &sha256_hex(script.as_bytes()), file))
        {
            return Err(InstallError::Refused {
                message: format!("Complete C1 scope requires exact {file}"),
            });
        }
    }
    let digest = compiled_native_c1_complete_scope_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C1_COMPLETE_SCOPE) {
        if !row_exact(
            row,
            VERSION_NATIVE_C1_COMPLETE_SCOPE,
            &digest,
            FILE_NATIVE_C1_COMPLETE_SCOPE,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C1_COMPLETE_SCOPE.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C1_COMPLETE_SCOPE).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C1_COMPLETE_SCOPE,
                &digest,
                FILE_NATIVE_C1_COMPLETE_SCOPE,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_MYSQL_UNAPPLIED_ABANDON: &str = "native-mysql-unapplied-abandon/v1";
const FILE_NATIVE_MYSQL_UNAPPLIED_ABANDON: &str = "0110_native_mysql_unapplied_delta_abandon.sql";
const SCRIPT_NATIVE_MYSQL_UNAPPLIED_ABANDON: &str =
    include_str!("../migrations/0110_native_mysql_unapplied_delta_abandon.sql");

pub async fn install_native_mysql_unapplied_abandon(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    for (version, file, script) in [
        (
            "native-c1-mysql-delta/v1",
            "0088_native_c1_mysql_delta.sql",
            include_str!("../migrations/0088_native_c1_mysql_delta.sql"),
        ),
        (
            "native-mysql-connection-expression/v1",
            "0101_native_mysql_connection_expression.sql",
            include_str!("../migrations/0101_native_mysql_connection_expression.sql"),
        ),
    ] {
        if !matches!(find_row(&rows,version),Some(row) if row_exact(row,version,&sha256_hex(script.as_bytes()),file))
        {
            return Err(InstallError::Refused {
                message: format!("Unapplied C1 abandon requires exact {file}"),
            });
        }
    }
    let digest = sha256_hex(SCRIPT_NATIVE_MYSQL_UNAPPLIED_ABANDON.as_bytes());
    if let Some(row) = find_row(&rows, VERSION_NATIVE_MYSQL_UNAPPLIED_ABANDON) {
        if !row_exact(
            row,
            VERSION_NATIVE_MYSQL_UNAPPLIED_ABANDON,
            &digest,
            FILE_NATIVE_MYSQL_UNAPPLIED_ABANDON,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_MYSQL_UNAPPLIED_ABANDON.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_MYSQL_UNAPPLIED_ABANDON).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_MYSQL_UNAPPLIED_ABANDON,
                &digest,
                FILE_NATIVE_MYSQL_UNAPPLIED_ABANDON,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_ORACLE_POSITION_CAPACITY: &str = "native-oracle-position-capacity/v1";
const FILE_NATIVE_ORACLE_POSITION_CAPACITY: &str = "0111_native_oracle_position_capacity.sql";
const SCRIPT_NATIVE_ORACLE_POSITION_CAPACITY: &str =
    include_str!("../migrations/0111_native_oracle_position_capacity.sql");

pub async fn install_native_mysql_c5_handoff(conn: &mut PgConnection) -> Result<(), InstallError> {
    const VERSION: &str = "native-mysql-c5-handoff/v1";
    const FILE: &str = "0112_native_mysql_c5_handoff.sql";
    const SCRIPT: &str = include_str!("../migrations/0112_native_mysql_c5_handoff.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    for (version, file, script) in [
        (
            "native-mysql-connection-revision/v1",
            "0100_native_mysql_connection_revision.sql",
            include_str!("../migrations/0100_native_mysql_connection_revision.sql"),
        ),
        (
            "native-oracle-initial-rebind/v1",
            "0102_native_oracle_initial_rebind.sql",
            include_str!("../migrations/0102_native_oracle_initial_rebind.sql"),
        ),
    ] {
        if !matches!(find_row(&rows,version),Some(row) if row_exact(row,version,&sha256_hex(script.as_bytes()),file))
        {
            return Err(InstallError::Refused {
                message: format!("MySQL handoff requires exact {file}"),
            });
        }
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_mysql_handoff_scope(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const VERSION: &str = "native-mysql-handoff-scope/v1";
    const FILE: &str = "0113_native_mysql_handoff_scope.sql";
    const SCRIPT: &str = include_str!("../migrations/0113_native_mysql_handoff_scope.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let previous = "native-mysql-c5-handoff/v1";
    if !matches!(find_row(&rows,previous),Some(row) if row_exact(row,previous,
        &sha256_hex(include_bytes!("../migrations/0112_native_mysql_c5_handoff.sql")),
        "0112_native_mysql_c5_handoff.sql"))
    {
        return Err(InstallError::Refused {
            message: "MySQL handoff scope requires exact 0112".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_mysql_handoff_initial_resume(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const VERSION: &str = "native-mysql-handoff-initial-resume/v1";
    const FILE: &str = "0114_native_mysql_handoff_initial_resume.sql";
    const SCRIPT: &str = include_str!("../migrations/0114_native_mysql_handoff_initial_resume.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    for (version, file, script) in [
        (
            "native-c1-mysql-initial-resume/v1",
            "0098_native_c1_mysql_initial_resume.sql",
            include_str!("../migrations/0098_native_c1_mysql_initial_resume.sql"),
        ),
        (
            "native-mysql-handoff-scope/v1",
            "0113_native_mysql_handoff_scope.sql",
            include_str!("../migrations/0113_native_mysql_handoff_scope.sql"),
        ),
    ] {
        if !matches!(find_row(&rows,version),Some(row) if row_exact(row,version,&sha256_hex(script.as_bytes()),file))
        {
            return Err(InstallError::Refused {
                message: format!("MySQL handoff recovery requires exact {file}"),
            });
        }
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

#[cfg(test)]
mod mysql_handoff_resume_tests {
    #[test]
    fn additive_recovery_keeps_every_initial_open_guard_except_authorized_generation() {
        let original = include_str!("../migrations/0098_native_c1_mysql_initial_resume.sql");
        let open = original
            .split_once("CREATE FUNCTION _oo.c1_mysql_initial_open(")
            .unwrap()
            .1
            .split_once("CREATE FUNCTION _oo.c1_mysql_initial_reserve(")
            .unwrap()
            .0;
        let expected = format!("CREATE OR REPLACE FUNCTION _oo.c1_mysql_initial_open({open}")
            .replace(
                "OR p->>'source_generation' IS DISTINCT FROM '1'",
                "OR NOT _oo.mysql_initial_generation_allowed(s,t,g,e,p)",
            );
        let script = include_str!("../migrations/0114_native_mysql_handoff_initial_resume.sql");
        assert!(script.trim_end().ends_with(expected.trim_end()));
        for guard in [
            "h.task IS DISTINCT FROM t",
            "h.source_epoch IS DISTINCT FROM e",
            "p->>'l0' IS DISTINCT FROM h.l0",
            "p->>'connection_config_sha256' IS DISTINCT FROM h.config_sha256",
            "p->'handoff' IS DISTINCT FROM expected",
            "_oo.mysql_handoff_single_scope(s,h.binding,t)",
        ] {
            assert!(script.contains(guard), "{guard}");
        }
        assert!(!script.contains("UPDATE _oo.") && !script.contains("DELETE FROM"));
    }
}

pub async fn install_native_mysql_complete_handoff(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const VERSION: &str = "native-mysql-complete-handoff/v1";
    const FILE: &str = "0115_native_mysql_complete_handoff.sql";
    const SCRIPT: &str = include_str!("../migrations/0115_native_mysql_complete_handoff.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let previous = "native-mysql-handoff-initial-resume/v1";
    if !matches!(find_row(&rows,previous),Some(row) if row_exact(row,previous,
        &sha256_hex(include_bytes!("../migrations/0114_native_mysql_handoff_initial_resume.sql")),
        "0114_native_mysql_handoff_initial_resume.sql"))
    {
        return Err(InstallError::Refused {
            message: "Complete MySQL handoff requires exact 0114".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_oracle_position_capacity(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    for (version, file, script) in [
        (
            "native-c1-source-identity/v1",
            "0079_native_c1_source_identity.sql",
            include_str!("../migrations/0079_native_c1_source_identity.sql"),
        ),
        (
            "native-c1-oracle-tail/v1",
            "0082_native_c1_oracle_tail.sql",
            include_str!("../migrations/0082_native_c1_oracle_tail.sql"),
        ),
        (
            VERSION_NATIVE_C1_COMPLETE_SCOPE,
            FILE_NATIVE_C1_COMPLETE_SCOPE,
            SCRIPT_NATIVE_C1_COMPLETE_SCOPE,
        ),
    ] {
        if !matches!(find_row(&rows, version), Some(row) if row_exact(row, version, &sha256_hex(script.as_bytes()), file))
        {
            return Err(InstallError::Refused {
                message: format!("Oracle position capacity requires exact {file}"),
            });
        }
    }
    let digest = sha256_hex(SCRIPT_NATIVE_ORACLE_POSITION_CAPACITY.as_bytes());
    if let Some(row) = find_row(&rows, VERSION_NATIVE_ORACLE_POSITION_CAPACITY) {
        if !row_exact(
            row,
            VERSION_NATIVE_ORACLE_POSITION_CAPACITY,
            &digest,
            FILE_NATIVE_ORACLE_POSITION_CAPACITY,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_ORACLE_POSITION_CAPACITY.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_ORACLE_POSITION_CAPACITY).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_ORACLE_POSITION_CAPACITY,
                &digest,
                FILE_NATIVE_ORACLE_POSITION_CAPACITY,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

#[cfg(test)]
mod oracle_position_capacity_tests {
    use super::*;

    #[test]
    fn capacity_migration_preserves_all_non_capacity_guards_and_permissions() {
        let original = include_str!("../migrations/0082_native_c1_oracle_tail.sql");
        let position = original
            .split_once("CREATE OR REPLACE FUNCTION _oo.native_position_closed(")
            .unwrap()
            .1
            .split_once("$$;")
            .unwrap()
            .0;
        let position = format!("CREATE OR REPLACE FUNCTION _oo.native_position_closed({position}$$;")
            .replacen("p_lineage ~ '^oracle", "p_lineage IS NOT NULL AND p_lineage ~ '^oracle", 1)
            .replace("AND p_pos IS NOT NULL AND p_pos >= 0 AND p_pos = trunc(p_pos) AND p_pos <= 1000000",
                "AND p_pos IS NOT NULL AND p_pos >= 0 AND p_pos = trunc(p_pos)\n      AND _oo.native_pos_text(p_pos) ~ '^(0|[1-9][0-9]{0,37})$'");
        let record = include_str!("../migrations/0079_native_c1_source_identity.sql")
            .split_once("CREATE FUNCTION _oo.c1_typed_record_oracle_position(")
            .unwrap()
            .1
            .split_once("END $fn$;")
            .unwrap()
            .0;
        let record = format!(
            "CREATE OR REPLACE FUNCTION _oo.c1_typed_record_oracle_position({record}END $fn$;"
        )
        .replace("n NOT BETWEEN 1 AND 100000", "n<1");
        let body = SCRIPT_NATIVE_ORACLE_POSITION_CAPACITY
            .split_once("CREATE OR REPLACE FUNCTION")
            .unwrap()
            .1;
        assert_eq!(
            format!("CREATE OR REPLACE FUNCTION{body}"),
            format!("{position}\n\n{record}\n")
        );
    }
}

#[cfg(test)]
mod complete_scope_tests {
    use super::*;

    #[test]
    fn migration_changes_only_total_cardinality_guards() {
        let publication = include_str!("../migrations/0084_native_c1_source_names.sql")
            .split_once("CREATE OR REPLACE FUNCTION _oo.publish_native_c1_collection")
            .unwrap()
            .1;
        let publication =
            format!("CREATE OR REPLACE FUNCTION _oo.publish_native_c1_collection{publication}")
                .replace("n<1 OR n>32 OR", "n<1 OR");
        let library = include_str!("../migrations/0092_native_library_mysql_receipt.sql")
            .split_once("CREATE OR REPLACE FUNCTION")
            .unwrap()
            .1;
        let library = format!("CREATE OR REPLACE FUNCTION{library}")
            .replace("n IS NULL OR n<1 OR n>32", "n IS NULL OR n<1");
        fn first(text: &str) -> String {
            let function = text
                .split_once("CREATE FUNCTION")
                .unwrap()
                .1
                .split_once("END $fn$;")
                .unwrap()
                .0;
            format!("CREATE FUNCTION{function}END $fn$;\n")
        }
        let fill = first(include_str!(
            "../migrations/0097_native_c1_oracle_fill_resume.sql"
        ))
        .replace(
            "CREATE FUNCTION _oo.c1_oracle_fill_check(",
            "CREATE OR REPLACE FUNCTION _oo.c1_oracle_fill_check_0099_base(",
        )
        .replace("total NOT BETWEEN 1 AND 4096", "total<1");
        let mixed = first(include_str!(
            "../migrations/0099_native_c1_oracle_mixed_resume.sql"
        ))
        .replacen("CREATE FUNCTION", "CREATE OR REPLACE FUNCTION", 1)
        .replace("total NOT BETWEEN 2 AND 4096", "total<2");
        let buffer = include_str!("../migrations/0087_native_c1_oracle_continuous.sql")
            .split_once("CREATE OR REPLACE FUNCTION _oo.c1_typed_record_buffer_0080")
            .unwrap()
            .1
            .split_once("END $fn$;")
            .unwrap()
            .0;
        let buffer = format!(
            "CREATE OR REPLACE FUNCTION _oo.c1_typed_record_buffer_0080{buffer}END $fn$;\n"
        )
        .replace(
            "(checkpoint->>'event_count')::bigint NOT BETWEEN 1 AND 100000",
            "(checkpoint->>'event_count')::bigint<1",
        );
        let body = SCRIPT_NATIVE_C1_COMPLETE_SCOPE
            .split_once("CREATE OR REPLACE FUNCTION")
            .unwrap()
            .1;
        assert_eq!(
            format!("CREATE OR REPLACE FUNCTION{body}").trim_end(),
            format!("{publication}\n\n{library}\n{fill}\n{mixed}\n{buffer}").trim_end()
        );
    }
}

pub async fn install_native_c5_range_parts_stage(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const VERSION: &str = "native-c5-range-parts-stage/v1";
    const FILE: &str = "0103_native_c5_range_parts_stage.sql";
    const SCRIPT: &str = include_str!("../migrations/0103_native_c5_range_parts_stage.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prerequisite =
        sha256_hex(include_str!("../migrations/0075_native_c5_indexed_exclusion.sql").as_bytes());
    if !matches!(find_row(&rows,"native-c5-indexed-exclusion/v1"),Some(row)
        if row_exact(row,"native-c5-indexed-exclusion/v1",&prerequisite,"0075_native_c5_indexed_exclusion.sql"))
    {
        return Err(InstallError::Refused {
            message: "C5 parts stage requires exact indexed C5 overlay".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        // Read-only control authority; the stage API never edits jobs or ACKs.
        exec_static(
            &mut tx,
            "GRANT USAGE ON SCHEMA jw_native TO oo_target_owner",
        )
        .await?;
        exec_static(&mut tx,"GRANT SELECT ON jw_native.bindings,jw_native.occurrences,jw_native.source_progress TO oo_target_owner").await?;
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_C5_RANGE_PARTS_APPLY: &str = "native-c5-range-parts-apply/v1";
pub fn compiled_native_c5_range_parts_apply_digest() -> String {
    sha256_hex(include_str!("../migrations/0104_native_c5_range_parts_apply.sql").as_bytes())
}

pub async fn install_native_c5_range_parts(conn: &mut PgConnection) -> Result<(), InstallError> {
    install_native_c5_range_parts_stage(conn).await?;
    const FILE: &str = "0104_native_c5_range_parts_apply.sql";
    const SCRIPT: &str = include_str!("../migrations/0104_native_c5_range_parts_apply.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let version = VERSION_NATIVE_C5_RANGE_PARTS_APPLY;
    let digest = compiled_native_c5_range_parts_apply_digest();
    if let Some(row) = find_row(&rows, version) {
        if !row_exact(row, version, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: version.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(version, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_C5_WIDENING_REBASE: &str = "native-c5-widening-rebase/v1";
pub fn compiled_native_c5_widening_rebase_digest() -> String {
    sha256_hex(include_str!("../migrations/0105_native_c5_widening_rebase.sql").as_bytes())
}
pub const VERSION_NATIVE_C5_WIDENING_TYPE_RECEIPT: &str = "native-c5-widening-type-receipt/v1";
pub fn compiled_native_c5_widening_type_receipt_digest() -> String {
    sha256_hex(include_str!("../migrations/0106_native_c5_widening_type_receipt.sql").as_bytes())
}
pub const VERSION_NATIVE_C5_WIDENING_SOURCE_TYPES: &str = "native-c5-widening-source-types/v1";
pub fn compiled_native_c5_widening_source_types_digest() -> String {
    sha256_hex(include_str!("../migrations/0107_native_c5_widening_source_types.sql").as_bytes())
}

pub const VERSION_NATIVE_C5_PRIVILEGED_OBSERVER: &str = "native-c5-privileged-observer/v1";
pub fn compiled_native_c5_privileged_observer_digest() -> String {
    sha256_hex(include_str!("../migrations/0108_native_c5_privileged_observer.sql").as_bytes())
}

pub async fn install_native_c5_privileged_observer(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const FILE: &str = "0108_native_c5_privileged_observer.sql";
    const SCRIPT: &str = include_str!("../migrations/0108_native_c5_privileged_observer.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prerequisite =
        sha256_hex(include_str!("../migrations/0068_native_c5_overlay_visibility.sql").as_bytes());
    if !matches!(find_row(&rows, "native-c5-overlay-visibility/v1"), Some(row)
        if row_exact(row, "native-c5-overlay-visibility/v1", &prerequisite, "0068_native_c5_overlay_visibility.sql"))
    {
        return Err(InstallError::Refused {
            message: "privileged observer requires exact C5 visibility".into(),
        });
    }
    let version = VERSION_NATIVE_C5_PRIVILEGED_OBSERVER;
    let digest = compiled_native_c5_privileged_observer_digest();
    if let Some(row) = find_row(&rows, version) {
        if !row_exact(row, version, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: version.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(version, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c5_widening_rebase(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    install_native_c5_range_parts(conn).await?;
    const FILE: &str = "0105_native_c5_widening_rebase.sql";
    const SCRIPT: &str = include_str!("../migrations/0105_native_c5_widening_rebase.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let version = VERSION_NATIVE_C5_WIDENING_REBASE;
    let digest = compiled_native_c5_widening_rebase_digest();
    if let Some(row) = find_row(&rows, version) {
        if !row_exact(row, version, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: version.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(version, &digest, FILE)]).await?;
    }
    const TYPE_FILE: &str = "0106_native_c5_widening_type_receipt.sql";
    let type_version = VERSION_NATIVE_C5_WIDENING_TYPE_RECEIPT;
    let type_digest = compiled_native_c5_widening_type_receipt_digest();
    if let Some(row) = find_row(&rows, type_version) {
        if !row_exact(row, type_version, &type_digest, TYPE_FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: type_version.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(
            &mut tx,
            include_str!("../migrations/0106_native_c5_widening_type_receipt.sql"),
        )
        .await?;
        insert_markers(&mut tx, &[marker(type_version, &type_digest, TYPE_FILE)]).await?;
    }
    const SOURCE_FILE: &str = "0107_native_c5_widening_source_types.sql";
    let source_version = VERSION_NATIVE_C5_WIDENING_SOURCE_TYPES;
    let source_digest = compiled_native_c5_widening_source_types_digest();
    if let Some(row) = find_row(&rows, source_version) {
        if !row_exact(row, source_version, &source_digest, SOURCE_FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: source_version.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(
            &mut tx,
            include_str!("../migrations/0107_native_c5_widening_source_types.sql"),
        )
        .await?;
        insert_markers(
            &mut tx,
            &[marker(source_version, &source_digest, SOURCE_FILE)],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_mysql_delta(conn: &mut PgConnection) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-mysql-delta/v1";
    const FILE: &str = "0088_native_c1_mysql_delta.sql";
    const SCRIPT: &str = include_str!("../migrations/0088_native_c1_mysql_delta.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    if !matches!(find_row(&rows,"native-c1-oracle-continuous/v1"),Some(row) if row_exact(row,
        "native-c1-oracle-continuous/v1",&sha256_hex(include_str!("../migrations/0087_native_c1_oracle_continuous.sql").as_bytes()),
        "0087_native_c1_oracle_continuous.sql"))
    {
        return Err(InstallError::Refused {
            message: "MySQL delta requires exact 0087".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_oracle_continuous(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-oracle-continuous/v1";
    const FILE: &str = "0087_native_c1_oracle_continuous.sql";
    const SCRIPT: &str = include_str!("../migrations/0087_native_c1_oracle_continuous.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    if !matches!(find_row(&rows,"native-c1-oracle-delta-verify/v1"),Some(row) if row_exact(row,
        "native-c1-oracle-delta-verify/v1",&sha256_hex(include_str!("../migrations/0086_native_c1_oracle_delta_verify.sql").as_bytes()),
        "0086_native_c1_oracle_delta_verify.sql"))
    {
        return Err(InstallError::Refused {
            message: "Oracle continuous delta requires exact 0086".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_oracle_delta_verify(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-oracle-delta-verify/v1";
    const FILE: &str = "0086_native_c1_oracle_delta_verify.sql";
    const SCRIPT: &str = include_str!("../migrations/0086_native_c1_oracle_delta_verify.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    if !matches!(find_row(&rows,"native-c1-oracle-delta/v1"),Some(row) if row_exact(row,
        "native-c1-oracle-delta/v1",&sha256_hex(include_str!("../migrations/0085_native_c1_oracle_delta.sql").as_bytes()),
        "0085_native_c1_oracle_delta.sql"))
    {
        return Err(InstallError::Refused {
            message: "Oracle delta final verification requires exact 0085".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_oracle_delta(conn: &mut PgConnection) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-oracle-delta/v1";
    const FILE: &str = "0085_native_c1_oracle_delta.sql";
    const SCRIPT: &str = include_str!("../migrations/0085_native_c1_oracle_delta.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    if !matches!(find_row(&rows,"native-c1-source-names/v1"),Some(row) if row_exact(row,
        "native-c1-source-names/v1",&sha256_hex(include_str!("../migrations/0084_native_c1_source_names.sql").as_bytes()),
        "0084_native_c1_source_names.sql"))
    {
        return Err(InstallError::Refused {
            message: "Oracle delta requires exact 0084".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_source_names(conn: &mut PgConnection) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-source-names/v1";
    const FILE: &str = "0084_native_c1_source_names.sql";
    const SCRIPT: &str = include_str!("../migrations/0084_native_c1_source_names.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    if !matches!(find_row(&rows,"native-c1-export-binding/v1"),Some(row) if row_exact(row,
        "native-c1-export-binding/v1",&sha256_hex(include_str!("../migrations/0083_native_c1_export_binding.sql").as_bytes()),
        "0083_native_c1_export_binding.sql"))
    {
        return Err(InstallError::Refused {
            message: "source name bridge requires exact 0083".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

/// Immutable 0044 version: the integer advisory-lock grant used by source-instance registration.
pub const VERSION_NATIVE_SOURCE_INSTANCE_GRANT: &str = "native-source-instance-advisory-grant/v1";
const FILE_NATIVE_SOURCE_INSTANCE_GRANT: &str = "0044_native_source_instance_advisory_grant.sql";
const SCRIPT_NATIVE_SOURCE_INSTANCE_GRANT: &str =
    include_str!("../migrations/0044_native_source_instance_advisory_grant.sql");

pub fn compiled_native_source_instance_grant_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_SOURCE_INSTANCE_GRANT.as_bytes())
}

/// Grant `oo_target_owner` the `pg_advisory_xact_lock(integer, integer)` overload (F2).
/// Runs after the exact 0043 install. It runs as the installing principal, because only
/// a superuser or the function owner may grant on `pg_catalog`.
pub async fn install_native_source_instance_grant(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_c1_chunked_baseline_final_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_C1_CHUNKED_BASELINE_FINAL), Some(row)
        if row_exact(row, VERSION_NATIVE_C1_CHUNKED_BASELINE_FINAL, &prior, FILE_NATIVE_C1_CHUNKED_BASELINE_FINAL))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "source instance grant requires exact 0043".into(),
        });
    }
    let digest = compiled_native_source_instance_grant_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_SOURCE_INSTANCE_GRANT) {
        if !row_exact(
            row,
            VERSION_NATIVE_SOURCE_INSTANCE_GRANT,
            &digest,
            FILE_NATIVE_SOURCE_INSTANCE_GRANT,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_SOURCE_INSTANCE_GRANT.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_script(&mut tx, SCRIPT_NATIVE_SOURCE_INSTANCE_GRANT).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_SOURCE_INSTANCE_GRANT,
                &digest,
                FILE_NATIVE_SOURCE_INSTANCE_GRANT,
            )],
        )
        .await
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Install v2 successor format handoff and task-scoped seal batch identities.
pub async fn install_native_c1_ack_seed_format(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const VERSION: &str = "native-c1-ack-seed-format/v1";
    const FILE: &str = "0045_native_c1_ack_seed_format.sql";
    const SCRIPT: &str = include_str!("../migrations/0045_native_c1_ack_seed_format.sql");
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_source_instance_grant_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_SOURCE_INSTANCE_GRANT), Some(row)
        if row_exact(row, VERSION_NATIVE_SOURCE_INSTANCE_GRANT, &prior, FILE_NATIVE_SOURCE_INSTANCE_GRANT))
    {
        return Err(InstallError::Refused {
            message: "C1 seed format requires exact 0044".into(),
        });
    }
    if find_row(&rows, "native-c1-v2-seal-batch-identity/v1").is_some()
        && find_row(&rows, VERSION).is_none()
    {
        return Err(InstallError::Refused {
            message: "C1 seal batch requires its seed format predecessor".into(),
        });
    }
    for (version, file, script) in [
        (VERSION, FILE, SCRIPT),
        (
            "native-c1-v2-seal-batch-identity/v1",
            "0046_native_c1_v2_seal_batch_identity.sql",
            include_str!("../migrations/0046_native_c1_v2_seal_batch_identity.sql"),
        ),
    ] {
        let digest = sha256_hex(script.as_bytes());
        if let Some(row) = find_row(&rows, version) {
            if !row_exact(row, version, &digest, file) {
                return mismatch(
                    tx,
                    Mismatch {
                        version: version.into(),
                        stored: row.checksum.clone(),
                    },
                )
                .await
                .map(|_| ());
            }
            continue;
        }
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, script).await?;
        insert_markers(&mut tx, &[marker(version, &digest, file)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

/// Install the chunked-typed/v2 final-load helpers and guards after the exact 0042 install.
pub async fn install_native_c1_chunked_baseline_final(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_c1_chunked_baseline_digest();
    if !matches!(find_row(&rows, VERSION_NATIVE_C1_CHUNKED_BASELINE), Some(row)
        if row_exact(row, VERSION_NATIVE_C1_CHUNKED_BASELINE, &prior, FILE_NATIVE_C1_CHUNKED_BASELINE))
    {
        let _ = tx.rollback().await;
        return Err(InstallError::Refused {
            message: "C1 chunked baseline final requires exact 0042".into(),
        });
    }
    let digest = compiled_native_c1_chunked_baseline_final_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C1_CHUNKED_BASELINE_FINAL) {
        if !row_exact(
            row,
            VERSION_NATIVE_C1_CHUNKED_BASELINE_FINAL,
            &digest,
            FILE_NATIVE_C1_CHUNKED_BASELINE_FINAL,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C1_CHUNKED_BASELINE_FINAL.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C1_CHUNKED_BASELINE_FINAL).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C1_CHUNKED_BASELINE_FINAL,
                &digest,
                FILE_NATIVE_C1_CHUNKED_BASELINE_FINAL,
            )],
        )
        .await
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Publish an ACKed multi-run C1 cut through the existing release transaction.
pub async fn install_native_c1_timed_receipt(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    for (version, digest, file) in [(
        VERSION_NATIVE_C1_COLLECTION_PUBLICATION,
        compiled_native_c1_collection_publication_digest(),
        FILE_NATIVE_C1_COLLECTION_PUBLICATION,
    )] {
        if !matches!(find_row(&rows,version),Some(row) if row_exact(row,version,&digest,file)) {
            let _ = tx.rollback().await;
            return Err(InstallError::Refused {
                message: "C1 timed receipts require exact 0036".into(),
            });
        }
    }
    let digest = compiled_native_c1_timed_receipt_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C1_TIMED_RECEIPT) {
        if !row_exact(
            row,
            VERSION_NATIVE_C1_TIMED_RECEIPT,
            &digest,
            FILE_NATIVE_C1_TIMED_RECEIPT,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C1_TIMED_RECEIPT.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C1_TIMED_RECEIPT).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C1_TIMED_RECEIPT,
                &digest,
                FILE_NATIVE_C1_TIMED_RECEIPT,
            )],
        )
        .await
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_c1_quiet_baseline_time(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        let _ = tx.rollback().await;
        return Err(err);
    }
    let rows = read_rows(&mut tx).await?;
    for (version, digest, file) in [(
        VERSION_NATIVE_C1_TIMED_RECEIPT,
        compiled_native_c1_timed_receipt_digest(),
        FILE_NATIVE_C1_TIMED_RECEIPT,
    )] {
        if !matches!(find_row(&rows,version),Some(row) if row_exact(row,version,&digest,file)) {
            let _ = tx.rollback().await;
            return Err(InstallError::Refused {
                message: "C1 timed receipts require exact 0037".into(),
            });
        }
    }
    let digest = compiled_native_c1_quiet_baseline_time_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C1_QUIET_BASELINE_TIME) {
        if !row_exact(
            row,
            VERSION_NATIVE_C1_QUIET_BASELINE_TIME,
            &digest,
            FILE_NATIVE_C1_QUIET_BASELINE_TIME,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C1_QUIET_BASELINE_TIME.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(());
    }
    let result = async {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C1_QUIET_BASELINE_TIME).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C1_QUIET_BASELINE_TIME,
                &digest,
                FILE_NATIVE_C1_QUIET_BASELINE_TIME,
            )],
        )
        .await
    }
    .await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Apply the jobs-owner mirror recovery successor only on a target with jobs.
pub async fn install_jobs_mirror_rebind(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal.superuser || !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    let result = async {
        exec_static(&mut tx, "SET LOCAL lock_timeout = '15s'").await?;
        exec_static(&mut tx, "SET LOCAL statement_timeout = '60s'").await?;
        exec_static(&mut tx, "SELECT pg_advisory_xact_lock(6401202601)").await?;
        exec_static(&mut tx, "SELECT id FROM jobs.migration_lock WHERE id = 1 FOR UPDATE").await?;
        for (version, file, script) in [
            ("jobs-control/1", "0001_jobs_control_expand.sql", include_str!("../../oo-jobs/migrations/0001_jobs_control_expand.sql")),
            ("jobs-event-project/1", "0002_event_project_binding_expand.sql", include_str!("../../oo-jobs/migrations/0002_event_project_binding_expand.sql")),
            ("jobs-lease-operations/1", "0003_job_lease_operations_expand.sql", include_str!("../../oo-jobs/migrations/0003_job_lease_operations_expand.sql")),
        ] {
            let row: Option<(String, String)> = sqlx::query_as(
                "SELECT checksum,filename FROM jobs.schema_migrations WHERE version=$1"
            ).bind(version).fetch_optional(&mut *tx).await.map_err(database_error)?;
            if row != Some((sha256_hex(script.as_bytes()), file.to_string())) {
                return Err(InstallError::Refused { message: "jobs predecessor differs".to_string() });
            }
        }
        let prior_digest = sha256_hex(SCRIPT_JOBS_MIRROR_REBIND.as_bytes());
        let installed: Option<(String, String)> = sqlx::query_as(
            "SELECT checksum,filename FROM jobs.schema_migrations WHERE version='jobs-mirror-rebind/1'"
        ).fetch_optional(&mut *tx).await.map_err(database_error)?;
        if let Some(row) = &installed
            && row != &(prior_digest.clone(), FILE_JOBS_MIRROR_REBIND.to_string()) {
                return Err(InstallError::Refused { message: "jobs mirror checksum differs".to_string() });
            }
        let retry_digest = sha256_hex(SCRIPT_JOBS_MIRROR_RETRY.as_bytes());
        let current: Option<(String, String)> = sqlx::query_as(
            "SELECT checksum,filename FROM jobs.schema_migrations WHERE version='jobs-mirror-retry-run-at/1'"
        ).fetch_optional(&mut *tx).await.map_err(database_error)?;
        if let Some(row) = &current
            && (installed.is_none() || row != &(retry_digest.clone(), FILE_JOBS_MIRROR_RETRY.to_string())) {
                return Err(InstallError::Refused { message: "jobs mirror retry checksum differs".to_string() });
            }
        let sync_digest = sha256_hex(SCRIPT_JOBS_MIRROR_SYNC.as_bytes());
        let synchronized: Option<(String, String)> = sqlx::query_as(
            "SELECT checksum,filename FROM jobs.schema_migrations WHERE version='jobs-mirror-sync/1'"
        ).fetch_optional(&mut *tx).await.map_err(database_error)?;
        if let Some(row) = &synchronized
            && (current.is_none() || row != &(sync_digest.clone(), FILE_JOBS_MIRROR_SYNC.to_string())) {
                return Err(InstallError::Refused { message: "jobs mirror sync checksum differs".to_string() });
            }
        let digest = compiled_jobs_mirror_rebind_digest();
        let read_grants: Option<(String, String)> = sqlx::query_as(
            "SELECT checksum,filename FROM jobs.schema_migrations WHERE version='jobs-mirror-read/1'"
        ).fetch_optional(&mut *tx).await.map_err(database_error)?;
        if let Some(row) = &read_grants
            && (synchronized.is_none() || row != &(digest.clone(), FILE_JOBS_MIRROR_READ.to_string())) {
                return Err(InstallError::Refused { message: "jobs mirror read checksum differs".to_string() });
            }
        let receipt_digest = compiled_jobs_receipt_read_digest();
        let receipt_grants: Option<(String, String)> = sqlx::query_as(
            "SELECT checksum,filename FROM jobs.schema_migrations WHERE version='jobs-native-receipt-read/1'"
        ).fetch_optional(&mut *tx).await.map_err(database_error)?;
        if let Some(row) = &receipt_grants {
            if read_grants.is_none() || row != &(receipt_digest.clone(), FILE_JOBS_RECEIPT_READ.to_string()) {
                return Err(InstallError::Refused { message: "jobs receipt read checksum differs".to_string() });
            }
            return Ok::<(), InstallError>(());
        }
        let prior: Option<String> = sqlx::query_scalar(
            "SELECT to_regprocedure('jobs.loader_hold(uuid,bigint)')::text"
        ).fetch_one(&mut *tx).await.map_err(database_error)?;
        if prior.is_none() {
            return Err(InstallError::Refused { message: "jobs loader hold is absent".to_string() });
        }
        exec_static(&mut tx, "GRANT EXECUTE ON FUNCTION pg_catalog.set_config(text,text,boolean) TO oo_jobs_owner").await?;
        exec_static(&mut tx, "SET LOCAL ROLE oo_jobs_owner").await?;
        if installed.is_none() {
            exec_script(&mut tx, SCRIPT_JOBS_MIRROR_REBIND).await?;
            sqlx::query("INSERT INTO jobs.schema_migrations(version,checksum,phase,filename) VALUES ('jobs-mirror-rebind/1',$1,'expand',$2)")
                .bind(&prior_digest).bind(FILE_JOBS_MIRROR_REBIND)
                .execute(&mut *tx).await.map_err(database_error)?;
        }
        if current.is_none() {
            exec_script(&mut tx, SCRIPT_JOBS_MIRROR_RETRY).await?;
            sqlx::query("INSERT INTO jobs.schema_migrations(version,checksum,phase,filename) VALUES ('jobs-mirror-retry-run-at/1',$1,'expand',$2)")
                .bind(&retry_digest).bind(FILE_JOBS_MIRROR_RETRY)
                .execute(&mut *tx).await.map_err(database_error)?;
        }
        if synchronized.is_none() {
            exec_script(&mut tx, SCRIPT_JOBS_MIRROR_SYNC).await?;
            sqlx::query("INSERT INTO jobs.schema_migrations(version,checksum,phase,filename) VALUES ('jobs-mirror-sync/1',$1,'expand',$2)")
                .bind(&sync_digest).bind(FILE_JOBS_MIRROR_SYNC)
                .execute(&mut *tx).await.map_err(database_error)?;
        }
        exec_static(&mut tx, "RESET ROLE").await?;
        if read_grants.is_none() {
            exec_script(&mut tx, SCRIPT_JOBS_MIRROR_READ).await?;
            sqlx::query("INSERT INTO jobs.schema_migrations(version,checksum,phase,filename) VALUES ('jobs-mirror-read/1',$1,'expand',$2)")
                .bind(&digest).bind(FILE_JOBS_MIRROR_READ)
                .execute(&mut *tx).await.map_err(database_error)?;
        }
        exec_script(&mut tx, SCRIPT_JOBS_RECEIPT_READ).await?;
        sqlx::query("INSERT INTO jobs.schema_migrations(version,checksum,phase,filename) VALUES ('jobs-native-receipt-read/1',$1,'expand',$2)")
            .bind(&receipt_digest).bind(FILE_JOBS_RECEIPT_READ)
            .execute(&mut *tx).await.map_err(database_error)?;
        Ok(())
    }.await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
}

/// Install an exact-task claim function on the control DB; no target schema is required.
pub async fn install_jobs_claim_selected(conn: &mut PgConnection) -> Result<(), InstallError> {
    if !read_principal(conn).await?.superuser {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    let result = async {
        exec_static(&mut tx, "SET LOCAL lock_timeout = '15s'").await?;
        exec_static(&mut tx, "SET LOCAL statement_timeout = '60s'").await?;
        exec_static(&mut tx, "SELECT pg_advisory_xact_lock(6401202601)").await?;
        exec_static(&mut tx, "SELECT id FROM jobs.migration_lock WHERE id = 1 FOR UPDATE").await?;
        for (version, file, script) in [
            ("jobs-control/1", "0001_jobs_control_expand.sql", include_str!("../../oo-jobs/migrations/0001_jobs_control_expand.sql")),
            ("jobs-event-project/1", "0002_event_project_binding_expand.sql", include_str!("../../oo-jobs/migrations/0002_event_project_binding_expand.sql")),
            ("jobs-lease-operations/1", "0003_job_lease_operations_expand.sql", include_str!("../../oo-jobs/migrations/0003_job_lease_operations_expand.sql")),
        ] {
            let row: Option<(String, String)> = sqlx::query_as(
                "SELECT checksum,filename FROM jobs.schema_migrations WHERE version=$1"
            ).bind(version).fetch_optional(&mut *tx).await.map_err(database_error)?;
            if row != Some((sha256_hex(script.as_bytes()), file.to_string())) {
                return Err(InstallError::Refused { message: "jobs predecessor differs".to_string() });
            }
        }
        let claim: Option<String> = sqlx::query_scalar(
            "SELECT to_regprocedure('jobs.claim(uuid,text[],text)')::text"
        ).fetch_one(&mut *tx).await.map_err(database_error)?;
        if claim.is_none() {
            return Err(InstallError::Refused { message: "jobs claim is absent".to_string() });
        }
        let digest = compiled_jobs_claim_selected_digest();
        let installed: Option<(String, String)> = sqlx::query_as(
            "SELECT checksum,filename FROM jobs.schema_migrations WHERE version='jobs-claim-selected/1'"
        ).fetch_optional(&mut *tx).await.map_err(database_error)?;
        if let Some(row) = installed {
            if row != (digest, FILE_JOBS_CLAIM_SELECTED.to_string()) {
                return Err(InstallError::Refused { message: "jobs selected claim checksum differs".to_string() });
            }
            return Ok::<(), InstallError>(());
        }
        exec_static(&mut tx, "SET LOCAL ROLE oo_jobs_owner").await?;
        exec_script(&mut tx, SCRIPT_JOBS_CLAIM_SELECTED).await?;
        sqlx::query("INSERT INTO jobs.schema_migrations(version,checksum,phase,filename) VALUES ('jobs-claim-selected/1',$1,'expand',$2)")
            .bind(&digest).bind(FILE_JOBS_CLAIM_SELECTED)
            .execute(&mut *tx).await.map_err(database_error)?;
        Ok(())
    }.await;
    if let Err(err) = result {
        let _ = tx.rollback().await;
        return Err(err);
    }
    tx.commit().await.map_err(database_error)
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

/// Install `0006` after `full-import/v1` is exact.
///
/// This does not call [`install`] or [`install_full_import`] and does not
/// change `0000` through `0005`. A stored checksum that differs raises `OO001`
/// and does not update the row. An exact row is a replay. The script does not
/// copy rows.
pub async fn install_sealed_catalog(
    conn: &mut PgConnection,
) -> Result<SealedCatalogInstallOutcome, InstallError> {
    let digest = compiled_sealed_catalog_digest();
    let full_digest = compiled_full_import_digest();
    let digests = compiled_digests();
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        return rollback_sealed(tx, err).await;
    }
    let catalog = match read_catalog(&mut tx).await {
        Ok(catalog) => catalog,
        Err(err) => return rollback_sealed(tx, err).await,
    };
    let rows = if catalog.migrations {
        match read_rows(&mut tx).await {
            Ok(rows) => rows,
            Err(err) => return rollback_sealed(tx, err).await,
        }
    } else {
        Vec::new()
    };
    if let Some(hit) = checksum_mismatch(&rows, &digests) {
        return mismatch_sealed(tx, hit).await;
    }
    if !production_exact(&rows, &digests) {
        return rollback_sealed(
            tx,
            InstallError::Refused {
                message: "sealed catalog requires the production target schema".to_string(),
            },
        )
        .await;
    }
    match find_row(&rows, VERSION_FULL_IMPORT) {
        Some(row) if row_exact(row, VERSION_FULL_IMPORT, &full_digest, FILE_FULL_IMPORT) => {}
        Some(row) => {
            return mismatch_sealed(
                tx,
                Mismatch {
                    version: VERSION_FULL_IMPORT.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await;
        }
        None => {
            return rollback_sealed(
                tx,
                InstallError::Refused {
                    message: "sealed catalog requires the full import schema".to_string(),
                },
            )
            .await;
        }
    }
    if let Some(row) = find_row(&rows, VERSION_SEALED_CATALOG) {
        if !row_exact(row, VERSION_SEALED_CATALOG, &digest, FILE_SEALED_CATALOG) {
            return mismatch_sealed(
                tx,
                Mismatch {
                    version: VERSION_SEALED_CATALOG.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await;
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(SealedCatalogInstallOutcome::Replay);
    }
    if let Err(err) = exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await {
        return rollback_sealed(tx, err).await;
    }
    if let Err(err) = exec_script(&mut tx, SCRIPT_SEALED_CATALOG).await {
        return rollback_sealed(tx, err).await;
    }
    if let Err(err) = insert_markers(
        &mut tx,
        &[marker(VERSION_SEALED_CATALOG, &digest, FILE_SEALED_CATALOG)],
    )
    .await
    {
        return rollback_sealed(tx, err).await;
    }
    tx.commit().await.map_err(database_error)?;
    Ok(SealedCatalogInstallOutcome::Installed)
}

/// Compare `sealed-import-catalog/v1` with the compiled file.
///
/// This does not install, `SET ROLE`, or take the migration lock. Full import
/// must already be exact.
pub async fn verify_sealed_catalog_readiness(
    conn: &mut PgConnection,
) -> Result<VerifiedSealedCatalog, ReadinessError> {
    let full = verify_full_import_readiness(conn).await?;
    if full.digest() != compiled_full_import_digest() {
        return Err(ReadinessError::Incomplete);
    }
    let mut tx = conn.begin().await.map_err(readiness_database)?;
    if let Err(err) = exec_static(&mut tx, "SET TRANSACTION READ ONLY").await {
        let _ = tx.rollback().await;
        return Err(readiness_from_install(err));
    }
    let rows = match read_rows(&mut tx).await {
        Ok(rows) => rows,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(readiness_from_install(err));
        }
    };
    tx.commit().await.map_err(readiness_database)?;
    let digest = compiled_sealed_catalog_digest();
    match find_row(&rows, VERSION_SEALED_CATALOG) {
        Some(row) if row_exact(row, VERSION_SEALED_CATALOG, &digest, FILE_SEALED_CATALOG) => {
            Ok(VerifiedSealedCatalog { digest })
        }
        Some(row) => Err(ReadinessError::ChecksumMismatch {
            version: row.version.clone(),
            stored: row.checksum.clone(),
        }),
        None => Err(ReadinessError::Incomplete),
    }
}

/// Install `0008` after `sealed-import-catalog/v1` is exact.
///
/// This does not execute `0006` and does not change `0000` through `0007`.
/// A stored checksum that differs raises `OO001` and does not update the row.
pub async fn install_native_sealed_catalog(
    conn: &mut PgConnection,
) -> Result<NativeCatalogInstallOutcome, InstallError> {
    let digest = compiled_native_catalog_digest();
    let sealed_digest = compiled_sealed_catalog_digest();
    let full_digest = compiled_full_import_digest();
    let digests = compiled_digests();
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    if let Err(err) = prepare_locked(&mut tx, &principal).await {
        return rollback_native(tx, err).await;
    }
    let catalog = match read_catalog(&mut tx).await {
        Ok(catalog) => catalog,
        Err(err) => return rollback_native(tx, err).await,
    };
    let rows = if catalog.migrations {
        match read_rows(&mut tx).await {
            Ok(rows) => rows,
            Err(err) => return rollback_native(tx, err).await,
        }
    } else {
        Vec::new()
    };
    if let Some(hit) = checksum_mismatch(&rows, &digests) {
        return mismatch_native(tx, hit).await;
    }
    if !production_exact(&rows, &digests) {
        return rollback_native(
            tx,
            InstallError::Refused {
                message: "native sealed catalog requires the production target schema".to_string(),
            },
        )
        .await;
    }
    match find_row(&rows, VERSION_FULL_IMPORT) {
        Some(row) if row_exact(row, VERSION_FULL_IMPORT, &full_digest, FILE_FULL_IMPORT) => {}
        Some(row) => {
            return mismatch_native(
                tx,
                Mismatch {
                    version: VERSION_FULL_IMPORT.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await;
        }
        None => {
            return rollback_native(
                tx,
                InstallError::Refused {
                    message: "native sealed catalog requires the full import schema".to_string(),
                },
            )
            .await;
        }
    }
    match find_row(&rows, VERSION_SEALED_CATALOG) {
        Some(row)
            if row_exact(
                row,
                VERSION_SEALED_CATALOG,
                &sealed_digest,
                FILE_SEALED_CATALOG,
            ) => {}
        Some(row) => {
            return mismatch_native(
                tx,
                Mismatch {
                    version: VERSION_SEALED_CATALOG.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await;
        }
        None => {
            return rollback_native(
                tx,
                InstallError::Refused {
                    message: "native sealed catalog requires the sealed import schema".to_string(),
                },
            )
            .await;
        }
    }
    if let Some(row) = find_row(&rows, VERSION_NATIVE_CATALOG) {
        if !row_exact(row, VERSION_NATIVE_CATALOG, &digest, FILE_NATIVE_CATALOG) {
            return mismatch_native(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_CATALOG.to_string(),
                    stored: row.checksum.clone(),
                },
            )
            .await;
        }
        tx.commit().await.map_err(database_error)?;
        return Ok(NativeCatalogInstallOutcome::Replay);
    }
    if let Err(err) = exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await {
        return rollback_native(tx, err).await;
    }
    if let Err(err) = exec_script(&mut tx, SCRIPT_NATIVE_CATALOG).await {
        return rollback_native(tx, err).await;
    }
    if let Err(err) = insert_markers(
        &mut tx,
        &[marker(VERSION_NATIVE_CATALOG, &digest, FILE_NATIVE_CATALOG)],
    )
    .await
    {
        return rollback_native(tx, err).await;
    }
    tx.commit().await.map_err(database_error)?;
    Ok(NativeCatalogInstallOutcome::Installed)
}

/// Compare `native-sealed-catalog/v1` with the compiled file.
///
/// This does not install. `0006` must already be exact.
pub async fn verify_native_catalog_readiness(
    conn: &mut PgConnection,
) -> Result<VerifiedNativeCatalog, ReadinessError> {
    let sealed = verify_sealed_catalog_readiness(conn).await?;
    if sealed.digest() != compiled_sealed_catalog_digest() {
        return Err(ReadinessError::Incomplete);
    }
    let mut tx = conn.begin().await.map_err(readiness_database)?;
    if let Err(err) = exec_static(&mut tx, "SET TRANSACTION READ ONLY").await {
        let _ = tx.rollback().await;
        return Err(readiness_from_install(err));
    }
    let rows = match read_rows(&mut tx).await {
        Ok(rows) => rows,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(readiness_from_install(err));
        }
    };
    tx.commit().await.map_err(readiness_database)?;
    let digest = compiled_native_catalog_digest();
    match find_row(&rows, VERSION_NATIVE_CATALOG) {
        Some(row) if row_exact(row, VERSION_NATIVE_CATALOG, &digest, FILE_NATIVE_CATALOG) => {
            Ok(VerifiedNativeCatalog { digest })
        }
        Some(row) => Err(ReadinessError::ChecksumMismatch {
            version: row.version.clone(),
            stored: row.checksum.clone(),
        }),
        None => Err(ReadinessError::Incomplete),
    }
}

async fn rollback_native(
    tx: sqlx::Transaction<'_, sqlx::Postgres>,
    err: InstallError,
) -> Result<NativeCatalogInstallOutcome, InstallError> {
    let _ = tx.rollback().await;
    Err(err)
}

async fn mismatch_native(
    mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
    hit: Mismatch,
) -> Result<NativeCatalogInstallOutcome, InstallError> {
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

async fn rollback_sealed(
    tx: sqlx::Transaction<'_, sqlx::Postgres>,
    err: InstallError,
) -> Result<SealedCatalogInstallOutcome, InstallError> {
    let _ = tx.rollback().await;
    Err(err)
}

async fn mismatch_sealed(
    mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
    hit: Mismatch,
) -> Result<SealedCatalogInstallOutcome, InstallError> {
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
    let visible: bool =
        sqlx::query_scalar("SELECT pg_catalog.pg_has_role('pg_read_all_stats', 'member')")
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

async fn rollback_jsonb(
    tx: sqlx::Transaction<'_, sqlx::Postgres>,
    err: InstallError,
) -> Result<JsonbDocumentInstallOutcome, InstallError> {
    let _ = tx.rollback().await;
    Err(err)
}

async fn rollback_native_update(
    tx: sqlx::Transaction<'_, sqlx::Postgres>,
    err: InstallError,
) -> Result<NativeUpdateInstallOutcome, InstallError> {
    let _ = tx.rollback().await;
    Err(err)
}

async fn mismatch_native_update(
    tx: sqlx::Transaction<'_, sqlx::Postgres>,
    hit: Mismatch,
) -> Result<NativeUpdateInstallOutcome, InstallError> {
    match mismatch(tx, hit).await {
        Err(err) => Err(err),
        Ok(_) => Err(InstallError::Refused {
            message: "checksum mismatch did not raise OO001".to_string(),
        }),
    }
}

async fn mismatch_jsonb(
    tx: sqlx::Transaction<'_, sqlx::Postgres>,
    hit: Mismatch,
) -> Result<JsonbDocumentInstallOutcome, InstallError> {
    match mismatch(tx, hit).await {
        Err(err) => Err(err),
        Ok(_) => Err(InstallError::Refused {
            message: "checksum mismatch did not raise OO001".to_string(),
        }),
    }
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
    // Roles are cluster-wide. The bootstrap route separately validates
    // their exact privilege definition before reusing them in a new DB.
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
        Some(row) => row_exact(
            row,
            VERSION_SEGMENT_V2,
            HISTORICAL_V2_SHA256,
            FILE_SEGMENT_V2,
        ),
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
            let roles = cluster_role_state(conn).await?;
            let bootstrap = match roles {
                ClusterRoles::Absent => SCRIPT_BOOTSTRAP,
                ClusterRoles::Exact => bootstrap_grant_sql()?,
                ClusterRoles::Wrong => {
                    return Err(InstallError::Refused {
                        message:
                            "existing cluster roles do not match the closed pipeline definition"
                                .to_string(),
                    });
                }
            };
            exec_script(conn, bootstrap).await?;
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
        && marker
            .checksum
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
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
    let superuser: bool =
        sqlx::query_scalar("SELECT rolsuper FROM pg_catalog.pg_roles WHERE rolname = current_user")
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
    scalar_bool(
        conn,
        "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_target_owner')",
    )
    .await?;
    scalar_bool(
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

enum ClusterRoles {
    Absent,
    Exact,
    Wrong,
}

fn closed_nologin_role(
    login: bool,
    superuser: bool,
    createdb: bool,
    createrole: bool,
    inherit: bool,
    bypassrls: bool,
    replication: bool,
) -> bool {
    !login && !superuser && !createdb && !createrole && !inherit && !bypassrls && !replication
}

fn bootstrap_grant_sql() -> Result<&'static str, InstallError> {
    const MARK: &str = "\nALTER ROLE oo_target_owner SET search_path = pg_catalog;\n";
    let Some(at) = SCRIPT_BOOTSTRAP.find(MARK) else {
        return Err(InstallError::Refused {
            message: "bootstrap script has no database grant section".to_string(),
        });
    };
    Ok(&SCRIPT_BOOTSTRAP[at + 1..])
}

async fn cluster_role_state(conn: &mut PgConnection) -> Result<ClusterRoles, InstallError> {
    type RoleRow = (String, bool, bool, bool, bool, bool, bool, bool);
    let rows: Vec<RoleRow> = sqlx::query_as(
        "SELECT rolname, rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, \
         rolbypassrls, rolreplication FROM pg_catalog.pg_roles \
         WHERE rolname IN ('oo_target_owner', 'oo_pipeline_runtime')",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(database_error)?;
    if rows.is_empty() {
        return Ok(ClusterRoles::Absent);
    }
    if rows.len() != 2 {
        return Ok(ClusterRoles::Wrong);
    }
    let mut owner = false;
    let mut runtime = false;
    for (name, login, superuser, createdb, createrole, inherit, bypass, replication) in rows {
        if !closed_nologin_role(
            login,
            superuser,
            createdb,
            createrole,
            inherit,
            bypass,
            replication,
        ) {
            return Ok(ClusterRoles::Wrong);
        }
        match name.as_str() {
            "oo_target_owner" => owner = true,
            "oo_pipeline_runtime" => runtime = true,
            _ => return Ok(ClusterRoles::Wrong),
        }
    }
    if owner && runtime {
        Ok(ClusterRoles::Exact)
    } else {
        Ok(ClusterRoles::Wrong)
    }
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
        InstallError::Database { sqlstate, message } => {
            ReadinessError::Database { sqlstate, message }
        }
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
    fn sqlserver_ct_keeps_installed_partition_codec() {
        assert!(SCRIPT_NATIVE_SQLSERVER_CT.contains("'partition-signature/v1'"));
        assert!(SCRIPT_NATIVE_SQLSERVER_CT.contains("'mssql-ct-version/v1'"));
        assert!(SCRIPT_NATIVE_SQLSERVER_CT.contains("native_sqlserver_ct_copy_forward"));
        assert!(install_order_calls_catalog_before_empty());
        assert!(SCRIPT_NATIVE_GTID_TRANSACTION.contains("_oo.native_gtid_transaction_read(text,text,text,integer,numeric)") || SCRIPT_NATIVE_GTID_TRANSACTION.contains("p_source text,\n  p_codec text,\n  p_lineage text,\n  p_epoch integer,\n  p_pos numeric"));
        assert!(SCRIPT_NATIVE_GTID_TRANSACTION.contains("native_gtid_transaction_commit"));
        assert!(SCRIPT_NATIVE_GTID_TRANSACTION.contains("native_source_instance_register"));
        assert!(
            !SCRIPT_NATIVE_GTID_TRANSACTION
                .contains("CREATE OR REPLACE FUNCTION _oo.native_transaction_commit")
        );
        assert!(!SCRIPT_NATIVE_GTID_TRANSACTION.contains("GREATEST"));
        assert!(SCRIPT_NATIVE_C1_SOURCE_EPOCH.contains("p_source_epoch integer"));
        assert!(
            SCRIPT_NATIVE_C1_SOURCE_EPOCH.contains("instance.measured_id = p_position_lineage")
        );
        assert!(SCRIPT_NATIVE_C1_SOURCE_EPOCH.contains("SET source_epoch = p_source_epoch"));
        assert!(!SCRIPT_NATIVE_C1_SOURCE_EPOCH.contains("DROP FUNCTION"));
        assert!(SCRIPT_NATIVE_C1_SOURCE_EPOCH_RUNTIME.contains("REVOKE ALL ON FUNCTION"));
        assert!(SCRIPT_NATIVE_C1_SOURCE_EPOCH_RUNTIME.contains("TO oo_pipeline_runtime"));
        assert!(SCRIPT_NATIVE_PUBLICATION.contains("_oo.publish_native_full_import"));
        assert!(SCRIPT_NATIVE_PUBLICATION.contains("native_publication_receipt_guard"));
        let source = include_str!("native_update.rs");
        let gtids = source.find("install_native_gtid_transaction").unwrap();
        let epoch = source
            .find("install_native_c1_source_epoch(&mut conn)")
            .unwrap();
        let runtime = source
            .find("install_native_c1_source_epoch_runtime(&mut conn)")
            .unwrap();
        assert!(gtids < epoch && epoch < runtime);
        if let Some(publication) = source.find("install_native_publication(&mut conn)") {
            assert!(runtime < publication);
        }
    }

    fn install_order_calls_catalog_before_empty() -> bool {
        let source = include_str!("native_update.rs");
        let catalog = source
            .find("install_catalog_before_empty")
            .unwrap_or(usize::MAX);
        let empty = source.find("install_native_empty_catalog").unwrap_or(0);
        let ct = source.find("install_native_sqlserver_ct").unwrap_or(0);
        catalog < empty && empty < ct
    }

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
        let grants = bootstrap_grant_sql().expect("grant section");
        assert!(grants.starts_with("ALTER ROLE oo_target_owner SET search_path = pg_catalog;"));
        assert!(!grants.contains("CREATE ROLE"));
        assert!(grants.contains("GRANT CONNECT ON DATABASE"));
        assert!(closed_nologin_role(
            false, false, false, false, false, false, false
        ));
        assert!(!closed_nologin_role(
            true, false, false, false, false, false, false
        ));
        assert!(!closed_nologin_role(
            false, true, false, false, false, false, false
        ));
        assert!(!closed_nologin_role(
            false, false, false, false, true, false, false
        ));
    }

    #[test]
    fn full_import_digest_matches_the_contract_file() {
        let digest = compiled_full_import_digest();
        assert_eq!(digest.len(), 64);
        assert!(
            digest
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert_ne!(digest, compiled_digests().mapping());
        assert!(!SCRIPT_FULL_IMPORT.contains("COPY "));
        assert!(!SCRIPT_SEALED_CATALOG.contains("set_config"));
        assert!(!SCRIPT_SEALED_CATALOG.contains("pg_attribute"));
        assert!(!SCRIPT_SEALED_CATALOG.contains("COPY "));
        assert!(SCRIPT_SEALED_CATALOG.contains("sealed import is partial"));
        assert!(
            SCRIPT_SEALED_CATALOG
                .contains("GRANT EXECUTE ON FUNCTION _oo.full_import_sealed_identity")
        );
        assert_eq!(compiled_sealed_catalog_digest().len(), 64);
        assert_eq!(compiled_native_catalog_digest().len(), 64);
        assert!(SCRIPT_NATIVE_CATALOG.contains("full_import_native_catalog"));
        assert!(SCRIPT_NATIVE_CATALOG.contains("pg_attribute"));
        assert!(!SCRIPT_NATIVE_CATALOG.contains("COPY"));
        assert!(!SCRIPT_NATIVE_CATALOG.contains("set_config"));
        assert!(SCRIPT_FULL_IMPORT_JSONB.contains("RETURN 'jsonb'"));
        assert!(SCRIPT_FULL_IMPORT_JSONB.contains("RETURN 'json'"));
        assert!(SCRIPT_FULL_IMPORT_JSONB.contains("p_oid = 3802"));
        assert!(SCRIPT_FULL_IMPORT_JSONB.contains("closed jsonb typmod"));
        assert!(!SCRIPT_FULL_IMPORT_JSONB.contains("COPY "));
        assert!(!SCRIPT_FULL_IMPORT.contains("3802"));
        assert_eq!(compiled_jsonb_document_digest().len(), 64);
        let contract = include_str!("../../../contracts/pipeline/full-import-v1.json");
        assert!(contract.contains(&digest));
        assert!(
            !contract.contains("37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208")
        );
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
        v2.push(stored(
            VERSION_SEGMENT_V2,
            HISTORICAL_V2_SHA256,
            FILE_SEGMENT_V2,
        ));
        assert!(matches!(
            route_for(&present, &v2, &digests),
            Ok(Route::UpgradeFromV2)
        ));
        let mut ready = v2.clone();
        ready.push(stored(
            VERSION_PREFLIGHT,
            digests.preflight(),
            FILE_PREFLIGHT,
        ));
        ready.push(stored(VERSION_MAPPING, digests.mapping(), FILE_MAPPING));
        assert!(matches!(
            route_for(&present, &ready, &digests),
            Ok(Route::Replay)
        ));
        let mut direct = v1.clone();
        direct.push(stored(
            VERSION_PREFLIGHT,
            digests.preflight(),
            FILE_PREFLIGHT,
        ));
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
            stored(
                VERSION_FENCE,
                "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                FILE_FENCE,
            ),
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

/// Immutable typed CDC-first stage bridge version.
pub const VERSION_NATIVE_C1_BASELINE_TYPED_BRIDGE: &str = "native-c1-baseline-typed-bridge/v1";
const FILE_NATIVE_C1_BASELINE_TYPED_BRIDGE: &str = "0031_native_c1_baseline_typed_bridge.sql";
const SCRIPT_NATIVE_C1_BASELINE_TYPED_BRIDGE: &str =
    include_str!("../migrations/0031_native_c1_baseline_typed_bridge.sql");
/// Compiled SHA-256 of the append-only typed bootstrap bridge.
pub fn compiled_native_c1_baseline_typed_bridge_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C1_BASELINE_TYPED_BRIDGE.as_bytes())
}

/// Immutable same-task C1 retry lease adoption version.
pub const VERSION_NATIVE_C1_BASELINE_LEASE_REBIND: &str = "native-c1-baseline-lease-rebind/v1";
const FILE_NATIVE_C1_BASELINE_LEASE_REBIND: &str = "0032_native_c1_baseline_lease_rebind.sql";
const SCRIPT_NATIVE_C1_BASELINE_LEASE_REBIND: &str =
    include_str!("../migrations/0032_native_c1_baseline_lease_rebind.sql");
/// SHA-256 of the same-task retry adoption migration.
pub fn compiled_native_c1_baseline_lease_rebind_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C1_BASELINE_LEASE_REBIND.as_bytes())
}

const VERSION_NATIVE_C1_BASELINE_DATE_QUALITY: &str = "native-c1-baseline-date-quality/v1";
const FILE_NATIVE_C1_BASELINE_DATE_QUALITY: &str = "0033_native_c1_baseline_date_quality.sql";
const SCRIPT_NATIVE_C1_BASELINE_DATE_QUALITY: &str =
    include_str!("../migrations/0033_native_c1_baseline_date_quality.sql");
pub fn compiled_native_c1_baseline_date_quality_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C1_BASELINE_DATE_QUALITY.as_bytes())
}

const VERSION_NATIVE_C1_BASELINE_KEY_JOIN: &str = "native-c1-baseline-key-join/v1";
const FILE_NATIVE_C1_BASELINE_KEY_JOIN: &str = "0034_native_c1_baseline_key_join.sql";
const SCRIPT_NATIVE_C1_BASELINE_KEY_JOIN: &str =
    include_str!("../migrations/0034_native_c1_baseline_key_join.sql");
pub fn compiled_native_c1_baseline_key_join_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C1_BASELINE_KEY_JOIN.as_bytes())
}

const VERSION_NATIVE_C1_ACK_SEED: &str = "native-c1-ack-seed/v1";
const FILE_NATIVE_C1_ACK_SEED: &str = "0035_native_c1_ack_seed.sql";
const SCRIPT_NATIVE_C1_ACK_SEED: &str = include_str!("../migrations/0035_native_c1_ack_seed.sql");
pub fn compiled_native_c1_ack_seed_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C1_ACK_SEED.as_bytes())
}

const VERSION_NATIVE_C1_COLLECTION_PUBLICATION: &str = "native-c1-collection-publication/v1";
const FILE_NATIVE_C1_COLLECTION_PUBLICATION: &str = "0036_native_c1_collection_publication.sql";
const SCRIPT_NATIVE_C1_COLLECTION_PUBLICATION: &str =
    include_str!("../migrations/0036_native_c1_collection_publication.sql");
pub fn compiled_native_c1_collection_publication_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C1_COLLECTION_PUBLICATION.as_bytes())
}

const VERSION_NATIVE_C1_TIMED_RECEIPT: &str = "native-c1-timed-receipt/v1";
const FILE_NATIVE_C1_TIMED_RECEIPT: &str = "0037_native_c1_timed_receipt.sql";
const SCRIPT_NATIVE_C1_TIMED_RECEIPT: &str =
    include_str!("../migrations/0037_native_c1_timed_receipt.sql");
pub fn compiled_native_c1_timed_receipt_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C1_TIMED_RECEIPT.as_bytes())
}

const VERSION_NATIVE_C1_QUIET_BASELINE_TIME: &str = "native-c1-quiet-baseline-time/v1";
const FILE_NATIVE_C1_QUIET_BASELINE_TIME: &str = "0038_native_c1_quiet_baseline_time.sql";
const SCRIPT_NATIVE_C1_QUIET_BASELINE_TIME: &str =
    include_str!("../migrations/0038_native_c1_quiet_baseline_time.sql");
pub fn compiled_native_c1_quiet_baseline_time_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C1_QUIET_BASELINE_TIME.as_bytes())
}

const VERSION_NATIVE_PER_TABLE_PUBLICATION: &str = "native-per-table-publication/v1";
const FILE_NATIVE_PER_TABLE_PUBLICATION: &str = "0039_native_per_table_publication.sql";
const SCRIPT_NATIVE_PER_TABLE_PUBLICATION: &str =
    include_str!("../migrations/0039_native_per_table_publication.sql");
pub fn compiled_native_per_table_publication_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_PER_TABLE_PUBLICATION.as_bytes())
}

const FILE_NATIVE_SOURCE_PROGRESS_READ: &str = "0040_native_source_progress_read.sql";
const SCRIPT_NATIVE_SOURCE_PROGRESS_READ: &str =
    include_str!("../migrations/0040_native_source_progress_read.sql");
pub fn compiled_native_source_progress_read_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_SOURCE_PROGRESS_READ.as_bytes())
}

const FILE_NATIVE_PARTITION_OBSERVATION_READ: &str = "0041_native_partition_observation_read.sql";
const SCRIPT_NATIVE_PARTITION_OBSERVATION_READ: &str =
    include_str!("../migrations/0041_native_partition_observation_read.sql");
pub fn compiled_native_partition_observation_read_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_PARTITION_OBSERVATION_READ.as_bytes())
}

/// Immutable chunked-typed/v2 C1 baseline migration version (0042).
pub const VERSION_NATIVE_C1_CHUNKED_BASELINE: &str = "native-c1-chunked-baseline/v1";
const FILE_NATIVE_C1_CHUNKED_BASELINE: &str = "0042_native_c1_chunked_baseline.sql";
const SCRIPT_NATIVE_C1_CHUNKED_BASELINE: &str =
    include_str!("../migrations/0042_native_c1_chunked_baseline.sql");
/// SHA-256 of the chunked C1 baseline migration bytes.
pub fn compiled_native_c1_chunked_baseline_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C1_CHUNKED_BASELINE.as_bytes())
}

/// Immutable chunked-typed/v2 final-load migration version (0043).
pub const VERSION_NATIVE_C1_CHUNKED_BASELINE_FINAL: &str = "native-c1-chunked-baseline-final/v1";
const FILE_NATIVE_C1_CHUNKED_BASELINE_FINAL: &str = "0043_native_c1_chunked_baseline_final.sql";
const SCRIPT_NATIVE_C1_CHUNKED_BASELINE_FINAL: &str =
    include_str!("../migrations/0043_native_c1_chunked_baseline_final.sql");
/// SHA-256 of the chunked C1 final-load migration bytes.
pub fn compiled_native_c1_chunked_baseline_final_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C1_CHUNKED_BASELINE_FINAL.as_bytes())
}

pub const VERSION_NATIVE_C5_RANGE: &str = "native-c5-range/v1";
const FILE_NATIVE_C5_RANGE: &str = "0047_native_c5_range.sql";
const SCRIPT_NATIVE_C5_RANGE: &str = include_str!("../migrations/0047_native_c5_range.sql");

pub fn compiled_native_c5_range_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C5_RANGE.as_bytes())
}

pub async fn install_native_c5_range(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior = sha256_hex(
        include_str!("../migrations/0046_native_c1_v2_seal_batch_identity.sql").as_bytes(),
    );
    if !matches!(find_row(&rows,"native-c1-v2-seal-batch-identity/v1"),Some(row)
        if row_exact(row,"native-c1-v2-seal-batch-identity/v1",&prior,"0046_native_c1_v2_seal_batch_identity.sql"))
    {
        return Err(InstallError::Refused {
            message: "C5 range requires exact 0046".into(),
        });
    }
    let digest = compiled_native_c5_range_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C5_RANGE) {
        if !row_exact(row, VERSION_NATIVE_C5_RANGE, &digest, FILE_NATIVE_C5_RANGE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C5_RANGE.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C5_RANGE).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C5_RANGE,
                &digest,
                FILE_NATIVE_C5_RANGE,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_C5_RANGE_APPLY: &str = "native-c5-range-apply/v1";
const FILE_NATIVE_C5_RANGE_APPLY: &str = "0048_native_c5_range_apply.sql";
const SCRIPT_NATIVE_C5_RANGE_APPLY: &str =
    include_str!("../migrations/0048_native_c5_range_apply.sql");

pub fn compiled_native_c5_range_apply_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C5_RANGE_APPLY.as_bytes())
}

pub async fn install_native_c5_range_apply(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior = sha256_hex(include_str!("../migrations/0047_native_c5_range.sql").as_bytes());
    if !matches!(find_row(&rows,"native-c5-range/v1"),Some(row)
        if row_exact(row,"native-c5-range/v1",&prior,"0047_native_c5_range.sql"))
    {
        return Err(InstallError::Refused {
            message: "C5 range requires exact 0047".into(),
        });
    }
    let digest = compiled_native_c5_range_apply_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C5_RANGE_APPLY) {
        if !row_exact(
            row,
            VERSION_NATIVE_C5_RANGE_APPLY,
            &digest,
            FILE_NATIVE_C5_RANGE_APPLY,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C5_RANGE_APPLY.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C5_RANGE_APPLY).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C5_RANGE_APPLY,
                &digest,
                FILE_NATIVE_C5_RANGE_APPLY,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_C5_SCAN: &str = "native-c5-scan/v1";
const FILE_NATIVE_C5_SCAN: &str = "0049_native_c5_scan.sql";
const SCRIPT_NATIVE_C5_SCAN: &str = include_str!("../migrations/0049_native_c5_scan.sql");

pub fn compiled_native_c5_scan_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C5_SCAN.as_bytes())
}

pub async fn install_native_c5_scan(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior = sha256_hex(include_str!("../migrations/0048_native_c5_range_apply.sql").as_bytes());
    if !matches!(find_row(&rows,"native-c5-range-apply/v1"),Some(row)
        if row_exact(row,"native-c5-range-apply/v1",&prior,"0048_native_c5_range_apply.sql"))
    {
        return Err(InstallError::Refused {
            message: "C5 range requires exact 0048".into(),
        });
    }
    let digest = compiled_native_c5_scan_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C5_SCAN) {
        if !row_exact(row, VERSION_NATIVE_C5_SCAN, &digest, FILE_NATIVE_C5_SCAN) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C5_SCAN.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C5_SCAN).await?;
        insert_markers(
            &mut tx,
            &[marker(VERSION_NATIVE_C5_SCAN, &digest, FILE_NATIVE_C5_SCAN)],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_C5_REPLAY_GUARD: &str = "native-c5-replay-guard/v1";
const FILE_NATIVE_C5_REPLAY_GUARD: &str = "0050_native_c5_replay_guard.sql";
const SCRIPT_NATIVE_C5_REPLAY_GUARD: &str =
    include_str!("../migrations/0050_native_c5_replay_guard.sql");

pub fn compiled_native_c5_replay_guard_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C5_REPLAY_GUARD.as_bytes())
}

pub async fn install_native_c5_replay_guard(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior = sha256_hex(include_str!("../migrations/0049_native_c5_scan.sql").as_bytes());
    if !matches!(find_row(&rows,"native-c5-scan/v1"),Some(row)
        if row_exact(row,"native-c5-scan/v1",&prior,"0049_native_c5_scan.sql"))
    {
        return Err(InstallError::Refused {
            message: "C5 range requires exact 0049".into(),
        });
    }
    let digest = compiled_native_c5_replay_guard_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C5_REPLAY_GUARD) {
        if !row_exact(
            row,
            VERSION_NATIVE_C5_REPLAY_GUARD,
            &digest,
            FILE_NATIVE_C5_REPLAY_GUARD,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C5_REPLAY_GUARD.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C5_REPLAY_GUARD).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C5_REPLAY_GUARD,
                &digest,
                FILE_NATIVE_C5_REPLAY_GUARD,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_C5_FULL_INITIALIZE: &str = "native-c5-full-initialize/v1";
const FILE_NATIVE_C5_FULL_INITIALIZE: &str = "0051_native_c5_full_initialize.sql";
const SCRIPT_NATIVE_C5_FULL_INITIALIZE: &str =
    include_str!("../migrations/0051_native_c5_full_initialize.sql");

pub fn compiled_native_c5_full_initialize_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C5_FULL_INITIALIZE.as_bytes())
}

pub async fn install_native_c5_full_initialize(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior =
        sha256_hex(include_str!("../migrations/0050_native_c5_replay_guard.sql").as_bytes());
    if !matches!(find_row(&rows,"native-c5-replay-guard/v1"),Some(row)
        if row_exact(row,"native-c5-replay-guard/v1",&prior,"0050_native_c5_replay_guard.sql"))
    {
        return Err(InstallError::Refused {
            message: "C5 range requires exact 0050".into(),
        });
    }
    let digest = compiled_native_c5_full_initialize_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C5_FULL_INITIALIZE) {
        if !row_exact(
            row,
            VERSION_NATIVE_C5_FULL_INITIALIZE,
            &digest,
            FILE_NATIVE_C5_FULL_INITIALIZE,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C5_FULL_INITIALIZE.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C5_FULL_INITIALIZE).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C5_FULL_INITIALIZE,
                &digest,
                FILE_NATIVE_C5_FULL_INITIALIZE,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_C5_CLOSED_OBSERVATION: &str = "native-c5-closed-observation/v1";
const FILE_NATIVE_C5_CLOSED_OBSERVATION: &str = "0052_native_c5_closed_observation.sql";
const SCRIPT_NATIVE_C5_CLOSED_OBSERVATION: &str =
    include_str!("../migrations/0052_native_c5_closed_observation.sql");

pub fn compiled_native_c5_closed_observation_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C5_CLOSED_OBSERVATION.as_bytes())
}

pub async fn install_native_c5_closed_observation(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior =
        sha256_hex(include_str!("../migrations/0051_native_c5_full_initialize.sql").as_bytes());
    if !matches!(find_row(&rows,"native-c5-full-initialize/v1"),Some(row)
        if row_exact(row,"native-c5-full-initialize/v1",&prior,"0051_native_c5_full_initialize.sql"))
    {
        return Err(InstallError::Refused {
            message: "C5 range requires exact 0051".into(),
        });
    }
    let digest = compiled_native_c5_closed_observation_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C5_CLOSED_OBSERVATION) {
        if !row_exact(
            row,
            VERSION_NATIVE_C5_CLOSED_OBSERVATION,
            &digest,
            FILE_NATIVE_C5_CLOSED_OBSERVATION,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C5_CLOSED_OBSERVATION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C5_CLOSED_OBSERVATION).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C5_CLOSED_OBSERVATION,
                &digest,
                FILE_NATIVE_C5_CLOSED_OBSERVATION,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_C5_MULTIPART: &str = "native-c5-multipart/v1";
const FILE_NATIVE_C5_MULTIPART: &str = "0053_native_c5_multipart.sql";
const SCRIPT_NATIVE_C5_MULTIPART: &str = include_str!("../migrations/0053_native_c5_multipart.sql");
pub fn compiled_native_c5_multipart_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C5_MULTIPART.as_bytes())
}

pub async fn install_native_c5_multipart(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior =
        sha256_hex(include_str!("../migrations/0052_native_c5_closed_observation.sql").as_bytes());
    if !matches!(find_row(&rows,"native-c5-closed-observation/v1"),Some(row)
        if row_exact(row,"native-c5-closed-observation/v1",&prior,"0052_native_c5_closed_observation.sql"))
    {
        return Err(InstallError::Refused {
            message: "C5 range requires exact 0052".into(),
        });
    }
    let digest = compiled_native_c5_multipart_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C5_MULTIPART) {
        if !row_exact(
            row,
            VERSION_NATIVE_C5_MULTIPART,
            &digest,
            FILE_NATIVE_C5_MULTIPART,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C5_MULTIPART.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C5_MULTIPART).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C5_MULTIPART,
                &digest,
                FILE_NATIVE_C5_MULTIPART,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_MASKED_REJECTS: &str = "masked-rejects/v1";
const FILE_MASKED_REJECTS: &str = "0054_masked_rejects.sql";
const SCRIPT_MASKED_REJECTS: &str = include_str!("../migrations/0054_masked_rejects.sql");
pub fn compiled_masked_rejects_digest() -> String {
    sha256_hex(SCRIPT_MASKED_REJECTS.as_bytes())
}
pub async fn install_masked_rejects(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior = sha256_hex(include_str!("../migrations/0053_native_c5_multipart.sql").as_bytes());
    if !matches!(find_row(&rows,"native-c5-multipart/v1"),Some(row)
        if row_exact(row,"native-c5-multipart/v1",&prior,"0053_native_c5_multipart.sql"))
    {
        return Err(InstallError::Refused {
            message: "masked rejects require exact 0053".into(),
        });
    }
    let digest = compiled_masked_rejects_digest();
    if let Some(row) = find_row(&rows, VERSION_MASKED_REJECTS) {
        if !row_exact(row, VERSION_MASKED_REJECTS, &digest, FILE_MASKED_REJECTS) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_MASKED_REJECTS.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_MASKED_REJECTS).await?;
        insert_markers(
            &mut tx,
            &[marker(VERSION_MASKED_REJECTS, &digest, FILE_MASKED_REJECTS)],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_MULTIPART_CELLS: &str = "multipart-cells/v1";
const FILE_MULTIPART_CELLS: &str = "0055_multipart_cells.sql";
const SCRIPT_MULTIPART_CELLS: &str = include_str!("../migrations/0055_multipart_cells.sql");
pub fn compiled_multipart_cells_digest() -> String {
    sha256_hex(SCRIPT_MULTIPART_CELLS.as_bytes())
}
pub async fn install_multipart_cells(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior = sha256_hex(include_str!("../migrations/0054_masked_rejects.sql").as_bytes());
    if !matches!(find_row(&rows,"masked-rejects/v1"),Some(row)
        if row_exact(row,"masked-rejects/v1",&prior,"0054_masked_rejects.sql"))
    {
        return Err(InstallError::Refused {
            message: "multipart cells require exact 0054".into(),
        });
    }
    let digest = compiled_multipart_cells_digest();
    if let Some(row) = find_row(&rows, VERSION_MULTIPART_CELLS) {
        if !row_exact(row, VERSION_MULTIPART_CELLS, &digest, FILE_MULTIPART_CELLS) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_MULTIPART_CELLS.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_MULTIPART_CELLS).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_MULTIPART_CELLS,
                &digest,
                FILE_MULTIPART_CELLS,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_C5_PUBLICATION: &str = "native-c5-publication/v1";
const FILE_NATIVE_C5_PUBLICATION: &str = "0056_native_c5_publication.sql";
const SCRIPT_NATIVE_C5_PUBLICATION: &str =
    include_str!("../migrations/0056_native_c5_publication.sql");

pub fn compiled_native_c5_publication_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C5_PUBLICATION.as_bytes())
}

pub async fn install_native_c5_publication(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior = sha256_hex(include_str!("../migrations/0055_multipart_cells.sql").as_bytes());
    if !matches!(find_row(&rows,"multipart-cells/v1"),Some(row)
        if row_exact(row,"multipart-cells/v1",&prior,"0055_multipart_cells.sql"))
    {
        return Err(InstallError::Refused {
            message: "C5 range requires exact 0055".into(),
        });
    }
    let digest = compiled_native_c5_publication_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C5_PUBLICATION) {
        if !row_exact(
            row,
            VERSION_NATIVE_C5_PUBLICATION,
            &digest,
            FILE_NATIVE_C5_PUBLICATION,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C5_PUBLICATION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C5_PUBLICATION).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C5_PUBLICATION,
                &digest,
                FILE_NATIVE_C5_PUBLICATION,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_C5_SINGLE_BASELINE: &str = "native-c5-single-baseline/v1";
const FILE_NATIVE_C5_SINGLE_BASELINE: &str = "0057_native_c5_single_baseline.sql";
const SCRIPT_NATIVE_C5_SINGLE_BASELINE: &str =
    include_str!("../migrations/0057_native_c5_single_baseline.sql");

pub fn compiled_native_c5_single_baseline_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C5_SINGLE_BASELINE.as_bytes())
}

pub async fn install_native_c5_single_baseline(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior = sha256_hex(include_str!("../migrations/0056_native_c5_publication.sql").as_bytes());
    if !matches!(find_row(&rows,"native-c5-publication/v1"),Some(row)
        if row_exact(row,"native-c5-publication/v1",&prior,"0056_native_c5_publication.sql"))
    {
        return Err(InstallError::Refused {
            message: "C5 range requires exact 0056".into(),
        });
    }
    let digest = compiled_native_c5_single_baseline_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C5_SINGLE_BASELINE) {
        if !row_exact(
            row,
            VERSION_NATIVE_C5_SINGLE_BASELINE,
            &digest,
            FILE_NATIVE_C5_SINGLE_BASELINE,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C5_SINGLE_BASELINE.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C5_SINGLE_BASELINE).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C5_SINGLE_BASELINE,
                &digest,
                FILE_NATIVE_C5_SINGLE_BASELINE,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_C5_MULTIPART_BASELINE_GUARD: &str =
    "native-c5-multipart-baseline-guard/v1";
const FILE_NATIVE_C5_MULTIPART_BASELINE_GUARD: &str = "0058_native_c5_multipart_baseline_guard.sql";
const SCRIPT_NATIVE_C5_MULTIPART_BASELINE_GUARD: &str =
    include_str!("../migrations/0058_native_c5_multipart_baseline_guard.sql");

pub fn compiled_native_c5_multipart_baseline_guard_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C5_MULTIPART_BASELINE_GUARD.as_bytes())
}

pub async fn install_native_c5_multipart_baseline_guard(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior =
        sha256_hex(include_str!("../migrations/0057_native_c5_single_baseline.sql").as_bytes());
    if !matches!(find_row(&rows,"native-c5-single-baseline/v1"),Some(row)
        if row_exact(row,"native-c5-single-baseline/v1",&prior,"0057_native_c5_single_baseline.sql"))
    {
        return Err(InstallError::Refused {
            message: "C5 range requires exact 0057".into(),
        });
    }
    let digest = compiled_native_c5_multipart_baseline_guard_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C5_MULTIPART_BASELINE_GUARD) {
        if !row_exact(
            row,
            VERSION_NATIVE_C5_MULTIPART_BASELINE_GUARD,
            &digest,
            FILE_NATIVE_C5_MULTIPART_BASELINE_GUARD,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C5_MULTIPART_BASELINE_GUARD.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C5_MULTIPART_BASELINE_GUARD).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C5_MULTIPART_BASELINE_GUARD,
                &digest,
                FILE_NATIVE_C5_MULTIPART_BASELINE_GUARD,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_C5_WIDE_COLUMNS: &str = "native-c5-wide-columns/v1";
const FILE_NATIVE_C5_WIDE_COLUMNS: &str = "0059_native_c5_wide_columns.sql";
const SCRIPT_NATIVE_C5_WIDE_COLUMNS: &str =
    include_str!("../migrations/0059_native_c5_wide_columns.sql");

pub fn compiled_native_c5_wide_columns_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C5_WIDE_COLUMNS.as_bytes())
}

pub async fn install_native_c5_wide_columns(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior = sha256_hex(
        include_str!("../migrations/0058_native_c5_multipart_baseline_guard.sql").as_bytes(),
    );
    if !matches!(find_row(&rows,"native-c5-multipart-baseline-guard/v1"),Some(row)
        if row_exact(row,"native-c5-multipart-baseline-guard/v1",&prior,"0058_native_c5_multipart_baseline_guard.sql"))
    {
        return Err(InstallError::Refused {
            message: "wide columns require exact 0058".into(),
        });
    }
    let digest = compiled_native_c5_wide_columns_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C5_WIDE_COLUMNS) {
        if !row_exact(
            row,
            VERSION_NATIVE_C5_WIDE_COLUMNS,
            &digest,
            FILE_NATIVE_C5_WIDE_COLUMNS,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C5_WIDE_COLUMNS.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C5_WIDE_COLUMNS).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C5_WIDE_COLUMNS,
                &digest,
                FILE_NATIVE_C5_WIDE_COLUMNS,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_C5_CELLS_ARRAY: &str = "native-c5-cells-array/v1";
const FILE_NATIVE_C5_CELLS_ARRAY: &str = "0060_native_c5_cells_array.sql";
const SCRIPT_NATIVE_C5_CELLS_ARRAY: &str =
    include_str!("../migrations/0060_native_c5_cells_array.sql");

pub fn compiled_native_c5_cells_array_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C5_CELLS_ARRAY.as_bytes())
}

pub async fn install_native_c5_cells_array(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior =
        sha256_hex(include_str!("../migrations/0059_native_c5_wide_columns.sql").as_bytes());
    if !matches!(find_row(&rows,"native-c5-wide-columns/v1"),Some(row)
        if row_exact(row,"native-c5-wide-columns/v1",&prior,"0059_native_c5_wide_columns.sql"))
    {
        return Err(InstallError::Refused {
            message: "cells array requires exact 0059".into(),
        });
    }
    let digest = compiled_native_c5_cells_array_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C5_CELLS_ARRAY) {
        if !row_exact(
            row,
            VERSION_NATIVE_C5_CELLS_ARRAY,
            &digest,
            FILE_NATIVE_C5_CELLS_ARRAY,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C5_CELLS_ARRAY.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C5_CELLS_ARRAY).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C5_CELLS_ARRAY,
                &digest,
                FILE_NATIVE_C5_CELLS_ARRAY,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_LIBRARY_C1: &str = "native-library-c1/v1";
const FILE_NATIVE_LIBRARY_C1: &str = "0089_native_library_c1.sql";
const SCRIPT_NATIVE_LIBRARY_C1: &str = include_str!("../migrations/0089_native_library_c1.sql");

pub fn compiled_native_library_c1_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_LIBRARY_C1.as_bytes())
}

pub async fn install_native_library_c1_target(
    target: &serde_json::Value,
) -> Result<(), &'static str> {
    let mut connection = crate::native_update::connect_target(target).await?;
    install_native_library_c1(&mut connection)
        .await
        .map_err(|_| "install_refused")?;
    install_native_library_mysql_receipt(&mut connection)
        .await
        .map_err(|_| "install_refused")
}

pub async fn install_native_library_mysql_receipt(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    const VERSION: &str = "native-library-mysql-receipt/v1";
    const FILE: &str = "0092_native_library_mysql_receipt.sql";
    const SCRIPT: &str = include_str!("../migrations/0092_native_library_mysql_receipt.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_library_c1_digest();
    if !matches!(find_row(&rows,VERSION_NATIVE_LIBRARY_C1),Some(row)
        if row_exact(row,VERSION_NATIVE_LIBRARY_C1,&prior,FILE_NATIVE_LIBRARY_C1))
    {
        return Err(InstallError::Refused {
            message: "MySQL receipt compatibility requires exact 0089".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_library_c1(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    for (version, file, script) in [
        (
            "native-library-existing-receipt-fields/v1",
            "0064_native_library_existing_receipt_fields.sql",
            include_str!("../migrations/0064_native_library_existing_receipt_fields.sql"),
        ),
        (
            "native-c1-source-names/v1",
            "0084_native_c1_source_names.sql",
            include_str!("../migrations/0084_native_c1_source_names.sql"),
        ),
    ] {
        if !matches!(find_row(&rows,version),Some(row) if row_exact(row,version,&sha256_hex(script.as_bytes()),file))
        {
            return Err(InstallError::Refused {
                message: format!("C1 library requires exact {file}"),
            });
        }
    }
    let digest = compiled_native_library_c1_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_LIBRARY_C1) {
        if !row_exact(
            row,
            VERSION_NATIVE_LIBRARY_C1,
            &digest,
            FILE_NATIVE_LIBRARY_C1,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_LIBRARY_C1.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_LIBRARY_C1).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_LIBRARY_C1,
                &digest,
                FILE_NATIVE_LIBRARY_C1,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_C5_ORACLE_RANGE: &str = "native-c5-oracle-range/v1";
const FILE_NATIVE_C5_ORACLE_RANGE: &str = "0090_native_c5_oracle_range.sql";
const SCRIPT_NATIVE_C5_ORACLE_RANGE: &str =
    include_str!("../migrations/0090_native_c5_oracle_range.sql");

pub fn compiled_native_c5_oracle_range_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C5_ORACLE_RANGE.as_bytes())
}

pub async fn install_native_c5_oracle_range_target(
    target: &serde_json::Value,
) -> Result<(), &'static str> {
    let mut connection = crate::native_update::connect_target(target).await?;
    install_native_c5_oracle_range(&mut connection)
        .await
        .map_err(|_| "install_refused")
}

pub async fn install_native_c5_oracle_range(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior =
        sha256_hex(include_str!("../migrations/0075_native_c5_indexed_exclusion.sql").as_bytes());
    if !matches!(find_row(&rows,"native-c5-indexed-exclusion/v1"),Some(row)
        if row_exact(row,"native-c5-indexed-exclusion/v1",&prior,"0075_native_c5_indexed_exclusion.sql"))
    {
        return Err(InstallError::Refused {
            message: "Oracle C5 ranges require exact 0075".into(),
        });
    }
    let digest = compiled_native_c5_oracle_range_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C5_ORACLE_RANGE) {
        if !row_exact(
            row,
            VERSION_NATIVE_C5_ORACLE_RANGE,
            &digest,
            FILE_NATIVE_C5_ORACLE_RANGE,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C5_ORACLE_RANGE.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C5_ORACLE_RANGE).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C5_ORACLE_RANGE,
                &digest,
                FILE_NATIVE_C5_ORACLE_RANGE,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_C5_ORACLE_PUBLICATION: &str = "native-c5-oracle-publication/v1";
const FILE_NATIVE_C5_ORACLE_PUBLICATION: &str = "0093_native_c5_oracle_publication.sql";
const SCRIPT_NATIVE_C5_ORACLE_PUBLICATION: &str =
    include_str!("../migrations/0093_native_c5_oracle_publication.sql");

pub fn compiled_native_c5_oracle_publication_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_C5_ORACLE_PUBLICATION.as_bytes())
}

pub async fn install_native_c5_oracle_publication_target(
    target: &serde_json::Value,
) -> Result<(), &'static str> {
    let mut conn = crate::native_update::connect_target(target).await?;
    install_native_c5_oracle_publication(&mut conn)
        .await
        .map_err(|_| "install_refused")
}

pub async fn install_native_c5_oracle_publication(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_c5_oracle_range_digest();
    if !matches!(find_row(&rows,VERSION_NATIVE_C5_ORACLE_RANGE),Some(row)
        if row_exact(row,VERSION_NATIVE_C5_ORACLE_RANGE,&prior,FILE_NATIVE_C5_ORACLE_RANGE))
    {
        return Err(InstallError::Refused {
            message: "Oracle C5 publication requires exact 0090".into(),
        });
    }
    let digest = compiled_native_c5_oracle_publication_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_C5_ORACLE_PUBLICATION) {
        if !row_exact(
            row,
            VERSION_NATIVE_C5_ORACLE_PUBLICATION,
            &digest,
            FILE_NATIVE_C5_ORACLE_PUBLICATION,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_C5_ORACLE_PUBLICATION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_C5_ORACLE_PUBLICATION).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_C5_ORACLE_PUBLICATION,
                &digest,
                FILE_NATIVE_C5_ORACLE_PUBLICATION,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_ORACLE_C5_CATALOG: &str = "native-oracle-c5-catalog/v1";
const FILE_NATIVE_ORACLE_C5_CATALOG: &str = "0094_native_oracle_c5_catalog.sql";
const SCRIPT_NATIVE_ORACLE_C5_CATALOG: &str =
    include_str!("../migrations/0094_native_oracle_c5_catalog.sql");

pub fn compiled_native_oracle_c5_catalog_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_ORACLE_C5_CATALOG.as_bytes())
}

pub async fn install_native_oracle_c5_catalog_target(
    target: &serde_json::Value,
) -> Result<(), &'static str> {
    let mut conn = crate::native_update::connect_target(target).await?;
    install_native_oracle_c5_catalog(&mut conn)
        .await
        .map_err(|_| "install_refused")
}

pub async fn install_native_oracle_c5_catalog(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_c5_oracle_publication_digest();
    if !matches!(find_row(&rows,VERSION_NATIVE_C5_ORACLE_PUBLICATION),Some(row)
        if row_exact(row,VERSION_NATIVE_C5_ORACLE_PUBLICATION,&prior,FILE_NATIVE_C5_ORACLE_PUBLICATION))
    {
        return Err(InstallError::Refused {
            message: "Oracle C5 catalog requires exact 0093".into(),
        });
    }
    let digest = compiled_native_oracle_c5_catalog_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_ORACLE_C5_CATALOG) {
        if !row_exact(
            row,
            VERSION_NATIVE_ORACLE_C5_CATALOG,
            &digest,
            FILE_NATIVE_ORACLE_C5_CATALOG,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_ORACLE_C5_CATALOG.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_ORACLE_C5_CATALOG).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_ORACLE_C5_CATALOG,
                &digest,
                FILE_NATIVE_ORACLE_C5_CATALOG,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_ORACLE_OBSERVATION: &str = "native-oracle-observation/v1";
const FILE_NATIVE_ORACLE_OBSERVATION: &str = "0096_native_oracle_observation.sql";
const SCRIPT_NATIVE_ORACLE_OBSERVATION: &str =
    include_str!("../migrations/0096_native_oracle_observation.sql");

pub fn compiled_native_oracle_observation_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_ORACLE_OBSERVATION.as_bytes())
}

pub async fn install_native_oracle_observation(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_oracle_c5_catalog_digest();
    if !matches!(find_row(&rows,VERSION_NATIVE_ORACLE_C5_CATALOG),Some(row)
        if row_exact(row,VERSION_NATIVE_ORACLE_C5_CATALOG,&prior,FILE_NATIVE_ORACLE_C5_CATALOG))
    {
        return Err(InstallError::Refused {
            message: "Oracle observation requires exact 0094".into(),
        });
    }
    let digest = compiled_native_oracle_observation_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_ORACLE_OBSERVATION) {
        if !row_exact(
            row,
            VERSION_NATIVE_ORACLE_OBSERVATION,
            &digest,
            FILE_NATIVE_ORACLE_OBSERVATION,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_ORACLE_OBSERVATION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_ORACLE_OBSERVATION).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_ORACLE_OBSERVATION,
                &digest,
                FILE_NATIVE_ORACLE_OBSERVATION,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_LIBRARY_PUBLICATION: &str = "native-library-publication/v1";
const FILE_NATIVE_LIBRARY_PUBLICATION: &str = "0061_native_library_publication.sql";
const SCRIPT_NATIVE_LIBRARY_PUBLICATION: &str =
    include_str!("../migrations/0061_native_library_publication.sql");

pub fn compiled_native_library_publication_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_LIBRARY_PUBLICATION.as_bytes())
}

pub async fn install_native_library_publication(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior = sha256_hex(include_str!("../migrations/0060_native_c5_cells_array.sql").as_bytes());
    if !matches!(find_row(&rows,"native-c5-cells-array/v1"),Some(row)
        if row_exact(row,"native-c5-cells-array/v1",&prior,"0060_native_c5_cells_array.sql"))
    {
        return Err(InstallError::Refused {
            message: "library publication requires exact 0060".into(),
        });
    }
    let digest = compiled_native_library_publication_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_LIBRARY_PUBLICATION) {
        if !row_exact(
            row,
            VERSION_NATIVE_LIBRARY_PUBLICATION,
            &digest,
            FILE_NATIVE_LIBRARY_PUBLICATION,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_LIBRARY_PUBLICATION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_LIBRARY_PUBLICATION).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_LIBRARY_PUBLICATION,
                &digest,
                FILE_NATIVE_LIBRARY_PUBLICATION,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_LIBRARY_CLOSED_POINTER: &str = "native-library-closed-pointer/v1";
const FILE_NATIVE_LIBRARY_CLOSED_POINTER: &str = "0062_native_library_closed_pointer.sql";
const SCRIPT_NATIVE_LIBRARY_CLOSED_POINTER: &str =
    include_str!("../migrations/0062_native_library_closed_pointer.sql");

pub fn compiled_native_library_closed_pointer_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_LIBRARY_CLOSED_POINTER.as_bytes())
}

pub async fn install_native_library_closed_pointer(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior =
        sha256_hex(include_str!("../migrations/0061_native_library_publication.sql").as_bytes());
    if !matches!(find_row(&rows,"native-library-publication/v1"),Some(row)
        if row_exact(row,"native-library-publication/v1",&prior,"0061_native_library_publication.sql"))
    {
        return Err(InstallError::Refused {
            message: "library closed pointer requires exact 0061".into(),
        });
    }
    let digest = compiled_native_library_closed_pointer_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_LIBRARY_CLOSED_POINTER) {
        if !row_exact(
            row,
            VERSION_NATIVE_LIBRARY_CLOSED_POINTER,
            &digest,
            FILE_NATIVE_LIBRARY_CLOSED_POINTER,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_LIBRARY_CLOSED_POINTER.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_LIBRARY_CLOSED_POINTER).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_LIBRARY_CLOSED_POINTER,
                &digest,
                FILE_NATIVE_LIBRARY_CLOSED_POINTER,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_LIBRARY_CLOSED_RECEIPT: &str = "native-library-closed-receipt/v1";
const FILE_NATIVE_LIBRARY_CLOSED_RECEIPT: &str = "0063_native_library_closed_receipt.sql";
const SCRIPT_NATIVE_LIBRARY_CLOSED_RECEIPT: &str =
    include_str!("../migrations/0063_native_library_closed_receipt.sql");

pub fn compiled_native_library_closed_receipt_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_LIBRARY_CLOSED_RECEIPT.as_bytes())
}

pub async fn install_native_library_closed_receipt(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior =
        sha256_hex(include_str!("../migrations/0062_native_library_closed_pointer.sql").as_bytes());
    if !matches!(find_row(&rows,"native-library-closed-pointer/v1"),Some(row)
        if row_exact(row,"native-library-closed-pointer/v1",&prior,"0062_native_library_closed_pointer.sql"))
    {
        return Err(InstallError::Refused {
            message: "library closed receipt requires exact 0062".into(),
        });
    }
    let digest = compiled_native_library_closed_receipt_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_LIBRARY_CLOSED_RECEIPT) {
        if !row_exact(
            row,
            VERSION_NATIVE_LIBRARY_CLOSED_RECEIPT,
            &digest,
            FILE_NATIVE_LIBRARY_CLOSED_RECEIPT,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_LIBRARY_CLOSED_RECEIPT.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_LIBRARY_CLOSED_RECEIPT).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_LIBRARY_CLOSED_RECEIPT,
                &digest,
                FILE_NATIVE_LIBRARY_CLOSED_RECEIPT,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

pub const VERSION_NATIVE_LIBRARY_EXISTING_RECEIPT_FIELDS: &str =
    "native-library-existing-receipt-fields/v1";
const FILE_NATIVE_LIBRARY_EXISTING_RECEIPT_FIELDS: &str =
    "0064_native_library_existing_receipt_fields.sql";
const SCRIPT_NATIVE_LIBRARY_EXISTING_RECEIPT_FIELDS: &str =
    include_str!("../migrations/0064_native_library_existing_receipt_fields.sql");

pub fn compiled_native_library_existing_receipt_fields_digest() -> String {
    sha256_hex(SCRIPT_NATIVE_LIBRARY_EXISTING_RECEIPT_FIELDS.as_bytes())
}

pub async fn install_native_library_existing_receipt_fields(
    conn: &mut PgConnection,
) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior =
        sha256_hex(include_str!("../migrations/0063_native_library_closed_receipt.sql").as_bytes());
    if !matches!(find_row(&rows,"native-library-closed-receipt/v1"),Some(row)
        if row_exact(row,"native-library-closed-receipt/v1",&prior,"0063_native_library_closed_receipt.sql"))
    {
        return Err(InstallError::Refused {
            message: "library existing receipt fields require exact 0063".into(),
        });
    }
    let digest = compiled_native_library_existing_receipt_fields_digest();
    if let Some(row) = find_row(&rows, VERSION_NATIVE_LIBRARY_EXISTING_RECEIPT_FIELDS) {
        if !row_exact(
            row,
            VERSION_NATIVE_LIBRARY_EXISTING_RECEIPT_FIELDS,
            &digest,
            FILE_NATIVE_LIBRARY_EXISTING_RECEIPT_FIELDS,
        ) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION_NATIVE_LIBRARY_EXISTING_RECEIPT_FIELDS.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT_NATIVE_LIBRARY_EXISTING_RECEIPT_FIELDS).await?;
        insert_markers(
            &mut tx,
            &[marker(
                VERSION_NATIVE_LIBRARY_EXISTING_RECEIPT_FIELDS,
                &digest,
                FILE_NATIVE_LIBRARY_EXISTING_RECEIPT_FIELDS,
            )],
        )
        .await?;
    }
    tx.commit().await.map_err(database_error)
}

const C5_INCREMENTAL_MIGRATIONS: [(&str, &str, &str); 11] = [
    (
        "native-c5-multipart-initialize/v1",
        "0065_native_c5_multipart_initialize.sql",
        include_str!("../migrations/0065_native_c5_multipart_initialize.sql"),
    ),
    (
        "native-c5-overlay/v1",
        "0066_native_c5_overlay.sql",
        include_str!("../migrations/0066_native_c5_overlay.sql"),
    ),
    (
        "native-c5-bounded-publication/v1",
        "0067_native_c5_bounded_publication.sql",
        include_str!("../migrations/0067_native_c5_bounded_publication.sql"),
    ),
    (
        "native-c5-overlay-visibility/v1",
        "0068_native_c5_overlay_visibility.sql",
        include_str!("../migrations/0068_native_c5_overlay_visibility.sql"),
    ),
    (
        "native-c5-published-baseline/v1",
        "0069_native_c5_published_baseline.sql",
        include_str!("../migrations/0069_native_c5_published_baseline.sql"),
    ),
    (
        "native-c5-delta-reconcile/v1",
        "0070_native_c5_delta_reconcile.sql",
        include_str!("../migrations/0070_native_c5_delta_reconcile.sql"),
    ),
    (
        "native-c5-empty-baseline/v1",
        "0071_native_c5_empty_baseline.sql",
        include_str!("../migrations/0071_native_c5_empty_baseline.sql"),
    ),
    (
        "native-c5-float-archive/v1",
        "0072_native_c5_float_archive.sql",
        include_str!("../migrations/0072_native_c5_float_archive.sql"),
    ),
    (
        "native-c5-digest-policy/v1",
        "0073_native_c5_digest_policy.sql",
        include_str!("../migrations/0073_native_c5_digest_policy.sql"),
    ),
    (
        "native-c5-resumed-baseline/v1",
        "0074_native_c5_resumed_baseline.sql",
        include_str!("../migrations/0074_native_c5_resumed_baseline.sql"),
    ),
    (
        "native-c5-indexed-exclusion/v1",
        "0075_native_c5_indexed_exclusion.sql",
        include_str!("../migrations/0075_native_c5_indexed_exclusion.sql"),
    ),
];

pub fn compiled_native_c5_incremental_digest() -> String {
    let mut hash = Sha256::new();
    for (version, filename, script) in C5_INCREMENTAL_MIGRATIONS {
        hash.update(version.as_bytes());
        hash.update([0]);
        hash.update(filename.as_bytes());
        hash.update([0]);
        hash.update(script.as_bytes());
        hash.update([0]);
    }
    format!("{:x}", hash.finalize())
}

/// Install the complete bounded path atomically; an initializer alone is not publishable.
pub async fn install_native_c5_incremental(conn: &mut PgConnection) -> Result<(), InstallError> {
    let principal = read_principal(conn).await?;
    let preview = read_catalog(conn).await?;
    if !principal_can_install(&principal, &preview) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let prior = compiled_native_library_existing_receipt_fields_digest();
    if !matches!(
        find_row(&rows, VERSION_NATIVE_LIBRARY_EXISTING_RECEIPT_FIELDS),
        Some(row) if row_exact(row, VERSION_NATIVE_LIBRARY_EXISTING_RECEIPT_FIELDS,
            &prior, FILE_NATIVE_LIBRARY_EXISTING_RECEIPT_FIELDS)
    ) {
        return Err(InstallError::Refused {
            message: "bounded C5 requires exact 0064".into(),
        });
    }
    let mut missing = false;
    for (version, filename, script) in C5_INCREMENTAL_MIGRATIONS {
        let digest = sha256_hex(script.as_bytes());
        if let Some(row) = find_row(&rows, version) {
            if missing {
                return Err(InstallError::Refused {
                    message: "bounded C5 migration chain has a gap".into(),
                });
            }
            if !row_exact(row, version, &digest, filename) {
                return mismatch(
                    tx,
                    Mismatch {
                        version: version.into(),
                        stored: row.checksum.clone(),
                    },
                )
                .await
                .map(|_| ());
            }
        } else {
            missing = true;
        }
    }
    exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
    for (version, filename, script) in C5_INCREMENTAL_MIGRATIONS {
        if find_row(&rows, version).is_none() {
            exec_script(&mut tx, script).await?;
            insert_markers(
                &mut tx,
                &[marker(version, &sha256_hex(script.as_bytes()), filename)],
            )
            .await?;
        }
    }
    tx.commit().await.map_err(database_error)
}
pub async fn install_native_pg_current(conn: &mut PgConnection) -> Result<(), InstallError> {
    const VERSION: &str = "native-pg-current/v1";
    const FILE: &str = "0116_native_pg_current.sql";
    const SCRIPT: &str = include_str!("../migrations/0116_native_pg_current.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let previous = "native-mysql-complete-handoff/v1";
    if !matches!(find_row(&rows, previous), Some(row) if row_exact(row, previous,
        &sha256_hex(include_bytes!("../migrations/0115_native_mysql_complete_handoff.sql")),
        "0115_native_mysql_complete_handoff.sql"))
    {
        return Err(InstallError::Refused {
            message: "PG current requires exact 0115".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

pub async fn install_native_pg_seal_order(conn: &mut PgConnection) -> Result<(), InstallError> {
    const VERSION: &str = "native-pg-seal-order/v1";
    const FILE: &str = "0117_native_pg_seal_order.sql";
    const SCRIPT: &str = include_str!("../migrations/0117_native_pg_seal_order.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    let previous = "native-pg-current/v1";
    if !matches!(find_row(&rows, previous), Some(row) if row_exact(row, previous,
        &sha256_hex(include_bytes!("../migrations/0116_native_pg_current.sql")),
        "0116_native_pg_current.sql"))
    {
        return Err(InstallError::Refused {
            message: "PG seal order requires exact 0116".into(),
        });
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}

/// PG-only publication bridge. Never rewrites the installed capture/ACK lane.
pub async fn install_native_pg_publication(conn: &mut PgConnection) -> Result<(), InstallError> {
    const VERSION: &str = "native-pg-current-publication/v1";
    const FILE: &str = "0118_native_pg_current_publication.sql";
    const SCRIPT: &str = include_str!("../migrations/0118_native_pg_current_publication.sql");
    let principal = read_principal(conn).await?;
    if !principal_can_install(&principal, &read_catalog(conn).await?) {
        return Err(InstallError::NotAdministrator);
    }
    let mut tx = conn.begin().await.map_err(database_error)?;
    prepare_locked(&mut tx, &principal).await?;
    let rows = read_rows(&mut tx).await?;
    for (version, filename, script) in [
        (
            "native-pg-current/v1",
            "0116_native_pg_current.sql",
            include_str!("../migrations/0116_native_pg_current.sql"),
        ),
        (
            "native-pg-seal-order/v1",
            "0117_native_pg_seal_order.sql",
            include_str!("../migrations/0117_native_pg_seal_order.sql"),
        ),
    ] {
        if !matches!(find_row(&rows, version), Some(row)
            if row_exact(row, version, &sha256_hex(script.as_bytes()), filename))
        {
            return Err(InstallError::Refused {
                message: "PG publication requires exact 0116/0117".into(),
            });
        }
    }
    let digest = sha256_hex(SCRIPT.as_bytes());
    if let Some(row) = find_row(&rows, VERSION) {
        if !row_exact(row, VERSION, &digest, FILE) {
            return mismatch(
                tx,
                Mismatch {
                    version: VERSION.into(),
                    stored: row.checksum.clone(),
                },
            )
            .await
            .map(|_| ());
        }
    } else {
        exec_static(&mut tx, "SET LOCAL ROLE oo_target_owner").await?;
        exec_script(&mut tx, SCRIPT).await?;
        insert_markers(&mut tx, &[marker(VERSION, &digest, FILE)]).await?;
    }
    tx.commit().await.map_err(database_error)
}
