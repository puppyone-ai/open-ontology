//! Turn declared capture state into one operator decision.

use crate::gate::{select, Eligible, RetentionAction, SelectError};
use crate::model::{
    ident_ok, next_generation, qualified_table, rank, Decision, Freshness, Grade, JoinDecision,
    PlanRequest, RequestedConsistency, SpoolReport, SyncMode, TableDecision, TableInput, CONTRACT,
};
use crate::receipts::{read_offset, read_spool, OffsetFact, SpoolFact};

pub fn plan_request(request: &PlanRequest) -> Result<Decision, &'static str> {
    validate(request)?;
    let spool = read_spool(request.spool_dir.as_deref());
    let offset = read_offset(request.offset.as_ref());
    let disagreement = positions_disagree(&spool, &offset);
    let mut tables = Vec::with_capacity(request.tables.len());
    for table in &request.tables {
        tables.push(plan_table(table, request, &spool, &offset, disagreement));
    }
    let joins = plan_joins(request, &tables)?;
    Ok(assemble(request, tables, joins, spool.report, offset.report, disagreement))
}

fn validate(request: &PlanRequest) -> Result<(), &'static str> {
    if request.contract != CONTRACT || !ident_ok(&request.source_id) || request.tables.is_empty() {
        return Err("input");
    }
    if request.tables.len() > 256 || request.joins.len() > 256 {
        return Err("input");
    }
    let mut names = Vec::new();
    for table in &request.tables {
        if !qualified_table(&table.name) || !ident_ok(&table.generation) || table.methods.is_empty()
        {
            return Err("input");
        }
        if table.methods.len() > 8 {
            return Err("input");
        }
        if names.contains(&table.name.as_str()) {
            return Err("input");
        }
        names.push(table.name.as_str());
        let mut seen = Vec::new();
        for method in &table.methods {
            if seen.contains(&method.method) || method.family.is_empty() || method.family.len() > 63
            {
                return Err("input");
            }
            seen.push(method.method);
        }
        match request.sync {
            SyncMode::Auto if table.explicit_method.is_some() => {
                return Err("explicit_method_unexpected");
            }
            SyncMode::Explicit if table.explicit_method.is_none() => {
                return Err("explicit_method_required");
            }
            SyncMode::Auto | SyncMode::Explicit => {}
        }
    }
    for join in &request.joins {
        if join.tables.is_empty() || join.source_ids.is_empty() || join.tables.len() > 64 {
            return Err("input");
        }
        for name in &join.tables {
            if !names.contains(&name.as_str()) {
                return Err("join_table_unknown");
            }
        }
        if join.source_ids.iter().any(|source| !ident_ok(source)) {
            return Err("input");
        }
    }
    Ok(())
}

fn plan_table(
    table: &TableInput,
    request: &PlanRequest,
    spool: &SpoolFact,
    offset: &OffsetFact,
    disagreement: bool,
) -> TableDecision {
    let eligible = match select(request, table) {
        Ok(eligible) => eligible,
        Err(SelectError::Explicit(code)) => return refused(table, code, vec![code]),
        Err(SelectError::NoneEligible(reasons)) => {
            return refused(table, "no_eligible_method", reasons);
        }
    };
    let bind_reason = table_offset_reason(table, &eligible, request, offset);
    let disagreement = disagreement || bind_reason == Some("position_disagreement");
    let offset_blocks = offset_blocks(offset);
    let spool_blocks = spool.report.state == "fault";
    let expired = eligible.retention == RetentionAction::Expired;
    let covered = bind_reason.is_none();
    let checkpoint_open =
        offset.report.checkpoint == "absent" || offset.report.checkpoint_advanced;
    let advance = eligible.comparable
        && !expired
        && !disagreement
        && !offset_blocks
        && !spool_blocks
        && covered
        && offset.report.checkpoint_advanced;
    let continue_from_latest = eligible.comparable
        && !expired
        && !disagreement
        && !offset_blocks
        && !spool_blocks
        && covered
        && checkpoint_open;
    let repair_allowed = eligible.comparable
        && !expired
        && !disagreement
        && !offset_blocks
        && !spool_blocks
        && eligible.position.is_some()
        && covered
        && checkpoint_open;
    let consistency_claim_allowed = repair_allowed && eligible.grade != Grade::B;
    let transactional_allowed = consistency_claim_allowed && eligible.grade == Grade::T;
    let resume_opaque = eligible.resume_opaque
        && !disagreement
        && !offset_blocks
        && !spool_blocks
        && covered
        && checkpoint_open;
    let (action, publish, next) = action_of(table, &eligible, expired);
    let mut reasons = eligible.reasons.clone();
    if request.repair_requested && !repair_allowed {
        push(&mut reasons, "repair_denied");
    }
    if let Some(reason) = bind_reason {
        push(&mut reasons, reason);
    }
    if disagreement {
        push(&mut reasons, "position_disagreement");
    }
    if expired {
        push(&mut reasons, "resnapshot_new_generation");
        push(&mut reasons, "old_position_not_advanced");
        if next.is_none() {
            push(&mut reasons, "generation_undecidable");
        }
    }
    TableDecision {
        name: table.name.clone(),
        generation: table.generation.clone(),
        next_generation: next,
        fault: None,
        capture_method: Some(eligible.method),
        consistency_grade: Some(eligible.grade),
        delete_detection: Some(eligible.delete_detection),
        token_order: Some(eligible.token_order),
        statuses: eligible.statuses.clone(),
        operator_action: action,
        initial_load: Some(eligible.initial_load),
        advance_capture_position: advance,
        continue_from_latest,
        resume_opaque,
        repair_allowed,
        consistency_claim_allowed,
        transactional_allowed,
        publish_new_generation: publish,
        old_generation_readable: true,
        precision_loss: eligible.precision_loss,
        token: eligible.token.clone(),
        freshness: Freshness {
            consistency_grade: Some(eligible.grade),
            capture_method: Some(eligible.method),
            lag_seconds: eligible.lag_seconds,
            position: eligible.position.clone(),
        },
        reasons,
    }
}

fn action_of(
    table: &TableInput,
    eligible: &Eligible,
    expired: bool,
) -> (&'static str, bool, Option<String>) {
    if expired {
        let next = next_generation(&table.generation);
        return ("resnapshot", false, next);
    }
    if eligible.retention == RetentionAction::Pause || table.budget_exhausted {
        return ("pause_publish", false, None);
    }
    if eligible.retention == RetentionAction::Stale {
        return ("alert_stale_risk", true, None);
    }
    ("continue", true, None)
}

fn offset_blocks(offset: &OffsetFact) -> bool {
    offset.report.present && offset.report.checkpoint == "refused"
}

/// A table may use this checkpoint only when its generation is selected or
/// required and that receipt matches the selected receipt. Position, codec,
/// lineage, and epoch use the offset store's string equality. Digits are
/// not parsed as floating point.
fn table_offset_reason(
    table: &TableInput,
    eligible: &Eligible,
    request: &PlanRequest,
    offset: &OffsetFact,
) -> Option<&'static str> {
    if !offset.report.present {
        return None;
    }
    if offset.report.checkpoint == "refused" {
        return Some(offset.report.fault.unwrap_or("receipt_rule"));
    }
    let Some(selected_generation) = &offset.selected_generation else {
        return Some("receipt_unbound");
    };
    let in_binding = offset
        .required_generations
        .iter()
        .any(|generation| generation == &table.generation)
        || selected_generation == &table.generation;
    if !in_binding {
        return Some("receipt_unbound");
    }
    let Some(row) = offset
        .receipts
        .iter()
        .find(|receipt| receipt.table_generation == table.generation)
    else {
        return Some("required_incomplete");
    };
    let Some(selected) = offset
        .receipts
        .iter()
        .find(|receipt| &receipt.table_generation == selected_generation)
    else {
        return Some("selected_receipt_absent");
    };
    if row.source != request.source_id || row.source != selected.source {
        return Some("receipt_source_mismatch");
    }
    let Some(method) = table.methods.iter().find(|item| item.method == eligible.method) else {
        return Some("receipt_unbound");
    };
    if row.source_epoch != selected.source_epoch || method.source_epoch != row.source_epoch {
        return Some("receipt_epoch_mismatch");
    }
    if row.position_codec != selected.position_codec
        || row.position_lineage != selected.position_lineage
        || row.applied_pos != selected.applied_pos
        || method.position_codec != row.position_codec
        || method.position_lineage != row.position_lineage
    {
        return Some("position_unbound");
    }
    if row.schema_digest != selected.schema_digest
        || row.config_digest != selected.config_digest
        || row.type_digest != selected.type_digest
        || row.hook_digest != selected.hook_digest
        || row.payload_digest != selected.payload_digest
    {
        return Some("receipt_digest_mismatch");
    }
    match &eligible.position {
        Some(position) if position == &row.applied_pos => {}
        _ => return Some("position_disagreement"),
    }
    if offset.report.checkpoint_advanced {
        None
    } else {
        Some(offset.report.watermark_hold.unwrap_or("receipt_not_advanced"))
    }
}

fn positions_disagree(spool: &SpoolFact, offset: &OffsetFact) -> bool {
    match (&spool.commit_position, &offset.applied_pos) {
        (Some(left), Some(right)) => left != right,
        _ => false,
    }
}

fn plan_joins(request: &PlanRequest, tables: &[TableDecision]) -> Result<Vec<JoinDecision>, &'static str> {
    let mut joins = Vec::new();
    for join in &request.joins {
        let warning = if join.tables.len() < 2 {
            None
        } else if join.source_ids.iter().collect::<std::collections::BTreeSet<_>>().len() > 1
            || join.tables.iter().any(|name| {
                let Some(table) = tables.iter().find(|table| &table.name == name) else {
                    return true;
                };
                table
                    .consistency_grade
                    .is_none_or(|grade| grade != Grade::T)
                    || !table.consistency_claim_allowed
            })
        {
            Some("CROSS_TABLE_SKEW")
        } else {
            None
        };
        joins.push(JoinDecision {
            tables: join.tables.clone(),
            warning,
        });
    }
    Ok(joins)
}

fn assemble(
    request: &PlanRequest,
    tables: Vec<TableDecision>,
    joins: Vec<JoinDecision>,
    spool: SpoolReport,
    offset: crate::model::OffsetReport,
    disagreement: bool,
) -> Decision {
    let selected = tables.iter().all(|table| table.fault.is_none());
    let source_grade = if selected {
        tables
            .iter()
            .filter_map(|table| table.consistency_grade)
            .min_by_key(|grade| rank(*grade))
    } else {
        None
    };
    let transactional_allowed = selected && tables.iter().all(|table| table.transactional_allowed);
    let same_instant = same_instant(&tables);
    let consistency_claim_allowed =
        selected && tables.iter().all(|table| table.consistency_claim_allowed) && same_instant;
    let repair_allowed = selected && tables.iter().all(|table| table.repair_allowed);
    let consistency_result = consistency_result(request, &tables, transactional_allowed, same_instant);
    let mut quality: Vec<&'static str> = Vec::new();
    for table in &tables {
        for status in &table.statuses {
            push(&mut quality, *status);
        }
    }
    let disagreement = disagreement
        || tables
            .iter()
            .any(|table| table.reasons.iter().any(|reason| closes_receipt(reason)));
    let blocked = !selected
        || consistency_result == "denied"
        || disagreement
        || spool.state == "fault"
        || offset.checkpoint == "refused";
    let operator_action = if blocked {
        "refuse"
    } else if tables.iter().any(|table| table.operator_action == "resnapshot") {
        "resnapshot"
    } else if tables.iter().any(|table| table.operator_action == "pause_publish") {
        "pause_publish"
    } else if tables.iter().any(|table| table.operator_action == "alert_stale_risk") {
        "alert_stale_risk"
    } else {
        "continue"
    };
    Decision {
        contract: CONTRACT,
        kind: "decision",
        fault: None,
        downgraded: false,
        configuration_accepted: selected && consistency_result != "denied" && !disagreement && spool.state != "fault" && offset.checkpoint != "refused",
        requested_consistency_result: consistency_result,
        source_grade,
        transactional_allowed,
        same_instant_claim_allowed: same_instant,
        repair_requested: request.repair_requested,
        repair_allowed,
        consistency_claim_allowed,
        advance_capture_position: selected && tables.iter().all(|table| table.advance_capture_position) && !blocked,
        continue_from_latest: selected && tables.iter().all(|table| table.continue_from_latest) && !blocked,
        publish_new_generation: selected && tables.iter().all(|table| table.publish_new_generation) && !blocked,
        operator_action,
        loader_integration: "unaccepted",
        scheduler_integration: "unaccepted",
        live_offset_mirror: "not_run",
        tables,
        joins,
        spool,
        offset,
        quality_statuses: quality,
    }
}

fn consistency_result(
    request: &PlanRequest,
    tables: &[TableDecision],
    transactional_allowed: bool,
    same_instant: bool,
) -> &'static str {
    let Some(mode) = request.requested_consistency else {
        return "not_requested";
    };
    if tables.iter().any(|table| table.fault.is_some()) {
        return "denied";
    }
    let accepted = match mode {
        RequestedConsistency::Transactional => transactional_allowed,
        RequestedConsistency::Snapshot => {
            tables.iter().all(|table| {
                matches!(table.consistency_grade, Some(Grade::T | Grade::S1))
            }) && (tables.iter().all(|table| table.consistency_grade == Some(Grade::T))
                || same_instant)
        }
        RequestedConsistency::PerTable | RequestedConsistency::BestEffort => true,
    };
    if accepted {
        "accepted"
    } else {
        "denied"
    }
}

fn same_instant(tables: &[TableDecision]) -> bool {
    if tables.is_empty() || tables.iter().any(|table| table.fault.is_some()) {
        return false;
    }
    if tables.iter().any(|table| !table.consistency_claim_allowed) {
        return false;
    }
    if tables.iter().all(|table| table.consistency_grade == Some(Grade::T)) {
        return true;
    }
    if tables.iter().all(|table| table.consistency_grade == Some(Grade::S1)) {
        let token = &tables[0].token;
        return token.is_some() && tables.iter().all(|table| &table.token == token);
    }
    false
}

fn refused(table: &TableInput, fault: &'static str, reasons: Vec<&'static str>) -> TableDecision {
    TableDecision {
        name: table.name.clone(),
        generation: table.generation.clone(),
        next_generation: None,
        fault: Some(fault),
        capture_method: None,
        consistency_grade: None,
        delete_detection: None,
        token_order: None,
        statuses: Vec::new(),
        operator_action: "refuse",
        initial_load: None,
        advance_capture_position: false,
        continue_from_latest: false,
        resume_opaque: false,
        repair_allowed: false,
        consistency_claim_allowed: false,
        transactional_allowed: false,
        publish_new_generation: false,
        old_generation_readable: true,
        precision_loss: false,
        token: None,
        freshness: Freshness {
            consistency_grade: None,
            capture_method: None,
            lag_seconds: None,
            position: None,
        },
        reasons,
    }
}

fn closes_receipt(reason: &str) -> bool {
    matches!(
        reason,
        "receipt_unbound"
            | "required_incomplete"
            | "receipt_source_mismatch"
            | "receipt_epoch_mismatch"
            | "receipt_digest_mismatch"
            | "position_unbound"
            | "selected_receipt_absent"
            | "position_disagreement"
            | "source_gap"
            | "epoch_barrier"
            | "lineage_barrier"
            | "partial_segment"
            | "segment_incomplete"
            | "cross_table_inconsistent"
            | "expected_previous_mismatch"
            | "already_checkpoint"
            | "position_regression"
            | "receipt_not_advanced"
            | "receipt_rule"
            | "input"
    )
}

fn push<T: Copy + PartialEq>(items: &mut Vec<T>, item: T) {
    if !items.contains(&item) {
        items.push(item);
    }
}
