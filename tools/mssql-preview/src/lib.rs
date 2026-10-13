//! SQL Server customer access.
//!
//! This crate does not admit a source, does not seal a native catalog, and
//! does not treat a fixture as a live TDS read. `datetime2(7)` stays in
//! 100-nanosecond ticks or the value is refused.

#![forbid(unsafe_code)]

mod arrow_batch;
mod config;
mod capture;
mod fail;
mod gates;
mod policy;
mod seal;
mod sql;
mod tds;
mod types;

pub use arrow_batch::{encode_cells, encode_fixture, read_ipc, ArrowPackage, ColumnSpec};
pub use capture::{run_capture, run_audit_export, Position, TablePosition, validate_version};
pub use config::{parse_customer, Customer, ObjectName, CUSTOMER_PROTOCOL};
pub use fail::{exit_class, Fail};
pub use gates::gates_document;
pub use policy::{command_budget, driver_report, preflight, Preflight};
pub use seal::{seal_request, SealStop, PRODUCT_ENGINE, SEALED_PRODUCTS};
pub use sql::{admit_product_sql, load_sql, CATALOG_SQL, PERMISSION_SQL, SERVER_SQL};
pub use tds::{build_config, cleanup_for, permission_decision, run_catalog, run_load, Cleanup, TrustPlan};
pub use types::{
    cell_from_column, datetime2_ticks, days_since_year_one, guid_wire_from_rfc, pg_timestamp6_for_datetime2,
    time_nanoseconds, Cell, SqlKind,
};

pub const DRIVER_NAME: &str = "tiberius";
pub const DRIVER_VERSION: &str = "0.13.0";
pub const DRIVER_LICENSE: &str = "MIT OR Apache-2.0";
pub const DRIVER_REPOSITORY: &str = "https://github.com/prisma/tiberius";
pub const DRIVER_CRATE_SHA256: &str =
    "e07324791de2bdaed058af4aa433a0b5ec5c0beac407a9797419c19805df2415";
pub const ADMISSION_PIN: &str = "4055237dfeb6df672388419dd3889a6fcaa4d356";
pub const NATIVE_CATALOG_PIN: &str = "a6825165dd83ffaa30e17fbb2da1a481a05f6559";
pub const SOURCE_SEMANTICS_PIN: &str = "29aae36b58d61c67f001893cd38aac28ec0f959f";
pub const CONSUMER_PIN: &str = "6d9d6cbb98f249709a462915afc8f45e4db3c047";
