//! Initial-fill recovery is opt-in and never replaces later delta capture.
use super::*;
use std::collections::BTreeSet;

pub const PROTOCOL: &str = "oo.mysql.initial-resume/v1";

pub fn enabled(config: &Value, typed: bool, oracle: bool, base: i64) -> Result<bool> {
    let Some(option) = config.get("c1_initial_resume") else { return Ok(false); };
    if !typed || oracle || base < 0
        || option.as_object().is_none_or(|o| o.len() != 1)
        || option["protocol"] != PROTOCOL
    { return Err(Error::new("mysql_initial_mode")); }
    Ok(base == 0)
}

pub fn handoff_enabled(config:&Value,base:i64,handoff:&Value)->Result<bool> {
    if handoff.is_null() {return Ok(false);}
    if base<1 || handoff["parent_source_generation"]!=base
        || handoff["protocol"]!="oo.mysql.c5-to-c1/v1"
        || config["c1_initial_resume"]!=json!({"protocol":PROTOCOL}) {
        return if config.get("c1_initial_resume").is_none() {Ok(false)}
            else {Err(Error::new("mysql_initial_handoff"))};
    }
    Ok(true)
}

pub fn open_request(frame: &Value, request: &mut Value) -> Result<()> {
    let header = &frame["initial_header"];
    let source_generation=request["source_generation"].as_u64().unwrap_or(1);
    if frame["typed_initial_resume"] != true || frame["typed_delta"] != false
        || header["protocol"] != "oo.mysql.initial-session/v1"
        || header["source_id"] != request["source_id"] || header["task_id"] != request["lease_id"]
        || header["source_generation"] != source_generation
        || header["handoff"] != request["handoff"]
        || (source_generation!=1 && (request["handoff"]["protocol"]!="oo.mysql.c5-to-c1/v1"
            || request["handoff"]["parent_source_generation"].as_u64().and_then(|n|n.checked_add(1))!=Some(source_generation)))
        || ["l0", "config_digest", "schema_digest", "typed_tables"].iter().any(|key| header[*key] != request[*key])
    { return Err(Error::new("mysql_initial_open")); }
    request["operation"] = json!("mysql_initial_open");
    request["source_engine"] = json!("mysql");
    request["initial_header"] = header.clone();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn open_ack(
    receipt: &Value, frame: &Value, previous: &Value, source: &str, task: &str,
    epoch: i64, tables: &BTreeSet<String>, reply: &mut Value,
) -> Result<(i64, u64, BTreeSet<String>)> {
    let state = &receipt["initial_resume"];
    let sequence = state["sequence"].as_i64().filter(|n| *n >= 0)
        .ok_or_else(|| Error::new("mysql_initial_checkpoint"))?;
    let maximum = state["max_sequence"].as_i64().ok_or_else(|| Error::new("mysql_initial_checkpoint"))?;
    let next = sequence.checked_add(1).ok_or_else(|| Error::new("mysql_initial_checkpoint"))?;
    let position = state["position"].as_str().and_then(|s| s.parse::<u64>().ok()
        .filter(|n| n.to_string() == s)).ok_or_else(|| Error::new("mysql_initial_position"))?;
    let completed: Vec<String> = serde_json::from_value(state["completed_tables"].clone())
        .map_err(|_| Error::new("mysql_initial_tables"))?;
    let complete = completed.iter().cloned().collect::<BTreeSet<_>>();
    if receipt["outcome"] != "open" || receipt["source_epoch"] != epoch
        || state["protocol"] != PROTOCOL || state["source_id"] != source || state["task_id"] != task
        || state["source_epoch"] != epoch || state["metadata"] != frame["initial_header"]
        || state["l0"] != frame["l0"] || !(sequence..=next).contains(&maximum)
        || complete.len() != completed.len() || !complete.is_subset(tables)
        || state["table_cursors"].as_object().is_none_or(|c| c.keys().cloned().collect::<BTreeSet<_>>() != *tables)
        || !matches!(state["phase"].as_str(), Some("fill" | "reconcile" | "sealed"))
        || (state["phase"] == "fill") != state["l_end"].is_null()
        || (sequence == 0) != state["digest"].is_null()
        || (!previous.is_null() && previous != state) || (previous.is_null() && sequence != 0)
    { return Err(Error::new("mysql_initial_open_proof")); }
    reply["initial_resume"] = state.clone();
    Ok((next, position, complete))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reinstall_preserves_extended_audit_stages_instead_of_narrowing_the_check() {
        let base = native_schema_base().unwrap();
        assert!(base.contains("CREATE TABLE IF NOT EXISTS jw_native.process_attempts"));
        assert!(!base.contains("DROP CONSTRAINT IF EXISTS process_attempts_stage_check"));
        assert!(!base.contains("ADD CONSTRAINT process_attempts_stage_check"));
        let extension = include_str!("../native_mysql_initial.sql");
        assert!(extension.contains("normalization_rejects_999"));
        assert!(extension.contains("loader_c1_initial_arbitrary'',false"));
    }

    #[test]
    fn opt_in_is_initial_only_and_retained_config_allows_later_deltas() {
        assert!(!enabled(&json!({}), true, false, 0).unwrap());
        let config = json!({"c1_initial_resume":{"protocol":PROTOCOL}});
        assert!(enabled(&config, true, false, 0).unwrap());
        assert!(!enabled(&config, true, false, 1).unwrap());
        assert!(enabled(&config, false, false, 0).is_err());
        assert!(enabled(&config, true, true, 0).is_err());
        assert!(enabled(&json!({"c1_initial_resume":{"protocol":PROTOCOL,"reset":true}}), true, false, 0).is_err());
        let handoff=json!({"protocol":"oo.mysql.c5-to-c1/v1","parent_source_generation":7});
        assert!(handoff_enabled(&config,7,&handoff).unwrap());
        assert!(handoff_enabled(&config,6,&handoff).is_err());
        assert!(!handoff_enabled(&json!({}),7,&handoff).unwrap());
        assert!(!handoff_enabled(&config,7,&Value::Null).unwrap());
    }

    #[test]
    fn initial_open_proof_keeps_acked_sequence_separate_from_pending_receipt() {
        let header = json!({"protocol":"oo.mysql.initial-session/v1","source_id":"s","task_id":"t",
            "source_generation":1,"l0":"uuid:1-500","config_digest":"c","schema_digest":"h","typed_tables":{"db.t":{}}});
        let frame = json!({"typed_initial_resume":true,"typed_delta":false,"initial_header":header,"l0":header["l0"]});
        let mut request = json!({"source_id":"s","lease_id":"t","l0":header["l0"],
            "config_digest":"c","schema_digest":"h","typed_tables":header["typed_tables"]});
        open_request(&frame, &mut request).unwrap();
        assert_eq!(request["operation"], "mysql_initial_open");
        let state = json!({"protocol":PROTOCOL,"source_id":"s","task_id":"t","source_epoch":1,
            "sequence":3,"max_sequence":4,"position":"500","metadata":header,"l0":header["l0"],
            "completed_tables":[],"table_cursors":{"db.t":{}},"phase":"fill","l_end":null,"digest":"a".repeat(64)});
        let receipt = json!({"outcome":"open","source_epoch":1,"initial_resume":state});
        let tables = BTreeSet::from(["db.t".to_owned()]);
        let mut reply = json!({});
        let (next, position, completed) = open_ack(&receipt, &frame, &state, "s", "t", 1, &tables, &mut reply).unwrap();
        assert_eq!((next, position, completed.len()), (4, 500, 0));
        for (key, value) in [("task_id", json!("other")), ("max_sequence", json!(5)),
            ("position", json!("0500")), ("completed_tables", json!(["other.table"])),
            ("source_epoch", json!(2)), ("phase", json!("reconcile"))] {
            let mut bad = receipt.clone();
            bad["initial_resume"][key] = value;
            assert!(open_ack(&bad, &frame, &state, "s", "t", 1, &tables, &mut reply).is_err(), "{key}");
        }
    }
}
