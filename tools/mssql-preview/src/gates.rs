use serde_json::{json, Value};

use crate::policy::driver_report;
use crate::seal::SEALED_PRODUCTS;
use crate::{ADMISSION_PIN, CONSUMER_PIN, DRIVER_CRATE_SHA256, NATIVE_CATALOG_PIN, SOURCE_SEMANTICS_PIN};

pub fn gates_document(admission_bin_env_present: bool) -> Value {
    json!({
        "protocol": "oo.mssql-preview.gates/v1",
        "source_acceptance": "pending",
        "passed": false,
        "self_approved": false,
        "matrix_grade_promoted": false,
        "cdc_ct_claimed": false,
        "driver": driver_report(),
        "driver_crate_sha256": DRIVER_CRATE_SHA256,
        "gates": {
            "source_identity": "pending",
            "admission": "pending",
            "full_audit": "pending",
            "cancel_observed": "pending",
            "builder": "pending",
            "pack": "pending",
            "local_consumer": "pending",
            "whole_source": "pending",
            "cdc": "not_claimed",
            "change_tracking": "implemented_live_validation_pending",
            "native_catalog_seal": "pending_current_native_producer_inputs"
        },
        "source_admission": {
            "pin": ADMISSION_PIN,
            "invoked": false,
            "status": "pending",
            "bin_env_present": admission_bin_env_present,
            "reason": "live source identity is not accepted from this host"
        },
        "missing_interfaces": [
            {
                "component": "oo-pipeline native sealed catalog",
                "pin": NATIVE_CATALOG_PIN,
                "requirement": "sealed_product and recognized_engine must include sqlserver before a native catalog seal can succeed",
                "current_set": SEALED_PRODUCTS,
                "product_behavior": "write oo.pipeline.native-sealed-catalog/v1 with no engine and no client authority keys, then exit 4"
            },
            {
                "component": "shared sql validator",
                "requirement": "no sqlserver validator is linked from the fixed archive",
                "product_behavior": "statements outside the fixed customer SELECT set fail closed before a socket"
            },
            {
                "component": "core source_semantics and typed consumer",
                "source_semantics_pin": SOURCE_SEMANTICS_PIN,
                "consumer_pin": CONSUMER_PIN,
                "requirement": "a sealed local consumer query needs a recognized native import, which this engine cannot produce",
                "product_behavior": "pending, not invoked"
            }
        ],
        "required_external": [
            "native linux/amd64 SQL Server 2022 host",
            "authorized EULA",
            "DBA-supplied endpoint and a mode-0600 reader secret file passed by path",
            "sqlserver added to the pinned sealed_product set by the pipeline owner"
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gates_stay_pending_and_do_not_approve() {
        let document = gates_document(false);
        let text = document.to_string();
        assert_eq!(document["passed"], false);
        assert_eq!(document["source_acceptance"], "pending");
        assert_eq!(document["self_approved"], false);
        assert!(!text.contains("\"admitted\""));
        assert_eq!(document["gates"]["whole_source"], "pending");
        assert_eq!(document["cdc_ct_claimed"], false);
    }
}
