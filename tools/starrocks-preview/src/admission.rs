//! True-token admission documents. `starrocks-native` is not a known compat mode.

use serde_json::{json, Value};

pub const REPORT_PROTOCOL: &str = "oo.source-admission.report/v1";
pub const CONFIG_PROTOCOL: &str = "oo.source-admission.config/v1";

pub fn documents(source_id: &str, secret_ref: &str, driver_digest: &str, row_cap: u64) -> (Value, Value) {
    let permissions = json!([
        {"name": "SELECT", "status": "known", "granted": true},
        {"name": "USAGE", "status": "known", "granted": true},
        {"name": "CONNECT", "status": "known", "granted": true}
    ]);
    let capability = json!({
        "name": "BatchFull",
        "cursor": null,
        "feed": null,
        "cdc": null,
        "token_order": null,
        "budget_unit": null,
        "level": "experimental",
        "write_class": "R0",
        "consistency": "S0"
    });
    let capture = json!({
        "method": "C4",
        "write_class": "R0",
        "consistency": "S0",
        "token_order": "None",
        "retention_window": null,
        "cursor": null,
        "feed": null,
        "cdc": null
    });
    let report = json!({
        "protocol": REPORT_PROTOCOL,
        "target_id": "starrocks_w45",
        "source_id": source_id,
        "secret_ref": secret_ref,
        "driver": {
            "kind": "native",
            "name": "starrocks-probe",
            "version": "3.5.21-85fccf8",
            "digest": driver_digest
        },
        "compat_mode": "starrocks-native",
        "license_flags": [],
        "permissions": permissions,
        "capabilities": [capability],
        "capture_methods": [capture],
        "cost_model": {
            "unit": "Rows",
            "confidence": "unknown",
            "estimate": row_cap.to_string()
        },
        "r1w": null
    });
    let config = json!({
        "protocol": CONFIG_PROTOCOL,
        "target_id": "starrocks_w45",
        "source_id": source_id,
        "secret_ref": secret_ref,
        "compat_mode": "starrocks-native",
        "permissions": permissions,
        "budget": {
            "unit": "Rows",
            "hard_limit": row_cap,
            "confidence": "unknown"
        },
        "write_options": {
            "signal_table": false,
            "heartbeat_write": false,
            "create_stream": false,
            "create_trigger": false
        },
        "debezium": {
            "signal_data_collection": null,
            "heartbeat_action_query": null,
            "snapshot_mode": "never",
            "create_stream": false,
            "create_trigger": false
        },
        "requested_write_class": "R0",
        "r1w": null
    });
    (report, config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_uses_the_starrocks_token() {
        let (report, config) = documents("sr_w45", "secret://oo-dl01w1-45/reader", &"ab".repeat(32), 1000);
        let text = serde_json::to_string(&report).expect("report") + &serde_json::to_string(&config).expect("config");
        assert!(text.contains("starrocks-native"));
        assert!(!text.contains("mysql-native"));
        assert!(!text.contains("mysql-mode"));
        assert_eq!(report["compat_mode"], "starrocks-native");
        assert_eq!(config["budget"]["hard_limit"], 1000);
        assert_eq!(config["budget"]["confidence"], "unknown");
        assert_eq!(config["debezium"]["snapshot_mode"], "never");
    }
}
