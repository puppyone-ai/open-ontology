//! Honest ClickHouse admission input for source-admission pin `5df40d24`.
//!
//! Permissions are the `system.grants` rows from attempt-06. `readonly` and
//! `max_execution_time` are the `system.settings` values from that same run.
//! The quota interval is the `oo_reader` interval in the users XML that run
//! mounted. This crate does not relabel the source as PostgreSQL or MySQL and
//! does not call the sealed-import or role-pack compilers.

use source_admission::{
    Budget, CaptureMethod, ClickHouseNative, ClickHouseQuota, CostModel, DebeziumOptions,
    DeclaredCapability, Driver, GrantScope, NativeFacts, Permission, SourceConfig, TargetReport,
    WriteOptions, CONFIG_PROTOCOL, REPORT_PROTOCOL,
};

pub const COMPAT_MODE: &str = "clickhouse-native";
pub const SOURCE_ID: &str = "clickhouse_268";
pub const TARGET_ID: &str = "clickhouse-26-8-selfmanaged";
pub const SECRET_REF: &str = "secret://oo-dl01w1-40/reader";
pub const CONFIGURED_MAX_RESULT_ROWS: u64 = 100_000;
pub const OBSERVED_READONLY: &str = "1";
pub const OBSERVED_MAX_EXECUTION_TIME: &str = "8";

pub fn inputs(driver_digest: &str) -> (TargetReport, SourceConfig) {
    let permissions = observed_permissions();
    let native = Some(observed_native());
    let report = TargetReport {
        protocol: REPORT_PROTOCOL.to_string(),
        target_id: TARGET_ID.to_string(),
        source_id: SOURCE_ID.to_string(),
        secret_ref: SECRET_REF.to_string(),
        driver: Driver {
            kind: "native-http".to_string(),
            name: "ch-reader".to_string(),
            version: "0.1.0".to_string(),
            digest: driver_digest.to_string(),
        },
        compat_mode: COMPAT_MODE.to_string(),
        license_flags: Vec::new(),
        permissions: permissions.clone(),
        capabilities: vec![
            capability("Catalog", None, None),
            capability("BatchFull", None, None),
            capability("SnapshotToken", Some("PerPartition".to_string()), None),
            capability("Budget", None, Some("Rows".to_string())),
        ],
        capture_methods: vec![CaptureMethod {
            method: "C4".to_string(),
            write_class: "R0".to_string(),
            consistency: "S0".to_string(),
            token_order: "PerPartition".to_string(),
            retention_window: None,
            cursor: None,
            feed: None,
            cdc: None,
        }],
        cost_model: CostModel {
            unit: "Rows".to_string(),
            confidence: "unknown".to_string(),
            estimate: "configured_max_result_rows".to_string(),
        },
        r1w: None,
        native: native.clone(),
    };
    let config = SourceConfig {
        protocol: CONFIG_PROTOCOL.to_string(),
        target_id: report.target_id.clone(),
        source_id: report.source_id.clone(),
        secret_ref: report.secret_ref.clone(),
        compat_mode: COMPAT_MODE.to_string(),
        permissions,
        budget: Budget {
            unit: "Rows".to_string(),
            hard_limit: CONFIGURED_MAX_RESULT_ROWS,
            confidence: "unknown".to_string(),
        },
        write_options: WriteOptions {
            signal_table: false,
            heartbeat_write: false,
            create_stream: false,
            create_trigger: false,
        },
        debezium: DebeziumOptions {
            signal_data_collection: None,
            heartbeat_action_query: None,
            snapshot_mode: "never".to_string(),
            create_stream: false,
            create_trigger: false,
        },
        requested_write_class: "R0".to_string(),
        r1w: None,
        native,
    };
    (report, config)
}

fn observed_permissions() -> Vec<Permission> {
    let mut items = vec![Permission {
        name: "SHOW DATABASES".to_string(),
        status: "known".to_string(),
        granted: true,
        scope: None,
    }];
    items.push(select("oo_dl01w40", None));
    for table in [
        "columns",
        "databases",
        "grants",
        "mutations",
        "parts",
        "processes",
        "query_log",
        "settings",
        "tables",
    ] {
        items.push(select("system", Some(table)));
    }
    items
}

fn select(database: &str, table: Option<&str>) -> Permission {
    Permission {
        name: "SELECT".to_string(),
        status: "known".to_string(),
        granted: true,
        scope: Some(GrantScope {
            database: Some(database.to_string()),
            table: table.map(str::to_string),
            index: None,
            observed: None,
        }),
    }
}

fn observed_native() -> NativeFacts {
    NativeFacts {
        clickhouse: Some(ClickHouseNative {
            readonly: OBSERVED_READONLY.to_string(),
            max_execution_time: OBSERVED_MAX_EXECUTION_TIME.to_string(),
            quota: ClickHouseQuota {
                duration_seconds: 3600,
                queries: 400,
                errors: 80,
                result_rows: 1_000_000,
                read_rows: 10_000_000,
                execution_time_seconds: 300,
            },
        }),
    }
}

fn capability(
    name: &str,
    token_order: Option<String>,
    budget_unit: Option<String>,
) -> DeclaredCapability {
    DeclaredCapability {
        name: name.to_string(),
        cursor: None,
        feed: None,
        cdc: None,
        token_order,
        budget_unit,
        level: "designed".to_string(),
        write_class: "R0".to_string(),
        consistency: "S0".to_string(),
    }
}

pub fn decision_for(driver_digest: &str) -> source_admission::Decision {
    let (report, config) = inputs(driver_digest);
    source_admission::admit(&report, &config)
}

pub fn denial_codes(driver_digest: &str) -> Vec<String> {
    decision_for(driver_digest)
        .blocks
        .into_iter()
        .map(|block| block.code)
        .collect()
}
