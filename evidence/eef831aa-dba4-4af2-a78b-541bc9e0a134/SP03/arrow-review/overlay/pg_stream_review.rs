//! Independent R2-S checks for fixed commit 75b993d0.
//! This file is a review overlay. It is not product source.
//! `OO_PG_STREAM_FIXTURE` is required. A missing pointer is a failure.

use std::fs;
use std::time::{Duration, Instant};

use arrow_array::{Array, BooleanArray, Int32Array, RecordBatch, StringArray};
use arrow_schema::DataType;
use oo_connectors::types::ColumnPlan;
use oo_connectors::{
    config_identity, type_policy_identity, AdmittedCatalog, AdmittedColumn, AdmittedTable,
    CutProbe, PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, ProbeValue,
    SecretRef, SelectedTable, SnapshotOwner, SnapshotRequest, SourceErrorKind,
    SourceId, StreamBudget, StreamItem, TableSelection, CLEANUP_OBSERVATION_BOUND_MS,
};
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};
use zeroize::Zeroizing;

const SCHEMA: &str = "rev";

#[derive(Deserialize)]
struct Pointer {
    host: String,
    port: u16,
    database: String,
    reader_user: String,
    writer_user: String,
    super_user: String,
    password_dir: String,
    ca_file: String,
    log_file: String,
}

struct Activity {
    state: String,
    wait_event: String,
    in_xact: bool,
    xact_start: String,
    xmin: String,
    query: String,
    app: String,
}

type ParentRow = (i32, bool, Option<String>);
type ChildRow = (Option<i32>, Option<String>);

struct Oracle {
    parent: Vec<ParentRow>,
    child: Vec<ChildRow>,
    empty: i64,
    zero: i64,
}

struct World {
    pointer: Pointer,
    reader_pass: Zeroizing<String>,
    writer_pass: Zeroizing<String>,
    super_pass: Zeroizing<String>,
    ca: Vec<u8>,
    admin: PgConnection,
    postmaster: String,
    in_recovery: bool,
    outcomes: Vec<String>,
    problems: Vec<String>,
    outcome_path: Option<String>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_r2s_review() {
    let mut world = World::start().await;
    world.main_cut().await;
    world.split_batch_bytes().await;
    world.oversize_batch_bytes().await;
    world.numeric_key_separate_from_probe().await;
    world.schema_guard().await;
    world.acl_guard().await;
    world.drop_releases().await;
    world.audit();
    world.note(format!("problem_count={}", world.problems.len()));
    if world.problems.is_empty() {
        world.note("review_complete".to_string());
    } else {
        world.note(format!("review_problems={}", world.problems.join(" | ")));
    }
    let problems = world.problems.clone();
    drop(world);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(path) = &self.outcome_path {
            let _ = fs::write(path, self.outcomes.join("\n") + "\n");
        }
    }
}

impl World {
    async fn start() -> Self {
        let path = std::env::var("OO_PG_STREAM_FIXTURE").expect("OO_PG_STREAM_FIXTURE is required");
        let pointer: Pointer = serde_json::from_str(&fs::read_to_string(&path).expect("pointer"))
            .expect("pointer json");
        let reader_pass = read_pass(&pointer.password_dir, "reader.pass");
        let writer_pass = read_pass(&pointer.password_dir, "writer.pass");
        let super_pass = read_pass(&pointer.password_dir, "super.pass");
        let ca = fs::read(&pointer.ca_file).expect("ca");
        let mut admin = dial(&pointer, &pointer.super_user, &super_pass, &ca, "oo-rev-admin").await;
        let (postmaster, in_recovery) = node_of(&mut admin).await;
        let outcome_path = std::env::var("OO_PG_REVIEW_OUTCOME").ok();
        Self {
            pointer,
            reader_pass,
            writer_pass,
            super_pass,
            ca,
            admin,
            postmaster,
            in_recovery,
            outcomes: Vec::new(),
            problems: Vec::new(),
            outcome_path,
        }
    }

    fn note(&mut self, line: String) {
        eprintln!("review {line}");
        self.outcomes.push(line);
    }

    fn problem(&mut self, line: impl Into<String>) {
        let line = line.into();
        eprintln!("problem {line}");
        self.problems.push(line.clone());
        self.outcomes.push(format!("problem {line}"));
    }

    async fn request(&mut self, source: &str, generation: u64, names: &[&str]) -> SnapshotRequest {
        let admitted = load_admitted(&mut self.admin, names).await;
        let config = PgSourceConfig::try_new(
            SourceId::new(source).unwrap(),
            PgEndpoint::new(&self.pointer.host, self.pointer.port, &self.pointer.database).unwrap(),
            PgCredentials::new(&self.pointer.reader_user, &*self.reader_pass).unwrap(),
            SecretRef::new("secret://pg/review/reader").unwrap(),
            PgTlsConfig::from_ca_pem(&self.ca).unwrap(),
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(30),
                Duration::from_secs(5),
                128,
                1024 * 1024,
            )
            .unwrap(),
            TableSelection::new(
                names
                    .iter()
                    .map(|name| SelectedTable::new(SCHEMA, *name).unwrap())
                    .collect(),
            )
            .unwrap(),
        )
        .unwrap();
        SnapshotRequest {
            config_identity: config_identity(&config),
            type_policy_identity: type_policy_identity(oo_connectors::PgTypePolicy::PRESERVE_ROW),
            config,
            admitted,
            policy: oo_connectors::PgTypePolicy::PRESERVE_ROW,
            generation,
            deadline: Instant::now() + Duration::from_secs(180),
            expected_principal: self.pointer.reader_user.clone(),
            expected_database: self.pointer.database.clone(),
            expected_in_recovery: self.in_recovery,
            expected_postmaster_start: self.postmaster.clone(),
        }
    }

    async fn open_at(&mut self, source: &str, generation: u64, names: &[&str]) -> SnapshotOwner {
        let request = self.request(source, generation, names).await;
        SnapshotOwner::open(request).await.expect("open owner")
    }

    async fn main_cut(&mut self) {
        let mut oracle_conn = dial(
            &self.pointer,
            &self.pointer.reader_user,
            &self.reader_pass,
            &self.ca,
            "oo-rev-oracle",
        )
        .await;
        sqlx::query("START TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut oracle_conn)
            .await
            .expect("oracle transaction");
        let oracle = read_oracle(&mut oracle_conn).await;
        self.note(format!(
            "oracle_before_writer parent={} child={} empty={} zero={} null_label={} null_child_id={} null_child_note={} payload={}",
            oracle.parent.len(),
            oracle.child.len(),
            oracle.empty,
            oracle.zero,
            oracle.parent.iter().filter(|row| row.2.is_none()).count(),
            oracle.child.iter().filter(|row| row.0.is_none()).count(),
            oracle.child.iter().filter(|row| row.1.is_none()).count(),
            oracle_payload(&oracle)
        ));
        assert_eq!(oracle.parent, vec![
            (10, true, Some("red".to_string())),
            (11, false, None),
            (12, true, Some("blue".to_string())),
        ]);
        assert_eq!(oracle.child.len(), 3);
        assert_eq!(oracle.empty, 0);
        assert_eq!(oracle.zero, 2);

        let mut owner = self.open_at("rev_main", 1, &["parent", "child", "empty", "zerocol"]).await;
        let binding = owner.binding().clone();
        let generation = binding.generation;
        let digest = binding.input_digest;
        let pid = binding.backend.pid;
        let start = binding.backend.backend_start.clone();
        assert_eq!(binding.protocol, oo_connectors::SNAPSHOT_PROTOCOL);
        assert_eq!(binding.source_id, "rev_main");
        let budget = StreamBudget::try_new(1, 1_048_576, 1024, 8192).expect("budget");
        assert_eq!(budget.queue_batches(), 1);
        owner.open_stream(budget).await.expect("open stream");
        let backends = self.oo_backends().await;
        if backends.len() != 2 {
            self.problem(format!("owner backends={}", backends.len()));
        }

        let mut parent = Vec::new();
        let mut child = Vec::new();
        let mut empty_batches = 0_u32;
        let mut zero_rows = 0_usize;
        let mut eofs = Vec::new();
        let mut kinds = Vec::new();
        let mut wrote = false;
        let mut held_xact = String::new();
        let mut held_xmin = String::new();
        let mut parent_key = false;
        let mut child_key = false;
        let mut child_digest_some = false;
        let mut parent_digest_missing = false;
        let accounting = loop {
            match owner.next_batch().await.expect("main next") {
                StreamItem::Batch(batch) => {
                    kinds.push(format!("batch:{}", batch.typed.table));
                    if batch.resumable {
                        self.problem("batch resumable".to_string());
                    }
                    if batch.typed.run_generation != generation || batch.typed.input_digest != digest {
                        self.problem("run identity changed on a batch".to_string());
                    }
                    if batch.accounting.retained_batches != 0 {
                        self.problem(format!("retained_batches={}", batch.accounting.retained_batches));
                    }
                    assert_field_types(&batch.record_batch, &batch.typed.table);
                    match batch.typed.table.as_str() {
                        "parent" => {
                            parent_key |= batch.typed.plans.iter().any(plan_key_named("id"));
                            if batch.typed.rows.iter().any(|row| row.key_digest.is_none()) {
                                parent_digest_missing = true;
                            }
                            let rows = parent_arrow(&batch.record_batch);
                            if rows.len() != 1 {
                                self.problem(format!("parent batch rows={}", rows.len()));
                            }
                            if !wrote {
                                let before = reader_lines(&self.pointer.log_file);
                                self.hold(pid, &start).await;
                                tokio::time::sleep(Duration::from_millis(400)).await;
                                let during = reader_lines(&self.pointer.log_file);
                                if during != before {
                                    self.problem(format!(
                                        "reader log grew during idle before={before} during={during}"
                                    ));
                                }
                                if let Some(row) = self.activity(pid, &start).await {
                                    held_xact = row.xact_start.clone();
                                    held_xmin = row.xmin.clone();
                                    self.note(format!(
                                        "held_state={} wait_event={} xact_start={} xmin={} query={}",
                                        row.state, row.wait_event, row.xact_start, row.xmin, row.query
                                    ));
                                    if !row.in_xact || !waiting(&row) {
                                        self.problem(format!(
                                            "idle consumer backend state={} wait={}",
                                            row.state, row.wait_event
                                        ));
                                    }
                                } else {
                                    self.problem("backend missing while batch held".to_string());
                                }
                                self.writer_commits().await;
                                wrote = true;
                            }
                            parent.extend(rows);
                        }
                        "child" => {
                            child_key |= batch.typed.plans.iter().any(|plan| plan.key);
                            child_digest_some |= batch.typed.rows.iter().any(|row| row.key_digest.is_some());
                            if let Some(row) = self.activity(pid, &start).await {
                                self.note(format!(
                                    "child_xact_start={} child_xmin={} same_pid_query={}",
                                    row.xact_start, row.xmin, row.query
                                ));
                                if row.xact_start != held_xact || held_xact.is_empty() {
                                    self.problem(format!(
                                        "xact_start drifted held={} child={}",
                                        held_xact, row.xact_start
                                    ));
                                }
                                if !held_xmin.is_empty() && row.xmin != held_xmin {
                                    self.problem(format!(
                                        "backend_xmin drifted held={} child={}",
                                        held_xmin, row.xmin
                                    ));
                                }
                            }
                            child.extend(child_arrow(&batch.record_batch));
                        }
                        "empty" => empty_batches += 1,
                        "zerocol" => {
                            if !batch.typed.arrow.is_empty() || batch.record_batch.num_columns() != 0 {
                                self.problem("zero-column batch has arrow columns".to_string());
                            }
                            zero_rows += batch.record_batch.num_rows();
                        }
                        other => self.problem(format!("unexpected table {other}")),
                    }
                }
                StreamItem::TableEof(eof) => {
                    kinds.push(format!("eof:{}", eof.table));
                    if eof.resumable_key || eof.schema != SCHEMA {
                        self.problem(format!("bad eof {}", eof.table));
                    }
                    if eof.table == "parent" {
                        if let Some(row) = self.activity(pid, &start).await {
                            self.note(format!(
                                "after_parent_eof state={} in_xact={} xact_start={} query={}",
                                row.state, row.in_xact, row.xact_start, row.query
                            ));
                            if row.xact_start != held_xact {
                                self.problem("xact_start changed at parent eof".to_string());
                            }
                        }
                    }
                    eofs.push(eof);
                }
                StreamItem::Exhausted(accounting) => {
                    kinds.push("exhausted".to_string());
                    break accounting;
                }
            }
        };
        let again = owner.next_batch().await.expect("second exhausted");
        if !matches!(again, StreamItem::Exhausted(_)) {
            self.problem("second demand after exhaustion was not Exhausted".to_string());
        }
        if owner.binding().generation != generation || owner.binding().input_digest != digest {
            self.problem("binding identity changed after the pull".to_string());
        }
        sort_parent(&mut parent);
        sort_child(&mut child);
        let oracle_child = {
            let mut rows = oracle.child.clone();
            sort_child(&mut rows);
            rows
        };
        if parent != oracle.parent || child != oracle_child {
            self.problem(format!(
                "arrow values diverged parent={parent:?} child={child:?} oracle_parent={:?} oracle_child={oracle_child:?}",
                oracle.parent
            ));
        }
        if empty_batches != 0 || zero_rows != 2 {
            self.problem(format!("empty_batches={empty_batches} zero_rows={zero_rows}"));
        }
        match (
            find_eof("parent", &eofs),
            find_eof("child", &eofs),
            find_eof("empty", &eofs),
            find_eof("zerocol", &eofs),
        ) {
            (Some(parent_eof), Some(child_eof), Some(empty_eof), Some(zero_eof)) => {
                if parent_eof.rows_emitted != 4 && parent_eof.rows_emitted != 3 {
                    self.problem(format!("parent rows_emitted={}", parent_eof.rows_emitted));
                }
                if parent_eof.rows_emitted != 3 || child_eof.rows_emitted != 3 || empty_eof.rows_emitted != 0 || zero_eof.rows_emitted != 2 {
                    self.problem(format!(
                        "eof counts parent={} child={} empty={} zero={}",
                        parent_eof.rows_emitted, child_eof.rows_emitted, empty_eof.rows_emitted, zero_eof.rows_emitted
                    ));
                }
                if parent_eof.zero_column || child_eof.zero_column || empty_eof.zero_column || !zero_eof.zero_column {
                    self.problem("zero_column flags".to_string());
                }
                if empty_eof.columns != 1 || zero_eof.columns != 0 {
                    self.problem(format!("eof columns empty={} zero={}", empty_eof.columns, zero_eof.columns));
                }
            }
            _ => self.problem(format!("eof set {}", kinds.join(","))),
        }
        if !parent_key || parent_digest_missing || child_key || child_digest_some {
            self.problem(format!(
                "key flags parent_key={parent_key} parent_digest_missing={parent_digest_missing} child_key={child_key} child_digest_some={child_digest_some}"
            ));
        }
        let payload = oracle_payload(&oracle);
        self.note(format!(
            "measured executes={} reserved_rows={} consumed_rows={} decoded_payload_bytes={} oracle_payload={} arrow_preimage_bytes={} arrow_assembly_bytes={} retained_rows={} wire_view_capacity_peak={} wire_accepted_logical_payload={} wire_assembly_bytes={} queue_batches=1 kinds={}",
            accounting.executes,
            accounting.reserved_rows,
            accounting.consumed_rows,
            accounting.decoded_payload_bytes,
            payload,
            accounting.arrow_preimage_bytes,
            accounting.arrow_assembly_bytes,
            accounting.retained_rows,
            accounting.wire_view_capacity_peak,
            accounting.wire_accepted_logical_payload,
            accounting.wire_assembly_bytes,
            kinds.join(",")
        ));
        if accounting.decoded_payload_bytes != payload || accounting.consumed_rows != 8 || accounting.retained_rows != 0 {
            self.problem(format!(
                "accounting payload={} oracle={} consumed={} retained={}",
                accounting.decoded_payload_bytes, payload, accounting.consumed_rows, accounting.retained_rows
            ));
        }
        if accounting.arrow_assembly_bytes == 0 || accounting.arrow_preimage_bytes == 0 {
            self.problem("arrow sizes were zero".to_string());
        }
        let still = self.oo_backends().await;
        if still.len() != 2 {
            self.problem(format!("backends at exhaustion={}", still.len()));
        }
        let late: i64 = sqlx::query("SELECT count(*)::int8 FROM rev.parent WHERE id = 99")
            .fetch_one(&mut self.admin)
            .await
            .expect("admin late")
            .try_get(0)
            .expect("count");
        let oracle_after = read_oracle(&mut oracle_conn).await;
        if oracle_after.parent != oracle.parent || oracle_after.child != oracle.child {
            self.problem("oracle snapshot saw the writer".to_string());
        }
        if late != 1 {
            self.problem(format!("admin late rows={late}"));
        }
        self.note(format!("admin_sees_late_parent={late}"));
        let _ = oracle_conn.close().await;

        let finished = owner.finish().await.expect("main finish");
        if finished.generation != generation || finished.source_id != "rev_main" || finished.backend.pid != pid {
            self.problem("finish identity differs".to_string());
        }
        self.wait_gone(pid, &start).await;
        for (guard_pid, guard_start) in &backends {
            self.wait_gone(*guard_pid, guard_start).await;
        }
        self.note(format!("finish_released pid={pid}"));
        match SnapshotOwner::open(self.request("rev_main", generation, &["parent"]).await).await {
            Err(err) if err.fact() == Some("generation_reused") => {
                self.note("generation_reused".to_string());
            }
            Err(err) => self.problem(format!("reuse open fact={:?}", err.fact())),
            Ok(_) => self.problem("generation was reused".to_string()),
        }
    }

    async fn split_batch_bytes(&mut self) {
        let mut owner = self.open_at("rev_split", 1, &["split"]).await;
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        owner
            .open_stream(StreamBudget::try_new(8, 5, 64, 64).expect("split budget"))
            .await
            .expect("split stream");
        let mut values = Vec::new();
        let mut retained = Vec::new();
        let mut executes = Vec::new();
        let mut eof_rows = None;
        loop {
            match owner.next_batch().await.expect("split next") {
                StreamItem::Batch(batch) => {
                    retained.push(batch.accounting.retained_rows);
                    executes.push(batch.accounting.executes);
                    let column = batch
                        .record_batch
                        .column_by_name("body")
                        .expect("body")
                        .as_any()
                        .downcast_ref::<StringArray>()
                        .expect("body utf8");
                    for index in 0..batch.record_batch.num_rows() {
                        values.push(column.value(index).to_string());
                    }
                }
                StreamItem::TableEof(eof) => eof_rows = Some(eof.rows_emitted),
                StreamItem::Exhausted(_) => break,
            }
        }
        values.sort();
        self.note(format!(
            "split values={values:?} retained={retained:?} executes={executes:?} eof={eof_rows:?}"
        ));
        if values != ["aa".to_string(), "bbbb".to_string()] || eof_rows != Some(2) {
            self.problem(format!("split coverage values={values:?} eof={eof_rows:?}"));
        }
        if retained.first().copied() != Some(1) || values.len() < 2 {
            self.problem(format!("split did not hold the overflow row retained={retained:?}"));
        }
        if retained.len() != 2 {
            self.problem(format!("split batch count={}", retained.len()));
        }
        let _ = owner.finish().await;
        self.wait_gone(pid, &start).await;
    }

    async fn oversize_batch_bytes(&mut self) {
        let mut owner = self.open_at("rev_over", 1, &["oversize"]).await;
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        let backends = self.oo_backends().await;
        owner
            .open_stream(StreamBudget::try_new(8, 5, 64, 64).expect("over budget"))
            .await
            .expect("over stream");
        match owner.next_batch().await {
            Err(err) => {
                let shown = err.to_string();
                self.note(format!("oversize kind={:?} fact={:?} display={shown}", err.kind(), err.fact()));
                if err.kind() != SourceErrorKind::Budget || err.fact() != Some("max_bytes") || shown.contains("zzzzzz") {
                    self.problem(format!("oversize error kind={:?} fact={:?} display={shown}", err.kind(), err.fact()));
                }
                match owner.finish().await {
                    Err(finish_err) => {
                        self.note(format!("oversize_finish fact={:?}", finish_err.fact()));
                        if finish_err.kind() != SourceErrorKind::Budget {
                            self.problem(format!("oversize finish kind={:?}", finish_err.kind()));
                        }
                    }
                    Ok(_) => self.problem("oversize finish returned SnapshotFinished".to_string()),
                }
            }
            Ok(item) => {
                self.problem(format!("oversize emitted {}", item_name(&item)));
                let _ = owner.abort().await;
            }
        }
        self.wait_gone(pid, &start).await;
        for (guard_pid, guard_start) in &backends {
            self.wait_gone(*guard_pid, guard_start).await;
        }
        self.note(format!("oversize_released pid={pid}"));
    }

    async fn numeric_key_separate_from_probe(&mut self) {
        let mut owner = self.open_at("rev_num", 1, &["numkey"]).await;
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        owner
            .open_stream(StreamBudget::try_new(4, 1024, 256, 1024).expect("num budget"))
            .await
            .expect("num stream");
        let mut payload = None;
        let mut key_set = false;
        let mut digest_some = false;
        loop {
            match owner.next_batch().await.expect("num next") {
                StreamItem::Batch(batch) => {
                    key_set |= batch.typed.plans.iter().any(|plan: &ColumnPlan| plan.key);
                    digest_some |= batch.typed.rows.iter().any(|row| row.key_digest.is_some());
                    let column = batch
                        .record_batch
                        .column_by_name("payload")
                        .expect("payload")
                        .as_any()
                        .downcast_ref::<Int32Array>()
                        .expect("payload int32");
                    if column.len() == 1 && !column.is_null(0) {
                        payload = Some(column.value(0));
                    }
                }
                StreamItem::TableEof(_) => {}
                StreamItem::Exhausted(_) => break,
            }
        }
        self.note(format!("numkey payload={payload:?} plan_key={key_set} digest_some={digest_some}"));
        if payload != Some(42) || key_set || digest_some {
            self.problem(format!(
                "numeric key stream payload={payload:?} plan_key={key_set} digest_some={digest_some}"
            ));
        }
        let _ = owner.finish().await;
        self.wait_gone(pid, &start).await;

        let mut probe = self.open_at("rev_num", 2, &["numkey"]).await;
        let probe_pid = probe.binding().backend.pid;
        let probe_start = probe.binding().backend.backend_start.clone();
        let probed = probe
            .read_probe(CutProbe {
                schema: SCHEMA.to_string(),
                table: "numkey".to_string(),
                key: vec![ProbeValue::Int8(1)],
            })
            .await;
        match probed {
            Err(err) if err.fact() == Some("probe_key") => self.note("probe_key_on_numeric".to_string()),
            Err(err) => self.problem(format!("numeric probe fact={:?}", err.fact())),
            Ok(_) => self.problem("numeric probe returned a batch".to_string()),
        }
        let aborted = probe.abort().await.expect("probe abort");
        if aborted.generation != 2 {
            self.problem("probe abort generation".to_string());
        }
        self.wait_gone(probe_pid, &probe_start).await;
        self.note(format!("probe_abort_released pid={probe_pid}"));
    }

    async fn schema_guard(&mut self) {
        let mut owner = self.open_at("rev_schema", 1, &["schema_a", "schema_b"]).await;
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        let backends = self.oo_backends().await;
        owner
            .open_stream(StreamBudget::try_new(4, 1024, 256, 1024).expect("schema budget"))
            .await
            .expect("schema stream");
        let mut saw_a = false;
        loop {
            match owner.next_batch().await.expect("schema a") {
                StreamItem::Batch(batch) if batch.typed.table == "schema_a" => saw_a = true,
                StreamItem::TableEof(eof) if eof.table == "schema_a" => break,
                StreamItem::Batch(batch) => {
                    self.problem(format!("schema saw {} before eof", batch.typed.table));
                    break;
                }
                other => {
                    self.problem(format!("schema ended early {}", item_name(&other)));
                    break;
                }
            }
        }
        sqlx::query("ALTER TABLE rev.schema_b ADD COLUMN extra int4")
            .execute(&mut self.admin)
            .await
            .expect("alter schema_b");
        match owner.next_batch().await {
            Err(err) => {
                self.note(format!("schema_guard kind={:?} fact={:?}", err.kind(), err.fact()));
                if err.fact() != Some("schema_drift") {
                    self.problem(format!("schema guard fact={:?}", err.fact()));
                }
                match owner.finish().await {
                    Ok(_) => self.problem("schema drift finish returned SnapshotFinished".to_string()),
                    Err(finish_err) => self.note(format!("schema_finish fact={:?}", finish_err.fact())),
                }
            }
            Ok(item) => {
                self.problem(format!("schema drift still produced {}", item_name(&item)));
                let _ = owner.abort().await;
            }
        }
        if !saw_a {
            self.problem("schema_a produced no batch".to_string());
        }
        self.wait_gone(pid, &start).await;
        for (guard_pid, guard_start) in &backends {
            self.wait_gone(*guard_pid, guard_start).await;
        }
    }

    async fn acl_guard(&mut self) {
        let mut owner = self.open_at("rev_acl", 1, &["acl_a", "acl_b"]).await;
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        let backends = self.oo_backends().await;
        owner
            .open_stream(StreamBudget::try_new(4, 1024, 256, 1024).expect("acl budget"))
            .await
            .expect("acl stream");
        loop {
            match owner.next_batch().await.expect("acl a") {
                StreamItem::TableEof(eof) if eof.table == "acl_a" => break,
                StreamItem::Batch(_) => {}
                other => {
                    self.problem(format!("acl ended early {}", item_name(&other)));
                    break;
                }
            }
        }
        sqlx::query("REVOKE SELECT ON rev.acl_b FROM oo_a131_reader")
            .execute(&mut self.admin)
            .await
            .expect("revoke acl_b");
        match owner.next_batch().await {
            Err(err) => {
                self.note(format!("acl_guard kind={:?} fact={:?}", err.kind(), err.fact()));
                if err.kind() != SourceErrorKind::Privilege || err.fact() != Some("select_not_granted") {
                    self.problem(format!("acl guard kind={:?} fact={:?}", err.kind(), err.fact()));
                }
                match owner.finish().await {
                    Ok(_) => self.problem("acl failure finish returned SnapshotFinished".to_string()),
                    Err(finish_err) => self.note(format!("acl_finish fact={:?}", finish_err.fact())),
                }
            }
            Ok(item) => {
                self.problem(format!("acl revoke still produced {}", item_name(&item)));
                let _ = owner.abort().await;
            }
        }
        self.wait_gone(pid, &start).await;
        for (guard_pid, guard_start) in &backends {
            self.wait_gone(*guard_pid, guard_start).await;
        }
    }

    async fn drop_releases(&mut self) {
        let mut owner = self.open_at("rev_drop", 1, &["parent"]).await;
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        let backends = self.oo_backends().await;
        owner
            .open_stream(StreamBudget::try_new(1, 1024, 256, 1024).expect("drop budget"))
            .await
            .expect("drop stream");
        let first = owner.next_batch().await.expect("drop batch");
        if !matches!(first, StreamItem::Batch(_)) {
            self.problem("drop path did not get a batch".to_string());
        }
        drop(owner);
        self.wait_gone(pid, &start).await;
        for (guard_pid, guard_start) in &backends {
            self.wait_gone(*guard_pid, guard_start).await;
        }
        self.note(format!("drop_released pid={pid}"));
    }

    fn audit(&mut self) {
        let raw = fs::read_to_string(&self.pointer.log_file).expect("postgres log");
        let secrets = [
            self.reader_pass.clone(),
            self.writer_pass.clone(),
            self.super_pass.clone(),
        ];
        for secret in &secrets {
            if !secret.is_empty() && raw.contains(secret.as_str()) {
                self.problem("postgres log contains a password".to_string());
                return;
            }
        }
        let mut reader_sql = 0_usize;
        let mut savepoints = 0_usize;
        let mut releases = 0_usize;
        let mut rollbacks = 0_usize;
        let mut starts = 0_usize;
        let mut selects = 0_usize;
        let mut writes = 0_usize;
        let mut portal_state = "outside";
        let mut saw_select = false;
        let mut portal_order_bad = false;
        for line in raw.lines() {
            if !line.contains("app=oo-pg-source") || !line.contains("user=oo_a131_reader") {
                continue;
            }
            let Some(sql) = statement_sql(line) else { continue };
            reader_sql += 1;
            if write_word(&strip_sql_literals(sql)).is_some() {
                writes += 1;
                self.problem(format!("reader write sql={sql}"));
            }
            let upper = sql.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
            match upper.as_str() {
                "SELECT" => {
                    selects += 1;
                    if portal_state == "saved" {
                        saw_select = true;
                    }
                }
                "START" | "BEGIN" => starts += 1,
                "SAVEPOINT" if sql.contains("oo_stream_lock") => {
                    savepoints += 1;
                    portal_state = "saved";
                    saw_select = false;
                }
                "ROLLBACK" if sql.contains("oo_stream_lock") => {
                    rollbacks += 1;
                    if portal_state != "saved" || !saw_select {
                        portal_order_bad = true;
                    }
                    portal_state = "rolled";
                }
                "RELEASE" if sql.contains("oo_stream_lock") => {
                    releases += 1;
                    if portal_state != "rolled" {
                        portal_order_bad = true;
                    }
                    portal_state = "outside";
                }
                _ => {}
            }
        }
        self.note(format!(
            "sql_audit reader_sql={reader_sql} select={selects} start={starts} savepoint={savepoints} rollback_to={rollbacks} release={releases} reader_writes={writes} portal_order_bad={portal_order_bad}"
        ));
        if reader_sql == 0 || starts == 0 || writes != 0 || portal_order_bad {
            self.problem(format!(
                "sql audit reader_sql={reader_sql} starts={starts} writes={writes} portal_order_bad={portal_order_bad}"
            ));
        }
    }

    async fn writer_commits(&mut self) {
        let mut writer = dial(
            &self.pointer,
            &self.pointer.writer_user,
            &self.writer_pass,
            &self.ca,
            "oo-rev-writer",
        )
        .await;
        sqlx::query("INSERT INTO rev.parent (id, flag, label) VALUES (99, true, 'late')")
            .execute(&mut writer)
            .await
            .expect("writer parent");
        sqlx::query("INSERT INTO rev.child (parent_id, note) VALUES (99, 'late')")
            .execute(&mut writer)
            .await
            .expect("writer child");
        let _ = writer.close().await;
    }

    async fn hold(&mut self, pid: i64, start: &str) {
        let began = Instant::now();
        loop {
            match self.activity(pid, start).await {
                Some(row) if row.in_xact && waiting(&row) => return,
                Some(_) if began.elapsed() > Duration::from_secs(2) => return,
                None if began.elapsed() > Duration::from_secs(2) => return,
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    }

    async fn wait_gone(&mut self, pid: i64, start: &str) {
        let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
        let began = Instant::now();
        loop {
            match self.activity(pid, start).await {
                None => return,
                Some(row) if began.elapsed() > bound => {
                    self.problem(format!(
                        "backend {pid} still {} in_xact={} after {}ms",
                        row.state,
                        row.in_xact,
                        began.elapsed().as_millis()
                    ));
                    return;
                }
                Some(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }

    async fn activity(&mut self, pid: i64, start: &str) -> Option<Activity> {
        let pid_i = i32::try_from(pid).ok()?;
        let row = sqlx::query(
            "SELECT state, COALESCE(wait_event, ''), xact_start IS NOT NULL, \
             COALESCE(xact_start::text, ''), COALESCE(backend_xmin::text, ''), \
             COALESCE(left(query, 180), ''), application_name \
             FROM pg_catalog.pg_stat_activity \
             WHERE pid = $1 AND backend_start::text = $2",
        )
        .bind(pid_i)
        .bind(start)
        .fetch_optional(&mut self.admin)
        .await
        .expect("activity");
        row.map(|row| Activity {
            state: row.try_get(0).unwrap_or_default(),
            wait_event: row.try_get(1).unwrap_or_default(),
            in_xact: row.try_get(2).unwrap_or(false),
            xact_start: row.try_get(3).unwrap_or_default(),
            xmin: row.try_get(4).unwrap_or_default(),
            query: row.try_get(5).unwrap_or_default(),
            app: row.try_get(6).unwrap_or_default(),
        })
    }

    async fn oo_backends(&mut self) -> Vec<(i64, String)> {
        let rows = sqlx::query(
            "SELECT pid::int8, backend_start::text FROM pg_catalog.pg_stat_activity \
             WHERE application_name = 'oo-pg-source' AND usename::text = $1",
        )
        .bind(&self.pointer.reader_user)
        .fetch_all(&mut self.admin)
        .await
        .expect("backends");
        rows.iter()
            .map(|row| (row.try_get(0).unwrap_or(0), row.try_get(1).unwrap_or_default()))
            .collect()
    }
}

async fn load_admitted(admin: &mut PgConnection, names: &[&str]) -> AdmittedCatalog {
    let mut tables = Vec::new();
    for name in names {
        let rows = sqlx::query(
            "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull \
             FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'rev' AND c.relname = $1 AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
        )
        .bind(*name)
        .fetch_all(&mut *admin)
        .await
        .expect("admitted columns");
        let mut columns = Vec::new();
        for row in &rows {
            columns.push(AdmittedColumn {
                attnum: row.try_get(0).unwrap(),
                name: row.try_get(1).unwrap(),
                type_oid: row.try_get(2).unwrap(),
                typmod: row.try_get(3).unwrap(),
                not_null: row.try_get(4).unwrap(),
            });
        }
        let keys = sqlx::query(
            "SELECT a.attname::text \
             FROM pg_catalog.pg_index i \
             JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS u(attnum, ord) ON true \
             JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = u.attnum \
             WHERE n.nspname = 'rev' AND c.relname = $1 AND i.indisprimary \
             ORDER BY u.ord",
        )
        .bind(*name)
        .fetch_all(&mut *admin)
        .await
        .expect("admitted keys");
        let primary_key = keys
            .iter()
            .map(|row| row.try_get::<String, _>(0).unwrap())
            .collect();
        tables.push(AdmittedTable {
            schema: SCHEMA.to_string(),
            name: (*name).to_string(),
            columns,
            primary_key,
        });
    }
    AdmittedCatalog { tables }
}

fn find_eof<'a>(name: &str, eofs: &'a [oo_connectors::TableEof]) -> Option<&'a oo_connectors::TableEof> {
    eofs.iter().find(|item| item.table == name)
}

fn plan_key_named(name: &'static str) -> impl Fn(&ColumnPlan) -> bool {
    move |plan| plan.key && plan.name == name
}

fn waiting(row: &Activity) -> bool {
    row.app == "oo-pg-source"
        && (row.wait_event == "ClientRead" || row.state == "idle in transaction")
}

fn item_name(item: &StreamItem) -> &'static str {
    match item {
        StreamItem::Batch(_) => "batch",
        StreamItem::TableEof(_) => "table_eof",
        StreamItem::Exhausted(_) => "exhausted",
    }
}

fn assert_field_types(batch: &RecordBatch, table: &str) {
    match table {
        "parent" => {
            assert_eq!(batch.schema().field_with_name("id").unwrap().data_type(), &DataType::Int32);
            assert_eq!(batch.schema().field_with_name("flag").unwrap().data_type(), &DataType::Boolean);
            assert_eq!(batch.schema().field_with_name("label").unwrap().data_type(), &DataType::Utf8);
        }
        "child" => {
            assert_eq!(batch.schema().field_with_name("parent_id").unwrap().data_type(), &DataType::Int32);
            assert_eq!(batch.schema().field_with_name("note").unwrap().data_type(), &DataType::Utf8);
        }
        "zerocol" => assert_eq!(batch.num_columns(), 0),
        _ => {}
    }
}

fn parent_arrow(batch: &RecordBatch) -> Vec<ParentRow> {
    let ids = batch.column_by_name("id").unwrap().as_any().downcast_ref::<Int32Array>().unwrap();
    let flags = batch.column_by_name("flag").unwrap().as_any().downcast_ref::<BooleanArray>().unwrap();
    let labels = batch.column_by_name("label").unwrap().as_any().downcast_ref::<StringArray>().unwrap();
    let mut rows = Vec::with_capacity(batch.num_rows());
    for index in 0..batch.num_rows() {
        let label = if labels.is_null(index) { None } else { Some(labels.value(index).to_string()) };
        rows.push((ids.value(index), flags.value(index), label));
    }
    rows
}

fn child_arrow(batch: &RecordBatch) -> Vec<ChildRow> {
    let ids = batch.column_by_name("parent_id").unwrap().as_any().downcast_ref::<Int32Array>().unwrap();
    let notes = batch.column_by_name("note").unwrap().as_any().downcast_ref::<StringArray>().unwrap();
    let mut rows = Vec::with_capacity(batch.num_rows());
    for index in 0..batch.num_rows() {
        let id = if ids.is_null(index) { None } else { Some(ids.value(index)) };
        let note = if notes.is_null(index) { None } else { Some(notes.value(index).to_string()) };
        rows.push((id, note));
    }
    rows
}

fn sort_parent(rows: &mut [ParentRow]) {
    rows.sort_by_key(|row| row.0);
}

fn sort_child(rows: &mut [ChildRow]) {
    rows.sort();
}

fn oracle_payload(oracle: &Oracle) -> u64 {
    let parent: u64 = oracle
        .parent
        .iter()
        .map(|(_, _, label)| 4 + 1 + label.as_ref().map(|text| text.len() as u64).unwrap_or(0))
        .sum();
    let child: u64 = oracle
        .child
        .iter()
        .map(|(id, note)| id.map(|_| 4_u64).unwrap_or(0) + note.as_ref().map(|text| text.len() as u64).unwrap_or(0))
        .sum();
    let zero = u64::try_from(oracle.zero).unwrap_or(0) * 4;
    parent + child + zero
}

async fn read_oracle(conn: &mut PgConnection) -> Oracle {
    let parent_rows = sqlx::query("SELECT id, flag, label FROM rev.parent ORDER BY id")
        .fetch_all(&mut *conn)
        .await
        .expect("oracle parent");
    let mut parent = Vec::new();
    for row in &parent_rows {
        parent.push((row.try_get(0).unwrap(), row.try_get(1).unwrap(), row.try_get(2).unwrap()));
    }
    let child_rows = sqlx::query(
        "SELECT parent_id, note FROM rev.child ORDER BY parent_id NULLS FIRST, note NULLS FIRST",
    )
    .fetch_all(&mut *conn)
    .await
    .expect("oracle child");
    let mut child = Vec::new();
    for row in &child_rows {
        child.push((row.try_get(0).unwrap(), row.try_get(1).unwrap()));
    }
    let empty: i64 = sqlx::query("SELECT count(*)::int8 FROM rev.empty")
        .fetch_one(&mut *conn)
        .await
        .expect("oracle empty")
        .try_get(0)
        .unwrap();
    let zero: i64 = sqlx::query("SELECT count(*)::int8 FROM rev.zerocol")
        .fetch_one(&mut *conn)
        .await
        .expect("oracle zero")
        .try_get(0)
        .unwrap();
    Oracle { parent, child, empty, zero }
}

fn read_pass(dir: &str, name: &str) -> Zeroizing<String> {
    Zeroizing::new(fs::read_to_string(format!("{dir}/{name}")).expect("password").trim().to_string())
}

async fn dial(pointer: &Pointer, user: &str, password: &str, ca: &[u8], app: &str) -> PgConnection {
    let options = PgConnectOptions::from_explicit()
        .host(&pointer.host)
        .port(pointer.port)
        .username(user)
        .password(password)
        .database(&pointer.database)
        .application_name(app)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    options.disable_statement_logging().connect().await.expect("dial")
}

async fn node_of(admin: &mut PgConnection) -> (String, bool) {
    let row = sqlx::query(
        "SELECT pg_catalog.pg_postmaster_start_time()::text, pg_catalog.pg_is_in_recovery()",
    )
    .fetch_one(&mut *admin)
    .await
    .expect("node");
    (row.try_get(0).unwrap(), row.try_get(1).unwrap())
}

fn reader_lines(path: &str) -> usize {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("app=oo-pg-source") && line.contains("user=oo_a131_reader"))
        .count()
}

fn strip_sql_literals(sql: &str) -> String {
    let mut out = String::new();
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\'' {
            while let Some(inner) = chars.next() {
                if inner == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                        continue;
                    }
                    break;
                }
            }
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

fn write_word(sql: &str) -> Option<&'static str> {
    const WORDS: &[&str] = &[
        "INSERT", "UPDATE", "DELETE", "TRUNCATE", "MERGE", "COPY", "CREATE", "ALTER", "DROP",
        "GRANT", "REVOKE", "COMMENT", "VACUUM", "CLUSTER", "REINDEX", "CALL", "DO", "COMMIT",
    ];
    for token in sql.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_') {
        let upper = token.to_ascii_uppercase();
        if let Some(word) = WORDS.iter().copied().find(|word| *word == upper) {
            return Some(word);
        }
    }
    None
}

fn statement_sql(line: &str) -> Option<&str> {
    if let Some(rest) = line.split_once("statement:") {
        return Some(rest.1.trim());
    }
    if let Some(rest) = line.split_once("execute ") {
        return rest.1.split_once(':').map(|(_, sql)| sql.trim());
    }
    None
}
