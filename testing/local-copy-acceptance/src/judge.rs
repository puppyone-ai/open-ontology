//! MP-01a–d decision. This module does not open a socket and does not spawn a process.

use std::collections::BTreeSet;

use crate::case::{
    Campaign, Case, Class, Conn, Fingerprint, FixtureClass, Producer, Provenance, Snapshot, Writes,
    CAMPAIGN_OWNER, FACT_GUARD, FACT_SINGLE, PRODUCER_OWNER,
};
use crate::json::escape;
use crate::sha::hex_ok;
use crate::typed::{reconcile_table, TableReport};

const FAULT_KINDS: [&str; 6] = [
    "partial",
    "cancel",
    "snapshot_disconnect",
    "restart",
    "idempotency",
    "mixed_version",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub id: &'static str,
    pub checks_pass: bool,
    pub reasons: Vec<&'static str>,
}

#[derive(Clone, Debug)]
pub struct Echo {
    pub sample: usize,
    pub class: &'static str,
    pub usename: String,
    pub datname: String,
    pub application_name: String,
    pub backend_type: String,
    pub xact_isolation: String,
    pub read_only: bool,
    pub state: String,
    pub pid: i64,
}

#[derive(Clone, Debug)]
pub struct Verdict {
    pub mp01_signed: bool,
    pub mp01abc_signed: bool,
    pub customer_complete: bool,
    pub enterprise: &'static str,
    pub deployment_profile: &'static str,
    pub cdc: &'static str,
    pub consistency_claim: &'static str,
    pub runner_source_sockets: u8,
    pub issued_stop: bool,
    pub business_peak: i64,
    pub business_peak_source: &'static str,
    pub monitor_peak: i64,
    pub guard_recorded: bool,
    pub guard_hidden: bool,
    pub producer_source_socket_fact: String,
    pub cards: Vec<Card>,
    pub sign_blockers: Vec<&'static str>,
    pub connections: Vec<Echo>,
    pub typed: Vec<TableReport>,
}

struct Peak {
    reasons: Vec<&'static str>,
    business_peak: i64,
    business_peak_source: &'static str,
    monitor_peak: i64,
    guard_recorded: bool,
    guard_hidden: bool,
}

pub fn judge(case: &Case) -> Verdict {
    let mut a = producer_reasons(&case.producer);
    a.extend(fingerprint_reasons(&case.source, &case.target));
    sort_reasons(&mut a);

    let peak = peak(case);
    let mut b = peak.reasons.clone();
    b.extend(budget_reasons(case));
    b.extend(write_reasons(&case.writes));
    b.extend(snapshot_reasons(&case.snapshot));
    let typed = case.tables.iter().map(reconcile_table).collect::<Vec<_>>();
    for report in &typed {
        b.extend(report.reasons.iter().copied());
    }
    if !counts_are_100k(case) {
        b.push("not_100k");
    }
    b.extend(type_reasons(case));
    sort_reasons(&mut b);

    let c = fault_reasons(case);
    let d = seal_reasons(case);
    let cards = vec![
        card("MP-01a", a),
        card("MP-01b", b),
        card("MP-01c", c),
        card("MP-01d", d),
    ];
    let mut sign_blockers = Vec::new();
    for item in &cards {
        sign_blockers.extend(item.reasons.iter().copied());
    }
    if !case.provenance.is_live() {
        sign_blockers.push("provenance_not_live");
    }
    sort_reasons(&mut sign_blockers);
    let mp01_signed = sign_blockers.is_empty();
    let mp01abc_signed = case.provenance.is_live()
        && cards.iter().take(3).all(|item| item.reasons.is_empty());
    Verdict {
        mp01_signed,
        mp01abc_signed,
        customer_complete: false,
        enterprise: "not_configured",
        deployment_profile: "local_private_fixture",
        cdc: "not_this_card",
        consistency_claim: if mp01abc_signed {
            "single_snapshot"
        } else {
            "not_a_snapshot"
        },
        runner_source_sockets: 0,
        issued_stop: false,
        business_peak: peak.business_peak,
        business_peak_source: peak.business_peak_source,
        monitor_peak: peak.monitor_peak,
        guard_recorded: peak.guard_recorded,
        guard_hidden: peak.guard_hidden,
        producer_source_socket_fact: case.producer.source_socket_fact.clone(),
        cards,
        sign_blockers,
        connections: echo(case),
        typed,
    }
}

fn card(id: &'static str, reasons: Vec<&'static str>) -> Card {
    Card {
        id,
        checks_pass: reasons.is_empty(),
        reasons,
    }
}

fn sort_reasons(reasons: &mut Vec<&'static str>) {
    reasons.sort_unstable();
    reasons.dedup();
}

fn producer_reasons(producer: &Producer) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if producer.owner != PRODUCER_OWNER {
        reasons.push("producer_owner");
    }
    if !producer.binary_present {
        reasons.push("producer_absent");
    } else if !hex_ok(&producer.binary_sha256) {
        reasons.push("producer_sha");
    }
    if producer.invoked {
        if producer.exit_code != Some(0) {
            reasons.push("producer_exit");
        }
    } else {
        reasons.push("producer_not_invoked");
    }
    if !producer.fixed_single_connection
        || producer.source_socket_fact != FACT_SINGLE
        || producer.source_socket_peak != Some(1)
    {
        reasons.push("producer_unfixed");
    }
    reasons
}

fn fingerprint_reasons(source: &Fingerprint, target: &Fingerprint) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    let source_ok = ident_ok(&source.system_identifier);
    let target_ok = ident_ok(&target.system_identifier);
    if !source_ok
        || !target_ok
        || source.postmaster_start.is_empty()
        || target.postmaster_start.is_empty()
        || source.database.is_empty()
        || target.database.is_empty()
    {
        reasons.push("fingerprint_incomplete");
    }
    if source_ok && target_ok && source.system_identifier == target.system_identifier {
        reasons.push("fingerprint_same_instance");
    }
    reasons
}

fn ident_ok(text: &str) -> bool {
    (1..=128).contains(&text.len())
        && text
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '-'))
}

fn peak(case: &Case) -> Peak {
    let mut reasons = Vec::new();
    let mut guard_seen = false;
    let mut business_seen = false;
    let mut max_business = 0_i64;
    let mut max_monitor = 0_i64;
    for sample in &case.samples {
        let mut pids = BTreeSet::new();
        let mut business = 0_i64;
        let mut monitor = 0_i64;
        for conn in sample {
            if !pids.insert(conn.pid) {
                reasons.push("connection_duplicate");
            }
            if attribute_missing(conn) {
                reasons.push("connection_attribute");
            }
            match conn.class {
                Class::Business => {
                    business += 1;
                    business_seen = true;
                    if conn.datname != case.source.database {
                        reasons.push("business_database");
                    }
                    if !conn.read_only {
                        reasons.push("business_not_readonly");
                    }
                    if conn.xact_isolation != "repeatable read" {
                        reasons.push("business_isolation");
                    }
                }
                Class::Guard => {
                    business += 1;
                    guard_seen = true;
                    if conn.datname != case.source.database {
                        reasons.push("guard_database");
                    }
                }
                Class::Monitor => monitor += 1,
            }
        }
        max_business = max_business.max(business);
        max_monitor = max_monitor.max(monitor);
    }
    let guard_recorded = guard_seen || case.producer.source_socket_fact == FACT_GUARD;
    let guard_hidden = case.provenance != Provenance::Archive
        && case.producer.source_socket_fact == FACT_GUARD
        && !guard_seen;
    if guard_hidden {
        reasons.push("guard_hidden");
    }
    if guard_seen || case.producer.source_socket_fact == FACT_GUARD {
        reasons.push("guard_not_a_single_connection");
    }
    let (business_peak, business_peak_source) = if case.samples.is_empty() {
        match case.provenance {
            Provenance::Archive => {
                let recorded = case.producer.source_socket_peak.unwrap_or(0);
                if case.producer.source_socket_peak != Some(1) {
                    reasons.push("business_peak");
                }
                let source = if case.producer.source_socket_peak.is_some() {
                    "producer_field"
                } else {
                    "unmeasured"
                };
                (recorded, source)
            }
            Provenance::Live | Provenance::Fixture => {
                reasons.push("data_backend_missing");
                reasons.push("business_peak");
                if case
                    .producer
                    .source_socket_peak
                    .is_some_and(|value| value != 0)
                {
                    reasons.push("peak_mismatch");
                }
                (0, "samples")
            }
        }
    } else {
        if !business_seen {
            reasons.push("data_backend_missing");
        }
        if max_business != 1 {
            reasons.push("business_peak");
        }
        match case.producer.source_socket_peak {
            Some(value) if value == max_business => {}
            _ => reasons.push("peak_mismatch"),
        }
        (max_business, "samples")
    };
    sort_reasons(&mut reasons);
    Peak {
        reasons,
        business_peak,
        business_peak_source,
        monitor_peak: max_monitor,
        guard_recorded,
        guard_hidden,
    }
}

fn attribute_missing(conn: &Conn) -> bool {
    conn.pid <= 0
        || conn.usename.is_empty()
        || conn.datname.is_empty()
        || conn.application_name.is_empty()
        || conn.backend_type.is_empty()
        || conn.xact_isolation.is_empty()
        || conn.state.is_empty()
}

fn budget_reasons(case: &Case) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if !(1..=1000).contains(&case.budgets.batch_rows) {
        reasons.push("budget_batch");
    }
    if !(1..=30_000).contains(&case.budgets.statement_timeout_ms) {
        reasons.push("budget_statement");
    }
    if !(1..=1000).contains(&case.budgets.lock_timeout_ms) {
        reasons.push("budget_lock");
    }
    if !(1..=300_000).contains(&case.budgets.snapshot_deadline_ms) {
        reasons.push("budget_snapshot");
    }
    reasons
}

fn write_reasons(writes: &Writes) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if writes.ddl || writes.dml || writes.heartbeat || writes.privilege {
        reasons.push("source_write");
    }
    if writes.explain {
        reasons.push("source_explain");
    }
    if writes.count_star {
        reasons.push("source_count_star");
    }
    reasons
}

fn snapshot_reasons(snapshot: &Snapshot) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if snapshot.isolation != "repeatable read" {
        reasons.push("snapshot_isolation");
    }
    if !snapshot.read_only {
        reasons.push("snapshot_not_readonly");
    }
    if snapshot.catalog_snapshot_id.is_empty()
        || snapshot.data_snapshot_id.is_empty()
        || snapshot.catalog_snapshot_id != snapshot.data_snapshot_id
    {
        reasons.push("snapshot_mismatch");
    }
    if snapshot.proof != "sealed_spool" {
        reasons.push("snapshot_proof");
    }
    if snapshot.later_count {
        reasons.push("later_count");
    }
    if snapshot.opened_second_source_connection {
        reasons.push("second_source_connection");
    }
    reasons
}

fn counts_are_100k(case: &Case) -> bool {
    let mut counts: Vec<u64> = case.tables.iter().map(|table| table.count).collect();
    counts.sort_unstable();
    counts == [2000, 8000, 90000]
}

fn type_reasons(case: &Case) -> Vec<&'static str> {
    let mut seen = BTreeSet::new();
    for table in &case.tables {
        for row in table.source_rows.iter().chain(table.target_rows.iter()) {
            for cell in row.cells.values() {
                seen.insert(cell.type_name());
            }
        }
    }
    let mut reasons = Vec::new();
    for (name, reason) in [
        ("int8", "missing_type_int8"),
        ("decimal", "missing_type_decimal"),
        ("null", "missing_type_null"),
        ("time", "missing_type_time"),
        ("binary", "missing_type_binary"),
    ] {
        if !seen.contains(name) {
            reasons.push(reason);
        }
    }
    reasons
}

fn fault_reasons(case: &Case) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if FAULT_KINDS
        .iter()
        .any(|kind| case.faults.iter().all(|fault| fault.kind != *kind))
    {
        reasons.push("fault_missing");
    }
    for fault in &case.faults {
        if fault.public {
            reasons.push("fault_public");
        }
        if fault.mirror_state == "complete" && !idempotent_replay(fault) {
            reasons.push("fault_complete");
        }
        if fault.consistency != "not_a_snapshot" {
            reasons.push("fault_consistency");
        }
        if fault.mixed_snapshot {
            reasons.push("fault_snapshot_mixed");
        }
        if fault.target_rows_rewritten {
            reasons.push("fault_rewritten");
        }
        if fault.source_backends_after != 0 {
            reasons.push("fault_backend");
        }
    }
    sort_reasons(&mut reasons);
    reasons
}

fn idempotent_replay(fault: &crate::case::Fault) -> bool {
    fault.kind == "idempotency"
        && !fault.public
        && !fault.target_rows_rewritten
        && !fault.mixed_snapshot
        && fault.source_backends_after == 0
        && fault.consistency == "not_a_snapshot"
}

fn seal_reasons(case: &Case) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    let seal = &case.after_seal;
    if seal.source_secret_in_subsequent_process {
        reasons.push("source_secret_retained");
    }
    if seal.network_to_source {
        reasons.push("source_network_open");
    }
    match case.fixture_class {
        FixtureClass::Public => {
            if !seal.public_fixture_source_blocked {
                reasons.push("fixture_not_blocked");
            }
        }
        FixtureClass::Enterprise => {
            reasons.push("enterprise_not_a_public_fixture");
            if seal.public_fixture_source_blocked {
                reasons.push("enterprise_block_flag");
            }
        }
    }
    if seal.enterprise_stop_requested {
        reasons.push("enterprise_stop_refused");
    }
    let has_405 = seal.audits.iter().any(|audit| audit.component == "405");
    let has_397 = seal.audits.iter().any(|audit| audit.component == "397");
    if !has_405 || !has_397 {
        reasons.push("audit_missing");
    }
    for audit in &seal.audits {
        if seal.generation.is_empty() || audit.generation != seal.generation {
            reasons.push("audit_generation");
        }
        if case.target.system_identifier.is_empty()
            || audit.target_instance != case.target.system_identifier
        {
            reasons.push("audit_target_instance");
        }
        if audit.source_access_count != 0 {
            reasons.push("audit_source_access");
        }
        if audit.query_id.is_empty() {
            reasons.push("audit_query");
        }
        if case.target.database.is_empty() || audit.database != case.target.database {
            reasons.push("audit_database");
        }
    }
    campaign_reasons(&mut reasons, &case.campaign);
    sort_reasons(&mut reasons);
    reasons
}

fn campaign_reasons(reasons: &mut Vec<&'static str>, campaign: &Campaign) {
    if campaign.owner != CAMPAIGN_OWNER {
        reasons.push("campaign_owner");
    }
    if campaign.repeated_here {
        reasons.push("repeated_campaign");
    }
}

fn echo(case: &Case) -> Vec<Echo> {
    let mut out = Vec::new();
    for (index, sample) in case.samples.iter().enumerate() {
        for conn in sample {
            out.push(Echo {
                sample: index,
                class: conn.class.as_str(),
                usename: conn.usename.clone(),
                datname: conn.datname.clone(),
                application_name: conn.application_name.clone(),
                backend_type: conn.backend_type.clone(),
                xact_isolation: conn.xact_isolation.clone(),
                read_only: conn.read_only,
                state: conn.state.clone(),
                pid: conn.pid,
            });
        }
    }
    out
}

pub fn verdict_json(verdict: &Verdict) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    field(
        &mut out,
        1,
        "protocol",
        &quote("oo.acceptance.local-copy-verdict/v1"),
        true,
    );
    field(
        &mut out,
        1,
        "mp01_signed",
        bool_text(verdict.mp01_signed),
        true,
    );
    field(
        &mut out,
        1,
        "mp01abc_signed",
        bool_text(verdict.mp01abc_signed),
        true,
    );
    field(&mut out, 1, "customer_complete", "false", true);
    field(&mut out, 1, "enterprise", &quote(verdict.enterprise), true);
    field(
        &mut out,
        1,
        "deployment_profile",
        &quote(verdict.deployment_profile),
        true,
    );
    field(&mut out, 1, "cdc", &quote(verdict.cdc), true);
    field(
        &mut out,
        1,
        "consistency_claim",
        &quote(verdict.consistency_claim),
        true,
    );
    field(&mut out, 1, "runner_source_sockets", "0", true);
    field(&mut out, 1, "issued_stop", "false", true);
    field(
        &mut out,
        1,
        "business_peak",
        &verdict.business_peak.to_string(),
        true,
    );
    field(
        &mut out,
        1,
        "business_peak_source",
        &quote(verdict.business_peak_source),
        true,
    );
    field(
        &mut out,
        1,
        "monitor_peak",
        &verdict.monitor_peak.to_string(),
        true,
    );
    field(
        &mut out,
        1,
        "guard_recorded",
        bool_text(verdict.guard_recorded),
        true,
    );
    field(
        &mut out,
        1,
        "guard_hidden",
        bool_text(verdict.guard_hidden),
        true,
    );
    field(
        &mut out,
        1,
        "producer_source_socket_fact",
        &quote(&verdict.producer_source_socket_fact),
        true,
    );
    out.push_str("  \"cards\": {\n");
    for (index, card) in verdict.cards.iter().enumerate() {
        out.push_str("    ");
        out.push_str(&quote(card.id));
        out.push_str(": {\"checks_pass\": ");
        out.push_str(bool_text(card.checks_pass));
        out.push_str(", \"reasons\": ");
        write_str_array(&mut out, &card.reasons);
        out.push('}');
        if index + 1 != verdict.cards.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  },\n");
    out.push_str("  \"sign_blockers\": ");
    write_str_array(&mut out, &verdict.sign_blockers);
    out.push_str(",\n");
    out.push_str("  \"connections\": [\n");
    for (index, conn) in verdict.connections.iter().enumerate() {
        out.push_str("    {");
        out.push_str(&format!("\"sample\": {}", conn.sample));
        pair(&mut out, "class", &quote(conn.class));
        pair(&mut out, "usename", &quote(&conn.usename));
        pair(&mut out, "datname", &quote(&conn.datname));
        pair(&mut out, "application_name", &quote(&conn.application_name));
        pair(&mut out, "backend_type", &quote(&conn.backend_type));
        pair(&mut out, "xact_isolation", &quote(&conn.xact_isolation));
        pair(&mut out, "read_only", bool_text(conn.read_only));
        pair(&mut out, "state", &quote(&conn.state));
        pair(&mut out, "pid", &conn.pid.to_string());
        out.push('}');
        if index + 1 != verdict.connections.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ],\n");
    out.push_str("  \"typed\": [\n");
    for (index, report) in verdict.typed.iter().enumerate() {
        out.push_str("    {");
        out.push_str(&format!("\"schema\": {}", quote(&report.schema)));
        pair(&mut out, "name", &quote(&report.name));
        pair(&mut out, "source_count", &report.source_count.to_string());
        pair(&mut out, "target_count", &report.target_count.to_string());
        pair(&mut out, "source_hash", &quote(&report.source_hash));
        pair(&mut out, "target_hash", &quote(&report.target_hash));
        pair(&mut out, "hash_equal", bool_text(report.hash_equal));
        pair(&mut out, "key_coverage", bool_text(report.key_coverage));
        out.push_str(", \"reasons\": ");
        write_str_array(&mut out, &report.reasons);
        out.push('}');
        if index + 1 != verdict.typed.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n}\n");
    out
}

fn field(out: &mut String, _indent: usize, key: &str, value: &str, comma: bool) {
    out.push_str("  ");
    out.push_str(&quote(key));
    out.push_str(": ");
    out.push_str(value);
    if comma {
        out.push(',');
    }
    out.push('\n');
}

fn pair(out: &mut String, key: &str, value: &str) {
    out.push_str(", ");
    out.push_str(&quote(key));
    out.push_str(": ");
    out.push_str(value);
}

fn write_str_array(out: &mut String, items: &[&str]) {
    out.push('[');
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&quote(item));
    }
    out.push(']');
}

fn quote(text: &str) -> String {
    format!("\"{}\"", escape(text))
}

fn bool_text(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}
