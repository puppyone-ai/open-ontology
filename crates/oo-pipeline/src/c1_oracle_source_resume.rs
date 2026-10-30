//! Admit a native AS OF snapshot only with its separate, immutable log proof.
use crate::c1_buffer_position::ArtifactRef;
use crate::c1_typed::TableSpec;
use serde_json::{Value, json};
use std::collections::BTreeSet;

type Result<T> = std::result::Result<T, &'static str>;
const SMALL: u64 = 1024 * 1024;

fn reference(value: &Value) -> Result<ArtifactRef> {
    serde_json::from_value(value.clone()).map_err(|_| "oracle_source_resume_pin")
}
fn document(value: &Value) -> Result<Value> { reference(value)?.document() }
fn scn(value: &Value) -> Result<u128> {
    let text = value.as_str().filter(|s| super::decimal(s) && *s != "0")
        .ok_or("oracle_source_resume_scn")?;
    text.parse().map_err(|_| "oracle_source_resume_scn")
}

pub(super) fn validate(bundle: &Value) -> Result<()> {
    let b = &bundle["boundary"];
    let proof = document(&b["source_resume_proof"])?;
    let snapshot = document(&b["native_snapshot"])?;
    let start = scn(&proof["log_start_scn"])?;
    let s = scn(&proof["snapshot_scn"])?;
    let cut = scn(&proof["cut_scn"])?;
    if b["mode"] != "oracle_scn_snapshot_and_log"
        || proof["protocol"] != "oo.oracle.initial-source-resume-proof/v1"
        || proof["source_id"] != bundle["source_id"] || proof["identity"] != b["identity"]
        || proof["snapshot_scn"] != b["snapshot_scn"] || proof["cut_scn"] != b["cut_scn"]
        || !(start <= s && s < cut) || proof["snapshot"] != b["native_snapshot"]
        || b["snapshot_origin"] != "native_as_of_scn" || b["snapshot_restarted"] != false
        || b["initial_position"] != "0" || !b["stream_mode"].is_null() || !bundle["rollover"].is_null()
        || proof["source_snapshot_resumed"] != true || proof["source_snapshot_restarted"] != false
        || proof["source_offset_synthesized"] != false
        || snapshot["protocol"] != "oo.oracle.sealed-snapshot/v1"
        || snapshot["complete"] != true || snapshot["scope_complete"] != true
        || snapshot["binding"]["source_id"] != bundle["source_id"]
        || snapshot["binding"]["identity"] != b["identity"] || snapshot["binding"]["scn"] != b["snapshot_scn"]
        || snapshot["binding"]["typed_verify"] != true
        || b["schema_manifest_sha256"] != b["native_snapshot"]["sha256"]
        || snapshot["binding"]["driver_sha256"] != proof["reader"]["sha256"]
        || b["independent_native_reader_sha256"] != proof["reader"]["sha256"]
        || proof["selected_log_spool"]["sha256"] != b["spool_sha256"]
        || proof["vendor_checkpoint_after"]["sha256"] != b["connector_offsets_sha256"]
        || proof["connector_history"]["sha256"] != b["connector_history_sha256"]
        || proof["vendor_checkpoint_before"]["sha256"] == proof["vendor_checkpoint_after"]["sha256"]
    { return Err("oracle_source_resume_binding"); }

    let inventory = snapshot["binding"]["inventory"].as_array().filter(|v| v.len() == 1)
        .ok_or("oracle_source_resume_scope")?;
    let table = proof["table"].as_str().ok_or("oracle_source_resume_scope")?;
    let native = &inventory[0];
    if Some(table) != native["owner"].as_str().zip(native["table"].as_str())
        .map(|(owner, name)| format!("{owner}.{name}")).as_deref()
        || b["scope"] != json!([table])
        || bundle["typed_tables"].as_object().is_none_or(|v| v.len() != 1 || !v.contains_key(table))
        || bundle["source_mappings"].as_object().is_none_or(|v| v.len() != 1 || !v.contains_key(table))
    { return Err("oracle_source_resume_scope"); }
    let spec: TableSpec = serde_json::from_value(bundle["typed_tables"][table].clone())
        .map_err(|_| "oracle_source_resume_schema")?;
    spec.validate()?;
    let columns = native["columns"].as_array().ok_or("oracle_source_resume_schema")?;
    let mappings = bundle["source_mappings"][table].as_array().ok_or("oracle_source_resume_schema")?;
    if columns.len() != spec.columns.len() || mappings.len() != columns.len()
        || native["keys"] != json!(spec.keys.iter().map(|n| &spec.columns[*n].name).collect::<Vec<_>>())
        || columns.iter().zip(mappings).zip(&spec.columns).any(|((source, mapping), typed)|
            mapping["source_column"] != *source || source["name"] != typed.name
                || mapping["name"] != typed.name || (source["nullable"] == "Y") != typed.nullable)
    { return Err("oracle_source_resume_schema"); }

    let transactions = proof["selected_transactions"].as_array().filter(|v| !v.is_empty())
        .ok_or("oracle_source_resume_cut")?;
    let checkpoints = bundle["checkpoints"].as_array().ok_or("oracle_source_resume_cut")?;
    if transactions.len() != checkpoints.len() || transactions.len() > 100000
        || b["final_position"] != transactions.len().to_string()
    { return Err("oracle_source_resume_cut"); }
    let mut previous = s;
    let mut ids = BTreeSet::new();
    for (n, (transaction, checkpoint)) in transactions.iter().zip(checkpoints).enumerate() {
        let commit = scn(&transaction["commit_scn"])?;
        let id = transaction["transaction_id"].as_str().filter(|id| super::token(id))
            .ok_or("oracle_source_resume_cut")?;
        if commit <= s || commit < previous || commit > cut || !ids.insert(id)
            || checkpoint["application_position"] != (n + 1).to_string()
            || checkpoint["transaction_id"] != id || checkpoint["commit_scn"] != transaction["commit_scn"]
            || checkpoint["event_count"] != transaction["event_count"]
            || transaction["event_count"].as_u64().is_none_or(|n| n == 0 || n > 100000)
            || checkpoint["event_sha256"].as_str().is_none_or(|s| !super::hex64(s))
        { return Err("oracle_source_resume_cut"); }
        previous = commit;
    }
    if previous != cut { return Err("oracle_source_resume_cut"); }

    let admission = document(&proof["log_admission"])?;
    let coverage = document(&proof["log_coverage"])?;
    let resume = document(&proof["snapshot_resume_state"])?;
    let subscription = document(&proof["subscription_resume"])?;
    if admission["protocol"] != "oo.oracle.logminer-admission/v1"
        || admission["source_id"] != bundle["source_id"] || admission["table"] != table
        || admission["snapshot_mode"] != "no_data" || admission["in_progress_transaction_mode"] != "all"
        || admission["vendor_offset_synthesized"] != false || admission["log_start_scn"] != proof["log_start_scn"]
        || admission["vendor_offset"]["sha256"] != proof["vendor_checkpoint_before"]["sha256"]
        || coverage["covered"] != true || coverage["required_first_scn"] != proof["log_start_scn"]
        || coverage["required_last_scn"] != proof["cut_scn"]
        || resume["protocol"] != "oo.oracle.initial-source-state/v1"
        || resume["status"] != "snapshot_sealed_pending_log_handoff" || resume["rebaseline_required"] != false
        || resume["source_id"] != bundle["source_id"] || resume["snapshot_scn"] != b["snapshot_scn"]
        || resume["snapshot_manifest"] != proof["snapshot"] || resume["snapshot_result"] != snapshot
        || resume["attempt"].as_u64().is_none_or(|n| n < 2) || resume["source_offset_from_target_ack"] != false
        || subscription["source_id"] != bundle["source_id"] || subscription["table"] != table
        || subscription["snapshot_mode"] != "no_data" || subscription["transaction_boundary_mode"] != "all"
        || subscription["receipt"]["incomplete_transactions"] != 0
    { return Err("oracle_source_resume_evidence"); }

    let partial = document(&proof["partial_snapshot"])?;
    let retained = partial["parts"].as_array().filter(|p| !p.is_empty()).ok_or("oracle_source_resume_prefix")?;
    let parts = snapshot["parts"].as_array().filter(|p| p.len() <= 4096)
        .ok_or("oracle_source_resume_prefix")?;
    if partial["snapshot_scn"] != b["snapshot_scn"] || partial["source_recapture_from_start"] != false
        || retained.len() >= parts.len() || partial["rows"].as_u64().is_none_or(|n| n == 0)
        || retained.iter().zip(parts).any(|(old, part)|
            old != &json!([part["file"],part["sha256"],part["upper"]]))
        || partial["next_lower_exclusive"] != serde_json::from_str::<Value>(
            parts[retained.len()-1]["upper"].as_str().ok_or("oracle_source_resume_prefix")?)
            .map_err(|_| "oracle_source_resume_prefix")?
    { return Err("oracle_source_resume_prefix"); }

    for (field, limit) in [
        ("reader", 32*SMALL), ("original_log_spool", 512*SMALL), ("selected_log_spool", 512*SMALL),
        ("vendor_checkpoint_before", SMALL), ("vendor_checkpoint_after", SMALL),
        ("connector_history", 32*SMALL), ("log_catalog", SMALL),
    ] { reference(&proof[field])?.verify(limit)?; }
    let boundary = document(&resume["boundary"])?;
    if boundary["protocol"] != "oo.oracle.snapshot-boundary/v1"
        || boundary["source_id"] != bundle["source_id"] || boundary["identity"] != b["identity"]
        || boundary["snapshot_scn"] != b["snapshot_scn"] || boundary["table"] != *native
        || boundary["log_admission"]["sha256"] != proof["log_admission"]["sha256"]
    { return Err("oracle_source_resume_boundary"); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires OO_ORACLE_SOURCE_RESUME_PACKET; immutable owned capture only"]
    fn real_resumed_source_packet_and_wrong_cut_identity_schema_are_checked() {
        let path = std::env::var("OO_ORACLE_SOURCE_RESUME_PACKET").unwrap();
        let packet: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let bundle = document(&packet["bundle"]).unwrap();
        validate(&bundle).unwrap();
        let capture = crate::c1_source_position::CapturePosition {
            protocol: "oo.c1.oracle-buffer-cursor/v1".into(),
            bundle: serde_json::from_value(packet["bundle"].clone()).unwrap(),
        };
        let frames = std::fs::read_to_string(
            capture.bundle.path.parent().unwrap().join("batches.jsonl"),
        ).unwrap();
        for line in [frames.lines().next().unwrap(), frames.lines().last().unwrap()] {
            let frame: Value = serde_json::from_str(line).unwrap();
            let batch = serde_json::from_value(frame["batch"].clone()).unwrap();
            let bound = capture.bound(bundle["source_id"].as_str().unwrap(), &batch).unwrap();
            assert_eq!(bound["measured_id"], capture.identity().unwrap().measured_id().unwrap());
            assert_eq!(bound["apply_ordinal"], frame["batch"]["position"]);
            assert_eq!(bound["source_offset_advanced"], false);
        }
        for (pointer, value) in [
            ("/source_id", json!("other")),
            ("/boundary/mode", json!("debezium_snapshot_and_log")),
            ("/boundary/snapshot_scn", json!("1")),
            ("/boundary/cut_scn", json!("1")),
            ("/boundary/snapshot_restarted", json!(true)),
            ("/boundary/source_resume_proof/sha256", json!("0".repeat(64))),
            ("/boundary/native_snapshot/sha256", json!("0".repeat(64))),
            ("/checkpoints/0/commit_scn", json!("1")),
            ("/checkpoints/0/event_count", json!(999)),
            ("/boundary/final_position", json!("2")),
        ] {
            let mut bad = bundle.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(validate(&bad).is_err(), "{pointer}");
        }
        let table = bundle["boundary"]["scope"][0].as_str().unwrap();
        let mut bad = bundle.clone();
        bad["source_mappings"][table][0]["source_column"]["type"] = json!("VARCHAR2");
        assert!(validate(&bad).is_err());
    }
}
