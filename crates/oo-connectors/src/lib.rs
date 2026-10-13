//! Read-only PostgreSQL source inspection, catalog, lossless decoding, and a
//! non-reusable snapshot owner.
//!
//! The catalog protocol stays [`PROTOCOL`] (`oo.source.pgsql/v1`).
//! `type_conversion` and [`TypeSupport::converted`] stay unsupported and false.
//! Binary decoding is the separate protocol [`types::TYPE_CONVERSION_PROTOCOL`].
//! [`PgSession::read_typed`] is that decoder. The snapshot owner protocol is
//! [`SNAPSHOT_PROTOCOL`]. [`STREAM_PROTOCOL`] is the bounded full-snapshot Arrow
//! pull on that same owner. [`WINDOW_PROTOCOL`] reads one selected table through
//! a paced portal and publishes Arrow only after that portal completes inside
//! the existing row, byte, and deadline caps. [`KEY_PLAN_PROTOCOL`] plans typed half-open chunks
//! on that same owner. It is not the whole reader. [`SnapshotOwner::cancel`]
//! stops an active pull and releases both backends. [`PgConnector::prepare_snapshot`]
//! builds a [`SnapshotRequest`] from the configured read-only session and releases
//! that session before it returns. It does not open the snapshot owner. Resume
//! cursors, CDC, and the registry API stay unsupported. [`JSONL_PROTOCOL`] is a
//! separate bounded file pull over [`std::io::Read`]. It does not admit an import.
//! [`DELIMITED_PROTOCOL`] is a separate bounded CSV or TSV pull over [`std::io::Read`].
//! It does not admit an import.
//! [`FILE_WORKER_PARENT_PROTOCOL`] supervises one trusted file-parser worker and
//! returns [`ParentParseComplete`] only after a verified IPC end. That receipt
//! does not admit an import.
//! Gateway crates do not depend on this crate.

#![forbid(unsafe_code)]

mod access;
mod bounded_read;
mod budget_report;
mod catalog;
mod config;
mod delimited;
mod error;
mod file_worker_parent;
mod ident;
mod jsonl;
mod key_plan;
mod model;
mod session;
mod snapshot;
mod sql;
mod stream;
pub mod types;

pub use bounded_read::{WINDOW_PAGE_ROWS_MAX, WINDOW_PROTOCOL, WindowReceipt, WindowRequest};
pub use budget_report::{
    BUDGET_PROTOCOL, BudgetBlock, BudgetBound, BudgetEvidence, BudgetIdentity, BudgetInspection,
    BudgetPermission, BudgetTls, BudgetVersion, Companion, DriverIdentity, ObjectPrivileges,
    ObservedSetting, ObservedSettings, OltpCostModel, driver_digest,
};
pub use config::{
    PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef, SelectedTable,
    SourceId, TableSelection,
};
pub use delimited::{
    BomPolicy, DELIMITED_LINEAGE_VERSION, DELIMITED_MAX_BATCH_BYTES, DELIMITED_MAX_BATCH_ROWS,
    DELIMITED_MAX_COLUMNS, DELIMITED_MAX_FIELD_BYTES, DELIMITED_MAX_FIELDS,
    DELIMITED_MAX_NAME_BYTES, DELIMITED_MAX_NULL_TOKEN_BYTES, DELIMITED_MAX_RECORD_BYTES,
    DELIMITED_MAX_ROWS, DELIMITED_MAX_TOTAL_BYTES, DELIMITED_MIN_RECORD_BYTES,
    DELIMITED_PROFILE_VERSION, DELIMITED_PROTOCOL, DELIMITED_RECEIPT_VERSION, DelimitedBatch,
    DelimitedBudget, DelimitedCancel, DelimitedCancelled, DelimitedColumn, DelimitedComplete,
    DelimitedDialect, DelimitedError, DelimitedErrorKind, DelimitedExhausted, DelimitedHeader,
    DelimitedItem, DelimitedProfile, DelimitedReader, DelimitedType,
    FieldLineage as DelimitedFieldLineage, FieldPresence as DelimitedFieldPresence, HeaderPolicy,
    NullPolicy, RowLineage as DelimitedRowLineage,
};
pub use error::{SourceError, SourceErrorKind};
pub use file_worker_parent::{
    ChildRequest, FILE_WORKER_PARENT_PROTOCOL, Failure, FailureKind, GroupSignal,
    KERNEL_RESTRICTION, KernelRestriction, PARENT_DEADLINE_MAX_MS, PARENT_DEADLINE_MIN_MS,
    ParentConfig, ParentError, ParentParseComplete, ParentPull, PullBudget, SourceProfile,
    Teardown, VerifiedBatch, VerifiedField, VerifiedRow,
};
pub use ident::{PG17_RESERVED, stable_ident};
pub use jsonl::{
    FieldLineage, FieldPresence, JSONL_LINEAGE_VERSION, JSONL_MAX_ARRAY_ELEMENTS,
    JSONL_MAX_BATCH_BYTES, JSONL_MAX_BATCH_ROWS, JSONL_MAX_COLUMNS, JSONL_MAX_DEPTH,
    JSONL_MAX_FIELDS, JSONL_MAX_NAME_BYTES, JSONL_MAX_RECORD_BYTES, JSONL_MAX_ROWS,
    JSONL_MAX_TOTAL_BYTES, JSONL_PROFILE_VERSION, JSONL_PROTOCOL, JSONL_RECEIPT_VERSION,
    JsonlBatch, JsonlBudget, JsonlCancel, JsonlCancelled, JsonlColumn, JsonlComplete, JsonlError,
    JsonlErrorKind, JsonlExhausted, JsonlHeader, JsonlItem, JsonlProfile, JsonlReader, JsonlType,
    RowLineage,
};
pub use key_plan::{
    DEFAULT_TARGET_ROWS, DeclaredKey, DeclaredKeyKind, KEY_PLAN_PROTOCOL, KeyColumnPlan, KeyPlan,
    KeyRange, MAX_PLAN_CHUNKS, PlannedChunk, SNAPSHOT_DEADLINE_DEFAULT, TableKeyPlan,
};
pub use model::{
    AccessReport, BlockReason, CapabilityStatus, CdcPrerequisite, ColumnPrivileges, DbaItem,
    DefinerFunction, EMPTY_SCHEMA_HASH, Fact, FactStatus, ForeignKey, ForeignKeyColumn, KeyColumn,
    PolicyRecord, Principal, PrivilegeGrant, RlsTableFact, RoleAttributes, RoleMembership,
    SourceCatalog, SourceColumn, SourceTable, TlsReport, TypeSupport, TypeUnsupportedReason,
    UniqueKey, UntrustedEvidence, VersionFact, WriteDetermination, WriteFinding,
    connector_capabilities, numeric_parts, schema_hash,
};
pub use session::{PgConnector, PgSession};
pub use snapshot::{
    AdmittedCatalog, AdmittedColumn, AdmittedTable, BackendKey, CLEANUP_OBSERVATION_BOUND_MS,
    CutProbe, ProbeValue, RunCancel, RunCancelled, RunComplete, SNAPSHOT_PROTOCOL, SnapshotAborted,
    SnapshotBinding, SnapshotFinished, SnapshotOwner, SnapshotRequest, TableAccess,
    config_identity, snapshot_input_digest, type_policy_identity,
};
pub use stream::{
    STREAM_PROTOCOL, StreamAccounting, StreamBatch, StreamBudget, StreamItem, TableEof,
};
pub use types::{
    ColumnMapping, PgTypePolicy, RunBinding, TYPE_CONVERSION_PROTOCOL, TypedBatch,
    type_capabilities,
};

/// Frozen protocol name for this source slice.
/// [`BUDGET_PROTOCOL`] is a separate document. This name does not grow budget fields.
pub const PROTOCOL: &str = "oo.source.pgsql/v1";

/// Identifier algorithm implemented by [`stable_ident`].
pub const IDENT_ALGORITHM: &str = "pgident/v1";

/// Declared server major floor. Versions below this block registration.
/// Majors at or above this floor are not claimed compatible unless listed in
/// [`TESTED_VERSIONS`] and exercised by the live fixture.
pub const DECLARED_MIN_MAJOR: i64 = 13;

/// Server versions this build has been tested against.
pub const TESTED_VERSIONS: &[&str] = &["17.10"];

/// Real use of the oo-core contract constant. Architecture requires this edge.
pub const CORE_BOUNDARY: &str = oo_core::CONTRACT_CONFIG;

#[cfg(test)]
mod tests {
    use sqlx::ConnectOptions;

    /// Links the pinned SQLx 0.9 PostgreSQL driver with hostname verification.
    /// This does not open a socket and must not print `PgConnectOptions`.
    #[test]
    fn driver_verify_full_options_link() {
        let options = sqlx::postgres::PgConnectOptions::from_explicit()
            .host("example.invalid")
            .port(1)
            .username("smoke-user")
            .password("smoke-secret")
            .database("smoke")
            .application_name("oo-pg-source")
            .extra_float_digits(2i8)
            .ssl_mode(sqlx::postgres::PgSslMode::VerifyFull)
            .ssl_root_cert_from_pem(
                b"-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n".to_vec(),
            );
        let options = options.disable_statement_logging();
        assert!(options.get_options().is_none());
        assert_eq!(options.get_username(), "smoke-user");
        assert_eq!(options.get_database(), Some("smoke"));
        assert_eq!(options.get_host(), "example.invalid");
        assert_eq!(options.get_port(), 1);
        assert!(matches!(
            options.get_ssl_mode(),
            sqlx::postgres::PgSslMode::VerifyFull
        ));
        let _linked = options;
        assert_eq!(super::CORE_BOUNDARY, oo_core::CONTRACT_CONFIG);
        let caps = super::connector_capabilities();
        let supported: Vec<&str> = caps
            .iter()
            .filter(|item| item.supported)
            .map(|item| item.name.as_str())
            .collect();
        let unsupported: Vec<&str> = caps
            .iter()
            .filter(|item| !item.supported)
            .map(|item| item.name.as_str())
            .collect();
        assert_eq!(supported, ["scan", "bounded_read"]);
        assert_eq!(
            unsupported,
            [
                "cdc",
                "type_conversion",
                "registry_api",
                "source_ddl",
                "source_write"
            ]
        );
    }

    /// Child process only. Proves `from_explicit` ignores libpq fallbacks
    /// without changing the parent environment.
    #[test]
    fn explicit_options_ignore_libpq_env() {
        if std::env::var("OO_A066_UNIT_PROBE").is_ok() {
            let options = sqlx::postgres::PgConnectOptions::from_explicit()
                .host("example.invalid")
                .port(1)
                .username("smoke-user")
                .password("smoke-secret")
                .database("smoke")
                .application_name("oo-pg-source")
                .extra_float_digits(2i8)
                .ssl_mode(sqlx::postgres::PgSslMode::VerifyFull);
            assert!(options.get_options().is_none());
            assert_eq!(options.get_username(), "smoke-user");
            assert_eq!(options.get_database(), Some("smoke"));
            assert_eq!(options.get_host(), "example.invalid");
            assert_eq!(options.get_port(), 1);
            assert_eq!(options.get_application_name(), Some("oo-pg-source"));
            assert!(matches!(
                options.get_ssl_mode(),
                sqlx::postgres::PgSslMode::VerifyFull
            ));
            assert_eq!(std::env::var("PGOPTIONS").as_deref(), Ok("-c role=forged"));
            assert_eq!(std::env::var("PGSSLMODE").as_deref(), Ok("disable"));
            assert_eq!(std::env::var("PGUSER").as_deref(), Ok("forged-user"));
            assert_eq!(std::env::var("PGDATABASE").as_deref(), Ok("forged-db"));
            assert_eq!(std::env::var("PGAPPNAME").as_deref(), Ok("forged-app"));
            return;
        }
        let before_options = std::env::var_os("PGOPTIONS");
        let before_mode = std::env::var_os("PGSSLMODE");
        let before_cert = std::env::var_os("PGSSLCERT");
        let before_key = std::env::var_os("PGSSLKEY");
        let exe = std::env::current_exe().expect("test exe");
        let output = std::process::Command::new(exe)
            .args(["--exact", "explicit_options_ignore_libpq_env"])
            .env("OO_A066_UNIT_PROBE", "1")
            .env("PGOPTIONS", "-c role=forged")
            .env("PGSSLMODE", "disable")
            .env("PGSSLCERT", "/tmp/oo-a066-not-a-cert")
            .env("PGSSLKEY", "/tmp/oo-a066-not-a-key")
            .env("PGUSER", "forged-user")
            .env("PGPASSWORD", "forged-secret")
            .env("PGDATABASE", "forged-db")
            .env("PGAPPNAME", "forged-app")
            .env("PGSSLROOTCERT", "/tmp/oo-a066-not-a-ca")
            .env("PGHOST", "127.0.0.1")
            .env("PGPORT", "1")
            .output()
            .expect("probe process");
        assert!(
            output.status.success(),
            "probe failed\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(std::env::var_os("PGOPTIONS"), before_options);
        assert_eq!(std::env::var_os("PGSSLMODE"), before_mode);
        assert_eq!(std::env::var_os("PGSSLCERT"), before_cert);
        assert_eq!(std::env::var_os("PGSSLKEY"), before_key);
    }
}
