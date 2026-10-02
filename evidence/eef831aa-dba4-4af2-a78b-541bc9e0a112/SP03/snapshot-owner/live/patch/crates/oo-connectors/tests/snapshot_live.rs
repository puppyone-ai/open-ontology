//! Live R1 snapshot-owner proof on PostgreSQL 17.10.
//! `OO_PG_SNAPSHOT_FIXTURE` is required. A missing pointer is a failure, not a pass.
//! `OO_PG_FIXTURE` and `OO_PG_TYPES_FIXTURE` are not this test.

use std::cell::Cell;
use std::fs;
use std::time::{Duration, Instant};

use oo_connectors::types::{ArrowColumn, ArrowScalar};
use oo_connectors::{
    config_identity, snapshot_input_digest, type_policy_identity, AdmittedCatalog, AdmittedColumn,
    AdmittedTable, CutProbe, PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSourceConfig,
    PgTlsConfig, ProbeValue, SecretRef, SelectedTable, SnapshotOwner, SnapshotRequest, SourceId,
    TableSelection, CLEANUP_OBSERVATION_BOUND_MS, SNAPSHOT_PROTOCOL, TYPE_CONVERSION_PROTOCOL,
    SourceError, SourceErrorKind,
};
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};
use zeroize::Zeroizing;

const SOURCE: &str = "snap_src";

#[derive(Deserialize)]
struct Pointer {
    host: String,
    port: u16,
    database: String,
    reader_user: String,
    writer_user: String,
    guc_user: String,
    super_user: String,
    password_dir: String,
    ca_file: String,
    wrong_ca_file: String,
    log_file: String,
}

struct Activity {
    state: String,
    in_xact: bool,
    app: String,
    user: String,
}

struct Counters {
    ins: i64,
    upd: i64,
    del: i64,
    touch: i32,
}

struct World {
    pointer: Pointer,
    reader_pass: Zeroizing<String>,
    writer_pass: Zeroizing<String>,
    guc_pass: Zeroizing<String>,
    super_pass: Zeroizing<String>,
    ca: Vec<u8>,
    wrong_ca: Vec<u8>,
    admin: PgConnection,
    generation: Cell<u64>,
    postmaster: String,
    in_recovery: bool,
    admitted: AdmittedCatalog,
    outcomes: Vec<String>,
    outcome_path: Option<String>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn snapshot_owner_live() {
    let mut world = World::start().await;
    world.regression_catalog().await;
    world.negative_opens().await;
    let before = world.counters().await;
    world.oo_only_reads().await;
    world.lifecycle().await;
    let after = world.counters().await;
    assert_eq!(before.ins, after.ins, "oo changed n_tup_ins");
    assert_eq!(before.upd, after.upd, "oo changed n_tup_upd");
    assert_eq!(before.del, after.del, "oo changed n_tup_del");
    assert_eq!(before.touch, after.touch, "oo changed the trigger counter");
    world.note(format!(
        "oo_only_counters ins={} upd={} del={} touch={}",
        after.ins, after.upd, after.del, after.touch
    ));
    world.same_cut().await;
    world.schema_and_privilege().await;
    world.audit();
    world.note("live_complete".to_string());
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
        let path = std::env::var("OO_PG_SNAPSHOT_FIXTURE")
            .expect("OO_PG_SNAPSHOT_FIXTURE is required");
        let pointer: Pointer = serde_json::from_str(
            &fs::read_to_string(&path).expect("snapshot pointer"),
        )
        .expect("snapshot pointer json");
        let reader_pass = read_pass(&pointer.password_dir, "reader.pass");
        let writer_pass = read_pass(&pointer.password_dir, "writer.pass");
        let guc_pass = read_pass(&pointer.password_dir, "guc.pass");
        let super_pass = read_pass(&pointer.password_dir, "super.pass");
        let ca = fs::read(&pointer.ca_file).expect("ca");
        let wrong_ca = fs::read(&pointer.wrong_ca_file).expect("wrong ca");
        let mut admin = dial(
            &pointer,
            &pointer.super_user,
            &super_pass,
            &ca,
            "oo-snapshot-admin",
        )
        .await;
        sqlx::query("UPDATE snap.touch SET n = 0 WHERE id = 1")
            .execute(&mut admin)
            .await
            .expect("reset touch");
        let (postmaster, in_recovery) = node_of(&mut admin).await;
        let admitted = load_admitted(&mut admin).await;
        let outcome_path = std::env::var("OO_PG_SNAPSHOT_OUTCOME").ok();
        Self {
            pointer,
            reader_pass,
            writer_pass,
            guc_pass,
            super_pass,
            ca,
            wrong_ca,
            admin,
            generation: Cell::new(0),
            postmaster,
            in_recovery,
            admitted,
            outcomes: Vec::new(),
            outcome_path,
        }
    }

    fn note(&mut self, line: String) {
        eprintln!("outcome {line}");
        self.outcomes.push(line);
    }

    fn next_gen(&self) -> u64 {
        let next = self.generation.get() + 1;
        self.generation.set(next);
        next
    }

    fn selection() -> TableSelection {
        TableSelection::new(vec![
            SelectedTable::new("snap", "parent").unwrap(),
            SelectedTable::new("snap", "child").unwrap(),
        ])
        .unwrap()
    }

    fn config(&self, user: &str, password: &str, ca: &[u8]) -> PgSourceConfig {
        PgSourceConfig::try_new(
            SourceId::new(SOURCE).unwrap(),
            PgEndpoint::new(&self.pointer.host, self.pointer.port, &self.pointer.database)
                .unwrap(),
            PgCredentials::new(user, password).unwrap(),
            SecretRef::new("secret://pg/snap/reader").unwrap(),
            PgTlsConfig::from_ca_pem(ca).unwrap(),
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(60),
                Duration::from_secs(5),
                1_000,
                1024 * 1024,
            )
            .unwrap(),
            Self::selection(),
        )
        .unwrap()
    }

    fn request(
        &self,
        config: PgSourceConfig,
        admitted: AdmittedCatalog,
        generation: u64,
        deadline: Instant,
        principal: &str,
        database: &str,
    ) -> SnapshotRequest {
        let policy = oo_connectors::PgTypePolicy::PRESERVE_ROW;
        SnapshotRequest {
            config_identity: config_identity(&config),
            type_policy_identity: type_policy_identity(policy),
            config,
            admitted,
            policy,
            generation,
            deadline,
            expected_principal: principal.to_string(),
            expected_database: database.to_string(),
            expected_in_recovery: self.in_recovery,
            expected_postmaster_start: self.postmaster.clone(),
        }
    }

    async fn open_reader(
        &mut self,
        generation: u64,
        admitted: AdmittedCatalog,
        deadline: Instant,
    ) -> Result<SnapshotOwner, SourceError> {
        let config = self.config(&self.pointer.reader_user, &self.reader_pass, &self.ca);
        let request = self.request(
            config,
            admitted,
            generation,
            deadline,
            &self.pointer.reader_user,
            &self.pointer.database,
        );
        SnapshotOwner::open(request).await
    }

    async fn open_next(
        &mut self,
        admitted: AdmittedCatalog,
        deadline: Instant,
    ) -> Result<SnapshotOwner, SourceError> {
        let generation = self.next_gen();
        self.open_reader(generation, admitted, deadline).await
    }

    async fn regression_catalog(&mut self) {
        let config = self.config(&self.pointer.reader_user, &self.reader_pass, &self.ca);
        let mut session = PgConnector::new(config)
            .connect()
            .await
            .expect("reader connect");
        let report = session.inspect_access().await.expect("reader inspect");
        assert!(
            report.registration_allowed,
            "reader blocked: {:?}",
            report
                .block_reasons
                .iter()
                .map(|reason| reason.code.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(report.write.state, "read_only");
        assert_eq!(report.version.admission, "tested");
        assert!(report.version.server_version.starts_with("17.10"));
        let catalog = session.catalog().await.expect("reader catalog");
        assert_eq!(catalog.tables.len(), 2);
        assert_eq!(catalog.type_conversion, "unsupported");
        assert!(catalog.tables.iter().all(|table| {
            table
                .columns
                .iter()
                .all(|column| !column.type_support.converted)
        }));
        self.note(format!("catalog_schema_hash={}", catalog.schema_hash));
        drop(session);
    }

    async fn negative_opens(&mut self) {
        let generation = self.next_gen();
        let mut config = self.config(&self.pointer.reader_user, &self.reader_pass, &self.ca);
        let mut identity = config_identity(&config);
        identity[0] ^= 0xff;
        let mut request = self.request(
            config,
            self.admitted.clone(),
            generation,
            Instant::now() + Duration::from_secs(30),
            &self.pointer.reader_user,
            &self.pointer.database,
        );
        request.config_identity = identity;
        let err = SnapshotOwner::open(request).await.expect_err("bad config identity");
        assert_eq!(err.fact(), Some("config_identity"), "{err}");

        config = self.config(&self.pointer.reader_user, &self.reader_pass, &self.ca);
        request = self.request(
            config,
            self.admitted.clone(),
            self.next_gen(),
            Instant::now() + Duration::from_secs(30),
            &self.pointer.reader_user,
            &self.pointer.database,
        );
        request.type_policy_identity[0] ^= 0xff;
        let err = SnapshotOwner::open(request).await.expect_err("bad policy identity");
        assert_eq!(err.fact(), Some("type_policy_identity"), "{err}");

        let err = self
            .open_reader(
                0,
                self.admitted.clone(),
                Instant::now() + Duration::from_secs(30),
            )
            .await
            .expect_err("generation 0");
        assert_eq!(err.fact(), Some("generation_invalid"), "{err}");

        let burned = self.next_gen();
        let wrong = self.config(&self.pointer.reader_user, &self.reader_pass, &self.wrong_ca);
        let request = self.request(
            wrong,
            self.admitted.clone(),
            burned,
            Instant::now() + Duration::from_secs(30),
            &self.pointer.reader_user,
            &self.pointer.database,
        );
        let err = SnapshotOwner::open(request).await.expect_err("wrong ca");
        assert_eq!(err.kind(), SourceErrorKind::Tls, "{err}");
        let err = self
            .open_reader(
                burned,
                self.admitted.clone(),
                Instant::now() + Duration::from_secs(30),
            )
            .await
            .expect_err("reused generation");
        assert_eq!(err.fact(), Some("generation_reused"), "{err}");

        let guc = self.config(&self.pointer.guc_user, &self.guc_pass, &self.ca);
        let request = self.request(
            guc,
            self.admitted.clone(),
            self.next_gen(),
            Instant::now() + Duration::from_secs(30),
            &self.pointer.guc_user,
            &self.pointer.database,
        );
        let err = SnapshotOwner::open(request).await.expect_err("guc role");
        assert!(
            matches!(
                err.fact(),
                Some("session_user_mismatch" | "startup_options")
            ),
            "{err}"
        );
        self.note(format!("guc_fact={}", err.fact().unwrap_or("none")));

        let writer = self.config(&self.pointer.writer_user, &self.writer_pass, &self.ca);
        let request = self.request(
            writer,
            self.admitted.clone(),
            self.next_gen(),
            Instant::now() + Duration::from_secs(30),
            &self.pointer.writer_user,
            &self.pointer.database,
        );
        let err = SnapshotOwner::open(request).await.expect_err("writer");
        assert_eq!(err.fact(), Some("write_capable"), "{err}");

        let mut request = self.request(
            self.config(&self.pointer.reader_user, &self.reader_pass, &self.ca),
            self.admitted.clone(),
            self.next_gen(),
            Instant::now() + Duration::from_secs(30),
            "not_reader",
            &self.pointer.database,
        );
        let err = SnapshotOwner::open(request).await.expect_err("principal");
        assert_eq!(err.fact(), Some("principal"), "{err}");

        request = self.request(
            self.config(&self.pointer.reader_user, &self.reader_pass, &self.ca),
            self.admitted.clone(),
            self.next_gen(),
            Instant::now() + Duration::from_secs(30),
            &self.pointer.reader_user,
            "not_snap",
        );
        let err = SnapshotOwner::open(request).await.expect_err("database");
        assert_eq!(err.fact(), Some("database"), "{err}");

        request = self.request(
            self.config(&self.pointer.reader_user, &self.reader_pass, &self.ca),
            self.admitted.clone(),
            self.next_gen(),
            Instant::now() + Duration::from_secs(30),
            &self.pointer.reader_user,
            &self.pointer.database,
        );
        request.expected_postmaster_start = "not-the-postmaster".to_string();
        let err = SnapshotOwner::open(request).await.expect_err("postmaster");
        assert_eq!(err.fact(), Some("read_node"), "{err}");

        request = self.request(
            self.config(&self.pointer.reader_user, &self.reader_pass, &self.ca),
            self.admitted.clone(),
            self.next_gen(),
            Instant::now() + Duration::from_secs(30),
            &self.pointer.reader_user,
            &self.pointer.database,
        );
        request.expected_in_recovery = !self.in_recovery;
        let err = SnapshotOwner::open(request).await.expect_err("recovery");
        assert_eq!(err.fact(), Some("read_node"), "{err}");

        let mut wrong_shape = self.admitted.clone();
        wrong_shape.tables[0].columns[0].type_oid = 1;
        let err = self
            .open_next(
                wrong_shape,
                Instant::now() + Duration::from_secs(30),
            )
            .await
            .expect_err("type oid");
        assert_eq!(err.fact(), Some("catalog_identity"), "{err}");
    }

    async fn oo_only_reads(&mut self) {
        let mut owner = self
            .open_next(
                self.admitted.clone(),
                Instant::now() + Duration::from_secs(50),
            )
            .await
            .expect("oo-only open");
        self.hold(&owner).await;
        let parent = owner
            .read_probe(key_probe("snap", "parent", ProbeValue::Int8(1)))
            .await
            .expect("parent probe");
        assert_eq!(utf8_at(&parent, "label"), "alpha");
        assert_eq!(int32_at(&parent, "id"), 1);
        assert_eq!(parent.protocol, TYPE_CONVERSION_PROTOCOL);
        assert_eq!(owner.binding().protocol, SNAPSHOT_PROTOCOL);
        let rendered = format!("{owner:?}");
        assert!(!rendered.contains(self.reader_pass.as_str()));
        let finished = owner.finish().await.expect("oo-only finish");
        self.wait_gone(finished.backend.pid, &finished.backend.backend_start)
            .await;
    }

    async fn lifecycle(&mut self) {
        let mut owner = self
            .open_next(
                self.admitted.clone(),
                Instant::now() + Duration::from_secs(8),
            )
            .await
            .expect("deadline open");
        let backend = owner.binding().backend.clone();
        tokio::time::sleep(Duration::from_secs(9)).await;
        let err = owner
            .read_probe(key_probe("snap", "parent", ProbeValue::Int8(1)))
            .await
            .expect_err("deadline probe");
        assert_eq!(err.fact(), Some("deadline"), "{err}");
        let err = owner.finish().await.expect_err("deadline finish");
        assert!(
            matches!(err.fact(), Some("deadline" | "cleanup_failed")),
            "{err}"
        );
        self.wait_gone(backend.pid, &backend.backend_start).await;
        self.note(format!("deadline_finish={}", err.fact().unwrap_or("none")));

        let owner = self
            .open_next(
                self.admitted.clone(),
                Instant::now() + Duration::from_secs(50),
            )
            .await
            .expect("abort open");
        let generation = owner.binding().generation;
        let backend = owner.binding().backend.clone();
        let aborted = owner.abort().await.expect("abort");
        assert_eq!(aborted.generation, generation);
        self.wait_gone(aborted.backend.pid, &aborted.backend.backend_start)
            .await;
        let err = self
            .open_reader(
                generation,
                self.admitted.clone(),
                Instant::now() + Duration::from_secs(30),
            )
            .await
            .expect_err("abort reuse");
        assert_eq!(err.fact(), Some("generation_reused"), "{err}");
        let _ = backend;

        let owner = self
            .open_next(
                self.admitted.clone(),
                Instant::now() + Duration::from_secs(50),
            )
            .await
            .expect("drop open");
        let backend = owner.binding().backend.clone();
        drop(owner);
        self.wait_gone(backend.pid, &backend.backend_start).await;
        self.note("drop_released".to_string());

        let mut owner = self
            .open_next(
                self.admitted.clone(),
                Instant::now() + Duration::from_secs(50),
            )
            .await
            .expect("error open");
        let backend = owner.binding().backend.clone();
        let err = owner
            .read_probe(CutProbe {
                schema: "snap".to_string(),
                table: "parent".to_string(),
                key: vec![ProbeValue::Int8(1), ProbeValue::Int8(2)],
            })
            .await
            .expect_err("bad key");
        assert_eq!(err.fact(), Some("probe_key"), "{err}");
        let err = owner.finish().await.expect_err("poisoned finish");
        assert_eq!(err.fact(), Some("probe_key"), "{err}");
        self.wait_gone(backend.pid, &backend.backend_start).await;

        let mut owner = self
            .open_next(
                self.admitted.clone(),
                Instant::now() + Duration::from_secs(50),
            )
            .await
            .expect("terminate open");
        let backend = owner.binding().backend.clone();
        let pid = i32::try_from(backend.pid).expect("pid");
        let terminated: bool = sqlx::query("SELECT pg_catalog.pg_terminate_backend($1)")
            .bind(pid)
            .fetch_one(&mut self.admin)
            .await
            .expect("terminate")
            .try_get(0)
            .expect("terminate bool");
        assert!(terminated);
        let err = owner
            .read_probe(key_probe("snap", "parent", ProbeValue::Int8(1)))
            .await
            .expect_err("terminated probe");
        assert_ne!(err.kind(), SourceErrorKind::Config, "{err}");
        let finished = owner.finish().await;
        assert!(finished.is_err(), "terminate returned finished");
        self.wait_gone(backend.pid, &backend.backend_start).await;
        self.note(format!(
            "terminate_probe={}",
            err.fact().unwrap_or("none")
        ));
    }

    async fn same_cut(&mut self) {
        let before = self.counters().await;
        let mut owner = self
            .open_next(
                self.admitted.clone(),
                Instant::now() + Duration::from_secs(50),
            )
            .await
            .expect("cut open");
        self.hold(&owner).await;
        let hash = owner.binding().schema_hash.clone();
        let digest = snapshot_input_digest(
            &owner.binding().config_identity,
            &owner.binding().type_policy_identity,
            &hash,
            owner.binding().generation,
        )
        .unwrap();
        assert_eq!(digest, owner.binding().input_digest);
        let mut oracle = dial(
            &self.pointer,
            &self.pointer.reader_user,
            &self.reader_pass,
            &self.ca,
            "oo-snapshot-oracle",
        )
        .await;
        sqlx::query("START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut oracle)
            .await
            .expect("oracle begin");
        let oracle_before = oracle_parent(&mut oracle).await;
        let first = owner
            .read_probe(key_probe("snap", "parent", ProbeValue::Int8(1)))
            .await
            .expect("cut probe");
        assert_eq!(utf8_at(&first, "label"), oracle_before);
        assert_eq!(utf8_at(&first, "label"), "alpha");
        let child = owner
            .read_probe(key_probe("snap", "child", ProbeValue::Int8(10)))
            .await
            .expect("child 10");
        assert_eq!(int32_at(&child, "qty"), 2);
        let missing = owner
            .read_probe(key_probe("snap", "child", ProbeValue::Int8(12)))
            .await
            .expect("child 12 absent");
        assert!(missing.rows.is_empty());
        self.writer_mutates().await;
        let second = owner
            .read_probe(key_probe("snap", "parent", ProbeValue::Int8(1)))
            .await
            .expect("same cut probe");
        assert_eq!(utf8_at(&second, "label"), "alpha");
        assert_eq!(first.rows[0].canonical_hash, second.rows[0].canonical_hash);
        let child_again = owner
            .read_probe(key_probe("snap", "child", ProbeValue::Int8(10)))
            .await
            .expect("child same cut");
        assert_eq!(int32_at(&child_again, "qty"), 2);
        let still_missing = owner
            .read_probe(key_probe("snap", "child", ProbeValue::Int8(12)))
            .await
            .expect("inserted key hidden");
        assert!(still_missing.rows.is_empty());
        let oracle_after = oracle_parent(&mut oracle).await;
        assert_eq!(oracle_after, "alpha");
        let oracle_qty = oracle_qty(&mut oracle, 10).await;
        assert_eq!(oracle_qty, 2);
        let oracle_new = oracle_qty_optional(&mut oracle, 12).await;
        assert!(oracle_new.is_none());
        self.oracle_binding(&mut oracle, owner.binding()).await;
        sqlx::query("ROLLBACK")
            .execute(&mut oracle)
            .await
            .expect("oracle rollback");
        let _ = oracle.close().await;
        let finished = owner.finish().await.expect("cut finish");
        self.wait_gone(finished.backend.pid, &finished.backend.backend_start)
            .await;
        let after = self.counters().await;
        assert_eq!(after.touch - before.touch, 3, "writer trigger delta");
        self.note(format!(
            "writer_counter_delta ins={} upd={} del={} touch={}",
            after.ins - before.ins,
            after.upd - before.upd,
            after.del - before.del,
            after.touch - before.touch
        ));

        let mut fresh = self
            .open_next(
                self.admitted.clone(),
                Instant::now() + Duration::from_secs(50),
            )
            .await
            .expect("fresh open");
        assert_eq!(fresh.binding().schema_hash, hash);
        let row = fresh
            .read_probe(key_probe("snap", "parent", ProbeValue::Int8(1)))
            .await
            .expect("fresh probe");
        assert_eq!(utf8_at(&row, "label"), "beta");
        assert_ne!(row.rows[0].canonical_hash, first.rows[0].canonical_hash);
        let qty = fresh
            .read_probe(key_probe("snap", "child", ProbeValue::Int8(10)))
            .await
            .expect("fresh child");
        assert_eq!(int32_at(&qty, "qty"), 9);
        let inserted = fresh
            .read_probe(key_probe("snap", "child", ProbeValue::Int8(12)))
            .await
            .expect("fresh insert");
        assert_eq!(int32_at(&inserted, "qty"), 4);
        fresh.finish().await.expect("fresh finish");
        self.note(format!("same_cut_hash={hash}"));
    }

    async fn schema_and_privilege(&mut self) {
        let old = self.admitted.clone();
        let mut owner = self
            .open_next(
                old.clone(),
                Instant::now() + Duration::from_secs(50),
            )
            .await
            .expect("schema open");
        sqlx::query("ALTER TABLE snap.parent ADD COLUMN note text")
            .execute(&mut self.admin)
            .await
            .expect("add column");
        let probed = owner
            .read_probe(key_probe("snap", "parent", ProbeValue::Int8(1)))
            .await;
        match probed {
            Ok(batch) => {
                assert!(batch.arrow.iter().all(|column| column.name != "note"));
                assert_eq!(utf8_at(&batch, "label"), "beta");
                owner.finish().await.expect("schema old cut finish");
                self.note("schema_inflight=old_cut".to_string());
            }
            Err(err) => {
                assert_eq!(err.fact(), Some("schema_drift"), "{err}");
                let finished = owner.finish().await;
                assert!(finished.is_err(), "schema drift finished");
                self.note(format!(
                    "schema_inflight={}",
                    err.fact().unwrap_or("none")
                ));
            }
        }
        let err = self
            .open_next(
                old,
                Instant::now() + Duration::from_secs(30),
            )
            .await
            .expect_err("new generation sees the column");
        assert_eq!(err.fact(), Some("catalog_identity"), "{err}");
        self.note("schema_new_generation=catalog_identity".to_string());

        self.admitted = load_admitted(&mut self.admin).await;
        let mut owner = self
            .open_next(
                self.admitted.clone(),
                Instant::now() + Duration::from_secs(50),
            )
            .await
            .expect("updated shape open");
        sqlx::query("REVOKE SELECT ON snap.parent FROM oo_snap_reader")
            .execute(&mut self.admin)
            .await
            .expect("revoke");
        let probed = owner
            .read_probe(key_probe("snap", "parent", ProbeValue::Int8(1)))
            .await;
        match probed {
            Err(err) => {
                assert!(
                    err.fact() == Some("select_not_granted") || err.sqlstate() == Some("42501"),
                    "{err}"
                );
                assert!(owner.finish().await.is_err(), "denied probe finished");
                self.note(format!(
                    "privilege_inflight={}",
                    err.fact().unwrap_or(err.sqlstate().unwrap_or("denied"))
                ));
            }
            Ok(batch) => {
                assert_eq!(utf8_at(&batch, "label"), "beta");
                owner.finish().await.expect("privilege old cut finish");
                self.note("privilege_inflight=old_cut".to_string());
            }
        }
        let err = self
            .open_next(
                self.admitted.clone(),
                Instant::now() + Duration::from_secs(30),
            )
            .await
            .expect_err("new generation sees the revoke");
        assert!(
            err.fact() == Some("select_not_granted") || err.sqlstate() == Some("42501"),
            "{err}"
        );
        self.note(format!(
            "privilege_new_generation={}",
            err.fact().unwrap_or("none")
        ));
    }

    async fn writer_mutates(&mut self) {
        let mut writer = dial(
            &self.pointer,
            &self.pointer.writer_user,
            &self.writer_pass,
            &self.ca,
            "oo-fixture-writer",
        )
        .await;
        sqlx::query("UPDATE snap.parent SET label = 'beta' WHERE id = 1")
            .execute(&mut writer)
            .await
            .expect("writer update parent");
        sqlx::query("UPDATE snap.child SET qty = 9 WHERE id = 10")
            .execute(&mut writer)
            .await
            .expect("writer update child");
        sqlx::query("INSERT INTO snap.child (id, parent_id, qty) VALUES (12, 1, 4)")
            .execute(&mut writer)
            .await
            .expect("writer insert");
        let _ = writer.close().await;
    }

    async fn hold(&mut self, owner: &SnapshotOwner) {
        let pid = i32::try_from(owner.binding().backend.pid).expect("pid");
        let start = owner.binding().backend.backend_start.clone();
        let began = Instant::now();
        loop {
            match activity(&mut self.admin, pid, &start).await {
                Some(row)
                    if row.state == "idle in transaction"
                        && row.in_xact
                        && row.app == "oo-pg-source"
                        && row.user == self.pointer.reader_user =>
                {
                    self.note(format!("held_state={}", row.state));
                    return;
                }
                Some(row) if began.elapsed() > Duration::from_secs(2) => {
                    panic!("backend {pid} state {}", row.state);
                }
                None if began.elapsed() > Duration::from_secs(2) => {
                    panic!("backend {pid} start {start} missing while open");
                }
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    }

    async fn wait_gone(&mut self, pid: i64, start: &str) {
        let pid = i32::try_from(pid).expect("pid");
        let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
        let began = Instant::now();
        loop {
            match activity(&mut self.admin, pid, start).await {
                None => return,
                Some(row) if began.elapsed() > bound => {
                    panic!(
                        "backend {pid} still {} in_xact={} after {}ms",
                        row.state,
                        row.in_xact,
                        began.elapsed().as_millis()
                    );
                }
                Some(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }

    async fn counters(&mut self) -> Counters {
        let mut ins = 0;
        let mut upd = 0;
        let mut del = 0;
        for name in ["parent", "child", "touch"] {
            let row = sqlx::query(
                "SELECT COALESCE(s.n_tup_ins, 0)::int8, COALESCE(s.n_tup_upd, 0)::int8, \
                 COALESCE(s.n_tup_del, 0)::int8 \
                 FROM pg_catalog.pg_class c \
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 LEFT JOIN pg_catalog.pg_stat_user_tables s ON s.relid = c.oid \
                 WHERE n.nspname = 'snap' AND c.relname = $1",
            )
            .bind(name)
            .fetch_one(&mut self.admin)
            .await
            .expect("counters");
            ins += row.try_get::<i64, _>(0).unwrap();
            upd += row.try_get::<i64, _>(1).unwrap();
            del += row.try_get::<i64, _>(2).unwrap();
        }
        let touch: i32 = sqlx::query("SELECT n FROM snap.touch WHERE id = 1")
            .fetch_one(&mut self.admin)
            .await
            .expect("touch")
            .try_get(0)
            .expect("touch n");
        Counters { ins, upd, del, touch }
    }

    async fn oracle_binding(&mut self, oracle: &mut PgConnection, binding: &oo_connectors::SnapshotBinding) {
        let select: bool = sqlx::query(
            "SELECT pg_catalog.has_table_privilege(CURRENT_USER, c.oid, 'SELECT') \
             FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'snap' AND c.relname = 'parent'",
        )
        .fetch_one(&mut *oracle)
        .await
        .expect("oracle privilege")
        .try_get(0)
        .expect("select");
        let access = binding
            .access
            .iter()
            .find(|item| item.schema == "snap" && item.name == "parent")
            .expect("parent access");
        assert_eq!(access.select, Some(select));
        let oid: i64 = sqlx::query(
            "SELECT a.atttypid::int8 FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'snap' AND c.relname = 'parent' AND a.attname = 'id'",
        )
        .fetch_one(&mut *oracle)
        .await
        .expect("oracle type")
        .try_get(0)
        .expect("oid");
        let column = binding
            .catalog
            .tables
            .iter()
            .find(|table| table.name == "parent")
            .expect("parent")
            .columns
            .iter()
            .find(|column| column.name == "id")
            .expect("id");
        assert_eq!(column.type_oid, oid);
    }

    fn audit(&mut self) {
        let raw = fs::read_to_string(&self.pointer.log_file).expect("postgres log");
        for secret in [
            self.reader_pass.as_str(),
            self.writer_pass.as_str(),
            self.guc_pass.as_str(),
            self.super_pass.as_str(),
        ] {
            if !secret.is_empty() && raw.contains(secret) {
                panic!("postgres log contains a closed string");
            }
        }
        let mut saw_start = false;
        let mut saw_probe = false;
        let mut probe_before_start = false;
        let mut oo_statements = 0_usize;
        let mut writer_writes = 0_usize;
        for line in raw.lines() {
            if line.contains("app=oo-fixture-writer")
                && let Some(sql) = statement_sql(line)
                && write_word(&strip_sql_literals(sql)).is_some()
            {
                writer_writes += 1;
            }
            if !line.contains("app=oo-pg-source") || !line.contains("user=oo_snap_reader") {
                continue;
            }
            let Some(sql) = statement_sql(line) else {
                continue;
            };
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
            if write_word(&strip_sql_literals(sql)).is_some() {
                panic!("oo statement contains a write verb");
            }
            if verb == "SELECT" && sql.contains("snap.parent") && !saw_start {
                probe_before_start = true;
            }
            if verb == "SELECT" && sql.contains("snap.parent") {
                saw_probe = true;
            }
        }
        assert!(oo_statements > 0, "oo statements were not logged");
        assert!(saw_start, "oo transaction start was not logged");
        assert!(saw_probe, "oo parent probe was not logged");
        assert!(!probe_before_start, "probe ran before start");
        assert!(writer_writes > 0, "fixture writer writes were not logged");
        self.note(format!(
            "sql_audit oo_statements={oo_statements} writer_writes={writer_writes}"
        ));
    }
}

fn key_probe(schema: &str, table: &str, value: ProbeValue) -> CutProbe {
    CutProbe {
        schema: schema.to_string(),
        table: table.to_string(),
        key: vec![value],
    }
}

fn utf8_at(batch: &oo_connectors::TypedBatch, name: &str) -> String {
    match column(batch, name).values.as_slice() {
        [ArrowScalar::Utf8(text)] => text.clone(),
        other => panic!("{name} {other:?}"),
    }
}

fn int32_at(batch: &oo_connectors::TypedBatch, name: &str) -> i32 {
    match column(batch, name).values.as_slice() {
        [ArrowScalar::Int32(value)] => *value,
        other => panic!("{name} {other:?}"),
    }
}

fn column<'a>(batch: &'a oo_connectors::TypedBatch, name: &str) -> &'a ArrowColumn {
    batch
        .arrow
        .iter()
        .find(|column| column.name == name)
        .unwrap_or_else(|| panic!("missing {name}"))
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

async fn node_of(admin: &mut PgConnection) -> (String, bool) {
    let row = sqlx::query(
        "SELECT pg_catalog.pg_postmaster_start_time()::text, pg_catalog.pg_is_in_recovery()",
    )
    .fetch_one(&mut *admin)
    .await
    .expect("node");
    (
        row.try_get(0).expect("postmaster"),
        row.try_get(1).expect("recovery"),
    )
}

async fn load_admitted(admin: &mut PgConnection) -> AdmittedCatalog {
    let mut tables = Vec::new();
    for name in ["parent", "child"] {
        let rows = sqlx::query(
            "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull \
             FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'snap' AND c.relname = $1 AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
        )
        .bind(name)
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
             WHERE n.nspname = 'snap' AND c.relname = $1 AND i.indisprimary \
             ORDER BY u.ord",
        )
        .bind(name)
        .fetch_all(&mut *admin)
        .await
        .expect("admitted keys");
        let primary_key = keys
            .iter()
            .map(|row| row.try_get::<String, _>(0).unwrap())
            .collect();
        tables.push(AdmittedTable {
            schema: "snap".to_string(),
            name: name.to_string(),
            columns,
            primary_key,
        });
    }
    AdmittedCatalog { tables }
}

async fn activity(admin: &mut PgConnection, pid: i32, start: &str) -> Option<Activity> {
    let row = sqlx::query(
        "SELECT state, xact_start IS NOT NULL, application_name, usename::text \
         FROM pg_catalog.pg_stat_activity \
         WHERE pid = $1 AND backend_start::text = $2",
    )
    .bind(pid)
    .bind(start)
    .fetch_optional(&mut *admin)
    .await
    .expect("activity");
    row.map(|row| Activity {
        state: row.try_get(0).unwrap_or_default(),
        in_xact: row.try_get(1).unwrap_or(false),
        app: row.try_get(2).unwrap_or_default(),
        user: row.try_get(3).unwrap_or_default(),
    })
}

async fn oracle_parent(oracle: &mut PgConnection) -> String {
    sqlx::query("SELECT label FROM snap.parent WHERE id = 1")
        .fetch_one(&mut *oracle)
        .await
        .expect("oracle parent")
        .try_get(0)
        .expect("label")
}

async fn oracle_qty(oracle: &mut PgConnection, id: i32) -> i32 {
    sqlx::query("SELECT qty FROM snap.child WHERE id = $1")
        .bind(id)
        .fetch_one(&mut *oracle)
        .await
        .expect("oracle qty")
        .try_get(0)
        .expect("qty")
}

async fn oracle_qty_optional(oracle: &mut PgConnection, id: i32) -> Option<i32> {
    sqlx::query("SELECT qty FROM snap.child WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut *oracle)
        .await
        .expect("oracle optional")
        .map(|row| row.try_get(0).expect("qty"))
}

fn read_pass(dir: &str, name: &str) -> Zeroizing<String> {
    let text = fs::read_to_string(format!("{dir}/{name}")).expect("password file");
    Zeroizing::new(text.trim().to_string())
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
        "GRANT", "REVOKE", "COMMENT", "VACUUM", "CLUSTER", "REINDEX", "CALL", "DO", "NEXTVAL",
        "LO_IMPORT", "LO_EXPORT", "LO_CREATE", "DBLINK", "PG_WRITE_FILE", "PG_READ_FILE",
        "PG_READ_BINARY_FILE",
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
