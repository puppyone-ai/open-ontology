//! Writer-blocked cut. Marker and entry-count samples cannot establish it.
//!
//! SHARE locks block writers, including the table owner, and stay compatible
//! with pg_dump's ACCESS SHARE. Each database also exports its own REPEATABLE
//! READ snapshot. Sequence values are not MVCC, so a sequence lock attempt is
//! recorded and a refused lock withholds the cut. The sessions are disconnected
//! on drop so a failed backup does not leave either database read-only.

use std::thread;
use std::time::{Duration, Instant};

use crate::pg::{checked_relation, Client, HeldLock};
use crate::protocol::{AGENT_DB, CONTROL_DB};
use crate::snapshot::{self, Capture, FenceAttempt, SeqState, SequenceFence, SequenceLock, SnapshotBind};
use crate::{fail, Fail};

pub const WRITERS_BLOCKED: &str = "writers_blocked";
pub const UNPROVEN: &str = "unproven";
const CONTROL_APP: &str = "oo_a356_cut_control";
const AGENT_APP: &str = "oo_a356_cut_agent";

struct Sealed {
    database: &'static str,
    relation: &'static str,
    gold_file: &'static str,
    columns: &'static [&'static str],
    order: &'static [&'static str],
}

const SEALED: &[Sealed] = &[
    Sealed { database: CONTROL_DB, relation: "oo_cdc_offset.install_manifest", gold_file: "cdc-install.copy", columns: &["singleton", "sql_sha256"], order: &["singleton"] },
    Sealed { database: CONTROL_DB, relation: "oo_cdc_offset.mirrored_receipt", gold_file: "cdc-receipt.copy", columns: &["source", "table_generation", "txn_or_batch_id", "config_version", "config_digest", "type_policy_version", "type_digest", "hook_digest", "schema_digest", "payload_digest", "protocol_version", "position_codec", "position_lineage", "source_epoch", "applied_pos", "partial_txn", "cross_table_consistent", "txn_complete", "segment_index", "segment_count", "segment_bitmap", "fence_shard", "fence_generation", "target_commit_at", "receipt_hash"], order: &["source", "table_generation", "txn_or_batch_id"] },
    Sealed { database: CONTROL_DB, relation: "oo_cdc_offset.watermark", gold_file: "cdc-watermark.copy", columns: &["source", "position_codec", "position_lineage", "source_epoch", "applied_pos", "applied_txn_id", "declared_next_txn", "declared_next_pos", "binding_provenance"], order: &["source", "position_codec", "position_lineage", "source_epoch"] },
    Sealed { database: CONTROL_DB, relation: "oo_calendar_control.install_manifest", gold_file: "calendar-install.copy", columns: &["singleton", "sql_sha256"], order: &["singleton"] },
    Sealed { database: CONTROL_DB, relation: "oo_calendar_control.snapshot", gold_file: "calendar-snapshot.copy", columns: &["snapshot_id", "project_id", "calendar_version", "closure_date", "closure_label", "closure_fingerprint"], order: &["snapshot_id"] },
    Sealed { database: CONTROL_DB, relation: "oo_calendar_control.published", gold_file: "calendar-published.copy", columns: &["snapshot_id", "project_id", "calendar_version", "last_closed_period_end_date", "last_closed_period_end_midnight_status", "last_closed_period_end_midnight"], order: &["snapshot_id"] },
    Sealed { database: CONTROL_DB, relation: "oo_session_control.session_head", gold_file: "session-head.copy", columns: &["project_id", "session_id", "revision", "owner_generation", "state_json", "journal_len"], order: &["project_id", "session_id"] },
    Sealed { database: CONTROL_DB, relation: "oo_key_fixture.issued_key", gold_file: "key-issued.copy", columns: &["entity_key", "source_member", "issued_generation"], order: &["entity_key"] },
    Sealed { database: CONTROL_DB, relation: "oo_key_fixture.negative_decision", gold_file: "key-negative.copy", columns: &["left_key", "right_key", "decision", "operator_label"], order: &["left_key", "right_key"] },
    Sealed { database: CONTROL_DB, relation: "oo_key_fixture.cut_marker", gold_file: "key-cut.copy", columns: &["singleton", "source_cut", "entry_count"], order: &["singleton"] },
    Sealed { database: AGENT_DB, relation: "oo_enum_control.published", gold_file: "enum-published.copy", columns: &["snapshot_id", "line_no", "code", "status", "enum_id"], order: &["snapshot_id", "line_no"] },
    Sealed { database: AGENT_DB, relation: "oo_enum_control.install_manifest", gold_file: "enum-install.copy", columns: &["singleton", "sql_sha256"], order: &["singleton"] },
    Sealed { database: AGENT_DB, relation: "shop.party", gold_file: "shop-party.copy", columns: &["party_key", "display_name", "active", "phone", "national_id"], order: &["party_key"] },
    Sealed { database: AGENT_DB, relation: "shop.account", gold_file: "shop-account.copy", columns: &["account_id", "party_key", "opened", "label"], order: &["account_id"] },
    Sealed { database: AGENT_DB, relation: "shop.entry", gold_file: "shop-entry.copy", columns: &["entry_id", "account_id", "qty", "amount", "memo"], order: &["entry_id"] },
];

const SESSIONS: &[(&str, &str)] = &[(CONTROL_DB, CONTROL_APP), (AGENT_DB, AGENT_APP)];

const USER_TABLES: &str = "\
SELECT n.nspname || '.' || c.relname \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname <> 'pg_catalog' \
  AND n.nspname <> 'information_schema' \
  AND c.relkind IN ('r','p')";

const USER_SEQUENCES: &str = "\
SELECT n.nspname || '.' || c.relname \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname <> 'pg_catalog' \
  AND n.nspname <> 'information_schema' \
  AND c.relkind = 'S'";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationDigest {
    pub content_md5: String,
    pub database: String,
    pub gold_sha256: String,
    pub relation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterBlock {
    pub application_name: String,
    pub database: String,
    pub lock_mode: String,
    pub probe_rejected: bool,
    pub relations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentDigest {
    pub database: &'static str,
    pub relation: &'static str,
    pub gold_file: &'static str,
    pub content_md5: String,
}

#[derive(Debug, Clone, Copy)]
pub struct Signals {
    pub marker_stable: bool,
    pub count_stable: bool,
    pub locks_held: bool,
    pub probes_rejected: bool,
    pub digests_stable: bool,
    pub snapshots_bound: bool,
    pub sequence_fence_held: bool,
}

pub struct Barrier {
    holds: Vec<HeldLock>,
    records: Vec<LockRecord>,
    snapshots: Vec<SnapshotBind>,
    catalog_text: String,
    state_moved: bool,
    sequence_shares_held: bool,
    inventory_matches: bool,
}

struct LockRecord {
    application_name: String,
    database: String,
    pid: String,
    relations: Vec<String>,
    fence: Vec<FenceAttempt>,
    sequence_names: Vec<String>,
    sequence_state: Vec<SeqState>,
}

/// Marker and count stability do not establish a cut. They can only withhold one.
pub fn proven(signals: &Signals) -> bool {
    let blocked = signals.locks_held && signals.probes_rejected && signals.digests_stable;
    blocked && signals.marker_stable && signals.count_stable && signals.snapshots_bound && signals.sequence_fence_held
}

pub fn application_name(database: &str) -> Option<&'static str> {
    SESSIONS.iter().find(|item| item.0 == database).map(|item| item.1)
}

pub fn sealed_gold<'a>(database: &str, relation: &str) -> Option<&'a str> {
    SEALED.iter().find(|item| item.database == database && item.relation == relation).map(|item| item.gold_file)
}

pub fn sealed_pairs() -> Vec<(&'static str, &'static str, &'static str)> {
    SEALED.iter().map(|item| (item.database, item.relation, item.gold_file)).collect()
}

pub struct CutProof<'a> {
    pub blocks: &'a [WriterBlock],
    pub catalog_sha256: &'a str,
    pub cut_mode: &'a str,
    pub digests: &'a [RelationDigest],
    pub fence: &'a SequenceFence,
    pub history_insufficient: bool,
    pub quiescent: bool,
    pub snapshots: &'a [SnapshotBind],
}

pub fn proof_shape(proof: &CutProof<'_>) -> Result<(), Fail> {
    if proof.quiescent == proof.history_insufficient {
        return Err(fail(4, "manifest quiescent flag disagrees"));
    }
    sealed_blocks(proof.blocks)?;
    sealed_digests(proof.digests)?;
    snapshot_shape(proof.snapshots)?;
    fence_shape(proof.fence)?;
    if !is_sha256(proof.catalog_sha256) {
        return Err(fail(4, "catalog digest is not hex"));
    }
    if proof.history_insufficient {
        if proof.cut_mode != UNPROVEN || proof.quiescent || proof.fence.held {
            return Err(fail(4, "unproven cut has a seal"));
        }
        return Ok(());
    }
    if proof.cut_mode != WRITERS_BLOCKED || !proof.quiescent || !proof.fence.held {
        return Err(fail(4, "quiescent cut is not writers_blocked"));
    }
    Ok(())
}

fn sealed_blocks(blocks: &[WriterBlock]) -> Result<(), Fail> {
    if blocks.len() != SESSIONS.len() {
        return Err(fail(4, "cut proof is incomplete"));
    }
    for (database, application) in SESSIONS {
        let Some(block) = blocks.iter().find(|item| item.database == *database) else {
            return Err(fail(4, "cut proof is missing a database"));
        };
        if block.application_name != *application || block.lock_mode != "SHARE" || !block.probe_rejected {
            return Err(fail(4, "writer block is not a held SHARE lock"));
        }
        let mut locked = block.relations.clone();
        locked.sort();
        if locked.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(fail(4, "writer block repeats a relation"));
        }
        for relation in &locked {
            checked_relation(relation)?;
        }
        for sealed in SEALED.iter().filter(|item| item.database == *database) {
            if !locked.iter().any(|relation| relation == sealed.relation) {
                return Err(fail(4, "writer block misses a sealed relation"));
            }
        }
    }
    Ok(())
}

fn sealed_digests(digests: &[RelationDigest]) -> Result<(), Fail> {
    if digests.len() != SEALED.len() {
        return Err(fail(4, "cut proof is incomplete"));
    }
    let mut seen = Vec::new();
    for digest in digests {
        if !is_md5(&digest.content_md5) || !is_sha256(&digest.gold_sha256) {
            return Err(fail(4, "relation digest is not hex"));
        }
        if sealed_gold(&digest.database, &digest.relation).is_none() {
            return Err(fail(4, "relation digest is not sealed"));
        }
        let key = format!("{} {}", digest.database, digest.relation);
        if seen.contains(&key) {
            return Err(fail(4, "relation digest repeats"));
        }
        seen.push(key);
    }
    for sealed in SEALED {
        if !digests.iter().any(|digest| digest.database == sealed.database && digest.relation == sealed.relation) {
            return Err(fail(4, "relation digest is missing"));
        }
    }
    Ok(())
}

fn snapshot_shape(snapshots: &[SnapshotBind]) -> Result<(), Fail> {
    if snapshots.len() != SESSIONS.len() {
        return Err(fail(4, "cut snapshots are not two databases"));
    }
    let mut ids = Vec::new();
    for (database, _) in SESSIONS {
        let Some(item) = snapshots.iter().find(|item| item.database == *database) else {
            return Err(fail(4, "cut snapshot is missing a database"));
        };
        if item.isolation != "REPEATABLE READ" {
            return Err(fail(4, "cut snapshot isolation is refused"));
        }
        if snapshot::valid_id(&item.id).is_err() {
            return Err(fail(4, "cut snapshot id is refused"));
        }
        ids.push(item.id.clone());
    }
    if ids[0] == ids[1] {
        return Err(fail(4, "cut snapshots are not distinct"));
    }
    Ok(())
}

fn fence_shape(fence: &SequenceFence) -> Result<(), Fail> {
    if fence.lock_mode != "SHARE" || fence.method != "lock_table_share" {
        return Err(fail(4, "sequence fence is not a share lock"));
    }
    let mut seen = Vec::new();
    for row in &fence.sequences {
        if row.database != CONTROL_DB && row.database != AGENT_DB {
            return Err(fail(4, "sequence fence database is refused"));
        }
        checked_relation(&row.relation)?;
        if row.sqlstate.len() != 5 || !row.sqlstate.bytes().all(|byte| byte.is_ascii_digit() || byte.is_ascii_uppercase()) {
            return Err(fail(4, "sequence fence sqlstate is refused"));
        }
        if row.held != (row.sqlstate == "00000") {
            return Err(fail(4, "sequence fence held flag disagrees"));
        }
        let key = format!("{} {}", row.database, row.relation);
        if seen.contains(&key) {
            return Err(fail(4, "sequence fence repeats"));
        }
        seen.push(key);
    }
    if fence.held && fence.sequences.iter().any(|row| !row.held) {
        return Err(fail(4, "sequence fence is not held"));
    }
    Ok(())
}

pub fn digests_bound(digests: &[RelationDigest], gold: &[(&str, &str)]) -> Result<(), Fail> {
    for digest in digests {
        let Some(file_name) = sealed_gold(&digest.database, &digest.relation) else {
            return Err(fail(4, "manifest_shape"));
        };
        match gold.iter().find(|item| item.0 == file_name) {
            Some((_, sha)) if *sha == digest.gold_sha256 => {}
            _ => return Err(fail(4, "gold_hash_mismatch")),
        }
    }
    Ok(())
}

impl Barrier {
    pub fn acquire(client: &Client) -> Result<Self, Fail> {
        let mut holds = Vec::new();
        let mut records = Vec::new();
        let mut snapshots = Vec::new();
        let mut catalog = Vec::new();
        let mut inventory_matches = true;
        for (database, application) in SESSIONS {
            let relations = user_tables(client, database)?;
            let sequences = user_sequences(client, database)?;
            for sealed in SEALED.iter().filter(|item| item.database == *database) {
                if !relations.iter().any(|relation| relation == sealed.relation) {
                    return Err(fail(1, format!("sealed relation missing: {}", sealed.relation)));
                }
            }
            let held = client.open_cut(database, application, &relations, &sequences)?;
            holds.push(held);
            let captured = holds.last().expect("hold was pushed").captured.clone();
            let parsed = snapshot::parse_capture(&captured)?;
            if parsed.tables != relations {
                inventory_matches = false;
            }
            catalog.extend(snapshot::catalog_lines(database, &parsed)?);
            snapshots.push(SnapshotBind {
                database: (*database).to_string(),
                id: parsed.snapshot_id.clone(),
                isolation: "REPEATABLE READ".to_string(),
            });
            let pid = wait_held(holds.last_mut().expect("hold was pushed"), client, database, application, &relations)?;
            records.push(lock_record(application, database, pid, relations, parsed));
        }
        catalog.sort();
        if catalog.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(fail(1, "catalog line repeats"));
        }
        Ok(Self {
            holds,
            records,
            snapshots,
            catalog_text: snapshot::catalog_text(&catalog),
            state_moved: false,
            sequence_shares_held: false,
            inventory_matches,
        })
    }

    pub fn snapshot_id(&self, database: &str) -> Result<&str, Fail> {
        self.snapshots
            .iter()
            .find(|item| item.database == database)
            .map(|item| item.id.as_str())
            .ok_or_else(|| fail(1, "cut snapshot is missing"))
    }

    pub fn snapshot_pairs(&self) -> Vec<(String, String)> {
        self.snapshots.iter().map(|item| (item.database.clone(), item.id.clone())).collect()
    }

    pub fn snapshots(&self) -> &[SnapshotBind] {
        &self.snapshots
    }

    pub fn catalog_text(&self) -> &str {
        &self.catalog_text
    }

    pub fn inventory_matches(&self) -> bool {
        self.inventory_matches
    }

    pub fn snapshots_bound(&self) -> bool {
        snapshot_shape(&self.snapshots).is_ok()
    }

    pub fn sequence_fence_held(&self) -> bool {
        if self.state_moved || !self.sequence_shares_held {
            return false;
        }
        for record in &self.records {
            for name in &record.sequence_names {
                let Some(row) = record.fence.iter().find(|row| row.relation == *name) else {
                    return false;
                };
                if !row.granted {
                    return false;
                }
            }
            for state in &record.sequence_state {
                if !record.sequence_names.iter().any(|name| name == &state.relation) {
                    return false;
                }
            }
        }
        true
    }

    pub fn sequence_fence(&self) -> SequenceFence {
        let mut sequences = Vec::new();
        for record in &self.records {
            for row in &record.fence {
                sequences.push(SequenceLock {
                    database: record.database.clone(),
                    held: row.granted,
                    relation: row.relation.clone(),
                    sqlstate: row.sqlstate.clone(),
                });
            }
        }
        sequences.sort_by(|left, right| left.database.cmp(&right.database).then(left.relation.cmp(&right.relation)));
        SequenceFence {
            held: self.sequence_fence_held(),
            lock_mode: "SHARE".to_string(),
            method: "lock_table_share".to_string(),
            sequences,
        }
    }

    pub fn confirm_sequence_locks(&mut self, client: &Client) -> Result<(), Fail> {
        let mut ok = true;
        for record in &self.records {
            let locked = lines(client, &record.database, &sequence_locks_sql(&record.application_name, &record.database))?;
            for row in &record.fence {
                if row.granted && !locked.iter().any(|name| name == &row.relation) {
                    ok = false;
                }
            }
        }
        self.sequence_shares_held = ok;
        Ok(())
    }

    pub fn reread_sequences(&mut self) -> Result<(), Fail> {
        let scripts: Vec<String> = self.records.iter().map(|record| snapshot::reread_sql(&record.sequence_state)).collect();
        let mut moved = false;
        for (index, script) in scripts.iter().enumerate() {
            let text = self.holds[index].push_sql(script, "seq2done|1")?;
            if snapshot::state_moved(&self.records[index].sequence_state, &text) {
                moved = true;
            }
        }
        if moved {
            self.state_moved = true;
        }
        Ok(())
    }

    pub fn still_held(&mut self, client: &Client) -> Result<bool, Fail> {
        let planned = {
            let mut planned = Vec::new();
            for record in &self.records {
                let Some(index) = holds_index(&self.holds, &record.database) else {
                    return Ok(false);
                };
                planned.push((
                    index,
                    record.pid.clone(),
                    record.application_name.clone(),
                    record.database.clone(),
                    record.relations.clone(),
                ));
            }
            planned
        };
        for (index, expect_pid, application, database, relations) in planned {
            if self.holds[index].exited()? {
                return Ok(false);
            }
            let pid = match optional_pid(client, &database, &pid_sql(&application, &database))? {
                Some(pid) => pid,
                None => return Ok(false),
            };
            if pid != expect_pid {
                return Ok(false);
            }
            let state = text_line(client, &database, &format!("SELECT state FROM pg_stat_activity WHERE pid = {pid}"))?;
            if state.as_deref() != Some("idle in transaction") {
                return Ok(false);
            }
            let locked = lines(client, &database, &locks_sql(&application, &database))?;
            if locked != relations {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn release(mut self) -> Result<(), Fail> {
        while let Some(held) = self.holds.pop() {
            held.commit()?;
        }
        Ok(())
    }

    pub fn blocks(&self) -> Vec<WriterBlock> {
        self.records
            .iter()
            .map(|record| WriterBlock {
                application_name: record.application_name.clone(),
                database: record.database.clone(),
                lock_mode: "SHARE".to_string(),
                probe_rejected: true,
                relations: record.relations.clone(),
            })
            .collect()
    }
}

pub fn probes_rejected(client: &Client) -> Result<bool, Fail> {
    let agent = client.write_rejected(AGENT_DB, "UPDATE shop.entry SET amount = amount WHERE entry_id = 3")?;
    let control = client.write_rejected(
        CONTROL_DB,
        "UPDATE oo_key_fixture.negative_decision SET operator_label = operator_label WHERE left_key = 'k-0001' AND right_key = 'k-0002'",
    )?;
    Ok(agent && control)
}

pub fn content_digests(client: &Client, snapshots: &[(String, String)]) -> Result<Vec<ContentDigest>, Fail> {
    let mut out = Vec::new();
    for item in SEALED {
        let sql = digest_sql(item)?;
        let Some(snapshot) = snapshots.iter().find(|pair| pair.0 == item.database).map(|pair| pair.1.as_str()) else {
            return Err(fail(1, "cut snapshot is missing"));
        };
        let content_md5 = client.query_in_snapshot(item.database, snapshot, &sql)?;
        if !is_md5(&content_md5) {
            return Err(fail(1, "relation digest is not md5"));
        }
        out.push(ContentDigest { database: item.database, relation: item.relation, gold_file: item.gold_file, content_md5 });
    }
    Ok(out)
}

pub fn same_content(left: &[ContentDigest], right: &[ContentDigest]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(before, after)| {
            before.database == after.database && before.relation == after.relation && before.content_md5 == after.content_md5
        })
}

fn holds_index(holds: &[HeldLock], _database: &str) -> Option<usize> {
    if holds.len() == SESSIONS.len() {
        SESSIONS.iter().position(|item| item.0 == _database)
    } else {
        None
    }
}

fn wait_held(held: &mut HeldLock, client: &Client, database: &str, application: &str, relations: &[String]) -> Result<String, Fail> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if held.exited()? {
            let detail = held.error_text();
            return Err(fail(1, format!("cut session exited: {detail}")));
        }
        if let Some(pid) = strict_pid(client, database, &pid_sql(application, database))? {
            let state = text_line(client, database, &format!("SELECT state FROM pg_stat_activity WHERE pid = {pid}"))?;
            let locked = lines(client, database, &locks_sql(application, database))?;
            if state.as_deref() == Some("idle in transaction") && locked == *relations {
                return Ok(pid);
            }
        }
        if Instant::now() >= deadline {
            return Err(fail(1, "cut locks were not granted"));
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn user_tables(client: &Client, database: &str) -> Result<Vec<String>, Fail> {
    let mut relations = lines(client, database, USER_TABLES)?;
    if relations.is_empty() || relations.len() > 128 {
        return Err(fail(1, "cut relation list is refused"));
    }
    for relation in &relations {
        checked_relation(relation)?;
    }
    relations.sort();
    Ok(relations)
}

fn user_sequences(client: &Client, database: &str) -> Result<Vec<String>, Fail> {
    let mut relations = lines(client, database, USER_SEQUENCES)?;
    if relations.len() > 128 {
        return Err(fail(1, "cut relation list is refused"));
    }
    for relation in &relations {
        checked_relation(relation)?;
    }
    relations.sort();
    Ok(relations)
}

fn lock_record(application: &str, database: &str, pid: String, relations: Vec<String>, parsed: Capture) -> LockRecord {
    LockRecord {
        application_name: application.to_string(),
        database: database.to_string(),
        pid,
        relations,
        fence: parsed.fence,
        sequence_names: parsed.sequence_names,
        sequence_state: parsed.sequences,
    }
}

fn digest_sql(item: &Sealed) -> Result<String, Fail> {
    for column in item.columns.iter().chain(item.order.iter()) {
        if !column_ok(column) {
            return Err(fail(1, "digest column is refused"));
        }
    }
    let projection = item
        .columns
        .iter()
        .map(|column| format!("coalesce({column}::text, '<NULL>')"))
        .collect::<Vec<_>>()
        .join(" || '|' || ");
    let order = item.order.join(", ");
    let relation = checked_relation(item.relation)?;
    Ok(format!(
        "SELECT pg_catalog.md5(coalesce(pg_catalog.string_agg(pg_catalog.md5({projection}), '' ORDER BY {order}), '')) FROM {relation}"
    ))
}

fn pid_sql(application: &str, database: &str) -> String {
    format!(
        "SELECT pid::text FROM pg_stat_activity WHERE application_name = '{application}' AND datname = '{database}'"
    )
}

fn locks_sql(application: &str, database: &str) -> String {
    format!(
        "SELECT n.nspname || '.' || c.relname \
         FROM pg_catalog.pg_locks l \
         JOIN pg_catalog.pg_class c ON c.oid = l.relation \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_catalog.pg_stat_activity a ON a.pid = l.pid \
         WHERE a.application_name = '{application}' \
           AND a.datname = '{database}' \
           AND l.granted \
           AND l.mode = 'ShareLock' \
           AND c.relkind IN ('r','p')"
    )
}

fn sequence_locks_sql(application: &str, database: &str) -> String {
    format!(
        "SELECT n.nspname || '.' || c.relname \
         FROM pg_catalog.pg_locks l \
         JOIN pg_catalog.pg_class c ON c.oid = l.relation \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_catalog.pg_stat_activity a ON a.pid = l.pid \
         WHERE a.application_name = '{application}' \
           AND a.datname = '{database}' \
           AND l.granted \
           AND l.mode = 'ShareLock' \
           AND c.relkind = 'S'"
    )
}

fn lines(client: &Client, database: &str, sql: &str) -> Result<Vec<String>, Fail> {
    let text = client.query(database, sql)?;
    let mut out: Vec<String> = text.lines().filter(|line| !line.is_empty()).map(str::to_string).collect();
    out.sort();
    out.dedup();
    Ok(out)
}

fn text_line(client: &Client, database: &str, sql: &str) -> Result<Option<String>, Fail> {
    let text = client.query(database, sql)?;
    let lines: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
    if lines.len() > 1 {
        return Err(fail(1, "cut session is ambiguous"));
    }
    Ok(lines.first().map(|line| (*line).to_string()))
}

fn strict_pid(client: &Client, database: &str, sql: &str) -> Result<Option<String>, Fail> {
    let Some(pid) = text_line(client, database, sql)? else {
        return Ok(None);
    };
    if !pid.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(fail(1, "cut session pid is refused"));
    }
    Ok(Some(pid))
}

fn optional_pid(client: &Client, database: &str, sql: &str) -> Result<Option<String>, Fail> {
    match text_line(client, database, sql) {
        Ok(Some(pid)) if pid.bytes().all(|byte| byte.is_ascii_digit()) => Ok(Some(pid)),
        Ok(_) => Ok(None),
        Err(err) if err.detail == "cut session is ambiguous" => Ok(None),
        Err(err) => Err(err),
    }
}

fn column_ok(name: &str) -> bool {
    let mut bytes = name.bytes();
    match bytes.next() {
        Some(byte) if byte.is_ascii_lowercase() || byte == b'_' => {}
        _ => return false,
    }
    bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn is_md5(text: &str) -> bool {
    is_lower_hex(text, 32)
}

fn is_sha256(text: &str) -> bool {
    is_lower_hex(text, 64)
}

fn is_lower_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_and_count_stability_is_not_a_cut() {
        let stable = Signals {
            marker_stable: true,
            count_stable: true,
            locks_held: false,
            probes_rejected: false,
            digests_stable: true,
            snapshots_bound: true,
            sequence_fence_held: true,
        };
        assert!(!proven(&stable));
        let blocked = Signals { locks_held: true, probes_rejected: true, digests_stable: true, ..stable };
        assert!(proven(&blocked));
        let drifted = Signals { marker_stable: false, ..blocked };
        assert!(!proven(&drifted));
        let unfenced = Signals { sequence_fence_held: false, ..blocked };
        assert!(!proven(&unfenced));
        let unbound = Signals { snapshots_bound: false, ..blocked };
        assert!(!proven(&unbound));
    }

    #[test]
    fn sealed_select_lists_match_the_gold_copies() {
        let specs = crate::protocol::copies();
        assert_eq!(specs.len(), SEALED.len());
        for item in SEALED {
            let Some(spec) = specs.iter().find(|spec| spec.file_name == item.gold_file) else {
                panic!("missing gold file");
            };
            assert_eq!(spec.source_db, item.database);
            let expect = format!(
                "SELECT {} FROM {} ORDER BY {}",
                item.columns.join(", "),
                item.relation,
                item.order.join(", ")
            );
            assert_eq!(spec.select_sql, expect);
        }
    }
}
