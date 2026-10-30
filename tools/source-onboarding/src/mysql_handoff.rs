//! Human-facing entry; all transition/lease rules live in the native worker/target.
use crate::io::*;
use std::path::Path;

pub const HELP: &str = "\
source-onboarding mysql-handoff plan --worker PINNED_WORKER --config WORKER_CONFIG --manifest C1_MANIFEST --out NEW_PLAN
source-onboarding mysql-handoff confirm --worker PINNED_WORKER --config WORKER_CONFIG --plan PLAN --confirm-sha256 SHA256

Disable and drain ALL bindings of the logical C5 source across all projects, including
historical full captures and current layered captures. Plan performs metadata-only MySQL C1
eligibility checks; log OFF, insufficient retention, wrong identity or outstanding work refuses.
Read the emitted plan and confirm its exact SHA256. Confirmation keeps the SAME logical
source and the existing source_progress anchor binding/project; the anchor remains disabled.
All other capture binding IDs, projects and history are retained but cannot be re-enabled.

The candidate producer config must explicitly declare c5_to_c1:
  protocol: oo.mysql.c5-to-c1/v1
  source_id: existing logical source
  binding / project_id: existing source_progress anchor, not a newly created binding
  expected_server_uuid: the independently verified server UUID
  baseline: new_snapshot_plus_log_reconciliation
  prior_history: not_recoverable_from_new_logs

A NEW baseline plus complete L0..L_end log reconciliation is required; old C5 data is NOT
binlog history. Nothing resets an offset or silently recaptures. Use the normal worker
enable/fire/step and publication commands afterward, and verify the replacement before
switching its Gateway. The old published generation remains readable until that switch.
Control/target must share one database. Full-C5 and layered-C5 adapters are supported.
The candidate's tables and columns must match the COMPLETE current published source:
no omitted table, missing column, extra projection or duplicate member is accepted.
No-change-only layered bindings require database baseline authority tied to a genuine
retained capture. Mutable paths or caller trust flags alone cannot authorize the switch.
After confirmation, only the anchor may put/enable/fire. Normal disabling stays available.

Enable recoverable fill before planning with:
  c1_initial_resume: {protocol: oo.mysql.initial-resume/v1}
After interruption, native-step resumes the SAME task after lease expiry, preserving its
original L0, config hash, deadline and durable ACK cursor. Missing journals/expired logs
refuse; no implicit recapture, new task, deadline extension or offset reset is allowed.
";

pub fn dispatch(args:&[String])->Result<()> {
    if args.is_empty() || args==["--help"] || args==["help"] {
        print!("{HELP}");return Ok(());
    }
    require(args.len()==9 && ["plan","confirm"].contains(&args[0].as_str())
        && args[1]=="--worker" && args[3]=="--config"
        && if args[0]=="plan" {args[5]=="--manifest" && args[7]=="--out"}
            else {args[5]=="--plan" && args[7]=="--confirm-sha256"}, HELP)?;
    binary(&args[2])?;
    private(Path::new(&args[4]))?;
    let status=std::process::Command::new(&args[2]).args(["--config",&args[4],
        &format!("native-mysql-handoff-{}",args[0]),&args[5],&args[6],&args[7],&args[8]])
        .status()?;
    require(status.success(),"mysql_handoff_refused")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn help_and_refusal_require_no_runtime_or_source() {
        dispatch(&["--help".into()]).unwrap();
        assert!(dispatch(&["confirm".into()]).is_err());
        assert!(HELP.contains("NEW baseline") && HELP.contains("remains disabled"));
        assert!(HELP.contains("ALL bindings") && HELP.contains("COMPLETE current published source"));
        assert!(!HELP.contains("single-table"));
    }
}
