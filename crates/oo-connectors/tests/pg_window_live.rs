//! Live paced read of a 90_000-row logistics table on PostgreSQL 17.
//! `OO_PG_WINDOW_FIXTURE` is required. A missing pointer is a failure.
//! `OO_PG_STREAM_FIXTURE` is not this test.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use arrow_array::{
    Array, BinaryArray, Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray,
};
use arrow_ipc::reader::StreamReader;
use arrow_schema::{DataType, TimeUnit};
use oo_connectors::{
    PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, PgTypePolicy,
    RunBinding, SecretRef, SelectedTable, SnapshotOwner, SourceId, StreamBudget, StreamItem,
    TableSelection, WindowRequest,
};
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};
use zeroize::Zeroizing;

const SOURCE: &str = "oo_dl01w1_38";
const SCHEMA: &str = "public";
const EVENTS: &str = "tracking_events";
const EVENT_ROWS: i64 = 90_000;
const HUB_ROWS: i64 = 2_000;
const SHIPMENT_ROWS: i64 = 8_000;
const SUM_EVENT_ID: i64 = 4_050_045_000;
/// `fetch_all` refuses when a catalog result is longer than `PgBudget::max_rows`.
/// The keyword probe returns the 79 PostgreSQL 17 reserved words before the
/// portal opens. One thousand covers that catalog. `cancel_gate` has one row,
/// so this cap is not the export limit under test.
const CANCEL_CATALOG_ROWS: u32 = 1_000;

#[derive(Deserialize)]
struct Pointer {
    host: String,
    port: u16,
    database: String,
    reader_user: String,
    dba_user: String,
    super_user: String,
    password_dir: String,
    ca_file: String,
    log_file: String,
}

struct World {
    pointer: Pointer,
    reader_pass: Zeroizing<String>,
    ca: Vec<u8>,
    admin: PgConnection,
    work: PathBuf,
    evidence: Option<PathBuf>,
    budget_bin: PathBuf,
    admit_bin: PathBuf,
    notes: Vec<String>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounded_window_reads_the_ninety_thousand_row_table() {
    let mut world = World::start().await;
    world.load_logistics().await;
    world.note_counts().await;
    world.library_precheck().await;
    world.full_read().await;
    world.gate_read_still_refuses().await;
    world.negative_caps().await;
    world.unreadable_table();
    world.cancel_while_locked().await;
    world.deadline_while_locked().await;
    world.same_snapshot_stream().await;
    world.stream_cancel_while_locked().await;
    world.stream_deadline_while_locked().await;
    world.reader_is_quiet().await;
    world.audit_log();
    world.note("live_complete".to_string());
    world.flush();
}

impl World {
    async fn start() -> Self {
        let path = std::env::var("OO_PG_WINDOW_FIXTURE").expect("OO_PG_WINDOW_FIXTURE is required");
        let pointer: Pointer = serde_json::from_str(&fs::read_to_string(&path).expect("pointer"))
            .expect("pointer json");
        let reader_pass = read_pass(&pointer.password_dir, "reader.pass");
        let super_pass = read_pass(&pointer.password_dir, "admin.pass");
        let _dba_pass = read_pass(&pointer.password_dir, "dba.pass");
        let ca = fs::read(&pointer.ca_file).expect("ca");
        let mut admin = dial(
            &pointer,
            &pointer.super_user,
            &super_pass,
            &ca,
            "oo-window-admin",
        )
        .await;
        sqlx::query("SET statement_timeout = 0")
            .execute(&mut admin)
            .await
            .expect("admin timeout");
        // PostgreSQL 15+ caches pg_stat_activity for the transaction after the
        // first read. The first poll happens before the reader blocks, so a
        // cached snapshot never shows the lock wait.
        sqlx::query("SET stats_fetch_consistency = none")
            .execute(&mut admin)
            .await
            .expect("admin stats consistency");
        assert_eq!(pointer.reader_user, "oo_dl01w1_38_reader");
        assert_eq!(pointer.dba_user, "oo_dl01w1_38_dba");
        let evidence = std::env::var("OO_PG_WINDOW_EVIDENCE")
            .ok()
            .map(PathBuf::from);
        let work = evidence.clone().unwrap_or_else(|| {
            std::env::temp_dir().join(format!("oo-pg-window-{}", std::process::id()))
        });
        fs::create_dir_all(&work).expect("work dir");
        let budget_bin = PathBuf::from(
            std::env::var("OO_PG_WINDOW_BUDGET_BIN").expect("OO_PG_WINDOW_BUDGET_BIN is required"),
        );
        let admit_bin = PathBuf::from(
            std::env::var("OO_PG_WINDOW_ADMIT_BIN").expect("OO_PG_WINDOW_ADMIT_BIN is required"),
        );
        Self {
            pointer,
            reader_pass,
            ca,
            admin,
            work,
            evidence,
            budget_bin,
            admit_bin,
            notes: Vec::new(),
        }
    }

    fn note(&mut self, line: String) {
        eprintln!("outcome {line}");
        self.notes.push(line);
    }

    fn flush(&self) {
        if let Some(dir) = &self.evidence {
            let _ = fs::write(dir.join("outcomes.txt"), self.notes.join("\n") + "\n");
        }
    }

    fn save(&self, name: &str, bytes: &[u8]) {
        if let Some(dir) = &self.evidence {
            let _ = fs::write(dir.join(name), bytes);
        }
    }

    async fn load_logistics(&mut self) {
        for sql in LOAD_SQL {
            sqlx::query(*sql)
                .execute(&mut self.admin)
                .await
                .expect("load logistics");
        }
        let row = sqlx::query(
            "SELECT \
             pg_catalog.has_table_privilege($1, 'public.tracking_events', 'SELECT'), \
             pg_catalog.has_table_privilege($1, 'public.tracking_events', 'INSERT'), \
             pg_catalog.has_table_privilege($1, 'public.secret_table', 'SELECT'), \
             pg_catalog.has_table_privilege($1, 'public.cancel_gate', 'SELECT'), \
             pg_catalog.has_table_privilege($1, 'public.cancel_gate', 'INSERT'), \
             pg_catalog.has_database_privilege($1, current_database(), 'TEMP'), \
             pg_catalog.has_database_privilege($1, current_database(), 'CONNECT'), \
             pg_catalog.has_schema_privilege($1, 'public', 'CREATE')",
        )
        .bind(&self.pointer.reader_user)
        .fetch_one(&mut self.admin)
        .await
        .expect("privileges");
        let select: bool = row.try_get(0).unwrap();
        let insert: bool = row.try_get(1).unwrap();
        let secret: bool = row.try_get(2).unwrap();
        let gate_select: bool = row.try_get(3).unwrap();
        let gate_insert: bool = row.try_get(4).unwrap();
        let temp: bool = row.try_get(5).unwrap();
        let connect: bool = row.try_get(6).unwrap();
        let schema_create: bool = row.try_get(7).unwrap();
        assert!(select && gate_select && connect);
        assert!(!insert && !secret && !gate_insert && !temp && !schema_create);
        self.note(format!(
            "privileges select={select} insert={insert} secret_select={secret} cancel_select={gate_select} cancel_insert={gate_insert} temp={temp} connect={connect} schema_create={schema_create}"
        ));
    }

    async fn note_counts(&mut self) {
        let row = sqlx::query(
            "SELECT \
             (SELECT count(*)::int8 FROM public.hubs), \
             (SELECT count(*)::int8 FROM public.shipments), \
             (SELECT count(*)::int8 FROM public.tracking_events), \
             (SELECT count(*)::int8 FROM public.type_probe), \
             (SELECT coalesce(sum(event_id), 0)::int8 FROM public.tracking_events)",
        )
        .fetch_one(&mut self.admin)
        .await
        .expect("counts");
        let hubs: i64 = row.try_get(0).unwrap();
        let shipments: i64 = row.try_get(1).unwrap();
        let events: i64 = row.try_get(2).unwrap();
        let probe: i64 = row.try_get(3).unwrap();
        let sum: i64 = row.try_get(4).unwrap();
        assert_eq!(hubs, HUB_ROWS);
        assert_eq!(shipments, SHIPMENT_ROWS);
        assert_eq!(events, EVENT_ROWS);
        assert_eq!(probe, 1);
        assert_eq!(sum, SUM_EVENT_ID);
        self.note(format!(
            "loaded hubs={hubs} shipments={shipments} tracking_events={events} type_probe={probe} sum_event_id={sum}"
        ));
    }

    fn config(
        &self,
        table: &str,
        max_rows: u32,
        max_bytes: u64,
        statement_ms: u64,
    ) -> PgSourceConfig {
        PgSourceConfig::try_new(
            SourceId::new(SOURCE).unwrap(),
            PgEndpoint::new(
                &self.pointer.host,
                self.pointer.port,
                &self.pointer.database,
            )
            .unwrap(),
            PgCredentials::new(&self.pointer.reader_user, self.reader_pass.as_str()).unwrap(),
            SecretRef::new("secret://oo-dl01w1-38/reader").unwrap(),
            PgTlsConfig::from_ca_pem(&self.ca).unwrap(),
            PgBudget::new(
                Duration::from_millis(30_000),
                Duration::from_millis(statement_ms),
                Duration::from_millis(statement_ms.min(120_000)),
                max_rows,
                max_bytes,
            )
            .unwrap(),
            TableSelection::new(vec![SelectedTable::new(SCHEMA, table).unwrap()]).unwrap(),
        )
        .unwrap()
    }

    async fn library_precheck(&mut self) {
        let config = self.config(EVENTS, 1_000, 1_048_576, 30_000);
        let mut session = PgConnector::new(config.clone())
            .connect()
            .await
            .expect("session");
        let mut guard = PgConnector::new(config).connect().await.expect("guard");
        let arrow = self.work.join("precheck.arrow");
        let _ = fs::remove_file(&arrow);
        let binding = RunBinding {
            source_id: "other_source".to_string(),
            schema: SCHEMA.to_string(),
            table: EVENTS.to_string(),
            run_generation: 1,
            input_digest: [38; 32],
        };
        let err = session
            .read_window(
                &mut guard,
                WindowRequest {
                    binding: &binding,
                    policy: PgTypePolicy::PRESERVE_ROW,
                    deadline: Instant::now() + Duration::from_secs(30),
                    cancel: &AtomicBool::new(false),
                    arrow_out: &arrow,
                    page_rows: 64,
                    snapshot_file: None,
                },
            )
            .await
            .expect_err("foreign source");
        assert_eq!(err.fact(), Some("binding_source"));
        assert!(!arrow.exists());
        let binding = RunBinding {
            source_id: SOURCE.to_string(),
            schema: SCHEMA.to_string(),
            table: "hubs".to_string(),
            run_generation: 1,
            input_digest: [38; 32],
        };
        let err = session
            .read_window(
                &mut guard,
                WindowRequest {
                    binding: &binding,
                    policy: PgTypePolicy::PRESERVE_ROW,
                    deadline: Instant::now() + Duration::from_secs(30),
                    cancel: &AtomicBool::new(false),
                    arrow_out: &arrow,
                    page_rows: 64,
                    snapshot_file: None,
                },
            )
            .await
            .expect_err("table outside the selection");
        assert_eq!(err.fact(), Some("binding_table"));
        let binding = RunBinding {
            source_id: SOURCE.to_string(),
            schema: SCHEMA.to_string(),
            table: EVENTS.to_string(),
            run_generation: 1,
            input_digest: [38; 32],
        };
        let err = session
            .read_window(
                &mut guard,
                WindowRequest {
                    binding: &binding,
                    policy: PgTypePolicy::PRESERVE_ROW,
                    deadline: Instant::now(),
                    cancel: &AtomicBool::new(false),
                    arrow_out: &arrow,
                    page_rows: 64,
                    snapshot_file: None,
                },
            )
            .await
            .expect_err("closed deadline");
        assert_eq!(err.fact(), Some("deadline"));
        assert!(!arrow.exists());
        drop(session);
        drop(guard);
        self.note("library_precheck binding_source binding_table deadline".to_string());
    }

    async fn full_read(&mut self) {
        let arrow = self.work.join("tracking_events.arrow");
        let snapshot = self.work.join("snapshot.txt");
        let _ = fs::remove_file(&arrow);
        let _ = fs::remove_file(&snapshot);
        let partial = partial_of(&arrow);
        let _ = fs::remove_file(&partial);
        let mut args = self.common_args(EVENTS, 100_000, 33_554_432, 120_000, 5_000);
        args.extend([
            "--phase".to_string(),
            "bounded-read".to_string(),
            "--arrow-out".to_string(),
            arrow.display().to_string(),
            "--snapshot-file".to_string(),
            snapshot.display().to_string(),
            "--page-rows".to_string(),
            "4096".to_string(),
            "--deadline-ms".to_string(),
            "900000".to_string(),
            "--expect-rows".to_string(),
            "90000".to_string(),
        ]);
        self.save("bounded-read.argv", format!("{args:?}\n").as_bytes());
        let started = Instant::now();
        let mut child = Command::new(&self.budget_bin)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn bounded-read");
        let snap = wait_file(&mut child, &snapshot, Duration::from_secs(120));
        if !snap {
            let _ = child.kill();
            let output = child.wait_with_output().expect("wait bounded-read");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                !stdout.contains(self.reader_pass.as_str())
                    && !stderr.contains(self.reader_pass.as_str()),
                "bounded-read leaked the reader password"
            );
            self.save("bounded-read.stdout", &output.stdout);
            self.save("bounded-read.stderr", &output.stderr);
            self.save(
                "bounded-read.exit",
                format!("{}\n", output.status.code().unwrap_or(127)).as_bytes(),
            );
            panic!(
                "snapshot file was not published while the export was open exit={} stdout={} stderr={}",
                output.status.code().unwrap_or(127),
                stdout.trim(),
                stderr.trim()
            );
        }
        let oracle = self.oracle_on_snapshot(&snapshot).await;
        let output = child.wait_with_output().expect("wait bounded-read");
        let code = output.status.code().unwrap_or(127);
        self.save("bounded-read.stdout", &output.stdout);
        self.save("bounded-read.stderr", &output.stderr);
        self.save("bounded-read.exit", format!("{code}\n").as_bytes());
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stdout.contains(self.reader_pass.as_str())
                && !stderr.contains(self.reader_pass.as_str()),
            "bounded-read leaked the reader password"
        );
        assert_eq!(code, 0, "bounded-read stdout={stdout} stderr={stderr}");
        let receipt = parse_receipt(stdout.trim());
        assert_eq!(receipt.rows, 90_000);
        assert_eq!(receipt.pages, 22);
        assert!(receipt.max_rows_in_window > 0 && receipt.max_rows_in_window <= 50_000);
        assert!(receipt.windows_used >= 2);
        assert_eq!(
            receipt.snapshot,
            fs::read_to_string(&snapshot).unwrap().trim()
        );
        assert!(!partial.exists(), "partial export remained");
        let digest = sha256_file(&arrow);
        assert_eq!(digest, receipt.arrow_sha256);
        let decoded = decode_events(&arrow);
        assert_eq!(decoded.count, oracle.count);
        assert_eq!(decoded.sum_event_id, oracle.sum_event_id);
        assert_eq!(decoded.sum_event_id, SUM_EVENT_ID);
        assert_eq!(decoded.min_event_id, oracle.min_event_id);
        assert_eq!(decoded.max_event_id, oracle.max_event_id);
        assert_eq!(oracle.min_event_id, 1);
        assert_eq!(oracle.max_event_id, EVENT_ROWS);
        assert_eq!(decoded.null_hub, oracle.null_hub);
        assert_eq!(decoded.null_weight, oracle.null_weight);
        assert_eq!(decoded.null_payload, oracle.null_payload);
        assert_eq!(decoded.null_comment, oracle.null_comment);
        assert_eq!(decoded.null_event_at, oracle.null_event_at);
        assert_eq!(decoded.count, EVENT_ROWS);
        for spot in &oracle.spots {
            let got = decoded
                .spots
                .iter()
                .find(|row| row.event_id == spot.event_id)
                .expect("spot row");
            assert_eq!(got, spot);
        }
        self.note(format!(
            "bounded-read exit=0 rows={} pages={} bytes={} windows_used={} max_rows_in_window={} rate_waits={} elapsed_ms={} sha256={} oracle_count={} oracle_sum={}",
            receipt.rows,
            receipt.pages,
            receipt.payload_bytes,
            receipt.windows_used,
            receipt.max_rows_in_window,
            receipt.rate_waits,
            started.elapsed().as_millis(),
            receipt.arrow_sha256,
            oracle.count,
            oracle.sum_event_id
        ));
    }

    async fn oracle_on_snapshot(&mut self, path: &Path) -> Oracle {
        let snapshot = fs::read_to_string(path)
            .expect("snapshot")
            .trim()
            .to_string();
        assert!(!snapshot.is_empty());
        let mut conn = dial(
            &self.pointer,
            &self.pointer.reader_user,
            &self.reader_pass,
            &self.ca,
            "oo-window-oracle",
        )
        .await;
        sqlx::query("START TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut conn)
            .await
            .expect("oracle begin");
        assert!(
            snapshot
                .chars()
                .all(|ch| ch.is_ascii_hexdigit() || ch == '-')
                && !snapshot.is_empty()
        );
        let import = format!("SET TRANSACTION SNAPSHOT '{snapshot}'");
        sqlx::query(sqlx::AssertSqlSafe(import.as_str()))
            .execute(&mut conn)
            .await
            .unwrap_or_else(|err| panic!("import snapshot while exporter is open: {err}"));
        let row = sqlx::query(
            "SELECT count(*)::int8, coalesce(sum(event_id), 0)::int8, \
             min(event_id)::int8, max(event_id)::int8, \
             count(*) FILTER (WHERE hub_id IS NULL)::int8, \
             count(*) FILTER (WHERE recorded_weight_grams IS NULL)::int8, \
             count(*) FILTER (WHERE payload IS NULL)::int8, \
             count(*) FILTER (WHERE comment IS NULL)::int8, \
             count(*) FILTER (WHERE event_at IS NULL)::int8 \
             FROM public.tracking_events",
        )
        .fetch_one(&mut conn)
        .await
        .expect("oracle aggregate");
        let mut spots = Vec::new();
        for id in [1_i64, 17, 29, 41, 47, 90_000] {
            spots.push(fetch_spot(&mut conn, id).await);
        }
        sqlx::query("ROLLBACK")
            .execute(&mut conn)
            .await
            .expect("oracle rollback");
        let _ = conn.close().await;
        Oracle {
            count: row.try_get(0).unwrap(),
            sum_event_id: row.try_get(1).unwrap(),
            min_event_id: row.try_get(2).unwrap(),
            max_event_id: row.try_get(3).unwrap(),
            null_hub: row.try_get(4).unwrap(),
            null_weight: row.try_get(5).unwrap(),
            null_payload: row.try_get(6).unwrap(),
            null_comment: row.try_get(7).unwrap(),
            null_event_at: row.try_get(8).unwrap(),
            spots,
        }
    }

    async fn gate_read_still_refuses(&mut self) {
        let mut args = self.common_args(EVENTS, 100_000, 33_554_432, 120_000, 5_000);
        args.extend([
            "--phase".to_string(),
            "gate-read".to_string(),
            "--expect-rows".to_string(),
            "90000".to_string(),
        ]);
        let run = self.run_cli("gate-read", &args);
        assert_eq!(run.code, 1, "stdout={} stderr={}", run.stdout, run.stderr);
        assert_eq!(run.stdout.trim(), "step=read fact=max_rows");
        self.note("gate-read exit=1 fact=max_rows".to_string());
    }

    async fn negative_caps(&mut self) {
        let arrow = self.work.join("cap-rows.arrow");
        let partial = partial_of(&arrow);
        let mut args = self.common_args(EVENTS, 1_000, 33_554_432, 120_000, 5_000);
        args.extend(arrow_args(&arrow, 1_000));
        let run = self.run_cli("cap-rows", &args);
        assert_eq!(run.code, 1, "stdout={} stderr={}", run.stdout, run.stderr);
        assert_eq!(run.stdout.trim(), "step=read fact=max_rows");
        assert!(!arrow.exists() && !partial.exists());
        self.note("cap-rows exit=1 fact=max_rows file_absent=1".to_string());

        let arrow = self.work.join("cap-bytes.arrow");
        let partial = partial_of(&arrow);
        let mut args = self.common_args(EVENTS, 100_000, 200_000, 120_000, 5_000);
        args.extend(arrow_args(&arrow, 90_000));
        let run = self.run_cli("cap-bytes", &args);
        assert_eq!(run.code, 1, "stdout={} stderr={}", run.stdout, run.stderr);
        assert_eq!(run.stdout.trim(), "step=read fact=max_bytes");
        assert!(!arrow.exists() && !partial.exists());
        self.note("cap-bytes exit=1 fact=max_bytes file_absent=1".to_string());

        let arrow = self.work.join("cap-deadline.arrow");
        let partial = partial_of(&arrow);
        let mut args = self.common_args(EVENTS, 100_000, 33_554_432, 30_000, 5_000);
        args.extend(arrow_args(&arrow, 90_000));
        let deadline = args
            .iter()
            .position(|item| item == "--deadline-ms")
            .expect("deadline flag");
        args[deadline + 1] = "0".to_string();
        let run = self.run_cli("cap-deadline", &args);
        assert_eq!(run.code, 1, "stdout={} stderr={}", run.stdout, run.stderr);
        assert_eq!(run.stdout.trim(), "step=read fact=deadline");
        assert!(!arrow.exists() && !partial.exists());
        self.note("cap-deadline exit=1 fact=deadline file_absent=1".to_string());
    }

    fn unreadable_table(&mut self) {
        let arrow = self.work.join("secret.arrow");
        let mut args = self.common_args("secret_table", 100, 1_048_576, 30_000, 5_000);
        args.extend(arrow_args(&arrow, 1));
        let run = self.run_cli("secret-table", &args);
        assert_ne!(run.code, 0);
        assert!(!arrow.exists());
        self.note(format!(
            "secret-table exit={} stdout={} stderr={}",
            run.code,
            run.stdout.trim(),
            run.stderr.trim()
        ));
    }

    async fn cancel_while_locked(&mut self) {
        self.hold_gate().await;
        let arrow = self.work.join("cancel.arrow");
        let _ = fs::remove_file(&arrow);
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let config = self.config("cancel_gate", CANCEL_CATALOG_ROWS, 1_048_576, 120_000);
        let arrow_task = arrow.clone();
        let task = tokio::spawn(async move {
            let mut session = PgConnector::new(config.clone())
                .connect()
                .await
                .map_err(show_err)?;
            let mut guard = PgConnector::new(config).connect().await.map_err(show_err)?;
            let binding = RunBinding {
                source_id: SOURCE.to_string(),
                schema: SCHEMA.to_string(),
                table: "cancel_gate".to_string(),
                run_generation: 1,
                input_digest: [38; 32],
            };
            session
                .read_window(
                    &mut guard,
                    WindowRequest {
                        binding: &binding,
                        policy: PgTypePolicy::PRESERVE_ROW,
                        deadline: Instant::now() + Duration::from_secs(90),
                        cancel: &flag,
                        arrow_out: &arrow_task,
                        page_rows: 16,
                        snapshot_file: None,
                    },
                )
                .await
                .map(|_| "ok".to_string())
                .map_err(show_err)
        });
        if !self.wait_until_locked(&task, Duration::from_secs(20)).await {
            let snap = self.reader_snapshot().await;
            panic!(
                "reader never waited inside the locked Execute: {} | {}",
                task_detail(task).await,
                snap
            );
        }
        cancel.store(true, Ordering::Relaxed);
        let result = tokio::time::timeout(Duration::from_secs(20), task)
            .await
            .expect("cancel timed out")
            .expect("cancel task");
        self.release_gate().await;
        let err = result.expect_err("cancel should fail the export");
        assert!(err.contains("cancelled"), "{err}");
        assert!(err.contains("57014"), "{err}");
        assert!(!arrow.exists() && !partial_of(&arrow).exists());
        self.reader_is_quiet().await;
        self.save("cancel.stdout", format!("{err}\n").as_bytes());
        self.save("cancel.exit", b"1\n");
        self.note(format!(
            "cancel-inflight fact={err} file_absent=1 backend_inactive=1"
        ));
    }

    async fn deadline_while_locked(&mut self) {
        self.hold_gate().await;
        let arrow = self.work.join("deadline-lock.arrow");
        let _ = fs::remove_file(&arrow);
        let config = self.config("cancel_gate", CANCEL_CATALOG_ROWS, 1_048_576, 120_000);
        let arrow_task = arrow.clone();
        let task = tokio::spawn(async move {
            let mut session = PgConnector::new(config.clone())
                .connect()
                .await
                .map_err(show_err)?;
            let mut guard = PgConnector::new(config).connect().await.map_err(show_err)?;
            let binding = RunBinding {
                source_id: SOURCE.to_string(),
                schema: SCHEMA.to_string(),
                table: "cancel_gate".to_string(),
                run_generation: 1,
                input_digest: [38; 32],
            };
            session
                .read_window(
                    &mut guard,
                    WindowRequest {
                        binding: &binding,
                        policy: PgTypePolicy::PRESERVE_ROW,
                        deadline: Instant::now() + Duration::from_secs(25),
                        cancel: &AtomicBool::new(false),
                        arrow_out: &arrow_task,
                        page_rows: 16,
                        snapshot_file: None,
                    },
                )
                .await
                .map(|_| "ok".to_string())
                .map_err(show_err)
        });
        if !self.wait_until_locked(&task, Duration::from_secs(20)).await {
            let snap = self.reader_snapshot().await;
            panic!(
                "deadline case never reached the locked Execute: {} | {}",
                task_detail(task).await,
                snap
            );
        }
        let result = tokio::time::timeout(Duration::from_secs(40), task)
            .await
            .expect("deadline timed out")
            .expect("deadline task");
        self.release_gate().await;
        let err = result.expect_err("deadline should fail the export");
        assert!(err.contains("deadline"), "{err}");
        assert!(err.contains("57014"), "{err}");
        assert!(!arrow.exists() && !partial_of(&arrow).exists());
        self.reader_is_quiet().await;
        self.save("deadline-lock.stdout", format!("{err}\n").as_bytes());
        self.save("deadline-lock.exit", b"1\n");
        self.note(format!("deadline-inflight fact={err} file_absent=1"));
    }

    async fn hold_gate(&mut self) {
        sqlx::query("BEGIN")
            .execute(&mut self.admin)
            .await
            .expect("lock begin");
        sqlx::query("LOCK TABLE public.cancel_gate IN ACCESS EXCLUSIVE MODE")
            .execute(&mut self.admin)
            .await
            .expect("lock");
        let row = sqlx::query(
            "SELECT count(*)::int8 FROM pg_catalog.pg_locks AS held \
             JOIN pg_catalog.pg_class AS rel ON rel.oid = held.relation \
             WHERE rel.relname = 'cancel_gate' \
               AND held.mode = 'AccessExclusiveLock' AND held.granted",
        )
        .fetch_one(&mut self.admin)
        .await
        .expect("lock visibility");
        let held: i64 = row.try_get(0).unwrap();
        assert!(held >= 1, "access exclusive lock was not held");
    }

    async fn release_gate(&mut self) {
        sqlx::query("ROLLBACK")
            .execute(&mut self.admin)
            .await
            .expect("unlock");
    }

    async fn wait_until_locked(
        &mut self,
        task: &tokio::task::JoinHandle<Result<String, String>>,
        bound: Duration,
    ) -> bool {
        let deadline = Instant::now() + bound;
        while Instant::now() < deadline {
            if task.is_finished() {
                return false;
            }
            sqlx::query("SELECT pg_catalog.pg_stat_clear_snapshot()")
                .execute(&mut self.admin)
                .await
                .expect("clear stats snapshot");
            let row = sqlx::query(
                "SELECT count(*)::int8 FROM pg_catalog.pg_stat_activity \
                 WHERE usename::text = $1 AND application_name = 'oo-pg-source' \
                 AND state = 'active' AND wait_event_type = 'Lock'",
            )
            .bind(&self.pointer.reader_user)
            .fetch_one(&mut self.admin)
            .await
            .expect("lock watch");
            let waiting: i64 = row.try_get(0).unwrap();
            let queued = sqlx::query(
                "SELECT count(*)::int8 FROM pg_catalog.pg_locks AS held \
                 JOIN pg_catalog.pg_class AS rel ON rel.oid = held.relation \
                 JOIN pg_catalog.pg_stat_activity AS act ON act.pid = held.pid \
                 WHERE rel.relname = 'cancel_gate' AND NOT held.granted \
                   AND act.usename::text = $1",
            )
            .bind(&self.pointer.reader_user)
            .fetch_one(&mut self.admin)
            .await
            .expect("lock queue");
            let queued: i64 = queued.try_get(0).unwrap();
            if waiting >= 1 || queued >= 1 {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    async fn reader_snapshot(&mut self) -> String {
        let _ = sqlx::query("SELECT pg_catalog.pg_stat_clear_snapshot()")
            .execute(&mut self.admin)
            .await;
        let activity = sqlx::query(
            "SELECT pid::int8, COALESCE(usename::text, '<null>'), \
             COALESCE(application_name, '<null>'), COALESCE(state, '<null>'), \
             COALESCE(wait_event_type, '<null>'), COALESCE(wait_event, '<null>'), \
             left(COALESCE(query, ''), 180) \
             FROM pg_catalog.pg_stat_activity \
             WHERE usename::text = $1 OR application_name = 'oo-pg-source'",
        )
        .bind(&self.pointer.reader_user)
        .fetch_all(&mut self.admin)
        .await;
        let locks = sqlx::query(
            "SELECT COALESCE(act.usename::text, '<null>'), held.mode, held.granted \
             FROM pg_catalog.pg_locks AS held \
             JOIN pg_catalog.pg_class AS rel ON rel.oid = held.relation \
             LEFT JOIN pg_catalog.pg_stat_activity AS act ON act.pid = held.pid \
             WHERE rel.relname = 'cancel_gate'",
        )
        .fetch_all(&mut self.admin)
        .await;
        let activity = match activity {
            Ok(rows) => rows
                .iter()
                .map(|row| {
                    format!(
                        "pid={} user={} app={} state={} wait_type={} wait={} query={}",
                        row.try_get::<i64, _>(0).unwrap_or(-1),
                        row.try_get::<String, _>(1).unwrap_or_default(),
                        row.try_get::<String, _>(2).unwrap_or_default(),
                        row.try_get::<String, _>(3).unwrap_or_default(),
                        row.try_get::<String, _>(4).unwrap_or_default(),
                        row.try_get::<String, _>(5).unwrap_or_default(),
                        row.try_get::<String, _>(6).unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join(" || "),
            Err(err) => format!("activity_query_failed {err}"),
        };
        let locks = match locks {
            Ok(rows) => rows
                .iter()
                .map(|row| {
                    format!(
                        "user={} mode={} granted={}",
                        row.try_get::<String, _>(0).unwrap_or_default(),
                        row.try_get::<String, _>(1).unwrap_or_default(),
                        row.try_get::<bool, _>(2).unwrap_or(false)
                    )
                })
                .collect::<Vec<_>>()
                .join(" || "),
            Err(err) => format!("lock_query_failed {err}"),
        };
        format!("activity=[{activity}] locks=[{locks}]")
    }

    async fn reader_is_quiet(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let row = sqlx::query(
                "SELECT count(*)::int8 FROM pg_catalog.pg_stat_activity \
                 WHERE usename::text = $1 AND state = 'active'",
            )
            .bind(&self.pointer.reader_user)
            .fetch_one(&mut self.admin)
            .await
            .expect("quiet");
            let n: i64 = row.try_get(0).unwrap();
            if n == 0 {
                self.note("reader_active_backends=0".to_string());
                return;
            }
            if Instant::now() >= deadline {
                panic!("reader still has {n} active backends");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn audit_log(&mut self) {
        let combined = self.work.join("reader.csv");
        let mut bytes = Vec::new();
        let dir = Path::new(&self.pointer.log_file);
        let mut names: Vec<PathBuf> = if dir.is_dir() {
            fs::read_dir(dir)
                .expect("log dir")
                .filter_map(|item| item.ok())
                .map(|item| item.path())
                .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("csv"))
                .collect()
        } else {
            vec![dir.to_path_buf()]
        };
        names.sort();
        for path in &names {
            bytes.extend(fs::read(path).unwrap_or_default());
        }
        fs::write(&combined, &bytes).expect("combined log");
        let text = String::from_utf8_lossy(&bytes);
        let savepoint = text.matches("SAVEPOINT").count();
        let release = text.matches("RELEASE").count();
        assert_eq!(savepoint, 0, "reader or loader log contains SAVEPOINT");
        assert_eq!(release, 0, "reader or loader log contains RELEASE");
        assert!(!text.contains("pg_terminate_backend"));
        let output = Command::new(&self.admit_bin)
            .args([
                "audit-log",
                "--reader",
                &self.pointer.reader_user,
                "--log",
                combined.to_str().unwrap(),
            ])
            .output()
            .expect("audit-log");
        self.save("audit.stdout", &output.stdout);
        self.save("audit.stderr", &output.stderr);
        self.save(
            "audit.exit",
            format!("{}\n", output.status.code().unwrap_or(127)).as_bytes(),
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("\"forbidden\": 0") || stdout.contains("\"forbidden\":0"),
            "{stdout}"
        );
        assert!(stdout.contains("\"reader_statements\""));
        assert!(!stdout.contains("SAVEPOINT"));
        assert!(!stdout.contains("RELEASE"));
        self.note(format!(
            "audit exit={} savepoint=0 release=0 forbidden_marker=0 bytes={}",
            output.status.code().unwrap_or(127),
            bytes.len()
        ));
    }

    fn stream_config(
        &self,
        tables: &[&str],
        max_rows: u32,
        max_bytes: u64,
        statement_ms: u64,
    ) -> PgSourceConfig {
        let selected = tables
            .iter()
            .map(|table| SelectedTable::new(SCHEMA, *table).unwrap())
            .collect::<Vec<_>>();
        PgSourceConfig::try_new(
            SourceId::new(SOURCE).unwrap(),
            PgEndpoint::new(
                &self.pointer.host,
                self.pointer.port,
                &self.pointer.database,
            )
            .unwrap(),
            PgCredentials::new(&self.pointer.reader_user, self.reader_pass.as_str()).unwrap(),
            SecretRef::new("secret://oo-dl01w1-38/reader").unwrap(),
            PgTlsConfig::from_ca_pem(&self.ca).unwrap(),
            PgBudget::new(
                Duration::from_millis(30_000),
                Duration::from_millis(statement_ms),
                Duration::from_millis(statement_ms.min(120_000)),
                max_rows,
                max_bytes,
            )
            .unwrap(),
            TableSelection::new(selected).unwrap(),
        )
        .unwrap()
    }

    async fn same_snapshot_stream(&mut self) {
        let oracle = self.admin_event_oracle().await;
        let config =
            self.stream_config(&["hubs", "shipments", EVENTS], 100_000, 33_554_432, 120_000);
        let request = PgConnector::new(config)
            .prepare_snapshot(
                PgTypePolicy::PRESERVE_ROW,
                38,
                Instant::now() + Duration::from_secs(1_800),
            )
            .await
            .expect("prepare same-snapshot stream");
        let mut owner = SnapshotOwner::open(request)
            .await
            .expect("open same-snapshot stream");
        let binding = owner.binding().clone();
        let pid = binding.backend.pid;
        let backend_start = binding.backend.backend_start.clone();
        let generation = binding.generation;
        let digest = binding.input_digest;
        let schema_hash = binding.schema_hash.clone();
        owner
            .open_stream(StreamBudget::try_new(4_096, 2_097_152, 8_192, 65_536).unwrap())
            .await
            .expect("open stream");
        let xact_open = self.xact_start(pid).await;
        assert!(xact_open.is_some(), "data backend has no transaction");
        let started = Instant::now();
        let mut hubs = 0_i64;
        let mut shipments = 0_i64;
        let mut events = 0_i64;
        let mut sum_event_id = 0_i64;
        let mut min_event_id = i64::MAX;
        let mut max_event_id = i64::MIN;
        let mut batches = 0_u32;
        let mut eofs = Vec::new();
        let accounting = loop {
            match owner.next_batch().await.expect("same-snapshot batch") {
                StreamItem::Batch(batch) => {
                    assert_eq!(batch.typed.schema, SCHEMA);
                    assert_eq!(batch.typed.run_generation, generation);
                    assert_eq!(batch.typed.input_digest, digest);
                    assert!(!batch.resumable);
                    assert!(batch.record_batch.num_rows() > 0);
                    assert!(batch.record_batch.num_rows() <= 4_096);
                    batches += 1;
                    match batch.typed.table.as_str() {
                        "hubs" => hubs += i64::try_from(batch.record_batch.num_rows()).unwrap(),
                        "shipments" => {
                            shipments += i64::try_from(batch.record_batch.num_rows()).unwrap()
                        }
                        EVENTS => {
                            let ids = batch
                                .record_batch
                                .column_by_name("event_id")
                                .expect("event_id")
                                .as_any()
                                .downcast_ref::<Int64Array>()
                                .expect("event_id i64");
                            for row in 0..ids.len() {
                                assert!(!ids.is_null(row));
                                let event_id = ids.value(row);
                                events += 1;
                                sum_event_id += event_id;
                                min_event_id = min_event_id.min(event_id);
                                max_event_id = max_event_id.max(event_id);
                            }
                        }
                        other => panic!("unexpected table {other}"),
                    }
                }
                StreamItem::TableEof(eof) => {
                    assert_eq!(eof.schema, SCHEMA);
                    assert!(!eof.resumable_key);
                    eofs.push((eof.table, eof.rows_emitted));
                }
                StreamItem::Exhausted(accounting) => break accounting,
            }
        };
        let xact_held = self.xact_start(pid).await;
        assert_eq!(
            xact_open, xact_held,
            "snapshot transaction changed between tables"
        );
        assert_eq!(hubs, HUB_ROWS);
        assert_eq!(shipments, SHIPMENT_ROWS);
        assert_eq!(events, EVENT_ROWS);
        assert_eq!(sum_event_id, oracle.1);
        assert_eq!(sum_event_id, SUM_EVENT_ID);
        assert_eq!(min_event_id, oracle.2);
        assert_eq!(max_event_id, oracle.3);
        assert_eq!(oracle.0, EVENT_ROWS);
        assert_eq!(eofs.len(), 3);
        let eof_rows = |name: &str| {
            eofs.iter()
                .find(|(table, _)| table == name)
                .map(|(_, rows)| *rows)
                .unwrap_or_else(|| panic!("missing eof {name}"))
        };
        assert_eq!(eof_rows("hubs"), 2_000);
        assert_eq!(eof_rows("shipments"), 8_000);
        assert_eq!(eof_rows(EVENTS), 90_000);
        assert!(batches >= 22, "paging produced {batches} batches");
        assert_eq!(accounting.reserved_rows, accounting.executes);
        assert!(accounting.executes >= 100_000);
        let finished = owner.finish_complete().await.expect("stream complete");
        assert_eq!(finished.source_id, SOURCE);
        assert_eq!(finished.generation, generation);
        assert_eq!(finished.input_digest, digest);
        assert_eq!(finished.schema_hash, schema_hash);
        assert_eq!(finished.selected.len(), 3);
        self.backend_gone(pid, &backend_start).await;
        self.reader_is_quiet().await;
        let elapsed_ms = started.elapsed().as_millis();
        self.note(format!(
            "same-snapshot hubs={hubs} shipments={shipments} tracking_events={events} sum={sum_event_id} batches={batches} executes={} generation={generation} schema_hash={schema_hash} xact_held=1 elapsed_ms={elapsed_ms} oracle_count={} oracle_sum={} backend_gone=1",
            accounting.executes, oracle.0, oracle.1
        ));
    }

    async fn stream_cancel_while_locked(&mut self) {
        self.drive_locked_stream(true).await;
    }

    async fn stream_deadline_while_locked(&mut self) {
        self.drive_locked_stream(false).await;
    }

    async fn drive_locked_stream(&mut self, cancel: bool) {
        self.hold_gate().await;
        let config = self.stream_config(&["cancel_gate"], CANCEL_CATALOG_ROWS, 1_048_576, 120_000);
        let deadline = if cancel {
            Instant::now() + Duration::from_secs(90)
        } else {
            Instant::now() + Duration::from_secs(35)
        };
        let request = PgConnector::new(config)
            .prepare_snapshot(
                PgTypePolicy::PRESERVE_ROW,
                if cancel { 39 } else { 40 },
                deadline,
            )
            .await
            .expect("prepare locked stream");
        let mut owner = SnapshotOwner::open(request)
            .await
            .expect("open locked stream");
        let pid = owner.binding().backend.pid;
        let backend_start = owner.binding().backend.backend_start.clone();
        owner
            .open_stream(StreamBudget::try_new(16, 1_048_576, 8_192, 65_536).unwrap())
            .await
            .expect("open locked pull");
        let flag = owner.run_cancel();
        let task = tokio::spawn(async move {
            let pulled = owner
                .next_batch()
                .await
                .map(|_| "ok".to_string())
                .map_err(show_err);
            let released = owner.cancel().await.map_err(show_err);
            match (pulled, released) {
                (Err(err), Ok(_)) => Err(err),
                (Err(err), Err(release)) => Err(format!("pull={err} release={release}")),
                (Ok(_), Ok(_)) => Ok("pulled".to_string()),
                (Ok(_), Err(release)) => Err(format!("release={release}")),
            }
        });
        if !self.wait_until_locked(&task, Duration::from_secs(20)).await {
            let snap = self.reader_snapshot().await;
            panic!(
                "stream never waited inside the locked Execute: {} | {}",
                task_detail(task).await,
                snap
            );
        }
        if cancel {
            flag.request();
        }
        let bound = if cancel {
            Duration::from_secs(20)
        } else {
            Duration::from_secs(50)
        };
        let result = tokio::time::timeout(bound, task)
            .await
            .expect("locked stream timed out")
            .expect("locked stream task");
        self.release_gate().await;
        let err = result.expect_err("locked stream should fail the pull");
        assert!(!err.contains("release="), "{err}");
        if cancel {
            assert!(err.contains("cancelled"), "{err}");
        } else {
            assert!(err.contains("deadline"), "{err}");
        }
        assert!(err.contains("57014"), "{err}");
        self.backend_gone(pid, &backend_start).await;
        self.reader_is_quiet().await;
        let name = if cancel {
            "stream-cancel"
        } else {
            "stream-deadline"
        };
        self.save(&format!("{name}.stdout"), format!("{err}\n").as_bytes());
        self.save(&format!("{name}.exit"), b"1\n");
        self.note(format!("{name} fact={err} backend_inactive=1 pid={pid}"));
    }

    async fn admin_event_oracle(&mut self) -> (i64, i64, i64, i64) {
        let row = sqlx::query(
            "SELECT count(*)::int8, coalesce(sum(event_id), 0)::int8, \
             min(event_id)::int8, max(event_id)::int8 \
             FROM public.tracking_events",
        )
        .fetch_one(&mut self.admin)
        .await
        .expect("admin oracle");
        (
            row.try_get(0).unwrap(),
            row.try_get(1).unwrap(),
            row.try_get(2).unwrap(),
            row.try_get(3).unwrap(),
        )
    }

    async fn xact_start(&mut self, pid: i64) -> Option<String> {
        sqlx::query("SELECT pg_catalog.pg_stat_clear_snapshot()")
            .execute(&mut self.admin)
            .await
            .expect("clear stats snapshot");
        let row =
            sqlx::query("SELECT xact_start::text FROM pg_catalog.pg_stat_activity WHERE pid = $1")
                .bind(i32::try_from(pid).expect("backend pid"))
                .fetch_optional(&mut self.admin)
                .await
                .expect("xact start");
        row.and_then(|row| row.try_get::<Option<String>, _>(0).unwrap())
    }

    async fn backend_gone(&mut self, pid: i64, backend_start: &str) {
        sqlx::query("SELECT pg_catalog.pg_stat_clear_snapshot()")
            .execute(&mut self.admin)
            .await
            .expect("clear stats snapshot");
        let row = sqlx::query(
            "SELECT count(*)::int8 FROM pg_catalog.pg_stat_activity \
             WHERE pid = $1 AND backend_start::text = $2",
        )
        .bind(i32::try_from(pid).expect("backend pid"))
        .bind(backend_start)
        .fetch_one(&mut self.admin)
        .await
        .expect("backend gone");
        let n: i64 = row.try_get(0).unwrap();
        assert_eq!(n, 0, "reader backend {pid} {backend_start} still present");
    }

    fn common_args(
        &self,
        table: &str,
        max_rows: u64,
        max_bytes: u64,
        statement_ms: u64,
        lock_ms: u64,
    ) -> Vec<String> {
        let admit_work = self.work.join(format!("admit-{table}"));
        let _ = fs::create_dir_all(&admit_work);
        vec![
            "--source-id".to_string(),
            SOURCE.to_string(),
            "--host".to_string(),
            self.pointer.host.clone(),
            "--port".to_string(),
            self.pointer.port.to_string(),
            "--database".to_string(),
            self.pointer.database.clone(),
            "--username".to_string(),
            self.pointer.reader_user.clone(),
            "--password-file".to_string(),
            format!("{}/reader.pass", self.pointer.password_dir),
            "--ca-file".to_string(),
            self.pointer.ca_file.clone(),
            "--secret-ref".to_string(),
            "secret://oo-dl01w1-38/reader".to_string(),
            "--schema".to_string(),
            SCHEMA.to_string(),
            "--table".to_string(),
            table.to_string(),
            "--statement-ms".to_string(),
            statement_ms.to_string(),
            "--lock-ms".to_string(),
            lock_ms.to_string(),
            "--connect-ms".to_string(),
            "30000".to_string(),
            "--max-rows".to_string(),
            max_rows.to_string(),
            "--max-bytes".to_string(),
            max_bytes.to_string(),
            "--admit-bin".to_string(),
            self.admit_bin.display().to_string(),
            "--access-out".to_string(),
            admit_work.join("access.json").display().to_string(),
            "--evidence-out".to_string(),
            admit_work.join("evidence.json").display().to_string(),
            "--config-out".to_string(),
            admit_work.join("config.json").display().to_string(),
            "--decision-out".to_string(),
            admit_work.join("decision.json").display().to_string(),
        ]
    }

    fn run_cli(&self, name: &str, args: &[String]) -> Run {
        let output = Command::new(&self.budget_bin)
            .args(args)
            .output()
            .unwrap_or_else(|err| panic!("run {name}: {err}"));
        let code = output.status.code().unwrap_or(127);
        self.save(&format!("{name}.stdout"), &output.stdout);
        self.save(&format!("{name}.stderr"), &output.stderr);
        self.save(&format!("{name}.exit"), format!("{code}\n").as_bytes());
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(
            !stdout.contains(self.reader_pass.as_str())
                && !stderr.contains(self.reader_pass.as_str()),
            "{name} leaked the reader password"
        );
        Run {
            code,
            stdout,
            stderr,
        }
    }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

struct Receipt {
    rows: u64,
    pages: u32,
    payload_bytes: u64,
    windows_used: u32,
    max_rows_in_window: u32,
    rate_waits: u32,
    snapshot: String,
    arrow_sha256: String,
}

struct Oracle {
    count: i64,
    sum_event_id: i64,
    min_event_id: i64,
    max_event_id: i64,
    null_hub: i64,
    null_weight: i64,
    null_payload: i64,
    null_comment: i64,
    null_event_at: i64,
    spots: Vec<Spot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Spot {
    event_id: i64,
    token_hex: String,
    shipment_id: i64,
    hub_id: Option<i64>,
    status: String,
    event_us: Option<i64>,
    weight: Option<i64>,
    payload_hex: Option<String>,
    comment: Option<String>,
}

struct Decoded {
    count: i64,
    sum_event_id: i64,
    min_event_id: i64,
    max_event_id: i64,
    null_hub: i64,
    null_weight: i64,
    null_payload: i64,
    null_comment: i64,
    null_event_at: i64,
    spots: Vec<Spot>,
}

fn arrow_args(arrow: &Path, expect_rows: u64) -> Vec<String> {
    vec![
        "--phase".to_string(),
        "bounded-read".to_string(),
        "--arrow-out".to_string(),
        arrow.display().to_string(),
        "--page-rows".to_string(),
        "1024".to_string(),
        "--deadline-ms".to_string(),
        "180000".to_string(),
        "--expect-rows".to_string(),
        expect_rows.to_string(),
    ]
}

fn parse_receipt(line: &str) -> Receipt {
    let mut rows = None;
    let mut pages = None;
    let mut payload_bytes = None;
    let mut windows_used = None;
    let mut max_rows_in_window = None;
    let mut rate_waits = None;
    let mut snapshot = None;
    let mut arrow_sha256 = None;
    for part in line.split_whitespace() {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "rows" => rows = value.parse().ok(),
            "pages" => pages = value.parse().ok(),
            "bytes" => payload_bytes = value.parse().ok(),
            "windows_used" => windows_used = value.parse().ok(),
            "max_rows_in_window" => max_rows_in_window = value.parse().ok(),
            "rate_waits" => rate_waits = value.parse().ok(),
            "snapshot" => snapshot = Some(value.to_string()),
            "arrow_sha256" => arrow_sha256 = Some(value.to_string()),
            _ => {}
        }
    }
    Receipt {
        rows: rows.expect("rows"),
        pages: pages.expect("pages"),
        payload_bytes: payload_bytes.expect("bytes"),
        windows_used: windows_used.expect("windows"),
        max_rows_in_window: max_rows_in_window.expect("window max"),
        rate_waits: rate_waits.expect("waits"),
        snapshot: snapshot.expect("snapshot"),
        arrow_sha256: arrow_sha256.expect("sha"),
    }
}

fn decode_events(path: &Path) -> Decoded {
    let file = File::open(path).expect("arrow");
    let reader = StreamReader::try_new(file, None).expect("ipc");
    let mut count = 0_i64;
    let mut sum_event_id = 0_i64;
    let mut min_event_id = i64::MAX;
    let mut max_event_id = i64::MIN;
    let mut null_hub = 0_i64;
    let mut null_weight = 0_i64;
    let mut null_payload = 0_i64;
    let mut null_comment = 0_i64;
    let mut null_event_at = 0_i64;
    let want = [1_i64, 17, 29, 41, 47, 90_000];
    let mut spots = Vec::new();
    let mut saw_schema = false;
    for batch in reader {
        let batch = batch.expect("batch");
        if !saw_schema {
            expect_event_schema(&batch);
            saw_schema = true;
        }
        let ids = col::<Int64Array>(&batch, "event_id");
        let tokens = col::<BinaryArray>(&batch, "event_token");
        let shipments = col::<Int64Array>(&batch, "shipment_id");
        let hubs = col::<Int64Array>(&batch, "hub_id");
        let status = col::<StringArray>(&batch, "status");
        let event_at = col::<TimestampMicrosecondArray>(&batch, "event_at");
        let weight = col::<Int64Array>(&batch, "recorded_weight_grams");
        let payload = col::<BinaryArray>(&batch, "payload");
        let comment = col::<StringArray>(&batch, "comment");
        for row in 0..batch.num_rows() {
            let event_id = ids.value(row);
            count += 1;
            sum_event_id += event_id;
            min_event_id = min_event_id.min(event_id);
            max_event_id = max_event_id.max(event_id);
            if hubs.is_null(row) {
                null_hub += 1;
            }
            if weight.is_null(row) {
                null_weight += 1;
            }
            if payload.is_null(row) {
                null_payload += 1;
            }
            if comment.is_null(row) {
                null_comment += 1;
            }
            if event_at.is_null(row) {
                null_event_at += 1;
            }
            assert_eq!(tokens.value(row).len(), 16);
            if want.contains(&event_id) {
                spots.push(Spot {
                    event_id,
                    token_hex: hex(tokens.value(row)),
                    shipment_id: shipments.value(row),
                    hub_id: opt_i64(hubs, row),
                    status: status.value(row).to_string(),
                    event_us: if event_at.is_null(row) {
                        None
                    } else {
                        Some(event_at.value(row))
                    },
                    weight: opt_i64(weight, row),
                    payload_hex: if payload.is_null(row) {
                        None
                    } else {
                        Some(hex(payload.value(row)))
                    },
                    comment: if comment.is_null(row) {
                        None
                    } else {
                        Some(comment.value(row).to_string())
                    },
                });
            }
        }
    }
    assert!(saw_schema);
    Decoded {
        count,
        sum_event_id,
        min_event_id,
        max_event_id,
        null_hub,
        null_weight,
        null_payload,
        null_comment,
        null_event_at,
        spots,
    }
}

fn expect_event_schema(batch: &RecordBatch) {
    let schema = batch.schema();
    let field = |name: &str| schema.field_with_name(name).unwrap().clone();
    let event_id = field("event_id");
    assert_eq!(event_id.data_type(), &DataType::Int64);
    assert!(!event_id.is_nullable());
    let token = field("event_token");
    assert_eq!(token.data_type(), &DataType::Binary);
    assert!(!token.is_nullable());
    assert_eq!(field("shipment_id").data_type(), &DataType::Int64);
    assert!(!field("shipment_id").is_nullable());
    assert_eq!(field("hub_id").data_type(), &DataType::Int64);
    assert!(field("hub_id").is_nullable());
    assert_eq!(field("status").data_type(), &DataType::Utf8);
    assert!(!field("status").is_nullable());
    assert_eq!(
        field("event_at").data_type(),
        &DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into()))
    );
    assert!(field("event_at").is_nullable());
    assert_eq!(field("recorded_weight_grams").data_type(), &DataType::Int64);
    assert!(field("recorded_weight_grams").is_nullable());
    assert_eq!(field("payload").data_type(), &DataType::Binary);
    assert!(field("payload").is_nullable());
    assert_eq!(field("comment").data_type(), &DataType::Utf8);
    assert!(field("comment").is_nullable());
}

fn col<'a, T: Array + 'static>(batch: &'a RecordBatch, name: &str) -> &'a T {
    batch
        .column_by_name(name)
        .unwrap_or_else(|| panic!("missing {name}"))
        .as_any()
        .downcast_ref::<T>()
        .unwrap_or_else(|| panic!("{name} type"))
}

fn opt_i64(array: &Int64Array, row: usize) -> Option<i64> {
    if array.is_null(row) {
        None
    } else {
        Some(array.value(row))
    }
}

async fn fetch_spot(conn: &mut PgConnection, id: i64) -> Spot {
    let row = sqlx::query(
        "SELECT event_id::int8, encode(pg_catalog.uuid_send(event_token), 'hex'), shipment_id::int8, hub_id::int8, \
         status, \
         CASE WHEN event_at IS NULL THEN NULL \
              ELSE (extract(epoch FROM event_at) * 1000000)::numeric::int8 END, \
         recorded_weight_grams::int8, encode(payload, 'hex'), comment \
         FROM public.tracking_events WHERE event_id = $1",
    )
    .bind(id)
    .fetch_one(&mut *conn)
    .await
    .expect("spot");
    Spot {
        event_id: row.try_get(0).unwrap(),
        token_hex: row.try_get(1).unwrap(),
        shipment_id: row.try_get(2).unwrap(),
        hub_id: row.try_get(3).unwrap(),
        status: row.try_get(4).unwrap(),
        event_us: row.try_get(5).unwrap(),
        weight: row.try_get(6).unwrap(),
        payload_hex: row.try_get(7).unwrap(),
        comment: row.try_get(8).unwrap(),
    }
}

async fn task_detail(task: tokio::task::JoinHandle<Result<String, String>>) -> String {
    if task.is_finished() {
        match task.await {
            Ok(finished) => format!("{finished:?}"),
            Err(err) => format!("join {err}"),
        }
    } else {
        "still running when the wait expired".to_string()
    }
}

fn show_err(err: oo_connectors::SourceError) -> String {
    match err.sqlstate() {
        Some(state) => format!("{} {state}", err.fact().unwrap_or("")),
        None => err.fact().unwrap_or("").to_string(),
    }
}

fn partial_of(arrow: &Path) -> PathBuf {
    let name = arrow.file_name().unwrap().to_string_lossy();
    arrow.with_file_name(format!("{name}.partial"))
}

fn wait_file(child: &mut Child, path: &Path, bound: Duration) -> bool {
    let deadline = Instant::now() + bound;
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return false;
        }
        if let Ok(text) = fs::read_to_string(path)
            && !text.trim().is_empty()
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn sha256_file(path: &Path) -> String {
    let output = Command::new("sha256sum")
        .arg(path)
        .output()
        .expect("sha256sum");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
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
    options
        .disable_statement_logging()
        .connect()
        .await
        .expect("dial")
}

fn read_pass(dir: &str, name: &str) -> Zeroizing<String> {
    let text = fs::read_to_string(format!("{dir}/{name}")).expect("password file");
    Zeroizing::new(text.trim().to_string())
}

const LOAD_SQL: &[&str] = &[
    "DROP TABLE IF EXISTS public.tracking_events",
    "DROP TABLE IF EXISTS public.shipments",
    "DROP TABLE IF EXISTS public.hubs",
    "DROP TABLE IF EXISTS public.type_probe",
    "DROP TABLE IF EXISTS public.secret_table",
    "DROP TABLE IF EXISTS public.cancel_gate",
    "CREATE TABLE public.hubs ( \
        hub_id bigint PRIMARY KEY, \
        hub_token uuid NOT NULL, \
        hub_code text NOT NULL, \
        hub_name text NOT NULL, \
        region_code text, \
        latitude numeric(38,4), \
        longitude numeric(38,4), \
        capacity_grams bigint NOT NULL, \
        opened_at timestamp with time zone, \
        badge bytea, \
        note text)",
    "CREATE TABLE public.shipments ( \
        shipment_id bigint PRIMARY KEY, \
        shipment_token uuid NOT NULL, \
        origin_hub_id bigint NOT NULL REFERENCES public.hubs (hub_id), \
        dest_hub_id bigint NOT NULL REFERENCES public.hubs (hub_id), \
        status text NOT NULL, \
        transport_mode text NOT NULL, \
        weight_grams bigint, \
        cost_amount numeric(38,4), \
        created_at timestamp with time zone NOT NULL, \
        promised_at timestamp with time zone, \
        seal bytea, \
        comment text, \
        CONSTRAINT shipments_status_ck CHECK (status IN ('delivered', 'in_transit', 'delayed')), \
        CONSTRAINT shipments_mode_ck CHECK (transport_mode IN ('road', 'rail', 'air', 'sea')))",
    "CREATE TABLE public.tracking_events ( \
        event_id bigint PRIMARY KEY, \
        event_token uuid NOT NULL, \
        shipment_id bigint NOT NULL REFERENCES public.shipments (shipment_id), \
        hub_id bigint REFERENCES public.hubs (hub_id), \
        status text NOT NULL, \
        event_at timestamp with time zone, \
        recorded_weight_grams bigint, \
        payload bytea, \
        comment text, \
        CONSTRAINT tracking_status_ck CHECK (status IN ('delivered', 'in_transit', 'delayed')))",
    "CREATE TABLE public.type_probe (probe_id bigint PRIMARY KEY, payload jsonb)",
    "CREATE TABLE public.secret_table (id bigint PRIMARY KEY)",
    "CREATE TABLE public.cancel_gate (event_id bigint PRIMARY KEY)",
    "INSERT INTO public.hubs \
     SELECT g, gen_random_uuid(), 'H' || g::text, 'Hub ' || g::text, \
            CASE WHEN g % 5 = 0 THEN NULL ELSE 'R' || (g % 7)::text END, \
            (g % 90)::numeric(38,4), (g % 180)::numeric(38,4), g * 1000, \
            timestamptz '2020-01-01 UTC' + g * interval '1 minute', \
            decode(md5(g::text), 'hex'), 'hub' \
     FROM generate_series(1, 2000) AS g",
    "INSERT INTO public.shipments \
     SELECT g, gen_random_uuid(), ((g - 1) % 2000) + 1, (g % 2000) + 1, \
            CASE g % 3 WHEN 0 THEN 'delivered' WHEN 1 THEN 'in_transit' ELSE 'delayed' END, \
            CASE g % 4 WHEN 0 THEN 'road' WHEN 1 THEN 'rail' WHEN 2 THEN 'air' ELSE 'sea' END, \
            CASE WHEN g % 13 = 0 THEN NULL ELSE g * 100 END, \
            (g % 1000)::numeric(38,4), \
            timestamptz '2021-01-01 UTC' + g * interval '1 second', \
            timestamptz '2021-01-02 UTC' + g * interval '1 second', \
            decode(md5((g + 1)::text), 'hex'), 'shipment' \
     FROM generate_series(1, 8000) AS g",
    "INSERT INTO public.tracking_events \
     SELECT g, gen_random_uuid(), ((g - 1) % 8000) + 1, \
            CASE WHEN g % 29 = 0 THEN NULL ELSE ((g - 1) % 2000) + 1 END, \
            CASE g % 3 WHEN 0 THEN 'delivered' WHEN 1 THEN 'in_transit' ELSE 'delayed' END, \
            timestamptz '2024-01-01 UTC' + g * interval '1 second', \
            CASE WHEN g % 17 = 0 THEN NULL ELSE g * 10 END, \
            CASE WHEN g % 41 = 0 THEN NULL ELSE decode(md5(g::text), 'hex') END, \
            CASE WHEN g % 47 = 0 THEN NULL ELSE 'note ' || g::text END \
     FROM generate_series(1, 90000) AS g",
    "INSERT INTO public.type_probe VALUES (1, '{\"kind\":\"probe\"}'::jsonb)",
    "INSERT INTO public.secret_table VALUES (1)",
    "INSERT INTO public.cancel_gate VALUES (1)",
    "REVOKE TEMPORARY ON DATABASE src FROM PUBLIC",
    "REVOKE TEMPORARY ON DATABASE src FROM oo_dl01w1_38_reader",
    "REVOKE ALL ON TABLE public.hubs, public.shipments, public.tracking_events, \
     public.type_probe, public.secret_table, public.cancel_gate FROM PUBLIC",
    "GRANT SELECT ON public.hubs, public.shipments, public.tracking_events, \
     public.type_probe, public.cancel_gate TO oo_dl01w1_38_reader",
];
