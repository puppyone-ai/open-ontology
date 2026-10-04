//! Process checks for `oo-pipeline import-pg`, plus one private PostgreSQL run.
//!
//! The ignored test uses the binary, two selected tables, and verify-full TLS.
//! Error cases do not open a database.

use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};

const SECRET: &str = "s3cret-a162-marker";
const OWNER: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1";

#[test]
fn usage_exits_2_without_echoing_arguments() {
    let (code, out, err) = run(&[], b"");
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(err.trim(), "oo-pipeline: usage");
    assert!(!err.contains("not implemented"));

    let (code, out, err) = run(&["inspect"], b"");
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(err.trim(), "oo-pipeline: usage");

    let (code, out, err) = run(&["import-pg", SECRET], b"");
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(err.trim(), "oo-pipeline: usage");
    assert!(!err.contains(SECRET));
    assert!(!out.contains(SECRET));
}

#[test]
fn invalid_json_version_field_and_duplicate_key() {
    assert_input(b"{", "invalid input");
    assert_input(b"\xff", "invalid input");
    assert_input(
        br#"{"protocol":"oo.pipeline.import-pg/v1","protocol":"oo.pipeline.import-pg/v1"}"#,
        "invalid input",
    );
    let mut body = minimal(7, None);
    body["protocol"] = json!("oo.pipeline.import-pg/v9");
    assert_body(&body, "invalid version");
    body = minimal(7, None);
    body["password"] = json!(SECRET);
    let (code, out, err) = run(&["import-pg"], body.to_string().as_bytes());
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(err.contains("invalid input"));
    assert!(!err.contains(SECRET));
    assert!(!out.contains(SECRET));
}

#[test]
fn file_budget_deadline_and_generation_errors_keep_the_secret() {
    let (code, out, err) = run(&["import-pg"], &with_budget(0));
    assert_eq!(code, 1, "{err}");
    assert!(out.is_empty());
    assert!(err.contains("invalid budget"));
    assert!(!err.contains(SECRET));

    let mut body = minimal(7, None);
    body["deadline_ms"] = json!(0);
    assert_body(&body, "invalid deadline");
    body["deadline_ms"] = json!(120_001);
    assert_body(&body, "invalid deadline");

    let dir = scratch("files");
    let missing = dir.join("zz-absent-a162-path");
    let mut body = minimal(7, None);
    body["source"]["password_file"] = json!(missing.display().to_string());
    let (code, out, err) = run(&["import-pg"], body.to_string().as_bytes());
    assert_eq!(code, 1, "{err}");
    assert!(out.is_empty());
    assert!(err.contains("source password file"));
    assert!(!err.contains("zz-absent-a162-path"));

    let folder = dir.join("not-a-file");
    fs::create_dir_all(&folder).unwrap();
    body["source"]["password_file"] = json!(folder.display().to_string());
    let (code, out, err) = run(&["import-pg"], body.to_string().as_bytes());
    assert_eq!(code, 1, "{err}");
    assert!(out.is_empty());
    assert!(err.contains("source password file"));
    assert!(!err.contains("not-a-file"));

    let password = dir.join("password");
    let ca = dir.join("ca.crt");
    fs::write(&password, format!("{SECRET}\n")).unwrap();
    fs::write(
        &ca,
        format!("-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n{SECRET}\n"),
    )
    .unwrap();
    let link = dir.join("linked");
    let _ = fs::remove_file(&link);
    std::os::unix::fs::symlink(&password, &link).unwrap();
    body["source"]["password_file"] = json!(link.display().to_string());
    body["source"]["ca_file"] = json!(ca.display().to_string());
    body["target"]["password_file"] = json!(password.display().to_string());
    body["target"]["ca_file"] = json!(ca.display().to_string());
    let (code, out, err) = run(&["import-pg"], body.to_string().as_bytes());
    assert_eq!(code, 1, "{err}");
    assert!(out.is_empty());
    assert!(err.contains("source password file"));
    assert!(!err.contains(SECRET));

    body["source"]["password_file"] = json!(password.display().to_string());
    body["source_generation"] = json!(0);
    let (code, out, err) = run(&["import-pg"], body.to_string().as_bytes());
    assert_eq!(code, 1, "{err}");
    assert!(out.is_empty());
    assert!(err.contains("invalid generation"));
    assert!(!out.contains(SECRET));
    assert!(!err.contains(SECRET));
    assert!(!err.contains("BEGIN CERTIFICATE"));

    body["source_generation"] = json!((i64::MAX as u64).saturating_add(1));
    let (code, out, err) = run(&["import-pg"], body.to_string().as_bytes());
    assert_eq!(code, 1, "{err}");
    assert!(out.is_empty());
    assert!(err.contains("invalid generation"));
    assert!(!out.contains(SECRET));
    assert!(!err.contains(SECRET));
    assert!(!err.contains("BEGIN CERTIFICATE"));
}

#[tokio::test]
#[ignore = "one private PostgreSQL 17.10; OO_IMPORT_PG_HOST must be set by the deciding run"]
async fn import_pg_binary_on_private_postgres() {
    let mut harness = Harness::connect().await;
    let result = harness.exercise().await;
    harness.cleanup().await;
    result.expect("import-pg live");
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
    ca_file: PathBuf,
    reader_file: PathBuf,
    runtime_file: PathBuf,
    evidence: PathBuf,
    postgres: sqlx::postgres::PgConnection,
    source: sqlx::postgres::PgConnection,
    target: sqlx::postgres::PgConnection,
}

impl Harness {
    async fn connect() -> Self {
        let host = env::var("OO_IMPORT_PG_HOST").expect("OO_IMPORT_PG_HOST");
        let port: u16 = env::var("OO_IMPORT_PG_PORT").expect("OO_IMPORT_PG_PORT").parse().unwrap();
        let admin_user = env::var("OO_IMPORT_PG_ADMIN_USER").expect("OO_IMPORT_PG_ADMIN_USER");
        let admin_password = password("OO_IMPORT_PG_ADMIN_PASSWORD_FILE");
        let reader_password = password("OO_IMPORT_PG_READER_PASSWORD_FILE");
        let runtime_password = password("OO_IMPORT_PG_RUNTIME_PASSWORD_FILE");
        let ca_file = PathBuf::from(env::var("OO_IMPORT_PG_CA_FILE").expect("OO_IMPORT_PG_CA_FILE"));
        let ca = fs::read(&ca_file).expect("ca");
        let evidence = PathBuf::from(
            env::var("OO_IMPORT_PG_EVIDENCE").unwrap_or_else(|_| "/tmp/oo-a162-import-evidence".to_string()),
        );
        fs::create_dir_all(&evidence).unwrap();
        let reader_file = PathBuf::from(env::var("OO_IMPORT_PG_READER_PASSWORD_FILE").unwrap());
        let runtime_file = PathBuf::from(env::var("OO_IMPORT_PG_RUNTIME_PASSWORD_FILE").unwrap());
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
            ca_file,
            reader_file,
            runtime_file,
            evidence,
            postgres,
            source,
            target,
        }
    }

    async fn exercise(&mut self) -> Result<(), String> {
        self.setup().await;
        self.not_ready().await?;
        self.install_import().await;
        self.succeed().await?;
        Ok(())
    }

    async fn setup(&mut self) {
        reset(&mut self.postgres).await;
        role(&mut self.postgres, "oo_a162_reader", &self.reader_password).await;
        role(&mut self.postgres, "oo_a162_writer", &self.writer_password).await;
        role(&mut self.postgres, "oo_a162_runtime", &self.runtime_password).await;
        role(&mut self.postgres, "oo_a162_agent", &self.agent_password).await;
        raw(&mut self.postgres, "CREATE DATABASE oo_a162_src TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
        raw(&mut self.postgres, "CREATE DATABASE oo_a162_dst TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
        self.source = dial(&self.host, self.port, &self.admin_user, &self.admin_password, "oo_a162_src", &self.ca).await;
        self.target = dial(&self.host, self.port, &self.admin_user, &self.admin_password, "oo_a162_dst", &self.ca).await;
        raw(&mut self.source, SOURCE_SQL).await;
        raw(&mut self.postgres, "GRANT CONNECT ON DATABASE oo_a162_dst TO oo_a162_runtime, oo_a162_agent").await;
        assert!(
            !flag(&mut self.source, "SELECT has_table_privilege('oo_a162_reader', 'app.crates', 'INSERT')").await
        );
    }

    async fn not_ready(&mut self) -> Result<(), String> {
        use oo_pipeline::target_installer::{InstallOutcome, install};
        assert_eq!(install(&mut self.target).await.expect("install"), InstallOutcome::Bootstrapped);
        let (code, out, err) = run(&["import-pg"], &self.request("a162early", 3));
        fs::write(self.evidence.join("not-ready.stdout"), &out).unwrap();
        fs::write(self.evidence.join("not-ready.stderr"), &err).unwrap();
        fs::write(self.evidence.join("not-ready.exit"), code.to_string()).unwrap();
        if code != 1 || !out.is_empty() || !err.contains("target schema is not ready") {
            return Err(format!("not ready code={code} err={err}"));
        }
        self.reject_secrets(&out, &err)?;
        let missing = flag(&mut self.target, "SELECT to_regclass('_oo.full_import_run') IS NULL").await;
        if !missing {
            return Err("0005 appeared without install_full_import".to_string());
        }
        self.quiet_source().await;
        Ok(())
    }

    async fn install_import(&mut self) {
        use oo_pipeline::target_installer::{FullImportInstallOutcome, install_full_import};
        assert_eq!(
            install_full_import(&mut self.target).await.expect("0005"),
            FullImportInstallOutcome::Installed
        );
        raw(&mut self.target, "GRANT oo_pipeline_runtime TO oo_a162_runtime").await;
    }

    async fn succeed(&mut self) -> Result<(), String> {
        let (code, out, err) = run(&["import-pg"], &self.request("a162main", 7));
        fs::write(self.evidence.join("success.stdout"), &out).unwrap();
        fs::write(self.evidence.join("success.stderr"), &err).unwrap();
        fs::write(self.evidence.join("success.exit"), code.to_string()).unwrap();
        if code != 0 {
            return Err(format!("import code={code} err={err}"));
        }
        self.reject_secrets(&out, &err)?;
        let doc: Value = serde_json::from_str(out.trim()).map_err(|err| err.to_string())?;
        if doc["protocol"] != "oo.pipeline.import-pg-result/v1" || doc["state"] != "awaiting_publish" {
            return Err(format!("result document {doc}"));
        }
        if doc["source_generation"] != 7 || doc["table_count"] != 2 {
            return Err(format!("counts {doc}"));
        }
        let tables = doc["tables"].as_array().ok_or("tables")?;
        self.compare_table(&doc["run_id"], &tables[0], "crates", "label", &[("1", "spruce-keel"), ("2", "oak-rib")]).await?;
        self.compare_table(&doc["run_id"], &tables[1], "tags", "name", &[("1", "hull"), ("2", "keel"), ("3", "deck")]).await?;
        let run_id = doc["run_id"].as_str().ok_or("run id")?.to_string();
        let mut again = dial(&self.host, self.port, &self.admin_user, &self.admin_password, "oo_a162_dst", &self.ca).await;
        let state: String = sqlx::query_scalar("SELECT state FROM _oo.full_import_run WHERE run_id = $1::uuid")
            .bind(&run_id)
            .fetch_one(&mut again)
            .await
            .map_err(|err| err.to_string())?;
        let sealed: bool = sqlx::query_scalar(
            "SELECT sealed_at IS NOT NULL FROM _oo.full_import_run WHERE run_id = $1::uuid",
        )
        .bind(&run_id)
        .fetch_one(&mut again)
        .await
        .map_err(|err| err.to_string())?;
        if state != "awaiting_publish" || !sealed {
            return Err(format!("seal state={state} sealed={sealed}"));
        }
        let publications = text(&mut again, "SELECT count(*)::text FROM pg_catalog.pg_publication").await;
        if publications != "0" {
            return Err(format!("publications={publications}"));
        }
        self.agent_denied(&tables[0]["physical_name"]).await?;
        let reader_writes = text(
            &mut self.source,
            "SELECT count(*)::text FROM app.write_audit WHERE who = 'oo_a162_reader'",
        )
        .await;
        if reader_writes != "0" {
            return Err(format!("reader writes={reader_writes}"));
        }
        let mut writer = dial(&self.host, self.port, "oo_a162_writer", &self.writer_password, "oo_a162_src", &self.ca).await;
        raw(&mut writer, "INSERT INTO app.crates (id, label) VALUES (3, 'after')").await;
        let reader_writes = text(
            &mut self.source,
            "SELECT count(*)::text FROM app.write_audit WHERE who = 'oo_a162_reader'",
        )
        .await;
        let writer_writes = text(
            &mut self.source,
            "SELECT count(*)::text FROM app.write_audit WHERE who = 'oo_a162_writer'",
        )
        .await;
        if reader_writes != "0" || writer_writes != "1" {
            return Err(format!("audit reader={reader_writes} writer={writer_writes}"));
        }
        self.quiet_source().await;
        fs::write(self.evidence.join("outcome.txt"), "import_pg_cli_live_ok\n").unwrap();
        Ok(())
    }

    async fn compare_table(
        &mut self,
        run_id: &Value,
        table: &Value,
        name: &str,
        label_column: &str,
        expect: &[(&str, &str)],
    ) -> Result<(), String> {
        let run_id = run_id.as_str().ok_or("run")?.to_string();
        if table["schema"] != "app" || table["name"] != name || table["rows"] != expect.len() {
            return Err(format!("table header {table}"));
        }
        let physical = table["physical_name"].as_str().ok_or("physical")?.to_string();
        let digest = table["canonical_digest"].as_str().ok_or("digest")?.to_string();
        if !safe_physical(&physical, "pt_") || digest.len() != 64 {
            return Err(format!("identity {physical} {digest}"));
        }
        let stored: (String, i64) = sqlx::query_as(
            "SELECT canonical_digest, applied_rows FROM _oo.full_import_table WHERE run_id = $1::uuid AND physical_name = $2",
        )
        .bind(&run_id)
        .bind(&physical)
        .fetch_one(&mut self.target)
        .await
        .map_err(|err| err.to_string())?;
        if stored.0 != digest || u64::try_from(stored.1).unwrap_or(0) != expect.len() as u64 {
            return Err(format!("stored {} {}", stored.0, stored.1));
        }
        let mapped: String = sqlx::query_scalar(
            "SELECT physical_name FROM _oo.physical_mappings WHERE source = 'a162src' AND level = 't' AND logical_namespace = 'app' AND logical_table = $1 AND logical_column = '' AND run_generation = 7",
        )
        .bind(name)
        .fetch_one(&mut self.target)
        .await
        .map_err(|err| err.to_string())?;
        if mapped != physical {
            return Err(format!("mapping {mapped}"));
        }
        let attrs = column(
            &mut self.target,
            &format!(
                "SELECT a.attname FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = '_oo' AND c.relname = '{physical}' AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum"
            ),
        )
        .await;
        let columns = table["columns"].as_array().ok_or("columns")?;
        if attrs.first().map(String::as_str) != Some("_oo_pos") || attrs.len() != columns.len() + 1 {
            return Err(format!("attributes {attrs:?}"));
        }
        let mut id_physical = String::new();
        let mut label_physical = String::new();
        for (index, column) in columns.iter().enumerate() {
            let got = column["physical_name"].as_str().ok_or("column physical")?;
            if attrs.get(index + 1).map(String::as_str) != Some(got) {
                return Err(format!("column order {got}"));
            }
            let logical = column["name"].as_str().ok_or("column name")?;
            let stored_column: String = sqlx::query_scalar(
                "SELECT physical_name FROM _oo.physical_mappings WHERE source = 'a162src' AND level = 'c' AND logical_namespace = 'app' AND logical_table = $1 AND logical_column = $2 AND run_generation = 7",
            )
            .bind(name)
            .bind(logical)
            .fetch_one(&mut self.target)
            .await
            .map_err(|err| err.to_string())?;
            if stored_column != got {
                return Err(format!("column mapping {stored_column}"));
            }
            if !safe_physical(got, "pc_") {
                return Err(format!("column physical {got}"));
            }
            if logical == "id" {
                id_physical = got.to_string();
            }
            if logical == label_column {
                label_physical = got.to_string();
            }
        }
        if id_physical.is_empty() || label_physical.is_empty() {
            return Err("missing id or label column".to_string());
        }
        let shadow = column(
            &mut self.target,
            &format!(
                "SELECT {id_physical}::text || '=' || {label_physical}::text FROM _oo.\"{physical}\" ORDER BY \"_oo_pos\""
            ),
        )
        .await;
        let mut expect_rows: Vec<String> = expect.iter().map(|(id, label)| format!("{id}={label}")).collect();
        let mut got_rows = shadow.clone();
        expect_rows.sort();
        got_rows.sort();
        if expect_rows != got_rows {
            return Err(format!("values {got_rows:?}"));
        }
        Ok(())
    }

    async fn agent_denied(&mut self, physical: &Value) -> Result<(), String> {
        let name = physical.as_str().ok_or("shadow")?.to_string();
        if !safe_physical(&name, "pt_") {
            return Err("shadow name".to_string());
        }
        let mut agent = dial(
            &self.host,
            self.port,
            "oo_a162_agent",
            &self.agent_password,
            "oo_a162_dst",
            &self.ca,
        )
        .await;
        let err = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT 1 FROM _oo.\"{name}\" LIMIT 1",
        )))
            .fetch_optional(&mut agent)
            .await
            .expect_err("agent read");
        let sqlx::Error::Database(db) = err else {
            return Err("agent error kind".to_string());
        };
        if db.code().as_deref() != Some("42501") {
            return Err(format!("agent sqlstate {:?}", db.code()));
        }
        Ok(())
    }

    async fn quiet_source(&mut self) {
        for _ in 0..20 {
            let n = text(
                &mut self.postgres,
                "SELECT count(*)::text FROM pg_catalog.pg_stat_activity WHERE application_name = 'oo-pg-source'",
            )
            .await;
            if n == "0" {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("source session remained");
    }

    fn request(&self, shard: &str, generation: u64) -> Vec<u8> {
        json!({
            "protocol": "oo.pipeline.import-pg/v1",
            "source": {
                "source_id": "a162src",
                "host": self.host,
                "port": self.port,
                "database": "oo_a162_src",
                "username": "oo_a162_reader",
                "secret_ref": "secret://pg/a162/reader",
                "password_file": self.reader_file.display().to_string(),
                "ca_file": self.ca_file.display().to_string(),
                "tables": [
                    {"schema": "app", "table": "crates"},
                    {"schema": "app", "table": "tags"}
                ]
            },
            "target": {
                "host": self.host,
                "port": self.port,
                "database": "oo_a162_dst",
                "runtime_principal": "oo_a162_runtime",
                "password_file": self.runtime_file.display().to_string(),
                "ca_file": self.ca_file.display().to_string()
            },
            "shard": shard,
            "owner": OWNER,
            "source_generation": generation
        })
        .to_string()
        .into_bytes()
    }

    fn reject_secrets(&self, out: &str, err: &str) -> Result<(), String> {
        for secret in [
            self.admin_password.as_str(),
            self.reader_password.as_str(),
            self.runtime_password.as_str(),
            self.writer_password.as_str(),
            self.agent_password.as_str(),
            "PRIVATE KEY",
        ] {
            if out.contains(secret) || err.contains(secret) {
                return Err("secret appeared in process output".to_string());
            }
        }
        Ok(())
    }

    async fn cleanup(&mut self) {
        reset(&mut self.postgres).await;
    }
}

const SOURCE_SQL: &str = "\
CREATE SCHEMA app;\n\
CREATE TABLE app.crates (id int4 PRIMARY KEY, label text NOT NULL);\n\
CREATE TABLE app.tags (id int4 PRIMARY KEY, name text NOT NULL);\n\
CREATE TABLE app.write_audit (who text NOT NULL);\n\
CREATE FUNCTION app.note_write() RETURNS trigger LANGUAGE plpgsql AS $audit$\n\
BEGIN\n\
  INSERT INTO app.write_audit (who) VALUES (current_user);\n\
  RETURN NEW;\n\
END;\n\
$audit$;\n\
CREATE TRIGGER crates_audit BEFORE INSERT OR UPDATE OR DELETE ON app.crates\n\
  FOR EACH ROW EXECUTE FUNCTION app.note_write();\n\
CREATE TRIGGER tags_audit BEFORE INSERT OR UPDATE OR DELETE ON app.tags\n\
  FOR EACH ROW EXECUTE FUNCTION app.note_write();\n\
INSERT INTO app.crates (id, label) VALUES (1, 'spruce-keel'), (2, 'oak-rib');\n\
INSERT INTO app.tags (id, name) VALUES (1, 'hull'), (2, 'keel'), (3, 'deck');\n\
GRANT USAGE ON SCHEMA app TO oo_a162_reader, oo_a162_writer;\n\
GRANT SELECT ON app.crates, app.tags TO oo_a162_reader;\n\
GRANT INSERT ON app.crates, app.write_audit TO oo_a162_writer;\n\
";

fn minimal(generation: u64, budget_rows: Option<u64>) -> Value {
    let mut source = json!({
        "source_id": "a162src",
        "host": "db.example",
        "port": 5432,
        "database": "srcdb",
        "username": "reader",
        "secret_ref": "secret://pg/a162/reader",
        "password_file": "reader-password",
        "ca_file": "ca.crt",
        "tables": [{"schema": "app", "table": "crates"}]
    });
    if let Some(max_rows) = budget_rows {
        source["budget"] = json!({
            "connect_ms": 30000,
            "statement_ms": 60000,
            "lock_ms": 30000,
            "max_rows": max_rows,
            "max_bytes": 1024
        });
    }
    json!({
        "protocol": "oo.pipeline.import-pg/v1",
        "source": source,
        "target": {
            "host": "db.example",
            "port": 5432,
            "database": "dstdb",
            "runtime_principal": "runtime",
            "password_file": "runtime-password",
            "ca_file": "ca.crt"
        },
        "shard": "a162main",
        "owner": OWNER,
        "source_generation": generation
    })
}

fn with_budget(max_rows: u64) -> Vec<u8> {
    minimal(7, Some(max_rows)).to_string().into_bytes()
}

fn assert_body(body: &Value, needle: &str) {
    let (code, out, err) = run(&["import-pg"], body.to_string().as_bytes());
    assert_eq!(code, 1, "{err}");
    assert!(out.is_empty(), "{out}");
    assert!(err.contains(needle), "{err}");
    assert!(!err.contains(SECRET));
}

fn assert_input(bytes: &[u8], needle: &str) {
    let (code, out, err) = run(&["import-pg"], bytes);
    assert_eq!(code, 1, "{err}");
    assert!(out.is_empty());
    assert!(err.contains(needle), "{err}");
}

fn run(args: &[&str], stdin: &[u8]) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_oo-pipeline"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let output = child.wait_with_output().unwrap();
    (
        output.status.code().unwrap_or(9),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn scratch(name: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!("oo-a162-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn password(var: &str) -> String {
    let text = fs::read_to_string(env::var(var).unwrap()).unwrap();
    let trimmed = text.trim().to_string();
    assert!(trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) && !trimmed.is_empty());
    trimmed
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
    let options = options.disable_statement_logging();
    sqlx::postgres::PgConnection::connect_with(&options).await.expect("dial")
}

async fn raw(conn: &mut sqlx::postgres::PgConnection, sql: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("sql failed: {err}"));
}

async fn text(conn: &mut sqlx::postgres::PgConnection, sql: &str) -> String {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("scalar failed: {err}"))
}

async fn flag(conn: &mut sqlx::postgres::PgConnection, sql: &str) -> bool {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("flag failed: {err}"))
}

async fn column(conn: &mut sqlx::postgres::PgConnection, sql: &str) -> Vec<String> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_all(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("rows failed: {err}"));
    rows.into_iter().map(|row| row.try_get(0).unwrap()).collect()
}

async fn reset(admin: &mut sqlx::postgres::PgConnection) {
    for name in ["oo_a162_src", "oo_a162_dst"] {
        raw(admin, &format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await;
    }
    for role_name in ["oo_a162_reader", "oo_a162_writer", "oo_a162_runtime", "oo_a162_agent"] {
        raw(admin, &format!("DROP ROLE IF EXISTS {role_name}")).await;
    }
    raw(admin, "DROP ROLE IF EXISTS oo_pipeline_runtime").await;
    raw(admin, "DROP ROLE IF EXISTS oo_target_owner").await;
}

async fn role(conn: &mut sqlx::postgres::PgConnection, name: &str, secret: &str) {
    assert!(secret.chars().all(|ch| ch.is_ascii_hexdigit()));
    raw(
        conn,
        &format!(
            "CREATE ROLE {name} LOGIN PASSWORD '{secret}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION"
        ),
    )
    .await;
}

fn safe_physical(name: &str, prefix: &str) -> bool {
    let bytes = name.as_bytes();
    let prefix = prefix.as_bytes();
    bytes.len() == 63
        && bytes.starts_with(prefix)
        && bytes[prefix.len()..]
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
