//! Independent review of `import-pg` on one private PostgreSQL 17.10.
//!
//! The fixture is `yard.hulls` and `yard.fasteners`. Admin SQL is setup and
//! the oracle. The import itself is the `oo-pipeline` binary on stdin, after
//! `PgConnector::prepare_snapshot` has been called on the same source.

use std::env;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use oo_connectors::{
    FactStatus, PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig,
    PgTypePolicy, SecretRef, SelectedTable, SnapshotOwner, SourceId, StreamBudget, StreamItem,
    TableSelection,
};
use oo_pipeline::full_import::{RunState, RunSummary};
use oo_pipeline::target_installer::{
    FullImportInstallOutcome, InstallOutcome, install, install_full_import,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Row};

const OWNER: &str = "16416416-1641-4641-8641-164164164164";

#[tokio::test]
#[ignore = "one private PostgreSQL 17.10; OO_IMPORT_PG_HOST must be set by the deciding run"]
async fn import_pg_seals_two_tables_and_refuses_the_rest() {
    let mut harness = Harness::connect().await;
    let result = harness.exercise().await;
    harness.cleanup().await;
    result.expect("a164 import review");
}

struct Harness {
    host: String,
    port: u16,
    admin_password: String,
    reader_password: String,
    writer_password: String,
    runtime_password: String,
    agent_password: String,
    ca: Vec<u8>,
    ca_file: std::path::PathBuf,
    reader_file: std::path::PathBuf,
    runtime_file: std::path::PathBuf,
    evidence: std::path::PathBuf,
    postgres: sqlx::postgres::PgConnection,
    source: sqlx::postgres::PgConnection,
    target: sqlx::postgres::PgConnection,
    lines: Vec<String>,
}

impl Harness {
    async fn connect() -> Self {
        let host = env::var("OO_IMPORT_PG_HOST").expect("OO_IMPORT_PG_HOST");
        let port: u16 = env::var("OO_IMPORT_PG_PORT").expect("OO_IMPORT_PG_PORT").parse().unwrap();
        let admin_user = env::var("OO_IMPORT_PG_ADMIN_USER").expect("OO_IMPORT_PG_ADMIN_USER");
        let admin_password = read_secret("OO_IMPORT_PG_ADMIN_PASSWORD_FILE");
        let reader_password = read_secret("OO_IMPORT_PG_READER_PASSWORD_FILE");
        let writer_password = read_secret("OO_IMPORT_PG_WRITER_PASSWORD_FILE");
        let runtime_password = read_secret("OO_IMPORT_PG_RUNTIME_PASSWORD_FILE");
        let agent_password = read_secret("OO_IMPORT_PG_AGENT_PASSWORD_FILE");
        let ca_file = env::var("OO_IMPORT_PG_CA_FILE").expect("ca").into();
        let ca = fs::read(&ca_file).expect("ca");
        let evidence = env::var("OO_IMPORT_PG_EVIDENCE").expect("evidence").into();
        fs::create_dir_all(&evidence).expect("evidence dir");
        let mut postgres = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
        reset(&mut postgres).await;
        role(&mut postgres, "oo_a164_reader", &reader_password).await;
        role(&mut postgres, "oo_a164_writer", &writer_password).await;
        role(&mut postgres, "oo_a164_runtime", &runtime_password).await;
        role(&mut postgres, "oo_a164_agent", &agent_password).await;
        raw(&mut postgres, "CREATE DATABASE oo_a164_src TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
        raw(&mut postgres, "CREATE DATABASE oo_a164_dst TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
        raw(&mut postgres, "GRANT CONNECT ON DATABASE oo_a164_dst TO oo_a164_runtime, oo_a164_agent").await;
        let mut source = dial(&host, port, &admin_user, &admin_password, "oo_a164_src", &ca).await;
        raw(&mut source, SOURCE_SQL).await;
        let target = dial(&host, port, &admin_user, &admin_password, "oo_a164_dst", &ca).await;
        Self {
            host,
            port,
            admin_password,
            reader_password,
            writer_password,
            runtime_password,
            agent_password,
            ca,
            ca_file,
            reader_file: env::var("OO_IMPORT_PG_READER_PASSWORD_FILE").unwrap().into(),
            runtime_file: env::var("OO_IMPORT_PG_RUNTIME_PASSWORD_FILE").unwrap().into(),
            evidence,
            postgres,
            source,
            target,
            lines: Vec::new(),
        }
    }

    async fn exercise(&mut self) -> Result<(), String> {
        self.privileges().await?;
        self.writer_is_refused().await?;
        self.reader_bindings().await?;
        self.not_ready().await?;
        self.changed_catalog().await?;
        self.sealed_import().await?;
        self.agent_and_writes().await?;
        self.constructed_summary_is_not_a_seal().await?;
        self.note("import_pg_a164_review=ok");
        fs::write(self.evidence.join("a164-outcome.txt"), self.lines.join("\n") + "\n").map_err(|_| "outcome".to_string())?;
        Ok(())
    }

    fn note(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    async fn privileges(&mut self) -> Result<(), String> {
        if flag(&mut self.source, "SELECT has_table_privilege('oo_a164_reader', 'yard.hulls', 'SELECT')").await
            && flag(&mut self.source, "SELECT has_table_privilege('oo_a164_reader', 'yard.fasteners', 'SELECT')").await
            && !flag(&mut self.source, "SELECT has_table_privilege('oo_a164_reader', 'yard.hulls', 'INSERT')").await
            && !flag(&mut self.source, "SELECT has_table_privilege('oo_a164_reader', 'yard.fasteners', 'UPDATE')").await
            && flag(&mut self.source, "SELECT has_table_privilege('oo_a164_writer', 'yard.hulls', 'INSERT')").await
        {
            self.note("select_only_reader=1");
            Ok(())
        } else {
            Err("privilege shape".to_string())
        }
    }

    async fn writer_is_refused(&mut self) -> Result<(), String> {
        let connector = self.connector("a164writer", "oo_a164_writer", &self.writer_password.clone())?;
        let err = connector
            .prepare_snapshot(PgTypePolicy::PRESERVE_ROW, 1, Instant::now() + Duration::from_secs(30))
            .await
            .expect_err("writer");
        if err.fact() != Some("registration_refused") {
            return Err(format!("writer fact {}", err.fact().unwrap_or("none")));
        }
        self.quiet_source().await?;
        if text(&mut self.source, "SELECT count(*)::text FROM yard.write_audit WHERE who IN ('oo_a164_writer', 'oo_a164_reader')").await != "0" {
            return Err("writer refusal wrote a row".to_string());
        }
        self.note("writer_registration=refused");
        Ok(())
    }

    async fn reader_bindings(&mut self) -> Result<(), String> {
        let connector = self.reader()?;
        let mut session = connector.connect().await.map_err(|err| fact_line("connect", err.fact()))?;
        let report = session.inspect_access().await.map_err(|err| fact_line("inspect", err.fact()))?;
        if !report.registration_allowed || report.principal.status != FactStatus::Known {
            return Err("reader registration".to_string());
        }
        let principal = report.principal.value.as_ref().ok_or("principal")?;
        if principal.current_user != "oo_a164_reader"
            || principal.session_user != "oo_a164_reader"
            || principal.database != "oo_a164_src"
            || report.in_recovery.value != Some(false)
        {
            return Err("access facts".to_string());
        }
        let catalog = session.catalog().await.map_err(|err| fact_line("catalog", err.fact()))?;
        drop(session);
        self.quiet_source().await?;
        let prepared = self
            .reader()?
            .prepare_snapshot(PgTypePolicy::PRESERVE_ROW, 2, Instant::now() + Duration::from_secs(45))
            .await
            .map_err(|err| fact_line("prepare", err.fact()))?;
        self.quiet_source().await?;
        if prepared.expected_principal != "oo_a164_reader"
            || prepared.expected_database != "oo_a164_src"
            || prepared.expected_in_recovery
            || prepared.expected_postmaster_start.is_empty()
            || prepared.admitted.tables.len() != 2
        {
            return Err("prepare binding".to_string());
        }
        self.note(format!("postmaster_bytes={}", prepared.expected_postmaster_start.len()));
        for table in &prepared.admitted.tables {
            plain_ident(&table.schema)?;
            plain_ident(&table.name)?;
            let expected = column(
                &mut self.source,
                &format!(
                    "SELECT a.attnum::text || '|' || a.attname || '|' || a.atttypid::text || '|' || a.attnotnull::text FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = '{schema}' AND c.relname = '{name}' AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
                    schema = table.schema,
                    name = table.name
                ),
            )
            .await;
            let got: Vec<String> = table
                .columns
                .iter()
                .map(|column| format!("{}|{}|{}|{}", column.attnum, column.name, column.type_oid, column.not_null))
                .collect();
            if got != expected {
                return Err(format!("catalog columns {}", table.name));
            }
            let pk = column(
                &mut self.source,
                &format!(
                    "SELECT a.attname FROM pg_catalog.pg_index i JOIN pg_catalog.pg_class c ON c.oid = i.indrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace JOIN unnest(i.indkey) WITH ORDINALITY AS k(attnum, ord) ON true JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum WHERE n.nspname = '{schema}' AND c.relname = '{name}' AND i.indisprimary ORDER BY k.ord",
                    schema = table.schema,
                    name = table.name
                ),
            )
            .await;
            if pk != table.primary_key {
                return Err(format!("primary key {}", table.name));
            }
        }
        if catalog.tables.len() != prepared.admitted.tables.len() || catalog.schema_hash.is_empty() {
            return Err("catalog hash".to_string());
        }
        self.note(format!("catalog_hash={}", catalog.schema_hash));
        self.note("facts_and_catalog=bound");
        let _ = prepared;
        Ok(())
    }

    async fn not_ready(&mut self) -> Result<(), String> {
        let outcome = install(&mut self.target).await.map_err(|_| "install".to_string())?;
        if outcome != InstallOutcome::Bootstrapped {
            return Err("install outcome".to_string());
        }
        let (code, out, err) = self.run_import("a164early", 3);
        fs::write(self.evidence.join("a164-not-ready.stdout"), &out).unwrap();
        fs::write(self.evidence.join("a164-not-ready.stderr"), &err).unwrap();
        fs::write(self.evidence.join("a164-not-ready.exit"), code.to_string()).unwrap();
        self.reject_secrets(&out, &err)?;
        if code != 1 || !out.is_empty() || err.trim() != "oo-pipeline: target schema is not ready" {
            return Err(format!("not ready code={code}"));
        }
        if !flag(&mut self.target, "SELECT to_regclass('_oo.full_import_run') IS NULL").await {
            return Err("0005 relation appeared".to_string());
        }
        if text(&mut self.target, "SELECT count(*)::text FROM _oo.schema_migrations WHERE version = 'full-import/v1'").await != "0" {
            return Err("full-import migration row appeared".to_string());
        }
        self.quiet_source().await?;
        if text(&mut self.source, "SELECT count(*)::text FROM yard.write_audit WHERE who IN ('oo_a164_writer', 'oo_a164_reader')").await != "0" {
            return Err("not-ready wrote the source".to_string());
        }
        self.note("target_not_ready=exit_1_no_0005");
        Ok(())
    }

    async fn changed_catalog(&mut self) -> Result<(), String> {
        let connector = self.reader()?;
        let mut prepared = connector
            .prepare_snapshot(PgTypePolicy::PRESERVE_ROW, 4, Instant::now() + Duration::from_secs(45))
            .await
            .map_err(|err| fact_line("drift prepare", err.fact()))?;
        self.quiet_source().await?;
        raw(&mut self.source, "ALTER TABLE yard.hulls ADD COLUMN extra int4").await;
        prepared.generation = 5;
        let err = SnapshotOwner::open(prepared).await.expect_err("drift");
        if err.fact() != Some("catalog_identity") {
            return Err(format!("drift fact {}", err.fact().unwrap_or("none")));
        }
        raw(&mut self.source, "ALTER TABLE yard.hulls DROP COLUMN extra").await;
        self.quiet_source().await?;
        self.note("changed_catalog=refused");
        Ok(())
    }

    async fn sealed_import(&mut self) -> Result<(), String> {
        let outcome = install_full_import(&mut self.target).await.map_err(|_| "0005".to_string())?;
        if outcome != FullImportInstallOutcome::Installed {
            return Err("0005 outcome".to_string());
        }
        raw(&mut self.target, "GRANT oo_pipeline_runtime TO oo_a164_runtime").await;
        let (code, out, err) = self.run_import("a164main", 11);
        fs::write(self.evidence.join("a164-success.stdout"), &out).unwrap();
        fs::write(self.evidence.join("a164-success.stderr"), &err).unwrap();
        fs::write(self.evidence.join("a164-success.exit"), code.to_string()).unwrap();
        self.reject_secrets(&out, &err)?;
        if code != 0 || !err.is_empty() {
            return Err(format!("import code={code}"));
        }
        let doc: Value = serde_json::from_str(out.trim()).map_err(|_| "result json".to_string())?;
        if doc["protocol"] != "oo.pipeline.import-pg-result/v1" || doc["state"] != "awaiting_publish" {
            return Err("result protocol".to_string());
        }
        if doc["source_generation"] != 11 || doc["table_count"] != 2 {
            return Err("result counts".to_string());
        }
        let run_id = doc["run_id"].as_str().ok_or("run id")?.to_string();
        if run_id.len() != 36 || run_id.chars().any(|ch| !ch.is_ascii_hexdigit() && ch != '-') {
            return Err("run id shape".to_string());
        }
        let tables = doc["tables"].as_array().ok_or("tables")?;
        if tables.len() != 2 {
            return Err("table array".to_string());
        }
        let hulls = &tables[0];
        let fasteners = &tables[1];
        if hulls["schema"] != "yard" || hulls["name"] != "hulls" || hulls["rows"] != 2 {
            return Err("hulls summary".to_string());
        }
        if fasteners["schema"] != "yard" || fasteners["name"] != "fasteners" || fasteners["rows"] != 3 {
            return Err("fasteners summary".to_string());
        }
        for table in tables {
            physical_ok(table["physical_name"].as_str().unwrap_or(""), "pt_")?;
            if !hex64(table["canonical_digest"].as_str().unwrap_or("")) {
                return Err("digest shape".to_string());
            }
        }
        let state = text(
            &mut self.target,
            &format!("SELECT state FROM _oo.full_import_run WHERE run_id = '{run_id}'::uuid"),
        )
        .await;
        let sealed = flag(
            &mut self.target,
            &format!("SELECT sealed_at IS NOT NULL AND retired_at IS NULL FROM _oo.full_import_run WHERE run_id = '{run_id}'::uuid"),
        )
        .await;
        if state != "awaiting_publish" || !sealed {
            return Err(format!("seal state={state} sealed={sealed}"));
        }
        let stored = column(
            &mut self.target,
            &format!(
                "SELECT table_ident || '|' || applied_rows::text || '|' || canonical_digest FROM _oo.full_import_table WHERE run_id = '{run_id}'::uuid ORDER BY table_index"
            ),
        )
        .await;
        let expected = vec![
            format!("hulls|2|{}", hulls["canonical_digest"].as_str().unwrap_or("")),
            format!("fasteners|3|{}", fasteners["canonical_digest"].as_str().unwrap_or("")),
        ];
        if stored != expected {
            return Err("stored table seal".to_string());
        }
        self.shadow_values(hulls, "SELECT id::text || '|' || name || '|' || draft_mm::text FROM yard.hulls ORDER BY id", "id", "name", "draft_mm").await?;
        self.shadow_values(fasteners, "SELECT id::text || '|' || kind FROM yard.fasteners ORDER BY id", "id", "kind", "").await?;
        let independent = self.independent_digests().await?;
        if independent.get("hulls").map(String::as_str) != hulls["canonical_digest"].as_str()
            || independent.get("fasteners").map(String::as_str) != fasteners["canonical_digest"].as_str()
        {
            return Err("independent canonical digest".to_string());
        }
        if text(&mut self.target, "SELECT count(*)::text FROM pg_catalog.pg_publication").await != "0" {
            return Err("publication exists".to_string());
        }
        self.note(format!("seal={run_id}"));
        self.note(format!("canonical_hulls={}", hulls["canonical_digest"].as_str().unwrap_or("")));
        self.note(format!("canonical_fasteners={}", fasteners["canonical_digest"].as_str().unwrap_or("")));
        self.note("awaiting_publish_row=authoritative");
        Ok(())
    }

    async fn shadow_values(&mut self, table: &Value, source_sql: &str, id: &str, second: &str, third: &str) -> Result<(), String> {
        let physical = physical_ok(table["physical_name"].as_str().unwrap_or(""), "pt_")?;
        let source_rows = column(&mut self.source, source_sql).await;
        let columns = table["columns"].as_array().ok_or("columns")?;
        let id_col = physical_ok(find_column(columns, id)?, "pc_")?;
        let second_col = physical_ok(find_column(columns, second)?, "pc_")?;
        let projection = if third.is_empty() {
            format!("SELECT {id_col}::text || '|' || {second_col}::text FROM _oo.{physical} ORDER BY {id_col}")
        } else {
            let third_col = physical_ok(find_column(columns, third)?, "pc_")?;
            format!("SELECT {id_col}::text || '|' || {second_col}::text || '|' || {third_col}::text FROM _oo.{physical} ORDER BY {id_col}")
        };
        let shadow_rows = column(&mut self.target, &projection).await;
        if shadow_rows != source_rows {
            return Err(format!("shadow values {}", table["name"]));
        }
        self.note(format!("{}={}", table["name"].as_str().unwrap_or("table"), source_rows.len()));
        Ok(())
    }

    async fn independent_digests(&mut self) -> Result<std::collections::BTreeMap<String, String>, String> {
        let mut owner = SnapshotOwner::open(
            self.reader()?
                .prepare_snapshot(PgTypePolicy::PRESERVE_ROW, 21, Instant::now() + Duration::from_secs(45))
                .await
                .map_err(|err| fact_line("reread", err.fact()))?,
        )
        .await
        .map_err(|err| fact_line("reread open", err.fact()))?;
        owner
            .open_stream(StreamBudget::try_new(1_024, 1_048_576, 65_536, 262_144).map_err(|_| "stream budget".to_string())?)
            .await
            .map_err(|err| fact_line("stream", err.fact()))?;
        let mut hashes: std::collections::BTreeMap<String, Sha256> = std::collections::BTreeMap::new();
        loop {
            match owner.next_batch().await.map_err(|err| fact_line("batch", err.fact()))? {
                StreamItem::Batch(batch) => {
                    let hasher = hashes.entry(batch.typed.table.clone()).or_insert_with(Sha256::new);
                    for row in &batch.typed.rows {
                        hasher.update(row.canonical_hash);
                    }
                }
                StreamItem::TableEof(_) => {}
                StreamItem::Exhausted(_) => break,
            }
        }
        owner.abort().await.map_err(|err| fact_line("abort", err.fact()))?;
        self.quiet_source().await?;
        Ok(hashes.into_iter().map(|(name, hasher)| (name, hex_bytes(&hasher.finalize()))).collect())
    }

    async fn agent_and_writes(&mut self) -> Result<(), String> {
        let mut agent = dial(&self.host, self.port, "oo_a164_agent", &self.agent_password, "oo_a164_dst", &self.ca).await;
        let denied = sqlx::query(sqlx::AssertSqlSafe(
            "SELECT 1 FROM _oo.full_import_run".to_string(),
        ))
        .fetch_optional(&mut agent)
        .await;
        let Err(err) = denied else {
            return Err("agent read the seal".to_string());
        };
        let state = err
            .as_database_error()
            .and_then(|db| db.code().map(|code| code.to_string()))
            .unwrap_or_default();
        if state != "42501" {
            return Err(format!("agent sqlstate={state}"));
        }
        if text(&mut self.source, "SELECT count(*)::text FROM yard.hulls").await != "2"
            || text(&mut self.source, "SELECT count(*)::text FROM yard.write_audit WHERE who = 'oo_a164_reader'").await != "0"
        {
            return Err("source changed during import".to_string());
        }
        let mut writer = dial(&self.host, self.port, "oo_a164_writer", &self.writer_password, "oo_a164_src", &self.ca).await;
        raw(&mut writer, "INSERT INTO yard.hulls (id, name, draft_mm) VALUES (99, 'probe-rib', 1)").await;
        if text(&mut self.source, "SELECT count(*)::text FROM yard.write_audit WHERE who = 'oo_a164_writer'").await != "1" {
            return Err("writer audit".to_string());
        }
        if text(&mut self.source, "SELECT count(*)::text FROM yard.write_audit WHERE who = 'oo_a164_reader'").await != "0" {
            return Err("reader audit".to_string());
        }
        self.note("agent_denied=42501");
        self.note("source_writes=reader_0_writer_1");
        Ok(())
    }

    async fn constructed_summary_is_not_a_seal(&mut self) -> Result<(), String> {
        let forged = RunSummary {
            run_id: "00000000-0000-4000-8000-000000000164".to_string(),
            state: RunState::AwaitingPublish,
            source_id: "constructed".to_string(),
            source_generation: 11,
            target_generation: 1,
            tables: Vec::new(),
        };
        if forged.state != RunState::AwaitingPublish {
            return Err("constructed state".to_string());
        }
        let rows = text(
            &mut self.target,
            "SELECT count(*)::text FROM _oo.full_import_run WHERE run_id = '00000000-0000-4000-8000-000000000164'::uuid",
        )
        .await;
        if rows != "0" {
            return Err("constructed run was stored".to_string());
        }
        if text(&mut self.target, "SELECT count(*)::text FROM pg_catalog.pg_publication").await != "0" {
            return Err("constructed summary published".to_string());
        }
        self.note("constructed_run_summary_rows=0");
        Ok(())
    }

    fn reader(&self) -> Result<PgConnector, String> {
        self.connector("a164src", "oo_a164_reader", &self.reader_password)
    }

    fn connector(&self, source_id: &str, username: &str, password: &str) -> Result<PgConnector, String> {
        let config = PgSourceConfig::try_new(
            SourceId::new(source_id).map_err(|_| "source id".to_string())?,
            PgEndpoint::new(&self.host, self.port, "oo_a164_src").map_err(|_| "endpoint".to_string())?,
            PgCredentials::new(username, password).map_err(|_| "credentials".to_string())?,
            SecretRef::new("secret://pg/a164/reader").map_err(|_| "secret".to_string())?,
            PgTlsConfig::from_ca_pem(&self.ca).map_err(|_| "tls".to_string())?,
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(60),
                Duration::from_secs(30),
                10_000,
                8_388_608,
            )
            .map_err(|_| "budget".to_string())?,
            TableSelection::new(vec![
                SelectedTable::new("yard", "hulls").map_err(|_| "hulls".to_string())?,
                SelectedTable::new("yard", "fasteners").map_err(|_| "fasteners".to_string())?,
            ])
            .map_err(|_| "selection".to_string())?,
        )
        .map_err(|_| "config".to_string())?;
        Ok(PgConnector::new(config))
    }

    fn run_import(&self, shard: &str, generation: u64) -> (i32, String, String) {
        let body = json!({
            "protocol": "oo.pipeline.import-pg/v1",
            "source": {
                "source_id": "a164src",
                "host": self.host,
                "port": self.port,
                "database": "oo_a164_src",
                "username": "oo_a164_reader",
                "secret_ref": "secret://pg/a164/reader",
                "password_file": self.reader_file.display().to_string(),
                "ca_file": self.ca_file.display().to_string(),
                "tables": [
                    {"schema": "yard", "table": "hulls"},
                    {"schema": "yard", "table": "fasteners"}
                ]
            },
            "target": {
                "host": self.host,
                "port": self.port,
                "database": "oo_a164_dst",
                "runtime_principal": "oo_a164_runtime",
                "password_file": self.runtime_file.display().to_string(),
                "ca_file": self.ca_file.display().to_string()
            },
            "shard": shard,
            "owner": OWNER,
            "source_generation": generation
        })
        .to_string();
        let mut child = Command::new(env!("CARGO_BIN_EXE_oo-pipeline"))
            .arg("import-pg")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn");
        child.stdin.take().unwrap().write_all(body.as_bytes()).unwrap();
        let output = child.wait_with_output().expect("wait");
        (
            output.status.code().unwrap_or(127),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }

    fn reject_secrets(&self, out: &str, err: &str) -> Result<(), String> {
        for secret in [
            self.admin_password.as_str(),
            self.reader_password.as_str(),
            self.writer_password.as_str(),
            self.runtime_password.as_str(),
            self.agent_password.as_str(),
            "PRIVATE KEY",
        ] {
            if out.contains(secret) || err.contains(secret) {
                return Err("secret in output".to_string());
            }
        }
        Ok(())
    }

    async fn quiet_source(&mut self) -> Result<(), String> {
        for _ in 0..30 {
            let n = text(
                &mut self.postgres,
                "SELECT count(*)::text FROM pg_catalog.pg_stat_activity WHERE application_name = 'oo-pg-source' AND datname = 'oo_a164_src'",
            )
            .await;
            if n == "0" {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err("source session remained".to_string())
    }

    async fn cleanup(&mut self) {
        reset(&mut self.postgres).await;
    }
}

const SOURCE_SQL: &str = "\
CREATE SCHEMA yard;\n\
CREATE TABLE yard.hulls (id int4 PRIMARY KEY, name text NOT NULL, draft_mm int4 NOT NULL);\n\
CREATE TABLE yard.fasteners (id int4 PRIMARY KEY, kind text NOT NULL);\n\
CREATE TABLE yard.write_audit (who text NOT NULL);\n\
CREATE FUNCTION yard.note_write() RETURNS trigger LANGUAGE plpgsql AS $audit$\n\
BEGIN\n\
  INSERT INTO yard.write_audit (who) VALUES (current_user);\n\
  RETURN NEW;\n\
END;\n\
$audit$;\n\
CREATE TRIGGER hulls_audit BEFORE INSERT OR UPDATE OR DELETE ON yard.hulls\n\
  FOR EACH ROW EXECUTE FUNCTION yard.note_write();\n\
CREATE TRIGGER fasteners_audit BEFORE INSERT OR UPDATE OR DELETE ON yard.fasteners\n\
  FOR EACH ROW EXECUTE FUNCTION yard.note_write();\n\
INSERT INTO yard.hulls (id, name, draft_mm) VALUES (1, 'cedar-stem', 420), (2, 'ash-stem', 380);\n\
INSERT INTO yard.fasteners (id, kind) VALUES (10, 'copper-rove'), (11, 'bronze-nail'), (12, 'iron-rivet');\n\
GRANT USAGE ON SCHEMA yard TO oo_a164_reader, oo_a164_writer;\n\
GRANT SELECT ON yard.hulls, yard.fasteners TO oo_a164_reader;\n\
GRANT INSERT ON yard.hulls, yard.write_audit TO oo_a164_writer;\n\
";

fn read_secret(key: &str) -> String {
    fs::read_to_string(env::var(key).unwrap()).unwrap().trim().to_string()
}

fn fact_line(step: &str, fact: Option<&str>) -> String {
    format!("{step} {}", fact.unwrap_or("none"))
}

fn plain_ident(name: &str) -> Result<&str, String> {
    if !name.is_empty() && name.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_') {
        Ok(name)
    } else {
        Err("ident".to_string())
    }
}

fn physical_ok<'a>(name: &'a str, prefix: &str) -> Result<&'a str, String> {
    let bytes = name.as_bytes();
    if bytes.len() == 63
        && name.starts_with(prefix)
        && bytes[prefix.len()..].iter().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        Ok(name)
    } else {
        Err("physical name".to_string())
    }
}

fn hex64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
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

fn find_column<'a>(columns: &'a [Value], name: &str) -> Result<&'a str, String> {
    columns
        .iter()
        .find(|column| column["name"] == name)
        .and_then(|column| column["physical_name"].as_str())
        .ok_or_else(|| format!("column {name}"))
}

async fn dial(host: &str, port: u16, user: &str, pass: &str, database: &str, ca: &[u8]) -> sqlx::postgres::PgConnection {
    let options = PgConnectOptions::from_explicit()
        .host(host)
        .port(port)
        .username(user)
        .password(pass)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    assert!(options.get_options().is_none());
    options.disable_statement_logging().connect().await.expect("dial")
}

async fn raw(conn: &mut sqlx::postgres::PgConnection, sql: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut *conn)
        .await
        .expect("sql");
}

async fn text(conn: &mut sqlx::postgres::PgConnection, sql: &str) -> String {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .expect("scalar")
}

async fn flag(conn: &mut sqlx::postgres::PgConnection, sql: &str) -> bool {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .expect("flag")
}

async fn column(conn: &mut sqlx::postgres::PgConnection, sql: &str) -> Vec<String> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_all(&mut *conn)
        .await
        .expect("rows");
    rows.into_iter().map(|row| row.try_get(0).unwrap()).collect()
}

async fn reset(admin: &mut sqlx::postgres::PgConnection) {
    for name in ["oo_a164_src", "oo_a164_dst", "oo_a164_depth"] {
        raw(admin, &format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await;
    }
    for role_name in ["oo_a164_reader", "oo_a164_writer", "oo_a164_runtime", "oo_a164_agent", "oo_a164_depth_reader"] {
        raw(admin, &format!("DROP ROLE IF EXISTS {role_name}")).await;
    }
    raw(admin, "DROP ROLE IF EXISTS oo_pipeline_runtime").await;
    raw(admin, "DROP ROLE IF EXISTS oo_target_owner").await;
}

async fn role(conn: &mut sqlx::postgres::PgConnection, name: &str, secret: &str) {
    assert!(secret.chars().all(|ch| ch.is_ascii_hexdigit()) && !secret.is_empty());
    raw(
        conn,
        &format!(
            "CREATE ROLE {name} LOGIN PASSWORD '{secret}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION"
        ),
    )
    .await;
}
