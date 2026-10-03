//! Public-API oracle for the current-catalog guard. Evidence overlay only.
//! Old and fresh row sets are hashed before the owner opens. Attribute numbers
//! come from the server.

use std::collections::BTreeMap;
use std::fs;
use std::time::{Duration, Instant};

use oo_connectors::types::ArrowScalar;
use oo_connectors::{
    config_identity, type_policy_identity, AdmittedCatalog, AdmittedColumn, AdmittedTable,
    CutProbe, PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSession, PgSourceConfig,
    PgTlsConfig, ProbeValue, SecretRef, SelectedTable, SnapshotOwner, SnapshotRequest, SourceId,
    TableSelection, CLEANUP_OBSERVATION_BOUND_MS, SourceError, SourceErrorKind,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};
use zeroize::Zeroizing;

const FRESH_TITLE_A: &str = "spruce";
const FRESH_TITLE_B: &str = "cedar";

#[derive(Deserialize)]
struct Pointer {
    host: String,
    port: u16,
    database: String,
    password_dir: String,
    ca_file: String,
    wrong_ca_file: String,
    log_file: String,
    super_user: String,
    lot_reader_user: String,
    lot_writer_user: String,
    lot_reader_password_file: String,
    lot_writer_password_file: String,
    super_password_file: String,
}

struct LiveBackend {
    pid: i64,
    start: String,
    state: String,
}

struct World {
    pointer: Pointer,
    lot_reader_pass: Zeroizing<String>,
    lot_writer_pass: Zeroizing<String>,
    super_pass: Zeroizing<String>,
    ca: Vec<u8>,
    wrong_ca: Vec<u8>,
    admin: PgConnection,
    postmaster: String,
    in_recovery: bool,
    generations: BTreeMap<String, u64>,
    outcomes: Vec<String>,
    outcome_path: Option<String>,
    sql_path: Option<String>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a125_oracle() {
    assert_eq!(
        sha256_hex(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    let mut world = World::start().await;
    world.rate().await;
    world.cut().await;
    world.index_rename().await;
    world.cap().await;
    world.cleanup_paths().await;
    world.terminate_guard().await;
    world.wrong_ca().await;
    world.principal_and_database().await;
    world.bad_postmaster().await;
    world.deadline_before_open().await;
    world.deadline_before_finish().await;
    world.other_selected_table().await;
    world.add_column_and_fresh().await;
    world.probe_then_ddl().await;
    world.after_last_probe().await;
    world.zero_probe_finish().await;
    world.column_and_key_drift().await;
    world.recreate_relation().await;
    world.audit();
    world.note("oracle_complete".to_string());
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(path) = &self.outcome_path {
            let _ = fs::write(path, self.outcomes.join("\n"));
        }
    }
}

impl World {
    async fn start() -> Self {
        let path = std::env::var("OO_A125_ORACLE_POINTER").expect("OO_A125_ORACLE_POINTER");
        let pointer: Pointer = serde_json::from_str(&fs::read_to_string(&path).expect("pointer"))
            .expect("pointer json");
        let lot_reader_pass = read_pass(&pointer.password_dir, &pointer.lot_reader_password_file);
        let lot_writer_pass = read_pass(&pointer.password_dir, &pointer.lot_writer_password_file);
        let super_pass = read_pass(&pointer.password_dir, &pointer.super_password_file);
        let ca = fs::read(&pointer.ca_file).expect("ca");
        let wrong_ca = fs::read(&pointer.wrong_ca_file).expect("wrong ca");
        let mut admin = dial(
            &pointer,
            &pointer.super_user,
            &super_pass,
            &ca,
            "oo-a125-admin",
        )
        .await;
        let row = sqlx::query(
            "SELECT pg_catalog.pg_is_in_recovery(), pg_catalog.pg_postmaster_start_time()::text, \
             current_setting('server_version')",
        )
        .fetch_one(&mut admin)
        .await
        .expect("node");
        let in_recovery: bool = row.try_get(0).expect("recovery");
        let postmaster: String = row.try_get(1).expect("postmaster");
        let version: String = row.try_get(2).expect("version");
        let outcome_path = std::env::var("OO_A125_ORACLE_OUTCOME").ok();
        let sql_path = std::env::var("OO_A125_ORACLE_SQL").ok();
        let mut world = Self {
            pointer,
            lot_reader_pass,
            lot_writer_pass,
            super_pass,
            ca,
            wrong_ca,
            admin,
            postmaster,
            in_recovery,
            generations: BTreeMap::new(),
            outcomes: Vec::new(),
            outcome_path,
            sql_path,
        };
        world.note(format!("server_version={version}"));
        world.note(format!("in_recovery={in_recovery}"));
        world
    }

    fn note(&mut self, line: String) {
        eprintln!("outcome {line}");
        self.outcomes.push(line);
    }

    fn next_gen(&mut self, source: &str) -> u64 {
        let slot = self.generations.entry(source.to_string()).or_insert(0);
        *slot += 1;
        *slot
    }

    async fn window(&mut self) {
        tokio::time::sleep(Duration::from_millis(1100)).await;
    }

    async fn rate(&mut self) {
        self.marker("A125_ORACLE_RATE").await;
        let tables = [("lot", "batch")];
        let admitted = self.load_admitted(&tables).await;
        let before = self.source_backends().await.len();
        let err = self
            .open_exact(
                "lotrate",
                1,
                &tables,
                admitted.clone(),
                50_000,
                Instant::now() + Duration::from_secs(50),
                &self.pointer.lot_reader_user.clone(),
                &self.pointer.database.clone(),
                self.in_recovery,
                &self.postmaster.clone(),
                &self.ca.clone(),
            )
            .await
            .expect_err("rate open");
        assert_eq!(err.fact(), Some("max_rows_per_sec"), "{err}");
        assert_eq!(err.kind(), SourceErrorKind::Budget, "{err}");
        assert!(err.sqlstate().is_none(), "{err}");
        self.wait_count(before).await;
        let log = self.log_after("A125_ORACLE_RATE");
        assert!(
            !log.contains("pg_get_keywords"),
            "keywords query was sent after the row budget was exhausted"
        );
        assert!(
            !log.contains("transaction_timestamp"),
            "captured_at ran after the keyword charge refused"
        );
        assert!(
            log.contains("pg_is_in_recovery")
                && log.contains("REPEATABLE READ")
                && log.contains("READ COMMITTED")
                && log.contains("ROLLBACK"),
            "rate refusal was not after both transactions and the node read"
        );
        let reused = self
            .open_exact(
                "lotrate",
                1,
                &tables,
                admitted.clone(),
                1_000,
                Instant::now() + Duration::from_secs(50),
                &self.pointer.lot_reader_user.clone(),
                &self.pointer.database.clone(),
                self.in_recovery,
                &self.postmaster.clone(),
                &self.ca.clone(),
            )
            .await
            .expect_err("rate generation reuse");
        assert_eq!(reused.fact(), Some("generation_reused"), "{reused}");
        let still = self.log_after("A125_ORACLE_RATE");
        assert!(!still.contains("pg_get_keywords"));
        self.note(
            "rate_refused=max_rows_per_sec keywords_sent=no boundary=after_read_node_before_keywords"
                .to_string(),
        );
        self.window().await;
        let mut owner = self
            .open_exact(
                "lotrate",
                2,
                &tables,
                admitted,
                1_000,
                Instant::now() + Duration::from_secs(50),
                &self.pointer.lot_reader_user.clone(),
                &self.pointer.database.clone(),
                self.in_recovery,
                &self.postmaster.clone(),
                &self.ca.clone(),
            )
            .await
            .expect("rate window refilled");
        let pair = self.pair(&owner).await;
        let _ = owner
            .read_probe(key("lot", "batch", ProbeValue::Int8(11)))
            .await
            .expect("rate probe");
        owner.finish().await.expect("rate finish");
        self.wait_gone(&[pair.0, pair.1]).await;
        assert!(self.log_after("A125_ORACLE_RATE").contains("pg_get_keywords"));
        self.note("rate_refilled=open_ok".to_string());
    }

    async fn cut(&mut self) {
        let (title, ready, code) = self.batch_row().await;
        let qty = self.child_qty(31).await;
        let qty32 = self.child_qty(32).await;
        let fresh_title = if title == FRESH_TITLE_A {
            FRESH_TITLE_B
        } else {
            FRESH_TITLE_A
        };
        let fresh_qty: i16 = if qty == 4 { 7 } else { 4 };
        assert_ne!(fresh_title, title);
        assert_ne!(fresh_qty, qty);
        let registered = format!(
            "old id=11 title={title} ready={ready} code={code}\n\
             old child id=31 qty={qty}\n\
             old child id=32 qty={qty32}\n\
             fresh id=11 title={fresh_title}\n\
             fresh child id=31 qty={fresh_qty}\n"
        );
        let registered_hash = sha256_hex(&registered);
        self.note(format!("preregistered_sha256={registered_hash}"));
        let tick_before = self.tick_hits().await;
        let tables = [("lot", "batch"), ("lot", "child row")];
        let mut owner = self
            .open_lot("lotcut", &tables, 1_000)
            .await
            .expect("cut open");
        let rendered = format!("{owner:?}");
        assert!(rendered.contains("guard_open: true"), "{rendered}");
        assert!(!rendered.contains(self.lot_reader_pass.as_str()));
        assert!(!rendered.contains(self.lot_writer_pass.as_str()));
        let live = self.source_backends().await;
        assert_eq!(live.len(), 2, "owner should hold data and guard");
        assert!(
            live.iter().all(|row| row.state == "idle in transaction"),
            "backends not idle in transaction"
        );
        let pair = self.pair(&owner).await;
        self.note(format!(
            "cut_pids data={} guard={}",
            pair.0.0, pair.1.0
        ));
        let parent = owner
            .read_probe(key("lot", "batch", ProbeValue::Int8(11)))
            .await
            .expect("cut parent");
        assert_eq!(utf8_at(&parent, "title"), title);
        assert_eq!(bool_at(&parent, "ready"), ready);
        assert_eq!(utf8_at(&parent, "code"), code);
        let child = owner
            .read_probe(key("lot", "child row", ProbeValue::Int8(31)))
            .await
            .expect("cut child");
        assert_eq!(i16_at(&child, "qty"), qty);
        let other = owner
            .read_probe(key("lot", "child row", ProbeValue::Int8(32)))
            .await
            .expect("cut child 32");
        assert_eq!(i16_at(&other, "qty"), qty32);
        let tick_held = self.tick_hits().await;
        assert_eq!(tick_held, tick_before, "owner probes changed lot.tick");
        self.writer_update(fresh_title, fresh_qty).await;
        let again = owner
            .read_probe(key("lot", "batch", ProbeValue::Int8(11)))
            .await
            .expect("old cut parent");
        assert_eq!(utf8_at(&again, "title"), title);
        assert_eq!(again.rows[0].canonical_hash, parent.rows[0].canonical_hash);
        let child_again = owner
            .read_probe(key("lot", "child row", ProbeValue::Int8(31)))
            .await
            .expect("old cut child");
        assert_eq!(i16_at(&child_again, "qty"), qty);
        let other_again = owner
            .read_probe(key("lot", "child row", ProbeValue::Int8(32)))
            .await
            .expect("old cut child 32");
        assert_eq!(i16_at(&other_again, "qty"), qty32);
        let tick_after_writer = self.tick_hits().await;
        assert_eq!(tick_after_writer - tick_held, 2, "writer trigger delta");
        let finished = owner.finish().await.expect("cut finish");
        self.wait_gone(&[pair.0, pair.1]).await;
        self.note(format!(
            "cut_old_finished generation={} hash={}",
            finished.generation, registered_hash
        ));
        self.window().await;
        let mut fresh = self
            .open_lot("lotcut", &tables, 1_000)
            .await
            .expect("fresh open");
        let seen = fresh
            .read_probe(key("lot", "batch", ProbeValue::Int8(11)))
            .await
            .expect("fresh parent");
        assert_eq!(utf8_at(&seen, "title"), fresh_title);
        assert_ne!(seen.rows[0].canonical_hash, parent.rows[0].canonical_hash);
        let seen_qty = fresh
            .read_probe(key("lot", "child row", ProbeValue::Int8(31)))
            .await
            .expect("fresh child");
        assert_eq!(i16_at(&seen_qty, "qty"), fresh_qty);
        let seen_32 = fresh
            .read_probe(key("lot", "child row", ProbeValue::Int8(32)))
            .await
            .expect("fresh child 32");
        assert_eq!(i16_at(&seen_32, "qty"), qty32);
        fresh.finish().await.expect("fresh finish");
        self.note(format!("cut_fresh_title={fresh_title} cut_fresh_qty={fresh_qty}"));
    }

    async fn index_rename(&mut self) {
        let tables = [("lot", "batch")];
        let (title, _, _) = self.batch_row().await;
        let mut owner = self
            .open_lot("lotidx", &tables, 1_000)
            .await
            .expect("index open");
        let old_index = self.pk_index("lot", "batch").await;
        let (sql, new_index) = match old_index.as_str() {
            "batch_pkey" => (
                "ALTER INDEX lot.batch_pkey RENAME TO batch_pk_a125",
                "batch_pk_a125",
            ),
            "batch_pk_a125" => (
                "ALTER INDEX lot.batch_pk_a125 RENAME TO batch_pkey",
                "batch_pkey",
            ),
            other => panic!("unexpected primary index {other}"),
        };
        self.exec(sql).await;
        let batch = owner
            .read_probe(key("lot", "batch", ProbeValue::Int8(11)))
            .await
            .expect("index probe");
        assert_eq!(utf8_at(&batch, "title"), title);
        owner.finish().await.expect("index finish");
        let now = self.pk_index("lot", "batch").await;
        assert_eq!(now, new_index);
        self.note(format!("index_rename {old_index} -> {new_index} finish=ok"));
    }

    async fn cap(&mut self) {
        let tables = [("lot", "batch")];
        let admitted = self.load_admitted(&tables).await;
        let mut holders = Vec::new();
        for _ in 0..3 {
            holders.push(self.connect_holder("lotcap").await);
        }
        self.wait_count(3).await;
        let before = self.backend_pids().await;
        let err = self
            .open_exact(
                "lotcap",
                41,
                &tables,
                admitted.clone(),
                1_000,
                Instant::now() + Duration::from_secs(50),
                &self.pointer.lot_reader_user.clone(),
                &self.pointer.database.clone(),
                self.in_recovery,
                &self.postmaster.clone(),
                &self.ca.clone(),
            )
            .await
            .expect_err("cap");
        assert_eq!(err.fact(), Some("max_connections"), "{err}");
        assert_eq!(self.backend_pids().await, before, "cap refusal opened a backend");
        drop(holders.remove(0));
        self.wait_count(2).await;
        let owner = self
            .open_exact(
                "lotcap",
                41,
                &tables,
                admitted.clone(),
                1_000,
                Instant::now() + Duration::from_secs(50),
                &self.pointer.lot_reader_user.clone(),
                &self.pointer.database.clone(),
                self.in_recovery,
                &self.postmaster.clone(),
                &self.ca.clone(),
            )
            .await
            .expect("generation 41 was consumed by the cap refusal");
        self.wait_count(4).await;
        let blocked = self
            .open_exact(
                "lotcap",
                42,
                &tables,
                admitted.clone(),
                1_000,
                Instant::now() + Duration::from_secs(50),
                &self.pointer.lot_reader_user.clone(),
                &self.pointer.database.clone(),
                self.in_recovery,
                &self.postmaster.clone(),
                &self.ca.clone(),
            )
            .await
            .expect_err("fifth socket");
        assert_eq!(blocked.fact(), Some("max_connections"), "{blocked}");
        assert_eq!(self.source_backends().await.len(), 4);
        let pair = self.pair_among(&owner, 4).await;
        owner.finish().await.expect("cap finish");
        self.wait_gone(&[pair.0, pair.1]).await;
        self.wait_count(2).await;
        let recovered = self
            .open_exact(
                "lotcap",
                42,
                &tables,
                admitted,
                1_000,
                Instant::now() + Duration::from_secs(50),
                &self.pointer.lot_reader_user.clone(),
                &self.pointer.database.clone(),
                self.in_recovery,
                &self.postmaster.clone(),
                &self.ca.clone(),
            )
            .await
            .expect("generation 42 was consumed by the cap refusal");
        recovered.finish().await.expect("cap recovered finish");
        drop(holders);
        self.wait_count(0).await;
        self.note("cap_before_connect=max_connections generation_kept=41,42".to_string());
    }

    async fn cleanup_paths(&mut self) {
        let tables = [("lot", "batch")];
        let owner = self
            .open_lot("lotclean", &tables, 1_000)
            .await
            .expect("finish open");
        let pair = self.pair(&owner).await;
        let finished = owner.finish().await.expect("c7 finish");
        self.wait_gone(&[pair.0.clone(), pair.1.clone()]).await;
        self.wait_count(0).await;
        self.note(format!("c7_finish generation={}", finished.generation));
        self.window().await;
        let owner = self
            .open_lot("lotclean", &tables, 1_000)
            .await
            .expect("abort open");
        let pair = self.pair(&owner).await;
        let aborted = owner.abort().await.expect("c7 abort");
        self.wait_gone(&[pair.0, pair.1]).await;
        self.wait_count(0).await;
        self.note(format!("c7_abort generation={}", aborted.generation));
        self.window().await;
        let owner = self
            .open_lot("lotclean", &tables, 1_000)
            .await
            .expect("drop open");
        let pair = self.pair(&owner).await;
        drop(owner);
        self.wait_gone(&[pair.0, pair.1]).await;
        self.wait_count(0).await;
        self.note("c7_drop=released".to_string());
        self.prove_slots("lotclean", &tables).await;
    }

    async fn terminate_guard(&mut self) {
        let tables = [("lot", "batch")];
        let mut owner = self
            .open_lot("lotkill", &tables, 1_000)
            .await
            .expect("terminate open");
        let pair = self.pair(&owner).await;
        let guard_pid = i32::try_from(pair.1.0).expect("guard pid");
        let terminated: bool = sqlx::query("SELECT pg_catalog.pg_terminate_backend($1)")
            .bind(guard_pid)
            .fetch_one(&mut self.admin)
            .await
            .expect("terminate")
            .try_get(0)
            .expect("terminate bool");
        assert!(terminated);
        let err = owner
            .read_probe(key("lot", "batch", ProbeValue::Int8(11)))
            .await
            .expect_err("terminated probe");
        assert_ne!(err.fact(), Some("schema_drift"), "{err}");
        assert!(
            matches!(err.kind(), SourceErrorKind::Connect | SourceErrorKind::Protocol),
            "{err}"
        );
        self.note(format!(
            "c8_probe kind={} fact={} sqlstate={}",
            kind_name(err.kind()),
            err.fact().unwrap_or("none"),
            err.sqlstate().unwrap_or("none")
        ));
        let finished = owner.finish().await;
        assert!(finished.is_err(), "terminate returned SnapshotFinished");
        let finish_err = finished.expect_err("finish err");
        assert!(
            matches!(
                finish_err.kind(),
                SourceErrorKind::Connect | SourceErrorKind::Protocol
            ),
            "{finish_err}"
        );
        self.note(format!(
            "c8_finish fact={}",
            finish_err.fact().unwrap_or("none")
        ));
        self.wait_gone(&[pair.0, pair.1]).await;
        self.wait_count(0).await;
        self.prove_slots("lotkill", &tables).await;
    }

    async fn wrong_ca(&mut self) {
        let tables = [("lot", "batch")];
        let admitted = self.load_admitted(&tables).await;
        let generation = self.next_gen("lotca");
        let before = self.source_backends().await.len();
        let err = self
            .open_exact(
                "lotca",
                generation,
                &tables,
                admitted.clone(),
                1_000,
                Instant::now() + Duration::from_secs(30),
                &self.pointer.lot_reader_user.clone(),
                &self.pointer.database.clone(),
                self.in_recovery,
                &self.postmaster.clone(),
                &self.wrong_ca.clone(),
            )
            .await
            .expect_err("wrong ca");
        assert_eq!(err.kind(), SourceErrorKind::Tls, "{err}");
        self.note(format!("c9_fact={}", err.fact().unwrap_or("none")));
        self.wait_count(before).await;
        let reused = self
            .open_exact(
                "lotca",
                generation,
                &tables,
                admitted,
                1_000,
                Instant::now() + Duration::from_secs(30),
                &self.pointer.lot_reader_user.clone(),
                &self.pointer.database.clone(),
                self.in_recovery,
                &self.postmaster.clone(),
                &self.ca.clone(),
            )
            .await
            .expect_err("ca generation reuse");
        assert_eq!(reused.fact(), Some("generation_reused"), "{reused}");
        self.prove_slots("lotca", &tables).await;
    }

    async fn principal_and_database(&mut self) {
        let tables = [("lot", "batch")];
        let admitted = self.load_admitted(&tables).await;
        let generation = self.next_gen("lotprin");
        let err = self
            .open_exact(
                "lotprin",
                generation,
                &tables,
                admitted.clone(),
                1_000,
                Instant::now() + Duration::from_secs(30),
                "not_lot",
                &self.pointer.database.clone(),
                self.in_recovery,
                &self.postmaster.clone(),
                &self.ca.clone(),
            )
            .await
            .expect_err("principal");
        assert_eq!(err.fact(), Some("principal"), "{err}");
        self.wait_count(0).await;
        let reused = self
            .open_exact(
                "lotprin",
                generation,
                &tables,
                admitted.clone(),
                1_000,
                Instant::now() + Duration::from_secs(30),
                &self.pointer.lot_reader_user.clone(),
                &self.pointer.database.clone(),
                self.in_recovery,
                &self.postmaster.clone(),
                &self.ca.clone(),
            )
            .await
            .expect_err("principal reuse");
        assert_eq!(reused.fact(), Some("generation_reused"), "{reused}");
        self.note("c10_principal=principal".to_string());
        self.prove_slots("lotprin", &tables).await;
        let admitted = self.load_admitted(&tables).await;
        let generation = self.next_gen("lotdb");
        let err = self
            .open_exact(
                "lotdb",
                generation,
                &tables,
                admitted.clone(),
                1_000,
                Instant::now() + Duration::from_secs(30),
                &self.pointer.lot_reader_user.clone(),
                "not_snap",
                self.in_recovery,
                &self.postmaster.clone(),
                &self.ca.clone(),
            )
            .await
            .expect_err("database");
        assert_eq!(err.fact(), Some("database"), "{err}");
        self.wait_count(0).await;
        let reused = self
            .open_exact(
                "lotdb",
                generation,
                &tables,
                admitted,
                1_000,
                Instant::now() + Duration::from_secs(30),
                &self.pointer.lot_reader_user.clone(),
                &self.pointer.database.clone(),
                self.in_recovery,
                &self.postmaster.clone(),
                &self.ca.clone(),
            )
            .await
            .expect_err("database reuse");
        assert_eq!(reused.fact(), Some("generation_reused"), "{reused}");
        self.note("c10_database=database".to_string());
        self.prove_slots("lotdb", &tables).await;
    }

    async fn bad_postmaster(&mut self) {
        let tables = [("lot", "batch")];
        let admitted = self.load_admitted(&tables).await;
        let generation = self.next_gen("lotnode");
        let err = self
            .open_exact(
                "lotnode",
                generation,
                &tables,
                admitted.clone(),
                1_000,
                Instant::now() + Duration::from_secs(30),
                &self.pointer.lot_reader_user.clone(),
                &self.pointer.database.clone(),
                self.in_recovery,
                "not-the-postmaster",
                &self.ca.clone(),
            )
            .await
            .expect_err("postmaster");
        assert_eq!(err.fact(), Some("read_node"), "{err}");
        self.wait_count(0).await;
        let reused = self
            .open_exact(
                "lotnode",
                generation,
                &tables,
                admitted,
                1_000,
                Instant::now() + Duration::from_secs(30),
                &self.pointer.lot_reader_user.clone(),
                &self.pointer.database.clone(),
                self.in_recovery,
                &self.postmaster.clone(),
                &self.ca.clone(),
            )
            .await
            .expect_err("postmaster reuse");
        assert_eq!(reused.fact(), Some("generation_reused"), "{reused}");
        self.note("c11=read_node".to_string());
        self.prove_slots("lotnode", &tables).await;
    }

    async fn deadline_before_open(&mut self) {
        let tables = [("lot", "batch")];
        let admitted = self.load_admitted(&tables).await;
        let generation = self.next_gen("lotdead");
        let before = self.source_backends().await.len();
        let err = self
            .open_exact(
                "lotdead",
                generation,
                &tables,
                admitted.clone(),
                1_000,
                Instant::now(),
                &self.pointer.lot_reader_user.clone(),
                &self.pointer.database.clone(),
                self.in_recovery,
                &self.postmaster.clone(),
                &self.ca.clone(),
            )
            .await
            .expect_err("stale deadline");
        assert_eq!(err.fact(), Some("deadline"), "{err}");
        assert_eq!(self.source_backends().await.len(), before);
        let owner = self
            .open_exact(
                "lotdead",
                generation,
                &tables,
                admitted,
                1_000,
                Instant::now() + Duration::from_secs(50),
                &self.pointer.lot_reader_user.clone(),
                &self.pointer.database.clone(),
                self.in_recovery,
                &self.postmaster.clone(),
                &self.ca.clone(),
            )
            .await
            .expect("deadline consumed the generation");
        owner.finish().await.expect("deadline recovery");
        self.wait_count(0).await;
        self.note("c12=deadline_before_connect".to_string());
    }

    async fn deadline_before_finish(&mut self) {
        let tables = [("lot", "batch")];
        let admitted = self.load_admitted(&tables).await;
        let generation = self.next_gen("lotexp");
        let owner = self
            .open_exact(
                "lotexp",
                generation,
                &tables,
                admitted,
                1_000,
                Instant::now() + Duration::from_secs(15),
                &self.pointer.lot_reader_user.clone(),
                &self.pointer.database.clone(),
                self.in_recovery,
                &self.postmaster.clone(),
                &self.ca.clone(),
            )
            .await
            .expect("expiring open");
        let pair = self.pair(&owner).await;
        tokio::time::sleep(Duration::from_secs(16)).await;
        let err = owner.finish().await.expect_err("expired finish");
        assert_eq!(err.fact(), Some("deadline"), "{err}");
        self.wait_gone(&[pair.0, pair.1]).await;
        self.wait_count(0).await;
        self.note("c13=deadline_before_finish".to_string());
        self.prove_slots("lotexp", &tables).await;
    }

    async fn other_selected_table(&mut self) {
        let tables = [("drift", "item"), ("life", "hold")];
        let mut owner = self
            .open_lot("lotother", &tables, 1_000)
            .await
            .expect("other open");
        self.exec("ALTER TABLE life.hold ADD COLUMN a125_other text")
            .await;
        let attnum = self.attnum("life", "hold", "a125_other").await;
        self.note(format!("other_table_attnum={attnum}"));
        assert!(attnum > 0);
        let err = owner
            .read_probe(key("drift", "item", ProbeValue::Int8(5)))
            .await
            .expect_err("other table drift");
        assert_drift(&err);
        let finished = owner.finish().await.expect_err("other finish");
        assert_drift(&finished);
        self.note("other_selected_table=schema_drift".to_string());
    }

    async fn add_column_and_fresh(&mut self) {
        let tables = [("drift", "item")];
        let mut before = self
            .open_lot("lotpre", &tables, 1_000)
            .await
            .expect("pre open");
        let old_hash = before.binding().schema_hash.clone();
        let batch = before
            .read_probe(key("drift", "item", ProbeValue::Int8(5)))
            .await
            .expect("pre probe");
        let label = utf8_at(&batch, "label");
        let old_cols = batch.plans.len();
        before.finish().await.expect("pre finish");
        self.window().await;
        let mut owner = self
            .open_lot("lotc1", &tables, 1_000)
            .await
            .expect("c1 open");
        let ms = self
            .exec("ALTER TABLE drift.item ADD COLUMN a125_c1 text")
            .await;
        let attnum = self.attnum("drift", "item", "a125_c1").await;
        self.note(format!("c1_attnum={attnum} c1_ddl_ms={ms}"));
        assert!(attnum > 0);
        let err = owner
            .read_probe(key("drift", "item", ProbeValue::Int8(5)))
            .await
            .expect_err("c1 probe");
        assert_drift(&err);
        let finished = owner.finish().await.expect_err("c1 finish");
        assert_drift(&finished);
        self.note("c1=schema_drift".to_string());
        self.window().await;
        let admitted = self.load_admitted(&tables).await;
        let fresh_att = admitted.tables[0]
            .columns
            .iter()
            .find(|column| column.name == "a125_c1")
            .expect("admitted new column")
            .attnum;
        assert_eq!(fresh_att, attnum);
        let mut owner = self
            .open_lot("lotc15", &tables, 1_000)
            .await
            .expect("c15 open");
        assert_ne!(owner.binding().schema_hash, old_hash);
        let seen = owner
            .read_probe(key("drift", "item", ProbeValue::Int8(5)))
            .await
            .expect("c15 probe");
        assert!(seen.plans.len() > old_cols, "fresh generation did not grow");
        let plan = seen
            .plans
            .iter()
            .find(|plan| plan.name == "a125_c1")
            .expect("c15 plan");
        assert_eq!(plan.attnum, attnum);
        assert!(matches!(arrow_one(&seen, "a125_c1"), ArrowScalar::Null));
        assert_eq!(utf8_at(&seen, "label"), label);
        owner.finish().await.expect("c15 finish");
        self.note(format!(
            "c15_attnum={attnum} c15_columns={} old_columns={old_cols}",
            seen.plans.len()
        ));
    }

    async fn probe_then_ddl(&mut self) {
        let tables = [("life", "hold")];
        let mut owner = self
            .open_lot("lotc2", &tables, 1_000)
            .await
            .expect("c2 open");
        let batch = owner
            .read_probe(key("life", "hold", ProbeValue::Int8(1)))
            .await
            .expect("c2 probe");
        let label = utf8_at(&batch, "label");
        self.exec("ALTER TABLE life.hold ADD COLUMN a125_c2 text")
            .await;
        let attnum = self.attnum("life", "hold", "a125_c2").await;
        self.note(format!("c2_attnum={attnum} c2_label={label}"));
        let err = owner.finish().await.expect_err("c2 finish");
        assert_drift(&err);
        self.note("c2=schema_drift_after_probe".to_string());
    }

    async fn after_last_probe(&mut self) {
        let tables = [("life", "hold")];
        let mut owner = self
            .open_lot("lotlast", &tables, 1_000)
            .await
            .expect("last open");
        let first = owner
            .read_probe(key("life", "hold", ProbeValue::Int8(1)))
            .await
            .expect("last first");
        let second = owner
            .read_probe(key("life", "hold", ProbeValue::Int8(1)))
            .await
            .expect("last second");
        assert_eq!(
            first.rows[0].canonical_hash,
            second.rows[0].canonical_hash
        );
        self.exec("ALTER TABLE life.hold ADD COLUMN a125_last text")
            .await;
        let attnum = self.attnum("life", "hold", "a125_last").await;
        self.note(format!("after_last_attnum={attnum}"));
        let err = owner.finish().await.expect_err("last finish");
        assert_drift(&err);
        self.note("after_last_probe=schema_drift".to_string());
    }

    async fn zero_probe_finish(&mut self) {
        let tables = [("life", "hold")];
        let owner = self
            .open_lot("lotc3", &tables, 1_000)
            .await
            .expect("c3 open");
        self.exec("ALTER TABLE life.hold ADD COLUMN a125_c3 text")
            .await;
        let attnum = self.attnum("life", "hold", "a125_c3").await;
        self.note(format!("c3_attnum={attnum}"));
        let err = owner.finish().await.expect_err("c3 finish");
        assert_drift(&err);
        self.note("c3_zero_probe=schema_drift".to_string());
    }

    async fn column_and_key_drift(&mut self) {
        let tables = [("life", "hold")];
        self.exec("ALTER TABLE life.hold ADD COLUMN a125_drop text")
            .await;
        let drop_attnum = self.attnum("life", "hold", "a125_drop").await;
        self.note(format!("drop_attnum={drop_attnum}"));
        let owner = self
            .open_lot("lotdrop", &tables, 1_000)
            .await
            .expect("drop open");
        self.exec("ALTER TABLE life.hold DROP COLUMN a125_drop")
            .await;
        self.expect_probe_drift(owner, "life", "hold", 1, "column_drop")
            .await;

        let owner = self
            .open_lot("lotren", &tables, 1_000)
            .await
            .expect("rename open");
        self.exec("ALTER TABLE life.hold RENAME COLUMN label TO label_a125")
            .await;
        self.expect_probe_drift(owner, "life", "hold", 1, "column_rename")
            .await;

        let owner = self
            .open_lot("lottype", &tables, 1_000)
            .await
            .expect("type open");
        self.exec(
            "ALTER TABLE life.hold ALTER COLUMN label_a125 TYPE varchar(16) USING label_a125::varchar(16)",
        )
        .await;
        self.expect_probe_drift(owner, "life", "hold", 1, "type_change")
            .await;

        let owner = self
            .open_lot("lotmod", &tables, 1_000)
            .await
            .expect("typmod open");
        self.exec("ALTER TABLE life.hold ALTER COLUMN label_a125 TYPE varchar(40)")
            .await;
        self.expect_probe_drift(owner, "life", "hold", 1, "typmod_change")
            .await;

        self.exec("ALTER TABLE life.hold ADD COLUMN a125_nn text")
            .await;
        let nn_attnum = self.attnum("life", "hold", "a125_nn").await;
        self.exec("UPDATE life.hold SET a125_nn = 'x'").await;
        self.note(format!("not_null_attnum={nn_attnum}"));
        let owner = self
            .open_lot("lotnn", &tables, 1_000)
            .await
            .expect("not null open");
        self.exec("ALTER TABLE life.hold ALTER COLUMN a125_nn SET NOT NULL")
            .await;
        self.expect_probe_drift(owner, "life", "hold", 1, "set_not_null")
            .await;

        self.exec("ALTER TABLE life.hold ADD COLUMN a125_pk int")
            .await;
        self.exec("UPDATE life.hold SET a125_pk = id").await;
        self.exec("ALTER TABLE life.hold ALTER COLUMN a125_pk SET NOT NULL")
            .await;
        let old_pk = self.pk_columns("life", "hold").await;
        let new_attnum = self.attnum("life", "hold", "a125_pk").await;
        let constraint = self.pk_constraint("life", "hold").await;
        assert!(
            constraint
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_'),
            "{constraint}"
        );
        let owner = self
            .open_lot("lotpk", &tables, 1_000)
            .await
            .expect("pk open");
        let drop_pk = match constraint.as_str() {
            "hold_pkey" => "ALTER TABLE life.hold DROP CONSTRAINT hold_pkey",
            other => panic!("unexpected primary constraint {other}"),
        };
        self.exec(drop_pk).await;
        self.exec("ALTER TABLE life.hold ADD PRIMARY KEY (a125_pk)")
            .await;
        let new_pk = self.pk_columns("life", "hold").await;
        self.note(format!(
            "pk_old={} pk_new_attnum={new_attnum} pk_new={}",
            format_pk(&old_pk),
            format_pk(&new_pk)
        ));
        assert_ne!(old_pk, new_pk);
        self.expect_probe_drift(owner, "life", "hold", 1, "primary_key")
            .await;
    }

    async fn recreate_relation(&mut self) {
        let tables = [("drift", "item")];
        let old_oid = self.relation_oid("drift", "item").await;
        let owner = self
            .open_lot("lotc4", &tables, 1_000)
            .await
            .expect("recreate open");
        self.exec("CREATE TABLE drift.item_a125 (LIKE drift.item INCLUDING ALL)")
            .await;
        self.exec("INSERT INTO drift.item_a125 SELECT * FROM drift.item")
            .await;
        self.exec("DROP TABLE drift.item").await;
        self.exec("ALTER TABLE drift.item_a125 RENAME TO item")
            .await;
        self.exec("GRANT SELECT ON drift.item TO lot_reader").await;
        self.exec("ALTER TABLE drift.item OWNER TO lot_writer")
            .await;
        let new_oid = self.relation_oid("drift", "item").await;
        self.note(format!("recreate_oid old={old_oid} new={new_oid}"));
        assert_ne!(old_oid, new_oid);
        self.expect_probe_drift(owner, "drift", "item", 5, "recreate_oid")
            .await;
    }

    async fn expect_probe_drift(
        &mut self,
        mut owner: SnapshotOwner,
        schema: &str,
        table: &str,
        id: i64,
        case_name: &str,
    ) {
        let err = owner
            .read_probe(key(schema, table, ProbeValue::Int8(id)))
            .await
            .expect_err(case_name);
        assert_drift(&err);
        let finished = owner.finish().await.expect_err(case_name);
        assert_drift(&finished);
        self.note(format!("{case_name}=schema_drift"));
    }

    async fn prove_slots(&mut self, source: &str, tables: &[(&str, &str)]) {
        self.window().await;
        let first = self
            .open_lot(source, tables, 1_000)
            .await
            .unwrap_or_else(|err| panic!("slots first {source} {err}"));
        let second = self
            .open_lot(source, tables, 1_000)
            .await
            .unwrap_or_else(|err| panic!("slots second {source} {err}"));
        let held = self.source_backends().await;
        assert_eq!(held.len(), 4, "{source}");
        let wanted: Vec<(i64, String)> = held
            .iter()
            .map(|row| (row.pid, row.start.clone()))
            .collect();
        first.finish().await.expect("slots first finish");
        second.finish().await.expect("slots second finish");
        self.wait_gone(&wanted).await;
        self.wait_count(0).await;
        self.note(format!("slots_recovered={source}"));
    }

    async fn open_lot(
        &mut self,
        source: &str,
        tables: &[(&str, &str)],
        max_rows: u32,
    ) -> Result<SnapshotOwner, SourceError> {
        let admitted = self.load_admitted(tables).await;
        let generation = self.next_gen(source);
        let principal = self.pointer.lot_reader_user.clone();
        let database = self.pointer.database.clone();
        let postmaster = self.postmaster.clone();
        let recovery = self.in_recovery;
        let ca = self.ca.clone();
        self.open_exact(
            source,
            generation,
            tables,
            admitted,
            max_rows,
            Instant::now() + Duration::from_secs(50),
            &principal,
            &database,
            recovery,
            &postmaster,
            &ca,
        )
        .await
    }

    async fn open_exact(
        &self,
        source: &str,
        generation: u64,
        tables: &[(&str, &str)],
        admitted: AdmittedCatalog,
        max_rows: u32,
        deadline: Instant,
        principal: &str,
        database: &str,
        recovery: bool,
        postmaster: &str,
        ca: &[u8],
    ) -> Result<SnapshotOwner, SourceError> {
        let config = self.config(source, tables, max_rows, ca);
        let policy = oo_connectors::PgTypePolicy::PRESERVE_ROW;
        let request = SnapshotRequest {
            config_identity: config_identity(&config),
            type_policy_identity: type_policy_identity(policy),
            config,
            admitted,
            policy,
            generation,
            deadline,
            expected_principal: principal.to_string(),
            expected_database: database.to_string(),
            expected_in_recovery: recovery,
            expected_postmaster_start: postmaster.to_string(),
        };
        SnapshotOwner::open(request).await
    }

    fn config(&self, source: &str, tables: &[(&str, &str)], max_rows: u32, ca: &[u8]) -> PgSourceConfig {
        let selected = tables
            .iter()
            .map(|(schema, table)| SelectedTable::new(*schema, *table).expect("table"))
            .collect();
        PgSourceConfig::try_new(
            SourceId::new(source).expect("source"),
            PgEndpoint::new(&self.pointer.host, self.pointer.port, &self.pointer.database)
                .expect("endpoint"),
            PgCredentials::new(&self.pointer.lot_reader_user, self.lot_reader_pass.as_str())
                .expect("credentials"),
            SecretRef::new("secret://pg/a125/lot").expect("secret"),
            PgTlsConfig::from_ca_pem(ca).expect("ca"),
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(60),
                Duration::from_secs(5),
                max_rows,
                1024 * 1024,
            )
            .expect("budget"),
            TableSelection::new(selected).expect("selection"),
        )
        .expect("config")
    }

    async fn connect_holder(&self, source: &str) -> PgSession {
        let config = self.config(source, &[("lot", "batch")], 1_000, &self.ca);
        PgConnector::new(config)
            .connect()
            .await
            .expect("holder connect")
    }

    async fn load_admitted(&mut self, tables: &[(&str, &str)]) -> AdmittedCatalog {
        let mut loaded = Vec::new();
        for (schema, table) in tables {
            let rows = sqlx::query(
                "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod::int4, \
                 a.attnotnull \
                 FROM pg_catalog.pg_attribute a \
                 JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
                 ORDER BY a.attnum",
            )
            .bind(*schema)
            .bind(*table)
            .fetch_all(&mut self.admin)
            .await
            .expect("admitted columns");
            let columns = rows
                .iter()
                .map(|row| AdmittedColumn {
                    attnum: row.try_get(0).expect("attnum"),
                    name: row.try_get(1).expect("name"),
                    type_oid: row.try_get(2).expect("oid"),
                    typmod: row.try_get(3).expect("typmod"),
                    not_null: row.try_get(4).expect("not null"),
                })
                .collect();
            let keys = sqlx::query(
                "SELECT a.attname::text \
                 FROM pg_catalog.pg_index i \
                 JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS u(attnum, ord) ON true \
                 JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = u.attnum \
                 WHERE n.nspname = $1 AND c.relname = $2 AND i.indisprimary \
                 ORDER BY u.ord",
            )
            .bind(*schema)
            .bind(*table)
            .fetch_all(&mut self.admin)
            .await
            .expect("admitted pk");
            let primary_key = keys
                .iter()
                .map(|row| row.try_get(0).expect("pk name"))
                .collect();
            loaded.push(AdmittedTable {
                schema: (*schema).to_string(),
                name: (*table).to_string(),
                columns,
                primary_key,
            });
        }
        AdmittedCatalog { tables: loaded }
    }

    async fn exec(&mut self, sql: &'static str) -> u128 {
        let began = Instant::now();
        sqlx::query(sql)
            .execute(&mut self.admin)
            .await
            .unwrap_or_else(|err| panic!("ddl failed: {sql}: {err}"));
        let ms = began.elapsed().as_millis();
        self.note(format!("ddl_ms={ms} stmt={sql}"));
        assert!(ms < 5_000, "ddl exceeded 5000ms: {ms} {sql}");
        ms
    }

    async fn writer_update(&mut self, title: &str, qty: i16) {
        let mut writer = dial(
            &self.pointer,
            &self.pointer.lot_writer_user,
            &self.lot_writer_pass,
            &self.ca,
            "oo-a125-writer",
        )
        .await;
        sqlx::query("UPDATE lot.batch SET title = $1 WHERE id = 11")
            .bind(title)
            .execute(&mut writer)
            .await
            .expect("writer title");
        sqlx::query("UPDATE lot.\"child row\" SET qty = $1 WHERE id = 31")
            .bind(qty)
            .execute(&mut writer)
            .await
            .expect("writer qty");
        let _ = writer.close().await;
    }

    async fn batch_row(&mut self) -> (String, bool, String) {
        let row = sqlx::query("SELECT title::text, ready, code::text FROM lot.batch WHERE id = 11")
            .fetch_one(&mut self.admin)
            .await
            .expect("batch row");
        (
            row.try_get(0).expect("title"),
            row.try_get(1).expect("ready"),
            row.try_get(2).expect("code"),
        )
    }

    async fn child_qty(&mut self, id: i32) -> i16 {
        sqlx::query("SELECT qty FROM lot.\"child row\" WHERE id = $1")
            .bind(id)
            .fetch_one(&mut self.admin)
            .await
            .expect("qty")
            .try_get(0)
            .expect("qty i16")
    }

    async fn tick_hits(&mut self) -> i64 {
        sqlx::query("SELECT hits FROM lot.tick WHERE n = 1")
            .fetch_one(&mut self.admin)
            .await
            .expect("tick")
            .try_get(0)
            .expect("hits")
    }

    async fn attnum(&mut self, schema: &str, table: &str, column: &str) -> i32 {
        sqlx::query(
            "SELECT a.attnum::int4 FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 AND a.attname = $3 \
             AND a.attnum > 0 AND NOT a.attisdropped",
        )
        .bind(schema)
        .bind(table)
        .bind(column)
        .fetch_one(&mut self.admin)
        .await
        .expect("attnum")
        .try_get(0)
        .expect("attnum i32")
    }

    async fn relation_oid(&mut self, schema: &str, table: &str) -> i64 {
        sqlx::query(
            "SELECT c.oid::int8 FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2",
        )
        .bind(schema)
        .bind(table)
        .fetch_one(&mut self.admin)
        .await
        .expect("oid")
        .try_get(0)
        .expect("oid i64")
    }

    async fn pk_columns(&mut self, schema: &str, table: &str) -> Vec<(i32, String)> {
        let rows = sqlx::query(
            "SELECT a.attnum::int4, a.attname::text \
             FROM pg_catalog.pg_index i \
             JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS u(attnum, ord) ON true \
             JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = u.attnum \
             WHERE n.nspname = $1 AND c.relname = $2 AND i.indisprimary \
             ORDER BY u.ord",
        )
        .bind(schema)
        .bind(table)
        .fetch_all(&mut self.admin)
        .await
        .expect("pk");
        rows.iter()
            .map(|row| {
                (
                    row.try_get(0).expect("pk attnum"),
                    row.try_get(1).expect("pk name"),
                )
            })
            .collect()
    }

    async fn pk_index(&mut self, schema: &str, table: &str) -> String {
        sqlx::query(
            "SELECT ic.relname::text \
             FROM pg_catalog.pg_index i \
             JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_catalog.pg_class ic ON ic.oid = i.indexrelid \
             WHERE n.nspname = $1 AND c.relname = $2 AND i.indisprimary",
        )
        .bind(schema)
        .bind(table)
        .fetch_one(&mut self.admin)
        .await
        .expect("index")
        .try_get(0)
        .expect("index name")
    }

    async fn pk_constraint(&mut self, schema: &str, table: &str) -> String {
        sqlx::query(
            "SELECT con.conname::text \
             FROM pg_catalog.pg_constraint con \
             JOIN pg_catalog.pg_class c ON c.oid = con.conrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 AND con.contype = 'p'",
        )
        .bind(schema)
        .bind(table)
        .fetch_one(&mut self.admin)
        .await
        .expect("constraint")
        .try_get(0)
        .expect("constraint name")
    }

    async fn source_backends(&mut self) -> Vec<LiveBackend> {
        let rows = sqlx::query(
            "SELECT pid::int8, backend_start::text, COALESCE(state, '') \
             FROM pg_catalog.pg_stat_activity \
             WHERE usename = $1 AND application_name = 'oo-pg-source'",
        )
        .bind(&self.pointer.lot_reader_user)
        .fetch_all(&mut self.admin)
        .await
        .expect("activity");
        rows.iter()
            .map(|row| LiveBackend {
                pid: row.try_get(0).expect("pid"),
                start: row.try_get(1).expect("start"),
                state: row.try_get(2).expect("state"),
            })
            .collect()
    }

    async fn backend_pids(&mut self) -> Vec<i64> {
        let mut pids: Vec<i64> = self
            .source_backends()
            .await
            .into_iter()
            .map(|row| row.pid)
            .collect();
        pids.sort_unstable();
        pids
    }

    async fn pair(&mut self, owner: &SnapshotOwner) -> ((i64, String), (i64, String)) {
        self.pair_among(owner, 2).await
    }

    async fn pair_among(
        &mut self,
        owner: &SnapshotOwner,
        expected: usize,
    ) -> ((i64, String), (i64, String)) {
        let live = self.source_backends().await;
        assert_eq!(live.len(), expected, "backend count");
        let data = owner.binding().backend.pid;
        let data_start = owner.binding().backend.backend_start.clone();
        let guard = live
            .iter()
            .find(|row| row.pid != data && row.state == "idle in transaction")
            .expect("guard backend");
        ((data, data_start), (guard.pid, guard.start.clone()))
    }

    async fn wait_gone(&mut self, wanted: &[(i64, String)]) {
        let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
        let began = Instant::now();
        loop {
            let live = self.source_backends().await;
            let still = wanted.iter().any(|(pid, start)| {
                live.iter().any(|row| row.pid == *pid && row.start == *start)
            });
            if !still {
                return;
            }
            if began.elapsed() > bound {
                panic!("backend still present after {}ms", bound.as_millis());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn wait_count(&mut self, want: usize) {
        let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
        let began = Instant::now();
        loop {
            let n = self.source_backends().await.len();
            if n == want {
                return;
            }
            if began.elapsed() > bound {
                panic!("backend count {n} wanted {want}");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn marker(&mut self, token: &str) {
        assert_eq!(token, "A125_ORACLE_RATE");
        sqlx::query("SELECT 'A125_ORACLE_RATE'")
            .execute(&mut self.admin)
            .await
            .expect("marker");
        let began = Instant::now();
        loop {
            if self.log_text().contains(token) {
                return;
            }
            if began.elapsed() > Duration::from_secs(3) {
                panic!("marker {token} missing from postgres log");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn log_text(&self) -> String {
        fs::read_to_string(&self.pointer.log_file).unwrap_or_default()
    }

    fn log_after(&self, token: &str) -> String {
        let raw = self.log_text();
        match raw.split_once(token) {
            Some((_, rest)) => rest.to_string(),
            None => raw,
        }
    }

    fn audit(&mut self) {
        let raw = self.log_text();
        for secret in password_values(&self.pointer.password_dir) {
            if !secret.is_empty() && raw.contains(secret.as_str()) {
                panic!("postgres log contains a closed string");
            }
        }
        let mut oo_statements = 0_usize;
        let mut writer_statements = 0_usize;
        let mut admin_statements = 0_usize;
        let mut writer_writes = 0_usize;
        let mut saw_start = false;
        let mut saw_savepoint = false;
        let mut saw_rollback_to = false;
        let mut saw_release = false;
        let mut saw_limit = false;
        let mut saw_repeatable = false;
        let mut saw_committed = false;
        let mut lines = Vec::new();
        for line in raw.lines() {
            let Some(sql) = statement_sql(line) else {
                continue;
            };
            let app = app_of(line);
            let user = user_of(line);
            lines.push(format!("app={app} user={user} sql={sql}"));
            if app == "oo-a125-admin" {
                admin_statements += 1;
            }
            if app == "oo-a125-writer" {
                writer_statements += 1;
                if write_word(&strip_sql_literals(sql)).is_some() {
                    writer_writes += 1;
                }
            }
            if app != "oo-pg-source" || user != self.pointer.lot_reader_user {
                continue;
            }
            let verb = sql
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_uppercase();
            if verb.is_empty() {
                continue;
            }
            oo_statements += 1;
            if matches!(verb.as_str(), "START" | "BEGIN") {
                saw_start = true;
            }
            if verb == "SAVEPOINT" {
                saw_savepoint = true;
            }
            if verb == "ROLLBACK" && sql.to_ascii_uppercase().contains("SAVEPOINT") {
                saw_rollback_to = true;
            }
            if verb == "RELEASE" {
                saw_release = true;
            }
            if sql.contains("LIMIT") && sql.contains("pg_attribute") {
                saw_limit = true;
            }
            if sql.contains("REPEATABLE READ") {
                saw_repeatable = true;
            }
            if sql.contains("READ COMMITTED") {
                saw_committed = true;
            }
            if let Some(word) = write_word(&strip_sql_literals(sql)) {
                panic!("oo statement contains write verb {word}");
            }
        }
        assert!(oo_statements > 0, "oo statements were not logged");
        assert!(admin_statements > 0, "admin statements were not logged");
        assert!(writer_statements > 0, "writer statements were not logged");
        assert!(writer_writes > 0, "writer writes were not logged");
        assert!(saw_start, "transaction start was not logged");
        assert!(saw_savepoint, "savepoint was not logged");
        assert!(saw_rollback_to, "rollback to savepoint was not logged");
        assert!(saw_release, "release savepoint was not logged");
        assert!(saw_limit, "current catalog limit was not logged");
        assert!(saw_repeatable, "repeatable read was not logged");
        assert!(saw_committed, "read committed was not logged");
        if let Some(path) = &self.sql_path {
            fs::write(path, lines.join("\n")).expect("sql evidence");
        }
        self.note(format!(
            "sql_audit oo={oo_statements} admin={admin_statements} writer={writer_statements} writer_writes={writer_writes}"
        ));
    }
}

fn assert_drift(err: &SourceError) {
    assert_eq!(err.fact(), Some("schema_drift"), "{err}");
    assert_eq!(err.kind(), SourceErrorKind::Catalog, "{err}");
    assert!(err.sqlstate().is_none(), "{err}");
}

fn kind_name(kind: SourceErrorKind) -> &'static str {
    match kind {
        SourceErrorKind::Config => "config",
        SourceErrorKind::Connect => "connect",
        SourceErrorKind::Auth => "auth",
        SourceErrorKind::Tls => "tls",
        SourceErrorKind::Privilege => "privilege",
        SourceErrorKind::Version => "version",
        SourceErrorKind::Budget => "budget",
        SourceErrorKind::Catalog => "catalog",
        SourceErrorKind::Identifier => "identifier",
        SourceErrorKind::Protocol => "protocol",
    }
}

fn format_pk(columns: &[(i32, String)]) -> String {
    columns
        .iter()
        .map(|(attnum, name)| format!("{attnum}:{name}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn key(schema: &str, table: &str, value: ProbeValue) -> CutProbe {
    CutProbe {
        schema: schema.to_string(),
        table: table.to_string(),
        key: vec![value],
    }
}

fn arrow_one<'a>(batch: &'a oo_connectors::TypedBatch, name: &str) -> &'a ArrowScalar {
    let column = batch
        .arrow
        .iter()
        .find(|column| column.name == name)
        .unwrap_or_else(|| panic!("missing {name}"));
    match column.values.as_slice() {
        [value] => value,
        other => panic!("{name} values {}", other.len()),
    }
}

fn utf8_at(batch: &oo_connectors::TypedBatch, name: &str) -> String {
    match arrow_one(batch, name) {
        ArrowScalar::Utf8(text) => text.clone(),
        other => panic!("{name} {other:?}"),
    }
}

fn bool_at(batch: &oo_connectors::TypedBatch, name: &str) -> bool {
    match arrow_one(batch, name) {
        ArrowScalar::Bool(value) => *value,
        other => panic!("{name} {other:?}"),
    }
}

fn i16_at(batch: &oo_connectors::TypedBatch, name: &str) -> i16 {
    match arrow_one(batch, name) {
        ArrowScalar::Int16(value) => *value,
        other => panic!("{name} {other:?}"),
    }
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn read_pass(dir: &str, name: &str) -> Zeroizing<String> {
    let text = fs::read_to_string(format!("{dir}/{name}")).expect("password file");
    Zeroizing::new(text.trim().to_string())
}

fn password_values(dir: &str) -> Vec<Zeroizing<String>> {
    let mut values = Vec::new();
    let entries = fs::read_dir(dir).expect("password dir");
    for entry in entries {
        let path = entry.expect("password entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("pass") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("password file");
        values.push(Zeroizing::new(text.trim().to_string()));
    }
    values
}

async fn dial(
    pointer: &Pointer,
    user: &str,
    password: &str,
    ca: &[u8],
    app: &str,
) -> PgConnection {
    let options = PgConnectOptions::from_explicit()
        .host(&pointer.host)
        .port(pointer.port)
        .username(user)
        .password(password)
        .database(&pointer.database)
        .application_name(app)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    options
        .disable_statement_logging()
        .connect()
        .await
        .expect("dial")
}

fn app_of(line: &str) -> String {
    field_of(line, "app=")
}

fn user_of(line: &str) -> String {
    field_of(line, "user=")
}

fn field_of(line: &str, key: &str) -> String {
    let Some(start) = line.find(key) else {
        return String::new();
    };
    let rest = &line[start + key.len()..];
    rest.split_whitespace().next().unwrap_or("").to_string()
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
        "NEXTVAL", "LO_IMPORT", "LO_EXPORT", "LO_CREATE", "DBLINK", "PG_WRITE_FILE",
        "PG_READ_FILE", "PG_READ_BINARY_FILE",
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
