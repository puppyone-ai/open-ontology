//! Re-check a scope report before any CI activation command is emitted.
//! An empty command list is not a suite run.

use serde_json::Value;

pub fn validate_activation(report: &Value) -> Result<Value, String> {
    let map = report.as_object().ok_or_else(|| "activation.report: not an object".to_owned())?;
    let schema = map.get("schema").and_then(Value::as_str).unwrap_or("");
    if schema != "oo-source-conformance/scope-report/v1" {
        return Err(format!("activation.schema: {schema}"));
    }
    if map.get("promoted").and_then(Value::as_bool) != Some(false) || map.get("matrix_rewritten").and_then(Value::as_bool) != Some(false) {
        return Err("activation.promoted: scope report claims a promotion".to_owned());
    }
    if map.get("sources_signed_off").and_then(Value::as_bool) != Some(false) || map.get("accepted_sources").and_then(Value::as_u64) != Some(0) {
        return Err("activation.sources: scope report claims source acceptance".to_owned());
    }
    let activation = map.get("ci_activation").and_then(Value::as_object).ok_or_else(|| "activation.missing".to_owned())?;
    let count = activation.get("activation_count").and_then(Value::as_u64).ok_or_else(|| "activation.count".to_owned())?;
    let commands = activation.get("commands").and_then(Value::as_array).ok_or_else(|| "activation.commands".to_owned())?;
    if count != commands.len() as u64 {
        return Err("activation.count_mismatch".to_owned());
    }
    if count > 0 || !commands.is_empty() {
        return Err("ci.activation_unbacked: no capability in this period has a passing CC set".to_owned());
    }
    if !products_unpassed(map.get("products"))? {
        return Err("ci.activation_unbacked: a product CC row is marked pass".to_owned());
    }
    Ok(serde_json::json!({
        "schema": "oo-source-conformance/ci-activation/v1",
        "activation_count": 0,
        "empty_run_refused": activation.get("empty_run_refused").and_then(Value::as_bool).unwrap_or(false),
        "empty_run_is_not_a_suite": true,
        "cli_exit_is_not_acceptance": true,
        "sources_signed_off": false,
        "commands": [],
    }))
}

fn products_unpassed(products: Option<&Value>) -> Result<bool, String> {
    let Some(list) = products.and_then(Value::as_array) else {
        return Err("activation.products".to_owned());
    };
    for product in list {
        let rows = product.get("cc").and_then(Value::as_array).ok_or_else(|| "activation.cc".to_owned())?;
        for row in rows {
            if row.get("status").and_then(Value::as_str) == Some("pass") || row.get("counts_as_probe").and_then(Value::as_bool) == Some(true) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
