//! Readiness decision. A logical snapshot is not an all-state certificate.

use crate::json::escape;

pub const REMAINING: &[&str] = &[
    "git_mirror",
    "object_store",
    "secret_store",
    "audit_anchors",
    "valkey_buffer",
    "jobs_control",
    "gateway_tokens",
    "file_hosting",
    "ontology_publication",
    "agent_pipeline_schema",
    "entity_materialized_tables",
    "cluster_roles",
    "wal_archive",
    "pgbackrest",
    "enum_calendar_same_database",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetDecision {
    pub min_used: bool,
    pub published: Option<String>,
    pub stored: String,
}

/// Never publishes the minimum of the control position and another position.
pub fn publish_offset(history_insufficient: bool, stored: &str, _other: Option<&str>) -> OffsetDecision {
    OffsetDecision {
        min_used: false,
        published: if history_insufficient { None } else { Some(stored.to_string()) },
        stored: stored.to_string(),
    }
}

#[derive(Debug, Clone)]
pub struct Report {
    pub replay_ready: bool,
    pub reason: String,
    pub reasons: Vec<String>,
    pub history_insufficient: bool,
    pub wal_moved: bool,
    pub checkpoint_copy_ok: bool,
    pub shop_gold_ok: bool,
    pub counts_ok: bool,
    pub cut_proven: bool,
    pub decimal_ok: bool,
    pub null_ok: bool,
    pub comment_ok: bool,
    pub key_reused: bool,
    pub key_count: u64,
    pub post_cut_absent_on_restore: bool,
    pub post_cut_present_on_source: bool,
    pub clock_anomaly: bool,
    pub offset_published: Option<String>,
    pub rpo_window_seconds: Option<u64>,
    pub rto_seconds: Option<u64>,
}

impl Report {
    pub fn refused(reason: &str) -> Self {
        Self {
            replay_ready: false,
            reason: reason.to_string(),
            reasons: vec![reason.to_string()],
            history_insufficient: true,
            wal_moved: false,
            checkpoint_copy_ok: false,
            shop_gold_ok: false,
            counts_ok: false,
            cut_proven: false,
            decimal_ok: false,
            null_ok: false,
            comment_ok: false,
            key_reused: false,
            key_count: 0,
            post_cut_absent_on_restore: false,
            post_cut_present_on_source: false,
            clock_anomaly: false,
            offset_published: None,
            rpo_window_seconds: None,
            rto_seconds: None,
        }
    }

    pub fn seal(&mut self) {
        self.push_unless(self.checkpoint_copy_ok, "checkpoint_mismatch");
        self.push_unless(self.shop_gold_ok, "shop_gold_mismatch");
        self.push_unless(self.counts_ok, "count_mismatch");
        self.push_unless(self.decimal_ok, "decimal_mismatch");
        self.push_unless(self.null_ok, "null_mismatch");
        self.push_unless(self.comment_ok, "comment_mismatch");
        self.push_unless(!self.key_reused, "key_reused");
        self.push_unless(self.key_count == 2, "key_count");
        self.push_unless(self.post_cut_absent_on_restore, "post_cut_present");
        self.push_unless(self.post_cut_present_on_source, "post_cut_missing_on_source");
        self.push_unless(!self.clock_anomaly, "clock_anomaly");
        if !self.cut_proven {
            self.history_insufficient = true;
            self.offset_published = None;
            self.push_unless(false, "cut_unproven");
        }
        if self.history_insufficient {
            self.offset_published = None;
            self.push_unless(false, "history_insufficient");
        }
        self.reasons.sort();
        self.reasons.dedup();
        self.replay_ready = self.reasons.is_empty();
        self.reason = if self.replay_ready {
            "ok".to_string()
        } else {
            self.reasons[0].clone()
        };
    }

    fn push_unless(&mut self, ok: bool, reason: &str) {
        if !ok {
            self.reasons.push(reason.to_string());
        }
    }
}

pub fn key_reused(lines: &str) -> bool {
    let mut keys = Vec::new();
    let mut members = Vec::new();
    for line in lines.lines() {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split('|');
        let Some(key) = parts.next() else { return true };
        let Some(member) = parts.next() else { return true };
        if keys.contains(&key) || members.contains(&member) {
            return true;
        }
        keys.push(key);
        members.push(member);
    }
    false
}

pub fn window_seconds(start: u64, end: u64) -> Option<u64> {
    end.checked_sub(start)
}

fn bool_json(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn opt_u64(value: Option<u64>) -> String {
    match value {
        Some(number) => number.to_string(),
        None => "null".to_string(),
    }
}

fn opt_string(value: &Option<String>) -> String {
    match value {
        Some(text) => escape(text),
        None => "null".to_string(),
    }
}

pub fn report_json(report: &Report) -> String {
    let mut reasons = String::from("[");
    for (index, reason) in report.reasons.iter().enumerate() {
        if index > 0 {
            reasons.push_str(", ");
        }
        reasons.push_str(&escape(reason));
    }
    reasons.push(']');
    let mut remaining = String::from("[");
    for (index, name) in REMAINING.iter().enumerate() {
        if index > 0 {
            remaining.push_str(", ");
        }
        remaining.push_str(&escape(name));
    }
    remaining.push(']');
    format!(
        "{{\n  \"all_state_certified\": false,\n  \"caller\": \"trusted_local_admin\",\n  \"checkpoint_copy_ok\": {checkpoint},\n  \"clock_anomaly\": {clock},\n  \"comment_ok\": {comment},\n  \"counts_ok\": {counts},\n  \"cut_proven\": {cut},\n  \"decimal_ok\": {decimal},\n  \"history_insufficient\": {history},\n  \"key_count\": {key_count},\n  \"key_reused\": {key_reused},\n  \"lineage_version\": \"pg-backup-restore/v1\",\n  \"null_ok\": {null_ok},\n  \"offset_min_used\": false,\n  \"offset_published\": {offset},\n  \"parent_acceptance\": false,\n  \"pitr_ready\": false,\n  \"post_cut_absent_on_restore\": {post_restore},\n  \"post_cut_present_on_source\": {post_source},\n  \"production_ops\": false,\n  \"production_slo_certified\": false,\n  \"reason\": {reason},\n  \"reasons\": {reasons},\n  \"remaining_state_families\": {remaining},\n  \"replay_ready\": {ready},\n  \"report_version\": \"oo-pg-backup-readiness/v1\",\n  \"rpo_window_seconds\": {rpo},\n  \"rto_seconds\": {rto},\n  \"shop_gold_ok\": {shop},\n  \"source_cut\": \"cut-a356-1\",\n  \"target_fence\": \"fence-a356-restore\",\n  \"w04_gate\": \"open\",\n  \"wal_moved\": {wal}\n}}\n",
        checkpoint = bool_json(report.checkpoint_copy_ok),
        clock = bool_json(report.clock_anomaly),
        comment = bool_json(report.comment_ok),
        counts = bool_json(report.counts_ok),
        cut = bool_json(report.cut_proven),
        decimal = bool_json(report.decimal_ok),
        history = bool_json(report.history_insufficient),
        key_count = report.key_count,
        key_reused = bool_json(report.key_reused),
        null_ok = bool_json(report.null_ok),
        offset = opt_string(&report.offset_published),
        post_restore = bool_json(report.post_cut_absent_on_restore),
        post_source = bool_json(report.post_cut_present_on_source),
        reason = escape(&report.reason),
        reasons = reasons,
        remaining = remaining,
        ready = bool_json(report.replay_ready),
        rpo = opt_u64(report.rpo_window_seconds),
        rto = opt_u64(report.rto_seconds),
        shop = bool_json(report.shop_gold_ok),
        wal = bool_json(report.wal_moved),
    )
}
