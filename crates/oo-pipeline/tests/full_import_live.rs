//! Unpublished full import against one private PostgreSQL 17.
//!
//! Source and target are two databases on that server. The ignored test uses
//! the public entry: source snapshot, reader, encoder, one target connection,
//! binary COPY, receipt, then `RunComplete`.

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
use oo_pipeline::target_installer::{
    install, install_full_import, verify_full_import_readiness, FullImportInstallOutcome,
    InstallOutcome,
};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{Connection, Row};

const SOURCE: &str = "a154src";
static NEXT_GEN: AtomicU64 = AtomicU64::new(1);

#[tokio::test]
#[ignore = "one private PostgreSQL 17.10; OO_FULL_IMPORT_HOST must be set by the deciding run"]
async fn unpublished_full_import_on_one_postgres() {
    let mut harness = Harness::connect().await;
    harness.setup().await;
    harness.main_import().await;
    harness.agent_denies().await;
    harness.backpressure().await;
    harness.oversized().await;
    harness.unsupported().await;
    harness.coherent_cut().await;
    harness.cancel_deadline_and_loss().await;
    harness.fence_cases().await;
    harness.durability().await;
    harness.reader_wrote_nothing().await;
    harness.report.push_str("full_import_live_ok\n");
    fs::write(harness.evidence.join("outcome.txt"), &harness.report).expect("outcome");
    print!("{}", harness.report);
    harness.cleanup().await;
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
    main_names: Vec<String>,
}

impl Harness {
    async fn connect() -> Self {
        let host = env::var("OO_FULL_IMPORT_HOST").expect("OO_FULL_IMPORT_HOST");
        let port: u16 = env::var("OO_FULL_IMPORT_PORT")
            .expect("OO_FULL_IMPORT_PORT")
            .parse()
            .expect("port");
        assert!(!host.contains(':') && !host.contains('/') && !host.contains('@'));
        let admin_user = env::var("OO_FULL_IMPORT_ADMIN_USER").expect("OO_FULL_IMPORT_ADMIN_USER");
        let admin_password = password("OO_FULL_IMPORT_ADMIN_PASSWORD_FILE");
        let reader_password = password("OO_FULL_IMPORT_READER_PASSWORD_FILE");
        let runtime_password = password("OO_FULL_IMPORT_RUNTIME_PASSWORD_FILE");
        let ca = fs::read(env::var("OO_FULL_IMPORT_CA_FILE").expect("OO_FULL_IMPORT_CA_FILE")).expect("ca");
        let evidence = PathBuf::from(env::var("OO_FULL_IMPORT_EVIDENCE").expect("OO_FULL_IMPORT_EVIDENCE"));
        fs::create_dir_all(&evidence).expect("evidence dir");
        let mut postgres = dial(&host, port, &admin_user, &admin_password, "postgres", false, &ca).await;
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
        let source = dial(&host, port, &admin_user, &admin_password, "postgres", false, &ca).await;
        let target = dial(&host, port, &admin_user, &admin_password, "postgres", false, &ca).await;
        Self {
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
            main_names: Vec::new(),
        }
    }

    async fn setup(&mut self) {
        reset_cluster(&mut self.postgres).await;
        exec_role(&mut self.postgres, "oo_a154_reader", &self.reader_password, false).await;
        exec_role(&mut self.postgres, "oo_a154_writer", &self.writer_password, false).await;
        exec_role(&mut self.postgres, "oo_a154_runtime", &self.runtime_password, false).await;
        exec_role(&mut self.postgres, "oo_a154_agent", &self.agent_password, false).await;
        raw(&mut self.postgres, "CREATE DATABASE oo_a154_src TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
        raw(&mut self.postgres, "CREATE DATABASE oo_a154_dst TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
        self.source = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            "oo_a154_src",
            false,
            &self.ca,
        )
        .await;
        self.target = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            "oo_a154_dst",
            false,
            &self.ca,
        )
        .await;
        raw(&mut self.source, SOURCE_SQL).await;
        exec_role_grant(&mut self.source).await;
        assert_eq!(install(&mut self.target).await.expect("install"), InstallOutcome::Bootstrapped);
        assert!(verify_full_import_readiness(&mut self.target).await.is_err());
        assert_eq!(
            install_full_import(&mut self.target).await.expect("0005"),
            FullImportInstallOutcome::Installed
        );
        raw(&mut self.target, "GRANT oo_pipeline_runtime TO oo_a154_runtime").await;
        raw(&mut self.target, "GRANT CONNECT ON DATABASE oo_a154_dst TO oo_a154_agent").await;
        let runs = text(&mut self.target, "SELECT count(*)::text FROM _oo.full_import_run").await;
        assert_eq!(runs, "0");
        let row = sqlx::query("SELECT pg_catalog.pg_postmaster_start_time()::text, pg_catalog.pg_is_in_recovery()")
            .fetch_one(&mut self.source)
            .await
            .expect("node");
        self.postmaster = row.try_get(0).expect("postmaster");
        self.in_recovery = row.try_get(1).expect("recovery");
        self.report.push_str("schema_setup_runs=0\n");
    }

    async fn main_import(&mut self) {
        let admitted = load_admitted(&mut self.source, &[("orders", true), ("lines", true)]).await;
        let mut runtime = self.runtime().await;
        let summary = import_unpublished(
            &mut runtime,
            self.request(&admitted, &[("app", "orders"), ("app", "lines")], 90, wide_copy(), stream_default()),
        )
        .await
        .expect("import");
        assert_eq!(summary.state, RunState::AwaitingPublish);
        assert_eq!(summary.source_id, SOURCE);
        assert_eq!(summary.tables.len(), 2);
        let state = text(
            &mut self.target,
            &format!(
                "SELECT state FROM _oo.full_import_run WHERE run_id = '{}'::uuid",
                summary.run_id
            ),
        )
        .await;
        assert_eq!(state, "awaiting_publish");
        let pid = text(&mut runtime, "SELECT pg_catalog.pg_backend_pid()::text").await;
        let bound = text(
            &mut self.target,
            "SELECT backend_pid::text FROM _oo.full_import_session WHERE shard_id = 'a154main'",
        )
        .await;
        assert_eq!(pid, bound);
        self.compare_tables(&summary.tables).await;
        self.main_names = summary.tables.iter().map(|table| table.physical_name.clone()).collect();
        let second = self.canonical_from_reader(&admitted).await;
        for table in &summary.tables {
            let got = second
                .iter()
                .find(|(name, _)| name == &table.name)
                .unwrap_or_else(|| panic!("missing {}", table.name));
            assert_eq!(got.1, table.canonical_digest, "{}", table.name);
            assert_eq!(table.rows, 2, "{}", table.name);
        }
        assert_eq!(text(&mut self.target, "SELECT count(*)::text FROM pg_catalog.pg_publication").await, "0");
        self.report.push_str(&format!(
            "main_state=awaiting_publish\nmain_rows={}\nmain_generation={}\n",
            summary.tables.iter().map(|table| table.rows).sum::<u64>(),
            summary.source_generation
        ));
        drop(runtime);
    }

    async fn compare_tables(&mut self, tables: &[oo_pipeline::full_import::TableSummary]) {
        raw(&mut self.source, "SET TIME ZONE 'UTC'").await;
        raw(&mut self.target, "SET TIME ZONE 'UTC'").await;
        for table in tables {
            assert!(table.physical_name.starts_with("pt_") && table.physical_name.len() == 63);
            let shadow = quote_physical(&table.physical_name);
            let logged = text(
                &mut self.target,
                &format!(
                    "SELECT relpersistence::text FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = '_oo' AND c.relname = '{name}'",
                    name = table.physical_name
                ),
            )
            .await;
            assert_eq!(logged, "p");
            let attrs = text(
                &mut self.target,
                &format!(
                    "SELECT string_agg(a.attname || ':' || a.atttypid::text || ':' || a.atttypmod::text || ':' || a.attnotnull::text, ',' ORDER BY a.attnum) FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = '_oo' AND c.relname = '{name}' AND a.attnum > 0 AND NOT a.attisdropped",
                    name = table.physical_name
                ),
            )
            .await;
            let mut parts = attrs.split(',');
            let position = parts.next().expect("position");
            assert_eq!(position, "_oo_pos:1700:2490372:true");
            let source_types = text(
                &mut self.source,
                &format!(
                    "SELECT string_agg(a.atttypid::text, ',' ORDER BY a.attnum) FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'app' AND c.relname = '{name}' AND a.attnum > 0 AND NOT a.attisdropped",
                    name = table.name
                ),
            )
            .await;
            let target_types: Vec<&str> = parts
                .map(|item| item.split(':').nth(1).unwrap_or(""))
                .collect();
            assert_eq!(target_types.join(","), source_types);
            let source_rows = self.projection_source(&table.name).await;
            let target_rows = self.projection_target(table).await;
            assert_eq!(source_rows, target_rows, "{}", table.name);
            let positions = text(
                &mut self.target,
                &format!("SELECT string_agg(_oo_pos::text, ',' ORDER BY _oo_pos) FROM _oo.{shadow}"),
            )
            .await;
            assert_eq!(positions, "0,1");
        }
    }

    async fn projection_source(&mut self, table: &str) -> Vec<String> {
        let sql = if table == "orders" {
            "SELECT id::text || '|' || active::text || '|' || amount::text || '|' || COALESCE(note, '<null>') || '|' || placed::text || '|' || encode(payload, 'hex') FROM app.orders ORDER BY id"
        } else {
            "SELECT line_id::text || '|' || qty::text || '|' || ref::text || '|' || due::text || '|' || weight::text || '|' || label::text FROM app.lines ORDER BY line_id"
        };
        column(&mut self.source, sql).await
    }

    async fn projection_target(&mut self, table: &oo_pipeline::full_import::TableSummary) -> Vec<String> {
        let shadow = quote_physical(&table.physical_name);
        let col = |name: &str| quote_physical(&find_column(table, name));
        let sql = if table.name == "orders" {
            format!(
                "SELECT {id}::text || '|' || {active}::text || '|' || {amount}::text || '|' || COALESCE({note}::text, '<null>') || '|' || {placed}::text || '|' || encode({payload}, 'hex') FROM _oo.{shadow} ORDER BY {id}",
                id = col("id"),
                active = col("active"),
                amount = col("amount"),
                note = col("note"),
                placed = col("placed"),
                payload = col("payload"),
            )
        } else {
            format!(
                "SELECT {id}::text || '|' || {qty}::text || '|' || {reference}::text || '|' || {due}::text || '|' || {weight}::text || '|' || {label}::text FROM _oo.{shadow} ORDER BY {id}",
                id = col("line_id"),
                qty = col("qty"),
                reference = col("ref"),
                due = col("due"),
                weight = col("weight"),
                label = col("label"),
            )
        };
        column(&mut self.target, &sql).await
    }

    async fn canonical_from_reader(&mut self, admitted: &AdmittedCatalog) -> Vec<(String, String)> {
        let request = self.request(
            admitted,
            &[("app", "orders"), ("app", "lines")],
            90,
            wide_copy(),
            stream_default(),
        );
        let mut owner = SnapshotOwner::open(request.snapshot)
            .await
            .expect("second reader");
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
        owner.finish_complete().await.expect("second complete");
        out.into_iter()
            .map(|(name, hasher)| (name, hex_bytes(hasher.finalize().as_ref())))
            .collect()
    }

    async fn agent_denies(&mut self) {
        let mut agent = dial(
            &self.host,
            self.port,
            "oo_a154_agent",
            &self.agent_password,
            "oo_a154_dst",
            false,
            &self.ca,
        )
        .await;
        let shadow = quote_physical(&self.main_names[0]);
        let select_shadow = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT 1 FROM _oo.{shadow}")))
            .fetch_optional(&mut agent)
            .await;
        assert_denied(select_shadow.err());
        let select_quality = sqlx::query("SELECT 1 FROM _oo.full_import_quality")
            .fetch_optional(&mut agent)
            .await;
        assert_denied(select_quality.err());
        let insert_shadow = sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO _oo.{shadow} (_oo_pos) VALUES (1)"
        )))
            .execute(&mut agent)
            .await;
        assert_denied(insert_shadow.err());
        self.report.push_str("agent_denied=1\n");
    }

    async fn backpressure(&mut self) {
        let admitted = load_admitted(&mut self.source, &[("orders", true)]).await;
        let mut runtime = self.runtime().await;
        let summary = import_unpublished(
            &mut runtime,
            self.request(
                &admitted,
                &[("app", "orders")],
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
        assert_eq!(summary.tables[0].rows, 2);
        assert!(summary.tables[0].row_chunks >= 2, "{}", summary.tables[0].row_chunks);
        self.report.push_str(&format!("backpressure_chunks={}\n", summary.tables[0].row_chunks));
    }

    async fn oversized(&mut self) {
        let before = text(&mut self.target, "SELECT count(*)::text FROM _oo.full_import_run WHERE state = 'awaiting_publish'").await;
        let admitted = load_admitted(&mut self.source, &[("wide", true)]).await;
        let mut runtime = self.runtime().await;
        let mut session = ImportSession::open(
            &mut runtime,
            self.request(
                &admitted,
                &[("app", "wide")],
                90,
                wide_copy(),
                StreamBudget::try_new(1, 4096, 16, 32).expect("tiny"),
            ),
        )
        .await
        .expect("wide open");
        let err = pull_all(&mut session, &mut runtime).await.expect_err("oversized");
        assert!(err.to_string().contains("source error"), "{err}");
        let state = run_state(&mut self.target, session.run_id()).await;
        assert_eq!(state, "retired");
        let after = text(&mut self.target, "SELECT count(*)::text FROM _oo.full_import_run WHERE state = 'awaiting_publish'").await;
        assert_eq!(before, after);
        self.report.push_str("oversized=retired\n");
    }

    async fn unsupported(&mut self) {
        let shadows = text(
            &mut self.target,
            "SELECT count(*)::text FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = '_oo' AND c.relname LIKE 'pt\\_%'",
        )
        .await;
        let admitted = load_admitted(&mut self.source, &[("docs", true)]).await;
        let mut runtime = self.runtime().await;
        let opened = ImportSession::open(
            &mut runtime,
            self.request(&admitted, &[("app", "docs")], 90, wide_copy(), stream_default()),
        )
        .await;
        let err = match opened {
            Err(err) => err,
            Ok(_) => panic!("jsonb was accepted"),
        };
        match err {
            ImportError::Unsupported { oid, .. } => assert_eq!(oid, 3802),
            other => panic!("jsonb was not refused: {other}"),
        }
        let shadows_after = text(
            &mut self.target,
            "SELECT count(*)::text FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = '_oo' AND c.relname LIKE 'pt\\_%'",
        )
        .await;
        assert_eq!(shadows, shadows_after);
        self.report.push_str("unsupported_jsonb=refused\n");
    }

    async fn coherent_cut(&mut self) {
        let admitted = load_admitted(&mut self.source, &[("orders", true)]).await;
        let mut runtime = self.runtime().await;
        let mut session = ImportSession::open(
            &mut runtime,
            self.request(&admitted, &[("app", "orders")], 90, wide_copy(), stream_default()),
        )
        .await
        .expect("cut open");
        let mut writer = dial(
            &self.host,
            self.port,
            "oo_a154_writer",
            &self.writer_password,
            "oo_a154_src",
            true,
            &self.ca,
        )
        .await;
        raw(
            &mut writer,
            "INSERT INTO app.orders (id, active, amount, note, placed, payload) VALUES (3, true, 1.00, 'late', timestamptz '2024-06-01 00:00:00+00', '\\x03')",
        )
        .await;
        pull_all(&mut session, &mut runtime).await.expect("cut pull");
        let finished = session.finish(&mut runtime).await.expect("cut finish");
        assert_eq!(finished.tables[0].rows, 2);
        drop(runtime);
        let admitted_fresh = load_admitted(&mut self.source, &[("orders", true)]).await;
        let mut runtime = self.runtime().await;
        let fresh = import_unpublished(
            &mut runtime,
            self.request(&admitted_fresh, &[("app", "orders")], 90, wide_copy(), stream_default()),
        )
        .await
        .expect("fresh cut");
        assert_eq!(fresh.tables[0].rows, 3);
        assert_ne!(fresh.tables[0].physical_name, finished.tables[0].physical_name);
        self.report.push_str("coherent_cut=2_then_3\n");
    }

    async fn cancel_deadline_and_loss(&mut self) {
        let admitted = load_admitted(&mut self.source, &[("lines", true)]).await;
        let mut runtime = self.runtime().await;
        let mut session = ImportSession::open(
            &mut runtime,
            self.request(&admitted, &[("app", "lines")], 90, wide_copy(), stream_default()),
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
            self.request(&admitted, &[("app", "lines")], 30, wide_copy(), stream_default()),
        )
        .await
        .expect("deadline open");
        let run = session.run_id().to_string();
        tokio::time::sleep(Duration::from_secs(31)).await;
        let err = pull_all(&mut session, &mut runtime).await.expect_err("deadline");
        assert!(err.to_string().contains("deadline"), "{err}");
        assert_eq!(run_state(&mut self.target, &run).await, "retired");
        drop(runtime);

        let mut runtime = self.runtime().await;
        let mut session = ImportSession::open(
            &mut runtime,
            self.request(&admitted, &[("app", "lines")], 90, wide_copy(), stream_default()),
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
        assert!(err.to_string().contains("source error"), "{err}");
        assert_eq!(run_state(&mut self.target, &run).await, "retired");
        self.report.push_str("cancel=retired\ndeadline=retired\nsnapshot_lost=retired\n");
    }

    async fn fence_cases(&mut self) {
        let admitted = load_admitted(&mut self.source, &[("lines", true)]).await;
        let mut held = self.runtime().await;
        let mut session = ImportSession::open(
            &mut held,
            self.request_on(&admitted, &[("app", "lines")], "a154same", 90),
        )
        .await
        .expect("same open");
        let mut other = self.runtime().await;
        let owner = uuid(&mut self.target).await;
        let failed = adopt_generation(&mut other, "a154same", &owner).await;
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
            self.request_on(&admitted, &[("app", "lines")], "a154other", 90),
        )
        .await
        .expect("other shard");
        assert_eq!(imported.state, RunState::AwaitingPublish);
        assert!(!self.main_names.contains(&imported.tables[0].physical_name));
        session.retire(&mut held).await.expect("retire held");
        drop(held);
        drop(other);

        let mut old = self.runtime().await;
        let owner_old = uuid(&mut self.target).await;
        let mut session = ImportSession::open(
            &mut old,
            self.request_on(&admitted, &[("app", "lines")], "a154take", 90),
        )
        .await
        .expect("takeover open");
        let old_generation = session.target_generation();
        let _ = session.pull_and_apply(&mut old).await.expect("one batch");
        session.fail_fence(&mut old).await.expect("fail fence");
        let mut newer = self.runtime().await;
        let owner_new = uuid(&mut self.target).await;
        let new_generation = adopt_generation(&mut newer, "a154take", &owner_new).await.expect("takeover");
        assert!(new_generation > old_generation, "{new_generation} vs {old_generation}");
        let err = session.pull_and_apply(&mut old).await.expect_err("stale owner");
        let state = err.sqlstate().unwrap_or("");
        assert!(state == "OO011" || state == "OO018", "{err}");
        let _ = owner_old;
        self.report.push_str("same_shard=blocked\nother_shard=imported\ntakeover=incremented\n");
    }

    async fn durability(&mut self) {
        let admitted = load_admitted(&mut self.source, &[("orders", true)]).await;
        let mut runtime = self.runtime().await;
        let mut session = ImportSession::open(
            &mut runtime,
            self.request_on(&admitted, &[("app", "orders")], "a154roll", 90),
        )
        .await
        .expect("rollback open");
        let pending = session.stage_next_uncommitted(&mut runtime).await.expect("stage");
        let identity = pending.identity().clone();
        let visible: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*)::int8 FROM _oo.{}",
            quote_physical(&identity.physical_name)
        )))
        .fetch_one(&mut self.target)
        .await
        .expect("visible");
        assert_eq!(visible, 0);
        pending.rollback().await.expect("rollback");
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
            self.request_on(&admitted, &[("app", "orders")], "a154before", 90),
        )
        .await
        .expect("crash before open");
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
            recover(
                &mut self.target,
                &identity.source_id,
                &identity.physical_name,
                &identity.batch_id,
                &identity.payload_digest
            )
            .await
            .expect("recover before"),
            Recovery::Absent
        );
        drop(runtime);
        drop(session);

        let mut runtime = self.runtime().await;
        let mut session = ImportSession::open(
            &mut runtime,
            self.request_on(&admitted, &[("app", "orders")], "a154after", 90),
        )
        .await
        .expect("crash after open");
        let pending = session.stage_next_uncommitted(&mut runtime).await.expect("stage after");
        let identity = pending.commit_ambiguous().await.expect("ambiguous");
        let count_sql = format!(
            "SELECT count(*)::text FROM _oo.{}",
            quote_physical(&identity.physical_name)
        );
        let committed = text(&mut self.target, &count_sql).await;
        let bad = "a".repeat(64);
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
        session.retire(&mut runtime).await.expect("retire after");
        drop(runtime);
        let mut runtime = self.runtime().await;
        let generation = adopt_generation(&mut runtime, "a154after", &identity.owner)
            .await
            .expect("replay adopt");
        let step = replay_batch(&mut runtime, generation, &identity).await.expect("replay");
        assert_eq!(
            step,
            ApplyStep::Replay {
                batch_id: identity.batch_id.clone()
            }
        );
        assert_eq!(text(&mut self.target, &count_sql).await, committed);
        self.report.push_str("rollback=absent\ncrash_before=absent\ncrash_after=replay\nconflict=aborted\n");
    }

    async fn reader_wrote_nothing(&mut self) {
        let writes = text(
            &mut self.source,
            "SELECT count(*)::text FROM app.write_audit WHERE who = 'oo_a154_reader'",
        )
        .await;
        assert_eq!(writes, "0");
        let allowed = text(
            &mut self.source,
            "SELECT has_table_privilege('oo_a154_reader', 'app.orders', 'INSERT')::text",
        )
        .await;
        assert_eq!(allowed, "false");
        self.report.push_str("source_reader_writes=0\n");
    }

    async fn cleanup(&mut self) {
        reset_cluster(&mut self.postgres).await;
    }

    async fn runtime(&self) -> PgConnection {
        dial(
            &self.host,
            self.port,
            "oo_a154_runtime",
            &self.runtime_password,
            "oo_a154_dst",
            false,
            &self.ca,
        )
        .await
    }

    fn request(
        &self,
        admitted: &AdmittedCatalog,
        tables: &[(&str, &str)],
        deadline_secs: u64,
        copy: CopyBudgets,
        stream: StreamBudget,
    ) -> ImportRequest {
        self.request_shard(admitted, tables, "a154main", deadline_secs, copy, stream)
    }

    fn request_on(
        &self,
        admitted: &AdmittedCatalog,
        tables: &[(&str, &str)],
        shard: &str,
        deadline_secs: u64,
    ) -> ImportRequest {
        self.request_shard(admitted, tables, shard, deadline_secs, wide_copy(), stream_default())
    }

    fn request_shard(
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
            PgEndpoint::new(&self.host, self.port, "oo_a154_src").expect("endpoint"),
            PgCredentials::new("oo_a154_reader", &self.reader_password).expect("credentials"),
            SecretRef::new("secret://a154/reader").expect("secret"),
            PgTlsConfig::from_ca_pem(&self.ca).expect("tls"),
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(60),
                Duration::from_secs(30),
                1000,
                8 * 1024 * 1024,
            )
            .expect("budget"),
            TableSelection::new(selected).expect("selection"),
        )
        .expect("config");
        let policy = PgTypePolicy::PRESERVE_ROW;
        ImportRequest {
            shard: shard.to_string(),
            owner: String::new(),
            snapshot: SnapshotRequest {
                config_identity: config_identity(&config),
                type_policy_identity: type_policy_identity(policy),
                config,
                admitted: admitted.clone(),
                policy,
                generation: NEXT_GEN.fetch_add(1, Ordering::Relaxed),
                deadline: Instant::now() + Duration::from_secs(deadline_secs),
                expected_principal: "oo_a154_reader".to_string(),
                expected_database: "oo_a154_src".to_string(),
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
CREATE SCHEMA app;\n\
CREATE TABLE app.orders (\n\
  id int4 PRIMARY KEY,\n\
  active bool NOT NULL,\n\
  amount numeric(10,2) NOT NULL,\n\
  note text,\n\
  placed timestamptz NOT NULL,\n\
  payload bytea NOT NULL\n\
);\n\
CREATE TABLE app.lines (\n\
  line_id int8 PRIMARY KEY,\n\
  qty int2 NOT NULL,\n\
  ref uuid NOT NULL,\n\
  due date NOT NULL,\n\
  weight float8 NOT NULL,\n\
  label varchar(16) NOT NULL\n\
);\n\
CREATE TABLE app.wide (\n\
  id int4 PRIMARY KEY,\n\
  blob text NOT NULL\n\
);\n\
CREATE TABLE app.docs (\n\
  id int4 PRIMARY KEY,\n\
  body jsonb NOT NULL\n\
);\n\
CREATE TABLE app.write_audit (\n\
  who text NOT NULL,\n\
  at timestamptz NOT NULL DEFAULT clock_timestamp()\n\
);\n\
CREATE FUNCTION app.note_write() RETURNS trigger\n\
LANGUAGE plpgsql AS $audit$\n\
BEGIN\n\
  INSERT INTO app.write_audit (who) VALUES (current_user);\n\
  RETURN NEW;\n\
END;\n\
$audit$;\n\
CREATE TRIGGER orders_audit BEFORE INSERT OR UPDATE OR DELETE ON app.orders\n\
  FOR EACH ROW EXECUTE FUNCTION app.note_write();\n\
CREATE TRIGGER lines_audit BEFORE INSERT OR UPDATE OR DELETE ON app.lines\n\
  FOR EACH ROW EXECUTE FUNCTION app.note_write();\n\
INSERT INTO app.orders (id, active, amount, note, placed, payload) VALUES\n\
  (1, true, 12.34, 'alpha', timestamptz '2024-01-02 03:04:05+00', '\\x0102'),\n\
  (2, false, 0.10, NULL, timestamptz '2024-05-06 07:08:09+00', '\\x00');\n\
INSERT INTO app.lines (line_id, qty, ref, due, weight, label) VALUES\n\
  (10, 2, '11111111-1111-1111-1111-111111111111', date '2024-02-02', 1.5, 'box'),\n\
  (11, 3, '22222222-2222-2222-2222-222222222222', date '2024-03-03', 1.5, 'tape');\n\
INSERT INTO app.wide (id, blob) VALUES (1, repeat('x', 400));\n\
INSERT INTO app.docs (id, body) VALUES (1, '{\"a\":1}'::jsonb);\n\
GRANT USAGE ON SCHEMA app TO oo_a154_reader, oo_a154_writer;\n\
GRANT SELECT ON app.orders, app.lines, app.wide, app.docs TO oo_a154_reader;\n\
GRANT INSERT ON app.orders TO oo_a154_writer;\n\
GRANT INSERT ON app.write_audit TO oo_a154_writer;\n\
";

async fn exec_role_grant(conn: &mut PgConnection) {
    let _ = conn;
}

async fn reset_cluster(admin: &mut PgConnection) {
    for name in ["oo_a154_src", "oo_a154_dst", "oo_a154_bad", "oo_a154_ok"] {
        raw(admin, &format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await;
    }
    for role in ["oo_a154_reader", "oo_a154_writer", "oo_a154_runtime", "oo_a154_agent"] {
        raw(admin, &format!("DROP ROLE IF EXISTS {role}")).await;
    }
    raw(admin, "DROP ROLE IF EXISTS oo_pipeline_runtime").await;
    raw(admin, "DROP ROLE IF EXISTS oo_target_owner").await;
}

async fn exec_role(conn: &mut PgConnection, name: &str, secret: &str, _login_marker: bool) {
    assert!(secret.chars().all(|ch| ch.is_ascii_hexdigit()));
    assert!(name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_'));
    raw(
        conn,
        &format!(
            "CREATE ROLE {name} LOGIN PASSWORD '{secret}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION"
        ),
    )
    .await;
}

fn password(var: &str) -> String {
    let path = env::var(var).unwrap_or_else(|_| panic!("{var}"));
    let text = fs::read_to_string(path).expect("password file");
    let trimmed = text.trim().to_string();
    assert!(trimmed.chars().all(|ch| ch.is_ascii_hexdigit()));
    trimmed
}

async fn dial(
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
    database: &str,
    verify: bool,
    ca: &[u8],
) -> PgConnection {
    let mut options = PgConnectOptions::new()
        .host(host)
        .port(port)
        .username(user)
        .password(pass)
        .database(database);
    options = if verify {
        options.ssl_mode(PgSslMode::VerifyFull).ssl_root_cert_from_pem(ca.to_vec())
    } else {
        options.ssl_mode(PgSslMode::Disable)
    };
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
        .unwrap_or_else(|err| panic!("scalar failed: {err} sql={sql}"))
}

async fn column(conn: &mut PgConnection, sql: &str) -> Vec<String> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_all(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("rows failed: {err}"));
    rows.iter().map(|row| row.try_get(0).expect("col")).collect()
}

async fn uuid(conn: &mut PgConnection) -> String {
    text(conn, "SELECT gen_random_uuid()::text").await
}

async fn run_state(conn: &mut PgConnection, run_id: &str) -> String {
    assert!(run_id.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-'));
    text(
        conn,
        &format!("SELECT state FROM _oo.full_import_run WHERE run_id = '{run_id}'::uuid"),
    )
    .await
}

async fn load_admitted(admin: &mut PgConnection, tables: &[(&str, bool)]) -> AdmittedCatalog {
    let mut out = Vec::new();
    for &(name, keyed) in tables {
        let rows = sqlx::query(
            "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull \
             FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'app' AND c.relname = $1 AND a.attnum > 0 AND NOT a.attisdropped \
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
                 WHERE n.nspname = 'app' AND c.relname = $1 AND i.indisprimary \
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
            schema: "app".to_string(),
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

fn assert_denied(err: Option<sqlx::Error>) {
    let err = err.expect("agent statement succeeded");
    let state = err
        .as_database_error()
        .and_then(|db| db.code().map(|code| code.into_owned()))
        .unwrap_or_default();
    assert!(state == "42501" || state == "3F000", "{err}");
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
