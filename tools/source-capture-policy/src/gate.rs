//! Method eligibility. Auto skips a method that fails a gate. Explicit
//! selection returns that gate and does not try another method.

use cdc_offset_store::{cmp_pos, pos_ok};

use crate::model::{
    rank, CaptureMethod, CursorSafety, DeleteDetection, Grade, MethodInput, PlanRequest,
    SupportGrade, SyncMode, TableInput, TokenOrder, WriteClass,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionAction {
    Ready,
    Stale,
    Pause,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eligible {
    pub method: CaptureMethod,
    pub grade: Grade,
    pub delete_detection: DeleteDetection,
    pub token_order: TokenOrder,
    pub statuses: Vec<&'static str>,
    pub retention: RetentionAction,
    pub initial_load: &'static str,
    pub reasons: Vec<&'static str>,
    pub position: Option<String>,
    pub comparable: bool,
    pub precision_loss: bool,
    pub token: Option<String>,
    pub lag_seconds: Option<u64>,
    pub resume_opaque: bool,
}

pub enum SelectError {
    Explicit(&'static str),
    NoneEligible(Vec<&'static str>),
}

pub fn select(request: &PlanRequest, table: &TableInput) -> Result<Eligible, SelectError> {
    match request.sync {
        SyncMode::Explicit => {
            let Some(wanted) = table.explicit_method else {
                return Err(SelectError::Explicit("explicit_method_required"));
            };
            let Some(spec) = table.methods.iter().find(|item| item.method == wanted) else {
                return Err(SelectError::Explicit("method_absent"));
            };
            consider(request, table, spec, true).map_err(SelectError::Explicit)
        }
        SyncMode::Auto => {
            let mut reasons = Vec::new();
            for kind in [
                CaptureMethod::C1,
                CaptureMethod::C2,
                CaptureMethod::C4,
                CaptureMethod::C3,
                CaptureMethod::C5,
            ] {
                let Some(spec) = table.methods.iter().find(|item| item.method == kind) else {
                    continue;
                };
                match consider(request, table, spec, false) {
                    Ok(eligible) => return Ok(eligible),
                    Err(code) => push_reason(&mut reasons, code),
                }
            }
            if reasons.is_empty() {
                reasons.push("no_eligible_method");
            }
            Err(SelectError::NoneEligible(reasons))
        }
    }
}

fn consider(
    request: &PlanRequest,
    table: &TableInput,
    spec: &MethodInput,
    explicit: bool,
) -> Result<Eligible, &'static str> {
    support_ok(request, spec, explicit)?;
    write_ok(spec, explicit)?;
    let codec_order = codec_order(&spec.position_codec, &spec.family)?;
    if spec.token_order != codec_order {
        return Err("order_codec_mismatch");
    }
    if rank(spec.consistency_ceiling) > rank(max_grade(spec.method)) {
        return Err("ceiling_exceeds_method");
    }
    method_shape(spec)?;
    key_ok(request, table, spec)?;
    delete_ok(spec)?;
    cursor_ok(spec, explicit)?;
    as_of_ok(spec)?;
    precision_ok(spec)?;
    let (retention, mut reasons) = retention_of(spec)?;
    let comparable = comparable_position(spec, retention);
    if spec.method == CaptureMethod::C5 && table.source_changing_without_token {
        reasons.push("source_changed_during_read");
    }
    if matches!(spec.position_codec.as_str(), "mysql-gtid/v1" | "mariadb-gtid/v1")
        && retention != RetentionAction::Expired
    {
        reasons.push("position_compare_unimplemented");
    }
    let grade = if spec.method == CaptureMethod::C5 && table.source_changing_without_token {
        Grade::B
    } else {
        spec.consistency_ceiling
    };
    let statuses = statuses_of(spec, retention, table.budget_exhausted);
    Ok(Eligible {
        method: spec.method,
        grade,
        delete_detection: spec.delete_detection,
        token_order: spec.token_order,
        statuses,
        retention: if table.budget_exhausted
            && matches!(retention, RetentionAction::Ready | RetentionAction::Stale)
        {
            RetentionAction::Pause
        } else {
            retention
        },
        initial_load: initial_load(spec.method),
        reasons,
        position: spec.applied_pos.clone(),
        comparable,
        precision_loss: spec.precision_truncated,
        token: spec.as_of.as_ref().map(|item| item.snapshot_token.clone()),
        lag_seconds: spec.retention.as_ref().map(|item| item.lag_seconds),
        resume_opaque: spec.token_order == TokenOrder::Opaque && retention != RetentionAction::Expired,
    })
}

fn support_ok(request: &PlanRequest, spec: &MethodInput, explicit: bool) -> Result<(), &'static str> {
    match spec.support_grade {
        SupportGrade::Verified | SupportGrade::Certified => Ok(()),
        SupportGrade::Experimental
            if explicit && matches!(request.profile, crate::model::Profile::NonProduction) =>
        {
            Ok(())
        }
        SupportGrade::Experimental => Err("capability_unverified"),
        SupportGrade::Candidate | SupportGrade::Designed => Err("capability_not_admitted"),
    }
}

fn write_ok(spec: &MethodInput, explicit: bool) -> Result<(), &'static str> {
    match spec.write_class {
        WriteClass::R0 | WriteClass::R1 => Ok(()),
        WriteClass::R1w if spec.r1w_adr && spec.r1w_audit && explicit => Ok(()),
        WriteClass::R1w if !explicit => Err("r1w_not_auto"),
        WriteClass::R1w => Err("r1w_disabled"),
        WriteClass::R2 | WriteClass::R3 => Err("write_class_blocked"),
    }
}

fn method_shape(spec: &MethodInput) -> Result<(), &'static str> {
    if spec.method == CaptureMethod::C1 {
        if spec.consistency_ceiling != Grade::T {
            return Err("c1_requires_t");
        }
        if !spec.transaction_boundary {
            return Err("transaction_boundary_required");
        }
        if spec.token_order != TokenOrder::Total {
            return Err("c1_requires_total");
        }
    }
    if spec.method == CaptureMethod::C2 && !spec.token_available {
        return Err("token_unavailable");
    }
    if spec.method == CaptureMethod::C4 && !spec.partition_metadata_trusted {
        return Err("partition_metadata_untrusted");
    }
    if spec.position_codec == "mysql-binlog-pos/v1" && !spec.same_instance {
        return Err("binlog_pos_instance");
    }
    if matches!(spec.position_codec.as_str(), "mysql-gtid/v1" | "mariadb-gtid/v1") && !spec.same_source_uuid
    {
        return Err("gtid_source");
    }
    Ok(())
}

fn key_ok(request: &PlanRequest, table: &TableInput, spec: &MethodInput) -> Result<(), &'static str> {
    let keyed = table.stable_key || table.declared_unique_key;
    if !keyed {
        let full_partition = spec.method == CaptureMethod::C4 && spec.full_partition_replay;
        if spec.method != CaptureMethod::C5 && !full_partition {
            return Err("key_required");
        }
    }
    if spec.method == CaptureMethod::C5 && table.row_estimate > request.volume_limit_rows {
        return Err("volume_exceeds_limit");
    }
    Ok(())
}

fn delete_ok(spec: &MethodInput) -> Result<(), &'static str> {
    if spec.soft_delete_only && spec.delete_detection != DeleteDetection::None {
        return Err("soft_delete_not_physical");
    }
    let allowed = match spec.method {
        CaptureMethod::C1 | CaptureMethod::C2 => matches!(
            spec.delete_detection,
            DeleteDetection::Native | DeleteDetection::Reconcile | DeleteDetection::None
        ),
        CaptureMethod::C3 | CaptureMethod::C5 => {
            matches!(spec.delete_detection, DeleteDetection::Reconcile | DeleteDetection::None)
        }
        CaptureMethod::C4 => spec.delete_detection == DeleteDetection::PartitionReplace,
    };
    if !allowed {
        return Err("delete_method_mismatch");
    }
    if spec.delete_detection == DeleteDetection::Reconcile {
        let Some(period) = spec.reconcile_period_seconds else {
            return Err("reconcile_unconfigured");
        };
        let Some(sla) = spec.delete_visibility_sla_seconds else {
            return Err("reconcile_unconfigured");
        };
        if period == 0 || sla == 0 || period > sla {
            return Err("delete_sla_unmet");
        }
    }
    Ok(())
}

fn cursor_ok(spec: &MethodInput, explicit: bool) -> Result<(), &'static str> {
    if spec.method != CaptureMethod::C3 {
        return if spec.cursor.is_some() {
            Err("cursor_unexpected")
        } else {
            Ok(())
        };
    }
    let Some(cursor) = &spec.cursor else {
        return Err("cursor_required");
    };
    if !cursor.equal_cursor_checkpoint {
        return Err("equal_cursor_checkpoint_required");
    }
    match cursor.safety {
        CursorSafety::Coarse => Err("coarse_cursor"),
        CursorSafety::CommitOrdered => {
            if cursor.upper_bound_is_commit_safe {
                Ok(())
            } else {
                Err("commit_upper_bound_required")
            }
        }
        CursorSafety::StatementTime => {
            overlap_ok(cursor)?;
            if cursor.reconcile_configured {
                Ok(())
            } else {
                Err("reconcile_required")
            }
        }
        CursorSafety::AppMaintained => {
            if !explicit {
                return Err("app_maintained_not_auto");
            }
            if !cursor.admin_confirmed {
                return Err("app_maintained_unconfirmed");
            }
            overlap_ok(cursor)?;
            if cursor.reconcile_configured {
                Ok(())
            } else {
                Err("reconcile_required")
            }
        }
    }
}

fn overlap_ok(cursor: &crate::model::CursorInput) -> Result<(), &'static str> {
    let Some(age) = cursor
        .max_transaction_age_seconds
        .or(cursor.conservative_max_transaction_age_seconds)
    else {
        return Err("transaction_age_unknown");
    };
    if cursor.overlap_seconds < age {
        Err("overlap_short")
    } else {
        Ok(())
    }
}

fn as_of_ok(spec: &MethodInput) -> Result<(), &'static str> {
    if spec.method != CaptureMethod::C2 {
        return if spec.as_of.is_some() {
            Err("as_of_unexpected")
        } else {
            Ok(())
        };
    }
    let Some(as_of) = &spec.as_of else {
        return Err("as_of_required");
    };
    if as_of.snapshot_token.is_empty() || as_of.snapshot_token.len() > 128 {
        return Err("as_of_token_required");
    }
    if as_of.read_seconds >= as_of.token_retention_seconds {
        return Err("as_of_exceeds_retention");
    }
    if spec.consistency_ceiling == Grade::S1 && as_of.read_seconds > 300 {
        return Err("snapshot_exceeds_300s");
    }
    if spec.consistency_ceiling == Grade::S1 && !as_of.same_token {
        return Err("as_of_token_required");
    }
    Ok(())
}

fn precision_ok(spec: &MethodInput) -> Result<(), &'static str> {
    let used = spec.method == CaptureMethod::C3 || spec.used_as_primary_key;
    if spec.precision_truncated && used && !spec.original_column_retained_as_key {
        Err("precision_truncated")
    } else {
        Ok(())
    }
}

fn retention_of(spec: &MethodInput) -> Result<(RetentionAction, Vec<&'static str>), &'static str> {
    if matches!(spec.method, CaptureMethod::C1 | CaptureMethod::C2) && spec.retention.is_none() {
        return Err("retention_required");
    }
    let mut action = RetentionAction::Ready;
    let mut reasons = Vec::new();
    if let Some(retention) = &spec.retention {
        if retention.window_seconds == 0 || retention.lag_seconds > retention.window_seconds {
            action = RetentionAction::Expired;
            reasons.push("position_outside_window");
        } else if over_eighty(retention.lag_seconds, retention.window_seconds) {
            action = RetentionAction::Pause;
            reasons.push("retention_pause");
        } else if over_half(retention.lag_seconds, retention.window_seconds) {
            action = RetentionAction::Stale;
            reasons.push("retention_stale");
        }
    }
    if position_outside(spec)? {
        action = RetentionAction::Expired;
        reasons.push("position_bounds");
    }
    if spec.position_codec == "mysql-binlog-pos/v1" && spec.failover_observed {
        action = RetentionAction::Expired;
        reasons.push("binlog_pos_failover");
    } else if spec.failover_observed && spec.failover_position_retained != Some(true) {
        action = RetentionAction::Expired;
        reasons.push("failover_unretained");
    }
    Ok((action, reasons))
}

fn position_outside(spec: &MethodInput) -> Result<bool, &'static str> {
    let (Some(position), Some(low), Some(high)) =
        (&spec.applied_pos, &spec.window_low, &spec.window_high)
    else {
        return Ok(false);
    };
    if !decimal_codec(&spec.position_codec) || spec.token_order != TokenOrder::Total {
        return Err("window_bounds_not_total");
    }
    if !pos_ok(position) || !pos_ok(low) || !pos_ok(high) {
        return Err("position_syntax");
    }
    Ok(cmp_pos(position, low).is_lt() || cmp_pos(position, high).is_gt())
}

fn comparable_position(spec: &MethodInput, retention: RetentionAction) -> bool {
    if retention == RetentionAction::Expired || spec.token_order != TokenOrder::Total {
        return false;
    }
    if spec.position_codec == "mysql-binlog-pos/v1" && (!spec.same_instance || spec.failover_observed)
    {
        return false;
    }
    if matches!(spec.position_codec.as_str(), "mysql-gtid/v1" | "mariadb-gtid/v1") {
        return false;
    }
    matches!(
        &spec.applied_pos,
        Some(position) if decimal_codec(&spec.position_codec) && pos_ok(position)
    )
}

fn statuses_of(
    spec: &MethodInput,
    retention: RetentionAction,
    budget_exhausted: bool,
) -> Vec<&'static str> {
    let mut statuses = Vec::new();
    if retention == RetentionAction::Expired {
        statuses.push("POSITION_EXPIRED");
    }
    if let Some(window) = &spec.retention
        && window.window_seconds > 0
        && over_half(window.lag_seconds, window.window_seconds)
    {
        statuses.push("STALE_RISK");
    }
    match spec.delete_detection {
        DeleteDetection::Reconcile => statuses.push("DELETES_LAGGED"),
        DeleteDetection::None => statuses.push("DELETES_UNDETECTED"),
        DeleteDetection::Native | DeleteDetection::PartitionReplace => {}
    }
    if budget_exhausted {
        statuses.push("SOURCE_BUDGET_EXHAUSTED");
    }
    statuses
}

fn initial_load(method: CaptureMethod) -> &'static str {
    match method {
        CaptureMethod::C1 => "cdc_first_snapshot_fill",
        CaptureMethod::C2 => "token_anchored_snapshot_replay",
        CaptureMethod::C3 => "full_then_cursor",
        CaptureMethod::C4 => "partition_replace_one_transaction",
        CaptureMethod::C5 => "shadow_diff",
    }
}

fn max_grade(method: CaptureMethod) -> Grade {
    match method {
        CaptureMethod::C1 => Grade::T,
        CaptureMethod::C2 => Grade::S1,
        CaptureMethod::C3 => Grade::B,
        CaptureMethod::C4 | CaptureMethod::C5 => Grade::S0,
    }
}

fn codec_order(codec: &str, family: &str) -> Result<TokenOrder, &'static str> {
    match codec {
        "pg-commit-lsn/v1"
        | "oracle-scn/v1"
        | "sqlserver-lsn/v1"
        | "mongo-cluster-time/v1"
        | "snowflake-timestamp/v1"
        | "bigquery-timestamp/v1"
        | "delta-version/v1"
        | "iceberg-sequence/v1"
        | "mysql-binlog-pos/v1" => Ok(TokenOrder::Total),
        "mysql-gtid/v1" if family == "mysql" => Ok(TokenOrder::Total),
        "mariadb-gtid/v1" if family == "mariadb" => Ok(TokenOrder::Total),
        "mysql-gtid/v1" | "mariadb-gtid/v1" => Err("gtid_family"),
        "dynamodb-shard/v1" | "kafka-offset/v1" => Ok(TokenOrder::PerPartition),
        "mongo-resume-token/v1" | "opaque-cursor/v1" | "batch-cursor-us/v1" => Ok(TokenOrder::Opaque),
        "snapshot-zero/v1" | "none/v1" => Ok(TokenOrder::None),
        _ => Err("codec_unknown"),
    }
}

fn decimal_codec(codec: &str) -> bool {
    matches!(
        codec,
        "pg-commit-lsn/v1"
            | "oracle-scn/v1"
            | "sqlserver-lsn/v1"
            | "mysql-binlog-pos/v1"
            | "delta-version/v1"
            | "iceberg-sequence/v1"
            | "mongo-cluster-time/v1"
            | "snowflake-timestamp/v1"
            | "bigquery-timestamp/v1"
    )
}

fn over_half(lag: u64, window: u64) -> bool {
    u128::from(lag) * 2 > u128::from(window)
}

fn over_eighty(lag: u64, window: u64) -> bool {
    u128::from(lag) * 5 > u128::from(window) * 4
}

fn push_reason(reasons: &mut Vec<&'static str>, code: &'static str) {
    if !reasons.contains(&code) {
        reasons.push(code);
    }
}
