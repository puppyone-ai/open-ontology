//! Admission gate. A passing decision is not a source zero-write proof.

use crate::checklist::from_target;
use crate::model::{
    BUDGET_UNITS, Block, Budget, CAPABILITY_NAMES, CAPTURE_METHODS, CDC_KINDS, CONFIG_PROTOCOL,
    CONSISTENCY, CURSOR_KINDS, CaptureMethod, DECISION_PROTOCOL, DebeziumOptions, Decision,
    DeclaredCapability, FEED_KINDS, Identity, LEVELS, Model, NativeFacts, Permission, R1wApproval,
    REPORT_PROTOCOL, SourceConfig, TOKEN_ORDERS, TargetReport, WRITE_CLASSES, WriteOptions,
    ZeroWrite, known,
};
use crate::native_map::{
    CLICKHOUSE_NATIVE, MONGODB_NATIVE, NativeGrant, OPENSEARCH_NATIVE, ORACLE, ScopeFault,
    clickhouse_companion_ok, clickhouse_profile_fault, clickhouse_select_fault, compat_recognized,
    interpret_native_grant, opensearch_read_fault, oracle_flashback_write, oracle_object_scope,
    oracle_scope_used, oracle_session_granted,
};

pub fn admit(report: &TargetReport, config: &SourceConfig) -> Decision {
    let mut blocks = Vec::new();
    if report.protocol != REPORT_PROTOCOL {
        blocks.push(block("PROTOCOL_UNSUPPORTED", "report.protocol"));
    }
    if config.protocol != CONFIG_PROTOCOL {
        blocks.push(block("PROTOCOL_UNSUPPORTED", "config.protocol"));
    }
    if report.source_id != config.source_id
        || report.secret_ref != config.secret_ref
        || report.target_id != config.target_id
        || report.compat_mode != config.compat_mode
        || report.native != config.native
    {
        blocks.push(block("IDENTITY_MISMATCH", "report_config"));
    }
    check_compat(&report.compat_mode, "report.compat_mode", &mut blocks);
    check_compat(&config.compat_mode, "config.compat_mode", &mut blocks);
    check_permissions(
        &report.permissions,
        &report.compat_mode,
        "report.permissions",
        &mut blocks,
    );
    check_permissions(
        &config.permissions,
        &config.compat_mode,
        "config.permissions",
        &mut blocks,
    );
    check_native_profile(
        &report.compat_mode,
        report.native.as_ref(),
        "report.native",
        &mut blocks,
    );
    check_native_profile(
        &config.compat_mode,
        config.native.as_ref(),
        "config.native",
        &mut blocks,
    );
    check_budget_unit(
        &report.cost_model.unit,
        "report.cost_model.unit",
        &mut blocks,
    );
    check_budget_unit(&config.budget.unit, "config.budget.unit", &mut blocks);
    if report.cost_model.unit != config.budget.unit {
        blocks.push(block("BUDGET_UNIT_UNKNOWN", "budget_mismatch"));
    }
    check_confidence(&report.cost_model.confidence, &config.budget, &mut blocks);
    for (index, cap) in report.capabilities.iter().enumerate() {
        check_capability(cap, index, &mut blocks);
    }
    for (index, method) in report.capture_methods.iter().enumerate() {
        check_capture(method, index, &mut blocks);
    }
    check_write_class(
        &config.requested_write_class,
        "config.requested_write_class",
        &mut blocks,
    );
    check_r1w(
        report_needs_r1w(report) || config.requested_write_class == "R1w",
        report.r1w.as_ref(),
        config.r1w.as_ref(),
        &mut blocks,
    );
    check_options(&config.write_options, &config.debezium, &mut blocks);
    if report.driver.digest.is_empty()
        || report.driver.name.is_empty()
        || report.driver.kind.is_empty()
    {
        blocks.push(block("DRIVER_MISSING", "report.driver"));
    }
    if !simple_id(&report.source_id) || !simple_id(&config.source_id) {
        blocks.push(block("SOURCE_ID_INVALID", "source_id"));
    }
    if !report.secret_ref.starts_with("secret://") || report.secret_ref.len() < 10 {
        blocks.push(block("SECRET_REF_INVALID", "secret_ref"));
    }
    let admitted = blocks.is_empty();
    Decision {
        protocol: DECISION_PROTOCOL.to_string(),
        admitted,
        matrix_mutated: false,
        blocks,
        model: Model {
            capabilities: report.capabilities.clone(),
            capture_methods: report.capture_methods.clone(),
        },
        identity: Identity {
            target_id: report.target_id.clone(),
            source_id: report.source_id.clone(),
            secret_ref: report.secret_ref.clone(),
            source_report_protocol: None,
            core_contract: None,
            source_report_sha256: None,
        },
        dba_checklist: from_target(report, config),
        zero_write: ZeroWrite {
            proven: false,
            basis: "not_audited".to_string(),
        },
    }
}

fn report_needs_r1w(report: &TargetReport) -> bool {
    report
        .capabilities
        .iter()
        .any(|cap| cap.write_class == "R1w")
        || report
            .capture_methods
            .iter()
            .any(|method| method.write_class == "R1w")
}

pub(crate) fn check_r1w(
    needed: bool,
    report: Option<&R1wApproval>,
    config: Option<&R1wApproval>,
    blocks: &mut Vec<Block>,
) {
    if !needed {
        if matches!(report, Some(item) if item.approved)
            || matches!(config, Some(item) if item.approved)
        {
            blocks.push(block("R1W_UNAPPROVED", "approval_without_r1w"));
        }
        return;
    }
    match (report, config) {
        (Some(left), Some(right))
            if left.approved
                && right.approved
                && left == right
                && valid_adr(&left.adr)
                && !left.audit_ref.is_empty()
                && !left.audit_ref.contains("PASSWORD") => {}
        _ => blocks.push(block("R1W_UNAPPROVED", "r1w")),
    }
}

fn valid_adr(adr: &str) -> bool {
    (adr.starts_with("DEC-") || adr.starts_with("ADR-")) && adr.len() > 4 && adr.len() < 80
}

pub(crate) fn check_options(
    options: &WriteOptions,
    debezium: &DebeziumOptions,
    blocks: &mut Vec<Block>,
) {
    if options.signal_table || debezium.signal_data_collection.is_some() {
        blocks.push(block("SIGNAL_WRITE", "signal_table"));
    }
    if options.heartbeat_write || debezium.heartbeat_action_query.is_some() {
        blocks.push(block("HEARTBEAT_WRITE", "heartbeat_write"));
    }
    if options.create_stream || debezium.create_stream {
        blocks.push(block("STREAM_WRITE", "create_stream"));
    }
    if options.create_trigger || debezium.create_trigger {
        blocks.push(block("TRIGGER_WRITE", "create_trigger"));
    }
    if debezium.snapshot_mode != "never" {
        blocks.push(block("SIGNAL_WRITE", "debezium.snapshot_mode"));
    }
}

fn check_capability(cap: &DeclaredCapability, index: usize, blocks: &mut Vec<Block>) {
    let fact = format!("capabilities[{index}]");
    if !known(CAPABILITY_NAMES, &cap.name) {
        blocks.push(block("CAPABILITY_UNKNOWN", &fact));
    }
    if !known(LEVELS, &cap.level) {
        blocks.push(block("LEVEL_UNKNOWN", &fact));
    }
    check_write_class(&cap.write_class, &fact, blocks);
    if !known(CONSISTENCY, &cap.consistency) {
        blocks.push(block("CONSISTENCY_UNKNOWN", &fact));
    }
    optional_known(
        CURSOR_KINDS,
        cap.cursor.as_deref(),
        "CURSOR_UNKNOWN",
        &fact,
        blocks,
    );
    optional_known(
        FEED_KINDS,
        cap.feed.as_deref(),
        "FEED_UNKNOWN",
        &fact,
        blocks,
    );
    optional_known(CDC_KINDS, cap.cdc.as_deref(), "CDC_UNKNOWN", &fact, blocks);
    optional_known(
        TOKEN_ORDERS,
        cap.token_order.as_deref(),
        "TOKEN_ORDER_UNKNOWN",
        &fact,
        blocks,
    );
    if let Some(unit) = cap.budget_unit.as_deref() {
        check_budget_unit(unit, &fact, blocks);
    }
    match cap.name.as_str() {
        "BatchIncremental" if cap.cursor.is_none() => {
            blocks.push(block("CURSOR_UNKNOWN", &fact));
        }
        "ChangeFeed" if cap.feed.is_none() => {
            blocks.push(block("FEED_UNKNOWN", &fact));
        }
        "Cdc" if cap.cdc.is_none() => {
            blocks.push(block("CDC_UNKNOWN", &fact));
        }
        "SnapshotToken" if cap.token_order.is_none() => {
            blocks.push(block("TOKEN_ORDER_UNKNOWN", &fact));
        }
        "Budget" if cap.budget_unit.is_none() => {
            blocks.push(block("BUDGET_UNIT_UNKNOWN", &fact));
        }
        _ => {}
    }
}

fn check_capture(method: &CaptureMethod, index: usize, blocks: &mut Vec<Block>) {
    let fact = format!("capture_methods[{index}]");
    if !known(CAPTURE_METHODS, &method.method) {
        blocks.push(block("CAPTURE_UNKNOWN", &fact));
    }
    check_write_class(&method.write_class, &fact, blocks);
    if !known(CONSISTENCY, &method.consistency) {
        blocks.push(block("CONSISTENCY_UNKNOWN", &fact));
    }
    if !known(TOKEN_ORDERS, &method.token_order) {
        blocks.push(block("TOKEN_ORDER_UNKNOWN", &fact));
    }
    optional_known(
        CURSOR_KINDS,
        method.cursor.as_deref(),
        "CURSOR_UNKNOWN",
        &fact,
        blocks,
    );
    optional_known(
        FEED_KINDS,
        method.feed.as_deref(),
        "FEED_UNKNOWN",
        &fact,
        blocks,
    );
    optional_known(
        CDC_KINDS,
        method.cdc.as_deref(),
        "CDC_UNKNOWN",
        &fact,
        blocks,
    );
}

fn optional_known(
    set: &[&str],
    value: Option<&str>,
    code: &str,
    fact: &str,
    blocks: &mut Vec<Block>,
) {
    if let Some(value) = value
        && !known(set, value)
    {
        blocks.push(block(code, fact));
    }
}

pub(crate) fn check_write_class(value: &str, fact: &str, blocks: &mut Vec<Block>) {
    if !known(WRITE_CLASSES, value) {
        blocks.push(block("WRITE_CLASS_UNKNOWN", fact));
        return;
    }
    if value == "R2" {
        blocks.push(block("WRITE_CLASS_R2", fact));
    }
    if value == "R3" {
        blocks.push(block("WRITE_CLASS_R3", fact));
    }
}

fn check_compat(value: &str, fact: &str, blocks: &mut Vec<Block>) {
    if !compat_recognized(value) {
        blocks.push(block("COMPAT_MODE_UNKNOWN", fact));
    }
}

fn check_budget_unit(value: &str, fact: &str, blocks: &mut Vec<Block>) {
    if !known(BUDGET_UNITS, value) {
        blocks.push(block("BUDGET_UNIT_UNKNOWN", fact));
    }
}

fn check_confidence(confidence: &str, budget: &Budget, blocks: &mut Vec<Block>) {
    match confidence {
        "known" => {
            if budget.confidence != "known" {
                blocks.push(block("BUDGET_UNIT_UNKNOWN", "config.budget.confidence"));
            }
        }
        "unknown" => {
            if budget.hard_limit == 0 || budget.confidence != "unknown" {
                blocks.push(block("BUDGET_UNIT_UNKNOWN", "confidence_unknown"));
            }
        }
        _ => blocks.push(block("BUDGET_UNIT_UNKNOWN", "cost_model.confidence")),
    }
}

fn check_permissions(items: &[Permission], mode: &str, fact: &str, blocks: &mut Vec<Block>) {
    if items.is_empty() {
        blocks.push(block("PERMISSION_UNKNOWN", fact));
        return;
    }
    let mut read_granted = false;
    for (index, item) in items.iter().enumerate() {
        let item_fact = format!("{fact}[{index}]");
        if classify_permission(item, items, mode, &item_fact, blocks) {
            read_granted = true;
        }
    }
    if mode == MONGODB_NATIVE && !mongo_read_granted(items) {
        blocks.push(block("PERMISSION_UNKNOWN", fact));
    }
    if (mode == CLICKHOUSE_NATIVE || mode == OPENSEARCH_NATIVE) && !read_granted {
        blocks.push(block("PERMISSION_UNKNOWN", fact));
    }
    if mode == ORACLE {
        if !oracle_session_granted(items) {
            blocks.push(block("PERMISSION_UNKNOWN", &format!("{fact}.connection")));
        }
        if !read_granted {
            blocks.push(block("PERMISSION_UNKNOWN", fact));
        }
    }
}

fn check_native_profile(
    mode: &str,
    native: Option<&NativeFacts>,
    fact: &str,
    blocks: &mut Vec<Block>,
) {
    let profile = native.and_then(|item| item.clickhouse.as_ref());
    if mode == CLICKHOUSE_NATIVE {
        match profile {
            None => blocks.push(block("PERMISSION_UNKNOWN", &format!("{fact}.clickhouse"))),
            Some(profile) => {
                if let Err(which) = clickhouse_profile_fault(profile) {
                    blocks.push(block(
                        "PERMISSION_UNKNOWN",
                        &format!("{fact}.clickhouse.{which}"),
                    ));
                }
            }
        }
        return;
    }
    if profile.is_some() {
        blocks.push(block("PERMISSION_UNKNOWN", fact));
    }
}

/// Returns whether this granted item is an exact scoped read for the engine.
fn classify_permission(
    item: &Permission,
    items: &[Permission],
    mode: &str,
    fact: &str,
    blocks: &mut Vec<Block>,
) -> bool {
    if item.status != "known" {
        blocks.push(block("PERMISSION_UNKNOWN", fact));
        return false;
    }
    if mode == ORACLE {
        return classify_oracle(item, items, fact, blocks);
    }
    match interpret_native_grant(mode, &item.name) {
        NativeGrant::ReadOnly if item.granted && needs_native_scope(mode) => {
            match native_scope_fault(mode, item) {
                ScopeFault::Ok => true,
                ScopeFault::Scope => {
                    blocks.push(block("PERMISSION_UNKNOWN", &format!("{fact}.scope")));
                    false
                }
                ScopeFault::Observed => {
                    blocks.push(block("PERMISSION_UNKNOWN", &format!("{fact}.observed")));
                    false
                }
            }
        }
        NativeGrant::ReadOnly => false,
        NativeGrant::NotReadOnly if item.granted => {
            blocks.push(block("PERMISSION_NOT_READONLY", fact));
            false
        }
        NativeGrant::NotReadOnly => false,
        NativeGrant::Companion => {
            if !clickhouse_companion_ok(item.scope.as_ref()) {
                blocks.push(block("PERMISSION_UNKNOWN", &format!("{fact}.scope")));
            }
            false
        }
        NativeGrant::Unknown => {
            blocks.push(block("PERMISSION_UNKNOWN", fact));
            false
        }
    }
}

fn classify_oracle(
    item: &Permission,
    items: &[Permission],
    fact: &str,
    blocks: &mut Vec<Block>,
) -> bool {
    if !item.granted {
        if interpret_native_grant(ORACLE, &item.name) == NativeGrant::Unknown {
            blocks.push(block("PERMISSION_UNKNOWN", fact));
        }
        return false;
    }
    match item.name.as_str() {
        "CREATE SESSION" => {
            if oracle_scope_used(item.scope.as_ref()) {
                blocks.push(block("PERMISSION_UNKNOWN", &format!("{fact}.scope")));
            }
            false
        }
        "SELECT" => match oracle_object_scope(item.scope.as_ref()) {
            Ok(_) => true,
            Err(()) => {
                blocks.push(block("PERMISSION_UNKNOWN", &format!("{fact}.scope")));
                false
            }
        },
        "FLASHBACK" => match oracle_object_scope(item.scope.as_ref()) {
            Ok((schema, table)) => {
                if oracle_flashback_write(items, schema, table) {
                    blocks.push(block("PERMISSION_NOT_READONLY", fact));
                }
                false
            }
            Err(()) => {
                blocks.push(block("PERMISSION_UNKNOWN", &format!("{fact}.scope")));
                false
            }
        },
        _ => match interpret_native_grant(ORACLE, &item.name) {
            NativeGrant::NotReadOnly => {
                blocks.push(block("PERMISSION_NOT_READONLY", fact));
                false
            }
            NativeGrant::ReadOnly => false,
            NativeGrant::Unknown | NativeGrant::Companion => {
                blocks.push(block("PERMISSION_UNKNOWN", fact));
                false
            }
        },
    }
}

fn needs_native_scope(mode: &str) -> bool {
    mode == CLICKHOUSE_NATIVE || mode == OPENSEARCH_NATIVE
}

fn native_scope_fault(mode: &str, item: &Permission) -> ScopeFault {
    if mode == CLICKHOUSE_NATIVE {
        clickhouse_select_fault(item.scope.as_ref())
    } else {
        opensearch_read_fault(item.scope.as_ref())
    }
}

fn mongo_read_granted(items: &[Permission]) -> bool {
    items
        .iter()
        .any(|item| item.status == "known" && item.granted && item.name == "read")
}

pub(crate) fn simple_id(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && value.len() <= 63
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn block(code: &str, fact: &str) -> Block {
    Block {
        code: code.to_string(),
        fact: fact.to_string(),
    }
}
