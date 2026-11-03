//! Common read-only SourceEngine diagnostics, also used by the lite MySQL helper.
use oo_connectors::engine::{
    Capability, CapabilityState, ProbeReport, ReadBudget, SourceEngine, TableRef,
};
use serde_json::{Value, json};

pub async fn inspect<E: SourceEngine>(engine: &mut E, tables: &[TableRef], mode: &str) -> Value {
    let probe = match engine.probe().await {
        Ok(probe) => probe,
        Err(error) => return json!({"status":"blocked","error":error.to_string()}),
    };
    let mut blocked = !available(&probe, Capability::ChunkedSnapshot)
        || (mode == "C1" && !available(&probe, Capability::LogCdc));
    let mut plans = Vec::new();
    for table in tables {
        match engine.plan_chunks(table, &ReadBudget::default()).await {
            Ok(plan) => {
                let estimate = engine.estimate_bytes(table).await;
                let mut report = json!({
                    "table": table, "strategy": plan.strategy, "key": plan.key,
                    "chunks": plan.chunks.len(), "estimated_rows": plan.estimated_rows,
                    "estimated_bytes": null, "bytes_basis": "unknown"
                });
                blocked |= apply_estimate(&mut report, estimate);
                plans.push(report);
            }
            Err(error) => {
                blocked = true;
                plans.push(json!({"table":table,"error":error.to_string()}));
            }
        }
    }
    json!({
        "status": if blocked { "blocked" } else { "ok" },
        "probe": probe, "dba_requirements": probe.requirements(), "chunk_plans": plans,
    })
}

fn apply_estimate(
    report: &mut Value,
    estimate: Result<Option<u64>, oo_connectors::SourceError>,
) -> bool {
    report["estimated_bytes"] = json!(null);
    report["bytes_basis"] = json!("unknown");
    match estimate {
        Ok(Some(bytes)) => {
            report["estimated_bytes"] = json!(bytes);
            report["bytes_basis"] = json!("source_engine_estimate");
            false
        }
        Ok(None) => false,
        Err(error) => {
            report["estimate_error"] = json!(error.to_string());
            true
        }
    }
}

fn available(probe: &ProbeReport, capability: Capability) -> bool {
    matches!(
        probe.capabilities.get(&capability),
        Some(CapabilityState::Available)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_connectors::SourceId;
    #[test]
    fn byte_estimates_preserve_unknown_zero_and_failure() {
        for value in [None, Some(0), Some(u64::MAX)] {
            let mut report = json!({});
            assert!(!apply_estimate(&mut report, Ok(value)));
            assert_eq!(report["estimated_bytes"], json!(value));
            assert_eq!(
                report["bytes_basis"],
                if value.is_some() {
                    "source_engine_estimate"
                } else {
                    "unknown"
                }
            );
        }
        let mut report = json!({});
        assert!(apply_estimate(
            &mut report,
            Err(SourceId::new("").err().unwrap())
        ));
        assert!(report["estimated_bytes"].is_null());
        assert!(report["estimate_error"].is_string());
    }
}
