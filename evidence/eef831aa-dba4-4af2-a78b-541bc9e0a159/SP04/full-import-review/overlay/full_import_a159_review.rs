//! Independent full-import composition oracle.
//!
//! Fixture is `yard.crates` and `yard.tags`, not the author's `app` tables.
//! The oracle compares source and target by its own projection and digest.
//! It calls `import_unpublished` and does not assemble batches by hand.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use oo_connectors::{
    config_identity, type_policy_identity, AdmittedCatalog, AdmittedColumn, AdmittedTable,
    PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, PgTypePolicy, SecretRef,
    SelectedTable, SnapshotOwner, SnapshotRequest, SourceId, StreamBudget, StreamItem,
    TableSelection,
};
use oo_pipeline::copy_encoder::CopyBudgets;
use oo_pipeline::full_import::{
    abort_conflicting, adopt_generation, import_unpublished, recover, replay_batch, ApplyStep,
    ImportError, ImportRequest, ImportSession, Recovery, RunState,
};
use oo_pipeline::physical_mapping::{plan_physical_mapping, Level, MappingRequest};
use oo_pipeline::target_installer::{
    install, install_full_import, verify_full_import_readiness, verify_readiness,
    FullImportInstallOutcome, InstallError, InstallOutcome, ReadinessError,
};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{Connection, Row};

const SOURCE: &str = "a159src";
static NEXT_GEN: AtomicU64 = AtomicU64::new(1);

#[tokio::test]
#[ignore = "one private PostgreSQL 17.10; OO_A159_HOST is set by the review probe"]
async fn independent_unpublished_import() {
    let mut harness = Harness::connect().await;
    harness.installer_gate().await;
    harness.installer_lie().await;
    harness.prepare_import().await;
    harness.main_import().await;
    harness.pending_fence_and_gap().await;
    harness.backpressure().await;
    harness.oversized().await;
    harness.unsupported().await;
    harness.coherent_cut().await;
    harness.cancel_deadline_and_loss().await;
    harness.shard_cases().await;
    harness.durability().await;
    harness.reader_wrote_nothing().await;
    harness.receipt_only().await;
    harness.report.push_str("full_import_review_ok\n");
    harness.flush();
    print!("{}", harness.report);
}

struct Harness {
    host: String,
    port: u16,
    admin_user: String,
    admin_password: String,
    reader_password: String,
    runtime_password: String,
    writer_password: String,
    agent_password: String,
    ca: Vec<u8>,
    evidence: PathBuf,
    postgres: PgConnection,
    source: PgConnection,
    target: PgConnection,
    postmaster: String,
    in_recovery: bool,
    report: String,
    main_physical: Vec<String>,
}

impl Harness {
    async fn connect() -> Self {
        let host = env::var("OO_A159_HOST").expect("OO_A159_HOST");
        let port: u16 = env::var("OO_A159_PORT").expect("OO_A159_PORT").parse().expect("port");
        assert!(!host.contains(':') && !host.contains('/') && !host.contains('@'));
        let admin_user = env::var("OO_A159_ADMIN_USER").expect("OO_A159_ADMIN_USER");
        let admin_password = password("OO_A159_ADMIN_PASSWORD_FILE");
        let reader_password = password("OO_A159_READER_PASSWORD_FILE");
        let runtime_password = password("OO_A159_RUNTIME_PASSWORD_FILE");
        let ca = fs::read(env::var("OO_A159_CA_FILE").expect("OO_A159_CA_FILE")).expect("ca");
        let evidence = PathBuf::from(env::var("OO_A159_EVIDENCE").expect("OO_A159_EVIDENCE"));
        fs::create_dir_all(&evidence).expect("evidence dir");
        let mut postgres = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
        let version = text(&mut postgres, "SELECT version()").await;
        assert!(version.contains("17.10"), "{version}");
        let writer_password = text(
            &mut postgres,
            "SELECT encode(sha256(clock_timestamp()::text::bytea), 'hex')",
        )
        .await;
        let agent_password = text(
            &mut postgres,
            "SELECT encode(sha256((clock_timestamp()::text || 'agent')::bytea), 'hex')",
        )
        .await;
        let source = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
        let target = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
        let harness = Self {
            host,
            port,
            admin_user,
            admin_password,
            reader_password,
            runtime_password,
            writer_password,
            agent_password,
            ca,
            evidence,
            postgres,
            source,
            target,
            postmaster: String::new(),
            in_recovery: false,
            report: format!("server={version}\n"),
            main_physical: Vec::new(),
        };
        harness.flush();
        harness
    }

    async fn installer_gate(&mut self) {
        self.reset_cluster().await;
        self.create_db("oo_a159_gate").await;
        let mut gate = self.admin_db("oo_a159_gate").await;
        assert_eq!(install(&mut gate).await.expect("install"), InstallOutcome::Bootstrapped);
        assert!(to_regclass(&mut gate, "_oo.full_import_run").await.is_none());
        match verify_full_import_readiness(&mut gate).await {
            Err(ReadinessError::Incomplete) => {}
            other => panic!("0005 readiness before install: {other:?}"),
        }
        verify_readiness(&mut gate).await.expect("frozen readiness");
        let before = applied_stamp(&mut gate).await;
        assert_eq!(
            install_full_import(&mut gate).await.expect("0005"),
            FullImportInstallOutcome::Installed
        );
        assert_eq!(
            install_full_import(&mut gate).await.expect("0005 replay"),
            FullImportInstallOutcome::Replay
        );
        let after = applied_stamp(&mut gate).await;
        for version in [
            "target-bootstrap/1",
            "target-fence-receipts/1",
            "target-fence-receipts/3",
            "physical-mapping/v1",
        ] {
            assert_eq!(stamp_of(&before, version), stamp_of(&after, version), "{version}");
        }
        assert_eq!(install(&mut gate).await.expect("frozen replay"), InstallOutcome::Replay);
        assert_eq!(applied_stamp(&mut gate).await, after);
        verify_readiness(&mut gate).await.expect("frozen readiness after 0005");
        let ready = verify_full_import_readiness(&mut gate).await.expect("0005 ready");
        assert_eq!(ready.digest().len(), 64);
        let grant = text(
            &mut gate,
            "SELECT m.inherit_option::text || '|' || m.set_option::text FROM pg_catalog.pg_auth_members m JOIN pg_catalog.pg_roles granted ON granted.oid = m.roleid JOIN pg_catalog.pg_roles member ON member.oid = m.member WHERE granted.rolname = 'pg_read_all_stats' AND member.rolname = 'oo_target_owner'",
        )
        .await;
        assert_eq!(grant, "true|false");
        assert_eq!(
            text(&mut gate, "SELECT pg_catalog.pg_has_role('oo_target_owner', 'pg_read_all_stats', 'set')::text").await,
            "false"
        );
        assert_eq!(
            text(&mut gate, "SELECT rolinherit::text FROM pg_catalog.pg_roles WHERE rolname = 'oo_target_owner'").await,
            "false"
        );
        assert_eq!(
            text(&mut gate, "SELECT pg_catalog.pg_has_role('oo_pipeline_runtime', 'pg_read_all_stats', 'member')::text").await,
            "false"
        );
        assert_eq!(
            text(&mut gate, "SELECT pg_catalog.pg_has_role('oo_target_owner', 'pg_read_all_stats', 'usage')::text").await,
            "true"
        );
        let fence = text(
            &mut gate,
            "SELECT (strpos(pg_catalog.pg_get_functiondef(p.oid), 'this backend does not hold the target lock') > 0)::text FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace WHERE n.nspname = '_oo' AND p.proname = '_load_held_generation'",
        )
        .await;
        assert_eq!(fence, "true");
        let receipt = text(
            &mut gate,
            "SELECT (strpos(pg_catalog.pg_get_functiondef(p.oid), '_load_held_generation') > 0)::text FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace WHERE n.nspname = '_oo' AND p.proname = 'commit_apply_receipt'",
        )
        .await;
        assert_eq!(receipt, "true");
        let bound = text(
            &mut gate,
            "SELECT (strpos(pg_catalog.pg_get_functiondef(p.oid), 'backend_start') > 0 AND strpos(pg_catalog.pg_get_functiondef(p.oid), 'pg_backend_pid') > 0)::text FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace WHERE n.nspname = '_oo' AND p.proname = 'full_import_assert_bound'",
        )
        .await;
        assert_eq!(bound, "true");
        let runtime_password = self.runtime_password.clone();
        self.create_login("oo_a159_runtime", &runtime_password).await;
        raw(&mut gate, "GRANT oo_pipeline_runtime TO oo_a159_runtime").await;
        let mut runtime = dial(
            &self.host,
            self.port,
            "oo_a159_runtime",
            &self.runtime_password,
            "oo_a159_gate",
            &self.ca,
        )
        .await;
        verify_full_import_readiness(&mut runtime).await.expect("runtime can read readiness");
        match install_full_import(&mut runtime).await {
            Err(InstallError::NotAdministrator) => {}
            other => panic!("runtime install: {other:?}"),
        }
        drop(runtime);
        assert_eq!(applied_stamp(&mut gate).await, after);
        assert_eq!(text(&mut gate, "SELECT count(*)::text FROM _oo.full_import_run").await, "0");
        drop(gate);
        self.report.push_str("installer_0005=installed_replay\nfrozen_applied_at=unchanged\nstats_grant=owner_inherit_true_set_false\nfence_checks=present\nruntime_install=not_administrator\n");
        self.flush();
        self.reset_cluster().await;
    }

    async fn installer_lie(&mut self) {
        self.create_db("oo_a159_lie").await;
        let mut lie = self.admin_db("oo_a159_lie").await;
        assert_eq!(install(&mut lie).await.expect("lie base"), InstallOutcome::Bootstrapped);
        raw(
            &mut lie,
            "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES ('full-import/v1', 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', 'expand', '0005_full_import_expand.sql')",
        )
        .await;
        let before = applied_stamp(&mut lie).await;
        match install_full_import(&mut lie).await {
            Err(err) => {
                assert_eq!(err.sqlstate(), Some("OO001"), "{err}");
                assert_eq!(err.version(), Some("full-import/v1"));
            }
            Ok(outcome) => panic!("bad 0005 installed: {outcome}"),
        }
        assert_eq!(applied_stamp(&mut lie).await, before);
        verify_readiness(&mut lie).await.expect("frozen readiness ignores the 0005 lie");
        match verify_full_import_readiness(&mut lie).await {
            Err(ReadinessError::ChecksumMismatch { version, stored }) => {
                assert_eq!(version, "full-import/v1");
                assert_eq!(stored, "e".repeat(64));
            }
            other => panic!("bad readiness: {other:?}"),
        }
        assert!(to_regclass(&mut lie, "_oo.full_import_run").await.is_none());
        drop(lie);
        self.report.push_str("installer_oo001=unchanged\n");
        self.flush();
        self.reset_cluster().await;
    }

    async fn prepare_import(&mut self) {
        let reader_password = self.reader_password.clone();
        let writer_password = self.writer_password.clone();
        let runtime_password = self.runtime_password.clone();
        let agent_password = self.agent_password.clone();
        self.create_login("oo_a159_reader", &reader_password).await;
        self.create_login("oo_a159_writer", &writer_password).await;
        self.create_login("oo_a159_runtime", &runtime_password).await;
        self.create_login("oo_a159_agent", &agent_password).await;
        self.create_db("oo_a159_src").await;
        self.create_db("oo_a159_dst").await;
        self.source = self.admin_db("oo_a159_src").await;
        self.target = self.admin_db("oo_a159_dst").await;
        raw(&mut self.source, SOURCE_SQL).await;
        raw(
            &mut self.source,
            &format!(
                "GRANT USAGE ON SCHEMA yard TO oo_a159_reader, oo_a159_writer; GRANT SELECT ON yard.crates, yard.tags, yard.bulk, yard.notes TO oo_a159_reader; GRANT INSERT ON yard.crates TO oo_a159_writer; GRANT INSERT ON yard.write_audit TO oo_a159_writer"
            ),
        )
        .await;
        assert_eq!(install(&mut self.target).await.expect("target install"), InstallOutcome::Bootstrapped);
        assert!(verify_full_import_readiness(&mut self.target).await.is_err());
        assert_eq!(
            install_full_import(&mut self.target).await.expect("target 0005"),
            FullImportInstallOutcome::Installed
        );
        raw(&mut self.target, "GRANT oo_pipeline_runtime TO oo_a159_runtime").await;
        raw(&mut self.target, "GRANT CONNECT ON DATABASE oo_a159_dst TO oo_a159_agent, oo_a159_runtime").await;
        raw(&mut self.source, "GRANT CONNECT ON DATABASE oo_a159_src TO oo_a159_reader, oo_a159_writer").await;
        assert_eq!(text(&mut self.target, "SELECT count(*)::text FROM _oo.full_import_run").await, "0");
        assert_eq!(
            text(&mut self.target, "SELECT pg_catalog.pg_has_role('oo_a159_runtime', 'pg_read_all_stats', 'member')::text").await,
            "false"
        );
        assert_eq!(
            text(&mut self.target, "SELECT pg_catalog.pg_has_role('oo_a159_agent', 'pg_read_all_stats', 'member')::text").await,
            "false"
        );
        let row = sqlx::query("SELECT pg_catalog.pg_postmaster_start_time()::text, pg_catalog.pg_is_in_recovery()")
            .fetch_one(&mut self.source)
            .await
            .expect("node");
        self.postmaster = row.try_get(0).expect("postmaster");
        self.in_recovery = row.try_get(1).expect("recovery");
        self.report.push_str("schema_setup_runs=0\n");
        self.flush();
    }

    async fn main_import(&mut self) {
        let admitted = load_admitted(&mut self.source, &[("crates", true), ("tags", true)]).await;
        let mut runtime = self.runtime().await;
        let summary = import_unpublished(
            &mut runtime,
            self.request(&admitted, &[("yard", "crates"), ("yard", "tags")], "a159main", 90, wide_copy(), stream_default()),
        )
        .await
        .expect("import");
        assert_eq!(summary.state, RunState::AwaitingPublish);
        assert_eq!(summary.source_id, SOURCE);
        assert_eq!(summary.tables.len(), 2);
        let state = run_state(&mut self.target, &summary.run_id).await;
        assert_eq!(state, "awaiting_publish");
        let pid = text(&mut runtime, "SELECT pg_catalog.pg_backend_pid()::text").await;
        let bound = text(
            &mut self.target,
            "SELECT backend_pid::text || '|' || backend_start FROM _oo.full_import_session WHERE shard_id = 'a159main'",
        )
        .await;
        let activity = text(
            &mut runtime,
            "SELECT pg_catalog.pg_backend_pid()::text || '|' || backend_start::text FROM pg_catalog.pg_stat_activity WHERE pid = pg_catalog.pg_backend_pid()",
        )
        .await;
        assert_eq!(bound, activity, "session identity");
        assert!(bound.starts_with(&format!("{pid}|")));
        assert!(bound.split('|').nth(1).unwrap_or("").len() > 8);
        self.compare_tables(&summary).await;
        self.mapping_stored(&summary).await;
        self.quality_mask(&summary).await;
        let second = self.product_digest(&admitted).await;
        for table in &summary.tables {
            let got = second.iter().find(|(name, _)| name == &table.name).expect("second digest");
            assert_eq!(got.1, table.canonical_digest, "{}", table.name);
        }
        assert_eq!(text(&mut self.target, "SELECT count(*)::text FROM pg_catalog.pg_publication").await, "0");
        let mut agent = self.agent().await;
        for name in &self.main_physical {
            let denied = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT 1 FROM _oo.{}", quote_physical(name))))
                .fetch_optional(&mut agent)
                .await;
            assert_denied(denied.err());
        }
        assert_denied(
            sqlx::query("SELECT 1 FROM _oo.full_import_quality")
                .fetch_optional(&mut agent)
                .await
                .err(),
        );
        drop(agent);
        let rows: u64 = summary.tables.iter().map(|table| table.rows).sum();
        assert_eq!(rows, 5);
        self.report.push_str(&format!(
            "main_state=awaiting_publish\nmain_rows={rows}\nmain_generation={}\nagent_denied=1\nsession_bound=1\n",
            summary.source_generation
        ));
        drop(runtime);
        self.flush();
    }

    async fn compare_tables(&mut self, summary: &oo_pipeline::full_import::RunSummary) {
        raw(&mut self.source, "SET TIME ZONE 'UTC'").await;
        raw(&mut self.target, "SET TIME ZONE 'UTC'").await;
        let mut names = Vec::new();
        for table in &summary.tables {
            assert_eq!(table.physical_name.len(), 63);
            assert!(table.physical_name.starts_with("pt_"));
            let logged = scalar_bind(
                &mut self.target,
                "SELECT c.relpersistence::text FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = '_oo' AND c.relname = $1",
                &table.physical_name,
            )
            .await;
            assert_eq!(logged, "p");
            let source_rows = projection_source(&mut self.source, &table.name).await;
            let target_rows = projection_target(&mut self.target, table).await;
            assert_eq!(source_rows, target_rows, "{}", table.name);
            let oracle = oracle_digest(&source_rows);
            let target_oracle = oracle_digest(&target_rows);
            assert_eq!(oracle, target_oracle, "{}", table.name);
            let positions = text(
                &mut self.target,
                &format!(
                    "SELECT string_agg(_oo_pos::text, ',' ORDER BY _oo_pos) FROM _oo.{}",
                    quote_physical(&table.physical_name)
                ),
            )
            .await;
            let expected_positions = if table.name == "crates" { "0,1,2" } else { "0,1" };
            assert_eq!(positions, expected_positions, "{}", table.name);
            assert_eq!(table.rows, source_rows.len() as u64);
            self.report.push_str(&format!(
                "oracle_digest_{}={oracle}\nrows_{}={}\n",
                table.name,
                table.name,
                table.rows
            ));
            names.push(table.physical_name.clone());
        }
        self.main_physical = names;
    }

    async fn mapping_stored(&mut self, summary: &oo_pipeline::full_import::RunSummary) {
        let config = scalar_bind(
            &mut self.target,
            "SELECT config_digest FROM _oo.full_import_run WHERE run_id = $1::uuid",
            &summary.run_id,
        )
        .await;
        let rows = sqlx::query(
            "SELECT schema_ident, table_ident, physical_name FROM _oo.full_import_table WHERE run_id = $1::uuid ORDER BY table_index",
        )
        .bind(&summary.run_id)
        .fetch_all(&mut self.target)
        .await
        .expect("mapping rows");
        assert_eq!(rows.len(), 2);
        for row in &rows {
            let namespace: String = row.try_get(0).unwrap();
            let table: String = row.try_get(1).unwrap();
            let stored: String = row.try_get(2).unwrap();
            let plan = plan_physical_mapping(&MappingRequest {
                source: summary.source_id.clone(),
                config_digest: config.clone(),
                run_generation: i64::try_from(summary.source_generation).unwrap(),
                level: Level::Table,
                logical_namespace: namespace.clone(),
                logical_table: table.clone(),
                logical_column: String::new(),
            })
            .expect("plan");
            assert_eq!(plan.physical_name, stored);
            let looked: String = sqlx::query_scalar(
                "SELECT _oo.lookup_physical_mapping($1, $2, $3, 'table', $4, $5, '')",
            )
            .bind(&summary.source_id)
            .bind(&config)
            .bind(i64::try_from(summary.source_generation).unwrap())
            .bind(&namespace)
            .bind(&table)
            .fetch_one(&mut self.target)
            .await
            .expect("lookup");
            assert_eq!(looked, stored);
        }
        self.report.push_str("mapping_stored=1\n");
    }

    async fn quality_mask(&mut self, summary: &oo_pipeline::full_import::RunSummary) {
        let columns = text(
            &mut self.target,
            "SELECT string_agg(column_name, ',' ORDER BY ordinal_position) FROM information_schema.columns WHERE table_schema = '_oo' AND table_name = 'full_import_quality'",
        )
        .await;
        for banned in ["raw_value", "raw", "cell", "payload_text", "value"] {
            assert!(!columns.split(',').any(|name| name == banned), "{columns}");
        }
        let leaked = text(
            &mut self.target,
            "SELECT count(*)::text FROM _oo.full_import_quality WHERE (run_id::text || COALESCE(reason, '') || COALESCE(key_digest, '') || input_digest || payload_digest || batch_id || physical_name) LIKE '%spruce-keel%'",
        )
        .await;
        assert_eq!(leaked, "0");
        let present = text(
            &mut self.target,
            &format!(
                "SELECT count(*)::text FROM _oo.{} WHERE {}::text = 'spruce-keel'",
                quote_physical(&summary.tables.iter().find(|table| table.name == "crates").unwrap().physical_name),
                quote_physical(&find_column(
                    summary.tables.iter().find(|table| table.name == "crates").unwrap(),
                    "label"
                ))
            ),
        )
        .await;
        assert_eq!(present, "1");
        let lineage = scalar_bind(
            &mut self.target,
            "SELECT count(*)::text FROM _oo.full_import_quality q JOIN _oo.full_import_run r ON r.run_id = q.run_id WHERE r.run_id = $1::uuid AND q.kind = 'batch' AND q.input_digest = r.input_digest",
            &summary.run_id,
        )
        .await;
        assert_ne!(lineage, "0");
        let accepted = scalar_bind(
            &mut self.target,
            "SELECT COALESCE(sum(accepted_rows), 0)::text FROM _oo.full_import_quality WHERE run_id = $1::uuid AND kind = 'batch'",
            &summary.run_id,
        )
        .await;
        assert_eq!(accepted, "5");
        self.report.push_str("quality_masked=1\nquality_lineage=1\n");
    }

    async fn product_digest(&mut self, admitted: &AdmittedCatalog) -> Vec<(String, String)> {
        let request = self.request(
            admitted,
            &[("yard", "crates"), ("yard", "tags")],
            "a159digest",
            90,
            wide_copy(),
            stream_default(),
        );
        let mut owner = SnapshotOwner::open(request.snapshot).await.expect("second reader");
        owner.open_stream(stream_default()).await.expect("stream");
        let mut out: Vec<(String, Sha256)> = Vec::new();
        loop {
            match owner.next_batch().await.expect("pull") {
                StreamItem::Batch(batch) => {
                    let entry = if let Some(found) = out.iter_mut().find(|(name, _)| name == &batch.typed.table) {
                        found
                    } else {
                        out.push((batch.typed.table.clone(), Sha256::new()));
                        out.last_mut().expect("pushed")
                    };
                    for row in &batch.typed.rows {
                        entry.1.update(row.canonical_hash);
                    }
                }
                StreamItem::TableEof(_) => {}
                StreamItem::Exhausted(_) => break,
            }
        }
        let complete = owner.finish_complete().await.expect("second complete");
        assert_eq!(complete.source_id, SOURCE);
        out.into_iter()
            .map(|(name, hasher)| (name, hex_bytes(hasher.finalize().as_ref())))
            .collect()
    }

    async fn pending_fence_and_gap(&mut self) {
        let admitted = load_admitted(&mut self.source, &[("crates", true), ("tags", true)]).await;
        let mut runtime = self.runtime().await;
        let session = ImportSession::open(
            &mut runtime,
            self.request(&admitted, &[("yard", "crates"), ("yard", "tags")], "a159open", 90, wide_copy(), stream_default()),
        )
        .await
        .expect("open pending");
        let run = session.run_id().to_string();
        assert_eq!(run_state(&mut self.target, &run).await, "open");
        let renewed = text(
            &mut self.target,
            "SELECT renewed_at::text FROM _oo.target_generations WHERE shard_id = 'a159open'",
        )
        .await;
        let direct = sqlx::query("UPDATE _oo.target_generations SET renewed_at = clock_timestamp() WHERE shard_id = 'a159open'")
            .execute(&mut self.target)
            .await;
        assert_eq!(sqlstate_of(&direct.expect_err("admin dml")), "OO014");
        assert_eq!(
            text(&mut self.target, "SELECT renewed_at::text FROM _oo.target_generations WHERE shard_id = 'a159open'").await,
            renewed
        );
        let shadow = quote_physical(&session.tables()[0].physical_name);
        let mut other = self.runtime().await;
        let inserted = sqlx::query(sqlx::AssertSqlSafe(format!("INSERT INTO _oo.{shadow} (_oo_pos) VALUES (0)")))
            .execute(&mut other)
            .await;
        assert_blocked(inserted.err());
        let receipts_before = text(&mut self.target, "SELECT count(*)::text FROM _oo.apply_receipts").await;
        let receipt = sqlx::query(
            "SELECT _oo.commit_apply_receipt('a159open', '00000000-0000-0000-0000-000000000009'::uuid, 1, 'a159src', 'pt_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 't0.b0', repeat('ab', 32), repeat('cd', 32), repeat('ef', 32), repeat('12', 32), repeat('34', 32), 'snapshot-zero/v1', 'full-import.snapshot', 0, 0, false, false, true, NULL, NULL, ''::bytea)",
        )
        .execute(&mut other)
        .await;
        assert_blocked(receipt.err());
        assert_eq!(text(&mut self.target, "SELECT count(*)::text FROM _oo.apply_receipts").await, receipts_before);
        drop(other);
        let err = session.finish(&mut runtime).await.expect_err("partial finish");
        assert!(err.to_string().contains("not exhausted"), "{err}");
        assert_eq!(run_state(&mut self.target, &run).await, "retired");
        assert_ne!(run_state(&mut self.target, &run).await, "awaiting_publish");
        drop(runtime);
        self.report.push_str("pending=open\nadmin_dml=OO014\nruntime_dml=blocked\nreceipt_only_unbound=blocked\ncoverage_gap=retired\n");
        self.flush();
    }

    async fn backpressure(&mut self) {
        let admitted = load_admitted(&mut self.source, &[("crates", true)]).await;
        let mut runtime = self.runtime().await;
        let summary = import_unpublished(
            &mut runtime,
            self.request(
                &admitted,
                &[("yard", "crates")],
                "a159press",
                90,
                CopyBudgets {
                    max_value_bytes: 1_048_576,
                    max_row_bytes: 4_194_304,
                    max_chunk_bytes: 8_388_608,
                    max_rows_per_chunk: 1,
                },
                StreamBudget::try_new(1, 1_048_576, 1_048_576, 2_097_152).expect("budget"),
            ),
        )
        .await
        .expect("backpressure");
        assert_eq!(summary.state, RunState::AwaitingPublish);
        assert_eq!(summary.tables[0].rows, 3);
        assert!(summary.tables[0].row_chunks >= 3, "{}", summary.tables[0].row_chunks);
        self.report.push_str(&format!("backpressure_chunks={}\n", summary.tables[0].row_chunks));
        drop(runtime);
        self.flush();
    }

    async fn oversized(&mut self) {
        let before = text(&mut self.target, "SELECT count(*)::text FROM _oo.full_import_run WHERE state = 'awaiting_publish'").await;
        let admitted = load_admitted(&mut self.source, &[("bulk", true)]).await;
        let mut runtime = self.runtime().await;
        let mut session = ImportSession::open(
            &mut runtime,
            self.request(
                &admitted,
                &[("yard", "bulk")],
                "a159wide",
                90,
                wide_copy(),
                StreamBudget::try_new(1, 4096, 16, 32).expect("tiny"),
            ),
        )
        .await
        .expect("wide open");
        let run = session.run_id().to_string();
        let err = pull_all(&mut session, &mut runtime).await.expect_err("oversized");
        assert!(matches!(err, ImportError::Source(_)), "{err}");
        assert_eq!(run_state(&mut self.target, &run).await, "retired");
        let after = text(&mut self.target, "SELECT count(*)::text FROM _oo.full_import_run WHERE state = 'awaiting_publish'").await;
        assert_eq!(before, after);
        drop(runtime);
        self.report.push_str("oversized=retired\n");
        self.flush();
    }

    async fn unsupported(&mut self) {
        let shadows = text(
            &mut self.target,
            "SELECT count(*)::text FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = '_oo' AND c.relname LIKE 'pt\\_%'",
        )
        .await;
        let admitted = load_admitted(&mut self.source, &[("notes", true)]).await;
        let mut runtime = self.runtime().await;
        let opened = ImportSession::open(
            &mut runtime,
            self.request(&admitted, &[("yard", "notes")], "a159json", 90, wide_copy(), stream_default()),
        )
        .await;
        match opened {
            Err(ImportError::Unsupported { oid, type_name }) => {
                assert_eq!(oid, 3802, "{type_name}");
                assert!(type_name.to_ascii_lowercase().contains("json"), "{type_name}");
            }
            Err(other) => panic!("jsonb was not refused: {other}"),
            Ok(_) => panic!("jsonb opened a session"),
        }
        let shadows_after = text(
            &mut self.target,
            "SELECT count(*)::text FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = '_oo' AND c.relname LIKE 'pt\\_%'",
        )
        .await;
        assert_eq!(shadows, shadows_after);
        drop(runtime);
        self.report.push_str("unsupported_jsonb=refused\n");
        self.flush();
    }

    async fn coherent_cut(&mut self) {
        let admitted = load_admitted(&mut self.source, &[("crates", true)]).await;
        let mut runtime = self.runtime().await;
        let mut session = ImportSession::open(
            &mut runtime,
            self.request(&admitted, &[("yard", "crates")], "a159cut", 90, wide_copy(), stream_default()),
        )
        .await
        .expect("cut open");
        let mut writer = dial(
            &self.host,
            self.port,
            "oo_a159_writer",
            &self.writer_password,
            "oo_a159_src",
            &self.ca,
        )
        .await;
        raw(
            &mut writer,
            "INSERT INTO yard.crates (crate_no, sealed, mass, label, opened, stamp, mark, ratio) VALUES (9, true, 1.000, 'late-board', timestamp '2024-12-01 00:00:00', '\\x09', 'zz', 1.5)",
        )
        .await;
        drop(writer);
        pull_all(&mut session, &mut runtime).await.expect("cut pull");
        let finished = session.finish(&mut runtime).await.expect("cut finish");
        assert_eq!(finished.tables[0].rows, 3);
        let old_name = finished.tables[0].physical_name.clone();
        let old_count = text(
            &mut self.target,
            &format!("SELECT count(*)::text FROM _oo.{}", quote_physical(&old_name)),
        )
        .await;
        assert_eq!(old_count, "3");
        drop(runtime);
        let admitted_fresh = load_admitted(&mut self.source, &[("crates", true)]).await;
        let mut runtime = self.runtime().await;
        let fresh = import_unpublished(
            &mut runtime,
            self.request(&admitted_fresh, &[("yard", "crates")], "a159cut", 90, wide_copy(), stream_default()),
        )
        .await
        .expect("fresh cut");
        assert_eq!(fresh.state, RunState::AwaitingPublish);
        assert_eq!(fresh.tables[0].rows, 4);
        assert_ne!(fresh.tables[0].physical_name, old_name);
        assert_eq!(
            text(&mut self.target, &format!("SELECT count(*)::text FROM _oo.{}", quote_physical(&old_name))).await,
            "3"
        );
        drop(runtime);
        self.report.push_str("coherent_cut=3_then_4\nold_shadow_unmixed=3\n");
        self.flush();
    }

    async fn cancel_deadline_and_loss(&mut self) {
        let admitted = load_admitted(&mut self.source, &[("tags", true)]).await;
        let mut runtime = self.runtime().await;
        let mut session = ImportSession::open(
            &mut runtime,
            self.request(&admitted, &[("yard", "tags")], "a159cancel", 90, wide_copy(), stream_default()),
        )
        .await
        .expect("cancel open");
        let run = session.run_id().to_string();
        session.cancel(&mut runtime).await.expect("cancel");
        assert_eq!(run_state(&mut self.target, &run).await, "retired");
        drop(runtime);

        let mut runtime = self.runtime().await;
        let mut session = ImportSession::open(
            &mut runtime,
            self.request(&admitted, &[("yard", "tags")], "a159deadline", 3, wide_copy(), stream_default()),
        )
        .await
        .expect("deadline open");
        let run = session.run_id().to_string();
        tokio::time::sleep(Duration::from_secs(4)).await;
        let err = pull_all(&mut session, &mut runtime).await.expect_err("deadline");
        assert!(err.to_string().contains("deadline"), "{err}");
        assert_eq!(run_state(&mut self.target, &run).await, "retired");
        drop(runtime);

        let mut runtime = self.runtime().await;
        let mut session = ImportSession::open(
            &mut runtime,
            self.request(&admitted, &[("yard", "tags")], "a159loss", 90, wide_copy(), stream_default()),
        )
        .await
        .expect("loss open");
        let run = session.run_id().to_string();
        let pid = i32::try_from(session.source_backend_pid()).expect("pid");
        let killed: bool = sqlx::query_scalar("SELECT pg_catalog.pg_terminate_backend($1)")
            .bind(pid)
            .fetch_one(&mut self.source)
            .await
            .expect("terminate reader");
        assert!(killed);
        let err = pull_all(&mut session, &mut runtime).await.expect_err("lost snapshot");
        assert!(matches!(err, ImportError::Source(_)), "{err}");
        assert_eq!(run_state(&mut self.target, &run).await, "retired");
        drop(runtime);
        self.report.push_str("cancel=retired\ndeadline=retired\nsnapshot_lost=retired\n");
        self.flush();
    }

    async fn shard_cases(&mut self) {
        let admitted = load_admitted(&mut self.source, &[("tags", true)]).await;
        let mut held = self.runtime().await;
        let mut session = ImportSession::open(
            &mut held,
            self.request(&admitted, &[("yard", "tags")], "a159same", 90, wide_copy(), stream_default()),
        )
        .await
        .expect("same open");
        let mut other = self.runtime().await;
        let owner = text(&mut self.target, "SELECT gen_random_uuid()::text").await;
        let failed = adopt_generation(&mut other, "a159same", &owner).await;
        match failed {
            Err(err) => {
                let state = err.sqlstate().unwrap_or("");
                assert!(state == "55P03" || state == "OO017", "{err}");
            }
            Ok(generation) => panic!("same shard adopted {generation}"),
        }
        drop(other);
        let mut other = self.runtime().await;
        let imported = import_unpublished(
            &mut other,
            self.request(&admitted, &[("yard", "tags")], "a159other", 90, wide_copy(), stream_default()),
        )
        .await
        .expect("other shard");
        assert_eq!(imported.state, RunState::AwaitingPublish);
        assert!(!self.main_physical.contains(&imported.tables[0].physical_name));
        session.retire(&mut held).await.expect("retire held");
        drop(held);
        drop(other);

        let mut old = self.runtime().await;
        let mut session = ImportSession::open(
            &mut old,
            self.request(&admitted, &[("yard", "tags")], "a159take", 90, wide_copy(), stream_default()),
        )
        .await
        .expect("takeover open");
        let old_generation = session.target_generation();
        let run = session.run_id().to_string();
        let _ = session.pull_and_apply(&mut old).await.expect("one batch");
        session.fail_fence(&mut old).await.expect("fail fence");
        let mut newer = self.runtime().await;
        let owner_new = text(&mut self.target, "SELECT gen_random_uuid()::text").await;
        let new_generation = adopt_generation(&mut newer, "a159take", &owner_new).await.expect("takeover");
        assert!(new_generation > old_generation, "{new_generation} vs {old_generation}");
        let err = session.pull_and_apply(&mut old).await.expect_err("stale owner");
        let state = err.sqlstate().unwrap_or("");
        assert!(state == "OO011" || state == "OO018" || state == "OO013", "{err}");
        let before_finish = run_state(&mut self.target, &run).await;
        assert_ne!(before_finish, "awaiting_publish");
        let finish_err = session.finish(&mut old).await.expect_err("finish after failed fence");
        let after_finish = run_state(&mut self.target, &run).await;
        assert_ne!(after_finish, "awaiting_publish");
        drop(old);
        drop(newer);
        self.report.push_str(&format!(
            "same_shard=blocked\nother_shard=imported\ntakeover=incremented\nfailed_fence=unsealed\nfailed_fence_state_before_finish={before_finish}\nfailed_fence_finish={finish_err}\nfailed_fence_state={after_finish}\n"
        ));
        self.flush();
    }

    async fn durability(&mut self) {
        let admitted = load_admitted(&mut self.source, &[("crates", true)]).await;
        let mut runtime = self.runtime().await;
        let mut session = ImportSession::open(
            &mut runtime,
            self.request(&admitted, &[("yard", "crates")], "a159roll", 90, wide_copy(), stream_default()),
        )
        .await
        .expect("rollback open");
        let pending = session.stage_next_uncommitted(&mut runtime).await.expect("stage");
        let identity = pending.identity().clone();
        let visible = text(
            &mut self.target,
            &format!("SELECT count(*)::text FROM _oo.{}", quote_physical(&identity.physical_name)),
        )
        .await;
        assert_eq!(visible, "0");
        let quality_before = text(&mut self.target, "SELECT count(*)::text FROM _oo.full_import_quality").await;
        pending.rollback().await.expect("rollback");
        assert_eq!(
            text(&mut self.target, &format!("SELECT count(*)::text FROM _oo.{}", quote_physical(&identity.physical_name))).await,
            "0"
        );
        assert_eq!(text(&mut self.target, "SELECT count(*)::text FROM _oo.full_import_quality").await, quality_before);
        assert_eq!(
            recover(&mut runtime, &identity.source_id, &identity.physical_name, &identity.batch_id, &identity.payload_digest)
                .await
                .expect("recover rollback"),
            Recovery::Absent
        );
        session.retire(&mut runtime).await.expect("retire rollback");
        drop(runtime);

        let mut runtime = self.runtime().await;
        let mut session = ImportSession::open(
            &mut runtime,
            self.request(&admitted, &[("yard", "crates")], "a159before", 90, wide_copy(), stream_default()),
        )
        .await
        .expect("crash before");
        let pending = session.stage_next_uncommitted(&mut runtime).await.expect("stage before");
        let identity = pending.identity().clone();
        let pid = i32::try_from(pending.backend_pid()).expect("backend");
        let killed: bool = sqlx::query_scalar("SELECT pg_catalog.pg_terminate_backend($1)")
            .bind(pid)
            .fetch_one(&mut self.target)
            .await
            .expect("terminate writer");
        assert!(killed);
        let _ = pending.rollback().await;
        assert_eq!(
            recover(&mut self.target, &identity.source_id, &identity.physical_name, &identity.batch_id, &identity.payload_digest)
                .await
                .expect("recover before"),
            Recovery::Absent
        );
        drop(runtime);
        drop(session);

        let mut runtime = self.runtime().await;
        let mut session = ImportSession::open(
            &mut runtime,
            self.request(&admitted, &[("yard", "crates")], "a159after", 90, wide_copy(), stream_default()),
        )
        .await
        .expect("crash after");
        let pending = session.stage_next_uncommitted(&mut runtime).await.expect("stage after");
        let identity = pending.commit_ambiguous().await.expect("ambiguous");
        let count_sql = format!("SELECT count(*)::text FROM _oo.{}", quote_physical(&identity.physical_name));
        let committed = text(&mut self.target, &count_sql).await;
        assert_ne!(committed, "0");
        let bad = "b".repeat(64);
        abort_conflicting(&mut runtime, session.target_generation(), &identity, &bad)
            .await
            .expect("conflict");
        assert_eq!(text(&mut self.target, &count_sql).await, committed);
        assert_eq!(
            recover(&mut self.target, &identity.source_id, &identity.physical_name, &identity.batch_id, &identity.payload_digest)
                .await
                .expect("recover after"),
            Recovery::Replay
        );
        assert_eq!(
            recover(&mut self.target, &identity.source_id, &identity.physical_name, &identity.batch_id, &bad)
                .await
                .expect("conflict recover"),
            Recovery::Conflict
        );
        session.retire(&mut runtime).await.expect("retire after");
        drop(runtime);
        let mut runtime = self.runtime().await;
        let generation = adopt_generation(&mut runtime, "a159after", &identity.owner)
            .await
            .expect("replay adopt");
        let step = replay_batch(&mut runtime, generation, &identity).await.expect("replay");
        assert_eq!(step, ApplyStep::Replay { batch_id: identity.batch_id.clone() });
        assert_eq!(text(&mut self.target, &count_sql).await, committed);
        drop(runtime);
        self.report.push_str("rollback=absent\ncrash_before=absent\ncrash_after=replay\nconflict=aborted\nreplay_rows=unchanged\n");
        self.flush();
    }

    async fn reader_wrote_nothing(&mut self) {
        let writes = text(
            &mut self.source,
            "SELECT count(*)::text FROM yard.write_audit WHERE who = 'oo_a159_reader'",
        )
        .await;
        assert_eq!(writes, "0");
        let writer_rows = text(
            &mut self.source,
            "SELECT count(*)::text FROM yard.write_audit WHERE who = 'oo_a159_writer'",
        )
        .await;
        assert_eq!(writer_rows, "1");
        let allowed = text(
            &mut self.source,
            "SELECT has_table_privilege('oo_a159_reader', 'yard.crates', 'INSERT')::text",
        )
        .await;
        assert_eq!(allowed, "false");
        self.report.push_str("source_reader_writes=0\nsource_audit=writer_only\n");
        self.flush();
    }

    async fn receipt_only(&mut self) {
        let before = text(&mut self.target, "SELECT count(*)::text FROM _oo.full_import_run WHERE state = 'awaiting_publish'").await;
        let receipts = text(&mut self.target, "SELECT count(*)::text FROM _oo.apply_receipts").await;
        let mut runtime = self.runtime().await;
        let err = sqlx::query(
            "SELECT _oo.commit_apply_receipt('a159solo', '00000000-0000-0000-0000-00000000000a'::uuid, 1, 'a159src', 'pt_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 'solo.b0', repeat('aa', 32), repeat('bb', 32), repeat('cc', 32), repeat('dd', 32), repeat('ee', 32), 'snapshot-zero/v1', 'full-import.snapshot', 0, 0, false, false, true, NULL, NULL, ''::bytea)",
        )
        .execute(&mut runtime)
        .await;
        assert_blocked(err.err());
        assert_eq!(text(&mut self.target, "SELECT count(*)::text FROM _oo.apply_receipts").await, receipts);
        assert_eq!(
            text(&mut self.target, "SELECT count(*)::text FROM _oo.full_import_run WHERE state = 'awaiting_publish'").await,
            before
        );
        drop(runtime);
        self.report.push_str("registration_only=not_complete\n");
        self.flush();
    }

    fn flush(&self) {
        fs::write(self.evidence.join("outcome.txt"), &self.report).expect("outcome");
    }

    async fn reset_cluster(&mut self) {
        for name in ["oo_a159_gate", "oo_a159_lie", "oo_a159_src", "oo_a159_dst"] {
            raw(&mut self.postgres, &format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await;
        }
        for role in ["oo_a159_reader", "oo_a159_writer", "oo_a159_runtime", "oo_a159_agent"] {
            raw(&mut self.postgres, &format!("DROP ROLE IF EXISTS {role}")).await;
        }
        raw(&mut self.postgres, "DROP ROLE IF EXISTS oo_pipeline_runtime").await;
        raw(&mut self.postgres, "DROP ROLE IF EXISTS oo_target_owner").await;
    }

    async fn create_db(&mut self, name: &str) {
        assert!(name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_'));
        raw(
            &mut self.postgres,
            &format!("CREATE DATABASE {name} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'"),
        )
        .await;
    }

    async fn create_login(&mut self, name: &str, secret: &str) {
        assert!(name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_'));
        assert!(secret.chars().all(|ch| ch.is_ascii_hexdigit()));
        raw(
            &mut self.postgres,
            &format!("CREATE ROLE {name} LOGIN PASSWORD '{secret}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION"),
        )
        .await;
    }

    async fn admin_db(&self, database: &str) -> PgConnection {
        dial(&self.host, self.port, &self.admin_user, &self.admin_password, database, &self.ca).await
    }

    async fn runtime(&self) -> PgConnection {
        dial(&self.host, self.port, "oo_a159_runtime", &self.runtime_password, "oo_a159_dst", &self.ca).await
    }

    async fn agent(&self) -> PgConnection {
        dial(&self.host, self.port, "oo_a159_agent", &self.agent_password, "oo_a159_dst", &self.ca).await
    }

    fn request(
        &self,
        admitted: &AdmittedCatalog,
        tables: &[(&str, &str)],
        shard: &str,
        deadline_secs: u64,
        copy: CopyBudgets,
        stream: StreamBudget,
    ) -> ImportRequest {
        let selected = tables
            .iter()
            .map(|(schema, table)| SelectedTable::new(*schema, *table).expect("table"))
            .collect();
        let config = PgSourceConfig::try_new(
            SourceId::new(SOURCE).expect("source"),
            PgEndpoint::new(&self.host, self.port, "oo_a159_src").expect("endpoint"),
            PgCredentials::new("oo_a159_reader", &self.reader_password).expect("credentials"),
            SecretRef::new("secret://a159/reader").expect("secret"),
            PgTlsConfig::from_ca_pem(&self.ca).expect("tls"),
            PgBudget::new(Duration::from_secs(30), Duration::from_secs(60), Duration::from_secs(30), 1000, 8 * 1024 * 1024).expect("budget"),
            TableSelection::new(selected).expect("selection"),
        )
        .expect("config");
        ImportRequest {
            shard: shard.to_string(),
            owner: String::new(),
            snapshot: SnapshotRequest {
                config_identity: config_identity(&config),
                type_policy_identity: type_policy_identity(PgTypePolicy::PRESERVE_ROW),
                config,
                admitted: admitted.clone(),
                policy: PgTypePolicy::PRESERVE_ROW,
                generation: NEXT_GEN.fetch_add(1, Ordering::Relaxed),
                deadline: Instant::now() + Duration::from_secs(deadline_secs),
                expected_principal: "oo_a159_reader".to_string(),
                expected_database: "oo_a159_src".to_string(),
                expected_in_recovery: self.in_recovery,
                expected_postmaster_start: self.postmaster.clone(),
            },
            stream,
            copy,
        }
    }
}

async fn pull_all(session: &mut ImportSession, conn: &mut PgConnection) -> Result<(), ImportError> {
    loop {
        match session.pull_and_apply(conn).await? {
            ApplyStep::Exhausted => return Ok(()),
            ApplyStep::Applied { .. } | ApplyStep::Replay { .. } | ApplyStep::TableDone { .. } => {}
        }
    }
}

const SOURCE_SQL: &str = "\
CREATE SCHEMA yard;\n\
CREATE TABLE yard.crates (\n\
  crate_no int4 PRIMARY KEY,\n\
  sealed bool NOT NULL,\n\
  mass numeric(8,3) NOT NULL,\n\
  label text,\n\
  opened timestamp NOT NULL,\n\
  stamp bytea NOT NULL,\n\
  mark char(3) NOT NULL,\n\
  ratio float4 NOT NULL\n\
);\n\
CREATE TABLE yard.tags (\n\
  tag_id int8 PRIMARY KEY,\n\
  qty int2 NOT NULL,\n\
  token uuid NOT NULL,\n\
  made date NOT NULL,\n\
  shift time NOT NULL,\n\
  density float8 NOT NULL,\n\
  note varchar(12) NOT NULL,\n\
  doc json NOT NULL\n\
);\n\
CREATE TABLE yard.bulk (\n\
  id int4 PRIMARY KEY,\n\
  blob text NOT NULL\n\
);\n\
CREATE TABLE yard.notes (\n\
  id int4 PRIMARY KEY,\n\
  body jsonb NOT NULL\n\
);\n\
CREATE TABLE yard.write_audit (\n\
  who text NOT NULL,\n\
  at timestamptz NOT NULL DEFAULT clock_timestamp()\n\
);\n\
CREATE FUNCTION yard.note_write() RETURNS trigger\n\
LANGUAGE plpgsql AS $audit$\n\
BEGIN\n\
  INSERT INTO yard.write_audit (who) VALUES (current_user);\n\
  RETURN NEW;\n\
END;\n\
$audit$;\n\
CREATE TRIGGER crates_audit BEFORE INSERT OR UPDATE OR DELETE ON yard.crates\n\
  FOR EACH ROW EXECUTE FUNCTION yard.note_write();\n\
CREATE TRIGGER tags_audit BEFORE INSERT OR UPDATE OR DELETE ON yard.tags\n\
  FOR EACH ROW EXECUTE FUNCTION yard.note_write();\n\
INSERT INTO yard.crates (crate_no, sealed, mass, label, opened, stamp, mark, ratio) VALUES\n\
  (1, true, 12.500, 'spruce-keel', timestamp '2024-07-08 09:10:11', '\\x0a0b', 'ab', 1.5),\n\
  (2, false, 0.125, NULL, timestamp '2024-08-09 10:11:12', '\\x00ff', 'xyz', 2.5),\n\
  (4, true, 9.000, 'oak-rib', timestamp '2024-09-10 11:12:13', '\\x10', 'q', 1.5);\n\
INSERT INTO yard.tags (tag_id, qty, token, made, shift, density, note, doc) VALUES\n\
  (70, 4, 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee', date '2024-09-10', time '13:14:15', 3.5, 'cedar', '{\"k\":\"v\"}'::json),\n\
  (71, 7, 'bbbbbbbb-cccc-4ddd-8eee-ffffffffffff', date '2024-10-11', time '01:02:03', 0.5, 'pine', '{\"n\":2}'::json);\n\
INSERT INTO yard.bulk (id, blob) VALUES (1, repeat('z', 400));\n\
INSERT INTO yard.notes (id, body) VALUES (1, '{\"a\":1}'::jsonb);\n\
";

fn projection_source_sql(table: &str) -> &'static str {
    if table == "crates" {
        "SELECT crate_no::text || '|' || sealed::text || '|' || mass::text || '|' || COALESCE(label, '<null>') || '|' || opened::text || '|' || encode(stamp, 'hex') || '|' || mark::text || '|' || ratio::text FROM yard.crates ORDER BY crate_no"
    } else {
        "SELECT tag_id::text || '|' || qty::text || '|' || token::text || '|' || made::text || '|' || shift::text || '|' || density::text || '|' || note::text || '|' || doc::text FROM yard.tags ORDER BY tag_id"
    }
}

async fn projection_source(conn: &mut PgConnection, table: &str) -> Vec<String> {
    column(conn, projection_source_sql(table)).await
}

async fn projection_target(conn: &mut PgConnection, table: &oo_pipeline::full_import::TableSummary) -> Vec<String> {
    let shadow = quote_physical(&table.physical_name);
    let col = |name: &str| quote_physical(&find_column(table, name));
    let sql = if table.name == "crates" {
        format!(
            "SELECT {id}::text || '|' || {sealed}::text || '|' || {mass}::text || '|' || COALESCE({label}::text, '<null>') || '|' || {opened}::text || '|' || encode({stamp}, 'hex') || '|' || {mark}::text || '|' || {ratio}::text FROM _oo.{shadow} ORDER BY {id}",
            id = col("crate_no"),
            sealed = col("sealed"),
            mass = col("mass"),
            label = col("label"),
            opened = col("opened"),
            stamp = col("stamp"),
            mark = col("mark"),
            ratio = col("ratio"),
        )
    } else {
        format!(
            "SELECT {id}::text || '|' || {qty}::text || '|' || {token}::text || '|' || {made}::text || '|' || {shift}::text || '|' || {density}::text || '|' || {note}::text || '|' || {doc}::text FROM _oo.{shadow} ORDER BY {id}",
            id = col("tag_id"),
            qty = col("qty"),
            token = col("token"),
            made = col("made"),
            shift = col("shift"),
            density = col("density"),
            note = col("note"),
            doc = col("doc"),
        )
    };
    column(conn, &sql).await
}

fn oracle_digest(rows: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"a159-oracle-v1\n");
    for row in rows {
        hasher.update(row.as_bytes());
        hasher.update(b"\n");
    }
    hex_bytes(hasher.finalize().as_ref())
}

async fn load_admitted(admin: &mut PgConnection, tables: &[(&str, bool)]) -> AdmittedCatalog {
    let mut out = Vec::new();
    for &(name, keyed) in tables {
        let rows = sqlx::query(
            "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull \
             FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'yard' AND c.relname = $1 AND a.attnum > 0 AND NOT a.attisdropped \
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
        let primary_key = if keyed {
            let keys = sqlx::query(
                "SELECT a.attname::text \
                 FROM pg_catalog.pg_index i \
                 JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS u(attnum, ord) ON true \
                 JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = u.attnum \
                 WHERE n.nspname = 'yard' AND c.relname = $1 AND i.indisprimary \
                 ORDER BY u.ord",
            )
            .bind(name)
            .fetch_all(&mut *admin)
            .await
            .expect("keys");
            keys.iter().map(|row| row.try_get::<String, _>(0).unwrap()).collect()
        } else {
            Vec::new()
        };
        out.push(AdmittedTable {
            schema: "yard".to_string(),
            name: name.to_string(),
            columns,
            primary_key,
        });
    }
    AdmittedCatalog { tables: out }
}

fn find_column(table: &oo_pipeline::full_import::TableSummary, name: &str) -> String {
    table
        .columns
        .iter()
        .find(|column| column.name == name)
        .unwrap_or_else(|| panic!("column {name}"))
        .physical_name
        .clone()
}

fn quote_physical(name: &str) -> String {
    assert!(
        name.len() == 63
            && (name.starts_with("pt_") || name.starts_with("pc_") || name.starts_with("pn_"))
            && name.bytes().skip(3).all(|byte| byte.is_ascii_hexdigit())
    );
    format!("\"{name}\"")
}

fn wide_copy() -> CopyBudgets {
    CopyBudgets {
        max_value_bytes: 1_048_576,
        max_row_bytes: 4_194_304,
        max_chunk_bytes: 8_388_608,
        max_rows_per_chunk: 1024,
    }
}

fn stream_default() -> StreamBudget {
    StreamBudget::try_new(100, 1_048_576, 1_048_576, 2_097_152).expect("stream")
}

fn password(var: &str) -> String {
    let path = env::var(var).unwrap_or_else(|_| panic!("{var}"));
    let text = fs::read_to_string(path).expect("password file");
    let trimmed = text.trim().to_string();
    assert!(trimmed.chars().all(|ch| ch.is_ascii_hexdigit()));
    trimmed
}

async fn dial(host: &str, port: u16, user: &str, pass: &str, database: &str, ca: &[u8]) -> PgConnection {
    let options = PgConnectOptions::new()
        .host(host)
        .port(port)
        .username(user)
        .password(pass)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    PgConnection::connect_with(&options).await.expect("dial")
}

async fn raw(conn: &mut PgConnection, sql: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("sql failed: {err}"));
}

async fn text(conn: &mut PgConnection, sql: &str) -> String {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("scalar failed: {err}"))
}

async fn column(conn: &mut PgConnection, sql: &str) -> Vec<String> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_all(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("rows failed: {err}"));
    rows.iter().map(|row| row.try_get(0).expect("col")).collect()
}

async fn scalar_bind(conn: &mut PgConnection, sql: &str, value: &str) -> String {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .bind(value)
        .fetch_one(&mut *conn)
        .await
        .expect("bind scalar")
}

async fn run_state(conn: &mut PgConnection, run_id: &str) -> String {
    scalar_bind(conn, "SELECT state FROM _oo.full_import_run WHERE run_id = $1::uuid", run_id).await
}

async fn applied_stamp(conn: &mut PgConnection) -> String {
    text(
        conn,
        "SELECT string_agg(version || '|' || checksum || '|' || applied_at::text, E'\\n' ORDER BY version) FROM _oo.schema_migrations",
    )
    .await
}

fn stamp_of(stamp: &str, version: &str) -> String {
    stamp
        .lines()
        .find(|line| line.starts_with(&format!("{version}|")))
        .unwrap_or_else(|| panic!("missing {version}"))
        .to_string()
}

async fn to_regclass(conn: &mut PgConnection, name: &str) -> Option<String> {
    sqlx::query_scalar("SELECT to_regclass($1)::text").bind(name).fetch_one(&mut *conn).await.expect("regclass")
}

fn assert_denied(err: Option<sqlx::Error>) {
    let err = err.expect("agent statement succeeded");
    let state = sqlstate_of(&err);
    assert!(state == "42501" || state == "3F000", "{err}");
}

fn assert_blocked(err: Option<sqlx::Error>) {
    let err = err.expect("statement succeeded");
    let state = sqlstate_of(&err);
    assert!(
        state == "42501" || state == "3F000" || state == "OO011" || state == "OO013" || state == "OO014" || state == "OO018" || state == "OO031",
        "{err}"
    );
}

fn sqlstate_of(err: &sqlx::Error) -> String {
    err.as_database_error()
        .and_then(|db| db.code().map(|code| code.into_owned()))
        .unwrap_or_default()
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
