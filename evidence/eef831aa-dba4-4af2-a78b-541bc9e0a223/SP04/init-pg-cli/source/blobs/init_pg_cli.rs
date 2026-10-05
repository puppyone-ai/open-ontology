//! Process checks for `init-pg`, plus one private PostgreSQL run.
//!
//! The ignored test uses the compiled binary, one source database, one target
//! database, and verify-full TLS. Error cases do not open a database.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use oo_pipeline::target_installer::{compiled_digests, compiled_full_import_digest};
use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};

const SECRET: &str = "s3cret-a223-marker";
const DECOY: &str = "decoy-a223-env";
const OWNER: &str = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb1";
const BAD_CHECKSUM: &str = "abababababababababababababababababababababababababababababababab";

#[test]
fn usage_exits_2_without_echoing_arguments() {
    assert_fail(&[], b"", 3, "invalid input");
    assert_fail(&[SECRET], SECRET.as_bytes(), 2, "usage");
    assert_fail(&["--help"], b"", 2, "usage");
    assert_fail(&["init-pg"], b"", 2, "usage");
}

#[test]
fn invalid_json_version_and_secret_field() {
    assert_fail(&[], b"{", 3, "invalid input");
    assert_fail(&[], b"\xff", 3, "invalid input");
    assert_fail(
        &[],
        br#"{"protocol":"oo.pipeline.init-pg/v1","protocol":"oo.pipeline.init-pg/v1"}"#,
        3,
        "invalid input",
    );
    let mut document = body("db.example", None);
    document["protocol"] = json!("oo.pipeline.init-pg/v9");
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid version");
    document = body("db.example", None);
    document["password"] = json!(SECRET);
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid input");
    document = body("db.example", None);
    document["target"]["password"] = json!(SECRET);
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid input");
    document = body("db.example", None);
    document["connect_ms"] = Value::Null;
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid input");
    let mut big = vec![b' '; 1_048_577];
    big.extend_from_slice(SECRET.as_bytes());
    assert_fail(&[], &big, 3, "invalid input");
}

#[test]
fn endpoint_and_budget_fail_before_files() {
    let missing = "/tmp/oo-a223-absent-password-file";
    let mut document = body(&format!("{SECRET}:5432"), None);
    document["target"]["password_file"] = json!(missing);
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid input");

    document = body("db.example", Some(0));
    document["target"]["password_file"] = json!(missing);
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid input");
    document["connect_ms"] = json!(120_001);
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid input");

    document = body("db.example", None);
    document["target"]["port"] = json!(0);
    document["target"]["password_file"] = json!(missing);
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid input");
    document = body("db.example", None);
    document["target"]["administrator"] = json!("has-hyphen");
    document["target"]["password_file"] = json!(missing);
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid input");
    document = body("postgres://db.example/app", None);
    document["target"]["password_file"] = json!(missing);
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid input");
}

#[test]
fn password_and_ca_files_fail_without_echoing_secrets() {
    let dir = scratch("files");
    let missing = dir.join(format!("zz-absent-{SECRET}"));
    let mut document = body("db.example", None);
    document["target"]["password_file"] = json!(missing.display().to_string());
    document["target"]["ca_file"] = json!(missing.display().to_string());
    assert_fail(&[], document.to_string().as_bytes(), 3, "target password file");

    let folder = dir.join(format!("not-a-file-{SECRET}"));
    fs::create_dir_all(&folder).unwrap();
    document["target"]["password_file"] = json!(folder.display().to_string());
    assert_fail(&[], document.to_string().as_bytes(), 3, "target password file");

    let password = dir.join("password");
    write_file(&password, format!("{SECRET}\n").as_bytes());
    let link = dir.join("linked");
    let _ = fs::remove_file(&link);
    std::os::unix::fs::symlink(&password, &link).unwrap();
    document["target"]["password_file"] = json!(link.display().to_string());
    assert_fail(&[], document.to_string().as_bytes(), 3, "target password file");

    write_file(&password, b"\r\n");
    document["target"]["password_file"] = json!(password.display().to_string());
    assert_fail(&[], document.to_string().as_bytes(), 3, "target password file");

    let mut nul = SECRET.as_bytes().to_vec();
    nul.push(0);
    write_file(&password, &nul);
    assert_fail(&[], document.to_string().as_bytes(), 3, "target password file");

    write_file(&password, &vec![b'a'; 1025]);
    assert_fail(&[], document.to_string().as_bytes(), 3, "target password file");

    write_file(&password, format!("{SECRET}\r\n").as_bytes());
    let ca = dir.join("ca.crt");
    write_file(
        &ca,
        format!("-----BEGIN PRIVATE KEY-----\n{SECRET}\n-----END PRIVATE KEY-----\n").as_bytes(),
    );
    document["target"]["ca_file"] = json!(ca.display().to_string());
    assert_fail(&[], document.to_string().as_bytes(), 3, "target ca file");

    write_file(&ca, format!("not-a-certificate\n{SECRET}\n").as_bytes());
    assert_fail(&[], document.to_string().as_bytes(), 3, "target ca file");
}

#[tokio::test]
#[ignore = "one private PostgreSQL 17.10; OO_INIT_PG_HOST must be set by the deciding run"]
async fn init_pg_binary_on_private_postgres() {
    let mut harness = Harness::connect().await;
    let result = harness.exercise().await;
    harness.cleanup().await;
    result.expect("init-pg live");
}

struct Harness {
    host: String,
    port: u16,
    admin_user: String,
    admin_password: String,
    admin_file: PathBuf,
    ca: Vec<u8>,
    ca_file: PathBuf,
    secrets: PathBuf,
    evidence: PathBuf,
    reader_password: String,
    reader_file: PathBuf,
    runtime_password: String,
    runtime_file: PathBuf,
    writer_password: String,
    limited_password: String,
    limited_file: PathBuf,
    postgres: sqlx::postgres::PgConnection,
    source: sqlx::postgres::PgConnection,
    target: sqlx::postgres::PgConnection,
}

impl Harness {
    async fn connect() -> Self {
        let host = env::var("OO_INIT_PG_HOST").expect("OO_INIT_PG_HOST");
        let port: u16 = env::var("OO_INIT_PG_PORT").expect("OO_INIT_PG_PORT").parse().unwrap();
        let admin_user = env::var("OO_INIT_PG_ADMIN_USER").expect("OO_INIT_PG_ADMIN_USER");
        let admin_file = PathBuf::from(env::var("OO_INIT_PG_ADMIN_PASSWORD_FILE").unwrap());
        let admin_password = read_secret(&admin_file);
        let ca_file = PathBuf::from(env::var("OO_INIT_PG_CA_FILE").unwrap());
        let ca = fs::read(&ca_file).expect("ca");
        let secrets = PathBuf::from(env::var("OO_INIT_PG_SECRET_DIR").unwrap());
        let evidence = PathBuf::from(env::var("OO_INIT_PG_EVIDENCE").unwrap());
        fs::create_dir_all(&secrets).unwrap();
        fs::create_dir_all(&evidence).unwrap();
        let mut postgres = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
        let version = text(&mut postgres, "SELECT version()").await;
        assert!(version.contains("17.10"), "{version}");
        let reader_password = hex_secret(&mut postgres, "reader").await;
        let runtime_password = hex_secret(&mut postgres, "runtime").await;
        let writer_password = hex_secret(&mut postgres, "writer").await;
        let limited_password = hex_secret(&mut postgres, "limited").await;
        let reader_file = secrets.join("reader.pass");
        let runtime_file = secrets.join("runtime.pass");
        let limited_file = secrets.join("limited.pass");
        write_secret(&reader_file, &reader_password);
        write_secret(&runtime_file, &runtime_password);
        write_secret(&limited_file, &limited_password);
        let source = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
        let target = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
        Self {
            host,
            port,
            admin_user,
            admin_password,
            admin_file,
            ca,
            ca_file,
            secrets,
            evidence,
            reader_password,
            reader_file,
            runtime_password,
            runtime_file,
            writer_password,
            limited_password,
            limited_file,
            postgres,
            source,
            target,
        }
    }

    async fn exercise(&mut self) -> Result<(), String> {
        self.setup().await;
        self.import_before_init().await?;
        let first = self.init_admin("oo_a223_dst", &self.ca_file).await?;
        self.save("init-first", &first);
        self.expect_init(&first, "bootstrapped", "installed", false)?;
        self.reject_secrets(&first.1, &first.2)?;
        let sealed = self.migration_text("oo_a223_dst").await;
        self.expect_no_publish("oo_a223_dst").await?;
        let runs = self.scalar("oo_a223_dst", "SELECT count(*)::text FROM _oo.full_import_run").await;
        if runs != "0" {
            return Err(format!("import rows before import-pg: {runs}"));
        }
        let replay = self.init_admin("oo_a223_dst", &self.ca_file).await?;
        self.save("init-replay", &replay);
        self.expect_init(&replay, "replay", "replay", false)?;
        let replay_rows = self.migration_text("oo_a223_dst").await;
        if replay_rows != sealed {
            return Err("replay changed schema_migrations".to_string());
        }
        self.grant_runtime().await;
        let imported = self.spawn_import();
        self.save("import", &imported);
        self.expect_import(&imported).await?;
        if self.migration_text("oo_a223_dst").await != sealed {
            return Err("import changed schema_migrations".to_string());
        }
        let again = self.init_admin("oo_a223_dst", &self.ca_file).await?;
        self.save("init-after-import", &again);
        self.expect_init(&again, "replay", "replay", false)?;
        if self.migration_text("oo_a223_dst").await != sealed {
            return Err("post-import replay changed schema_migrations".to_string());
        }
        let runs = self.scalar("oo_a223_dst", "SELECT count(*)::text FROM _oo.full_import_run").await;
        if runs != "1" {
            return Err(format!("import rows after replay: {runs}"));
        }
        self.wrong_ca(&sealed).await?;
        self.privilege(&sealed).await?;
        self.second_empty(&sealed).await?;
        self.partial_schema().await?;
        self.bad_checksum().await?;
        if self.migration_text("oo_a223_dst").await != sealed {
            return Err("negative cases changed the installed database".to_string());
        }
        fs::write(self.evidence.join("outcome.txt"), "init_pg_cli_ok\n").unwrap();
        Ok(())
    }

    async fn setup(&mut self) {
        self.reset().await;
        create_login(&mut self.postgres, "oo_a223_reader", &self.reader_password).await;
        create_login(&mut self.postgres, "oo_a223_writer", &self.writer_password).await;
        create_login(&mut self.postgres, "oo_a223_runtime", &self.runtime_password).await;
        create_login(&mut self.postgres, "oo_a223_limited", &self.limited_password).await;
        for name in ["oo_a223_src", "oo_a223_dst", "oo_a223_second", "oo_a223_partial", "oo_a223_checksum"] {
            create_db(&mut self.postgres, name).await;
        }
        self.source = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            "oo_a223_src",
            &self.ca,
        )
        .await;
        raw(&mut self.source, SOURCE_SQL).await;
        self.target = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            "oo_a223_dst",
            &self.ca,
        )
        .await;
    }

    async fn import_before_init(&mut self) -> Result<(), String> {
        let (code, out, err) = self.spawn_import();
        self.save("import-not-ready", &(code, out.clone(), err.clone()));
        if code != 1 || !out.is_empty() || err != "oo-pipeline: target schema is not ready\n" {
            return Err(format!("not ready code={code} err={err}"));
        }
        self.reject_secrets(&out, &err)?;
        let schema = self.scalar(
            "oo_a223_dst",
            "SELECT count(*)::text FROM pg_catalog.pg_namespace WHERE nspname = '_oo'",
        )
        .await;
        if schema != "0" {
            return Err("import-pg installed a schema".to_string());
        }
        Ok(())
    }

    async fn grant_runtime(&mut self) {
        raw(
            &mut self.postgres,
            "GRANT CONNECT ON DATABASE oo_a223_dst TO oo_a223_runtime",
        )
        .await;
        raw(&mut self.postgres, "GRANT oo_pipeline_runtime TO oo_a223_runtime").await;
    }

    async fn wrong_ca(&mut self, sealed: &str) -> Result<(), String> {
        let path = self.secrets.join("wrong-ca.crt");
        write_secret(
            &path,
            &format!("-----BEGIN CERTIFICATE-----\n{SECRET}\n-----END CERTIFICATE-----\n"),
        );
        let got = self.init_as("oo_a223_dst", &self.admin_user, &self.admin_file, &path).await?;
        self.save("wrong-ca", &got);
        if got.0 != 4 || !got.1.is_empty() || got.2 != "init-pg: target connect\n" {
            return Err(format!("wrong ca code={} err={}", got.0, got.2));
        }
        self.reject_secrets(&got.1, &got.2)?;
        if self.migration_text("oo_a223_dst").await != sealed {
            return Err("wrong ca changed migrations".to_string());
        }
        Ok(())
    }

    async fn privilege(&mut self, sealed: &str) -> Result<(), String> {
        raw(
            &mut self.postgres,
            "GRANT CONNECT ON DATABASE oo_a223_dst TO oo_a223_limited",
        )
        .await;
        let got = self
            .init_as("oo_a223_dst", "oo_a223_limited", &self.limited_file, &self.ca_file)
            .await?;
        self.save("privilege", &got);
        if got.0 != 5 || !got.1.is_empty() || got.2 != "init-pg: target administrator\n" {
            return Err(format!("privilege code={} err={}", got.0, got.2));
        }
        self.reject_secrets(&got.1, &got.2)?;
        if self.migration_text("oo_a223_dst").await != sealed {
            return Err("privilege changed migrations".to_string());
        }
        let owner = self
            .scalar(
                "oo_a223_dst",
                "SELECT pg_catalog.pg_has_role('oo_pipeline_runtime', 'oo_target_owner', 'MEMBER')::text",
            )
            .await;
        if owner != "false" {
            return Err(format!("runtime owns target: {owner}"));
        }
        Ok(())
    }

    async fn second_empty(&mut self, sealed: &str) -> Result<(), String> {
        let limited = self
            .init_as("oo_a223_second", "oo_a223_limited", &self.limited_file, &self.ca_file)
            .await?;
        self.save("second-privilege", &limited);
        if limited.0 != 5 || limited.2 != "init-pg: target administrator\n" {
            return Err(format!("second privilege code={} err={}", limited.0, limited.2));
        }
        raw(
            &mut self.postgres,
            "GRANT CONNECT ON DATABASE oo_a223_second TO oo_a223_limited",
        )
        .await;
        let limited = self
            .init_as("oo_a223_second", "oo_a223_limited", &self.limited_file, &self.ca_file)
            .await?;
        self.save("second-privilege-connect", &limited);
        if limited.0 != 5 || limited.2 != "init-pg: target administrator\n" {
            return Err(format!("second connect privilege code={} err={}", limited.0, limited.2));
        }
        let got = self.init_admin("oo_a223_second", &self.ca_file).await?;
        self.save("second-empty", &got);
        if got.0 != 6 || !got.1.is_empty() || got.2 != "init-pg: target catalog refused\n" {
            return Err(format!("second code={} err={}", got.0, got.2));
        }
        if got.2.contains("OO002") || got.2.contains("segment relation") {
            return Err("second stderr echoed the server".to_string());
        }
        self.reject_secrets(&got.1, &got.2)?;
        let schema = self.scalar(
            "oo_a223_second",
            "SELECT count(*)::text FROM pg_catalog.pg_namespace WHERE nspname = '_oo'",
        )
        .await;
        if schema != "0" {
            return Err(format!("second schema count {schema}"));
        }
        if self.migration_text("oo_a223_dst").await != sealed {
            return Err("second empty changed the first database".to_string());
        }
        Ok(())
    }

    async fn partial_schema(&mut self) -> Result<(), String> {
        let mut partial = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            "oo_a223_partial",
            &self.ca,
        )
        .await;
        raw(
            &mut partial,
            "CREATE SCHEMA _oo AUTHORIZATION oo_target_owner; \
             REVOKE ALL ON SCHEMA _oo FROM PUBLIC; \
             CREATE TABLE public.kept (id int PRIMARY KEY, note text NOT NULL); \
             INSERT INTO public.kept (id, note) VALUES (1, 'partial-a223');",
        )
        .await;
        drop(partial);
        let got = self.init_admin("oo_a223_partial", &self.ca_file).await?;
        self.save("partial", &got);
        if got.0 != 6 || got.2 != "init-pg: target catalog refused\n" {
            return Err(format!("partial code={} err={}", got.0, got.2));
        }
        if got.2.contains("segment relation") || got.2.contains("OO002") {
            return Err("partial stderr echoed the server".to_string());
        }
        let note = self.scalar("oo_a223_partial", "SELECT note FROM public.kept WHERE id = 1").await;
        let migrations = self.scalar(
            "oo_a223_partial",
            "SELECT (pg_catalog.to_regclass('_oo.schema_migrations') IS NULL)::text",
        )
        .await;
        if note != "partial-a223" || migrations != "true" {
            return Err(format!("partial preserved note={note} migrations_absent={migrations}"));
        }
        Ok(())
    }

    async fn bad_checksum(&mut self) -> Result<(), String> {
        let mut db = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            "oo_a223_checksum",
            &self.ca,
        )
        .await;
        let sql = format!(
            "CREATE SCHEMA _oo; \
             CREATE TABLE public.kept (id int PRIMARY KEY, note text NOT NULL); \
             INSERT INTO public.kept (id, note) VALUES (1, 'checksum-a223'); \
             CREATE TABLE _oo.schema_migrations ( \
               version text PRIMARY KEY, \
               checksum text NOT NULL, \
               phase text NOT NULL, \
               filename text NOT NULL, \
               applied_at timestamptz NOT NULL DEFAULT clock_timestamp(), \
               applied_by text NOT NULL DEFAULT current_user, \
               CONSTRAINT schema_migrations_checksum_check CHECK (checksum ~ '^[0-9a-f]{{64}}$') \
             ); \
             INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) \
             VALUES ('target-bootstrap/1', '{BAD_CHECKSUM}', 'expand', '0000_bootstrap_roles.sql');"
        );
        raw(&mut db, &sql).await;
        drop(db);
        let got = self.init_admin("oo_a223_checksum", &self.ca_file).await?;
        self.save("checksum", &got);
        if got.0 != 7 || got.2 != "init-pg: migration checksum\n" {
            return Err(format!("checksum code={} err={}", got.0, got.2));
        }
        if got.2.contains(BAD_CHECKSUM) || got.1.contains(BAD_CHECKSUM) || got.2.contains("OO001") {
            return Err("checksum stderr echoed the stored digest".to_string());
        }
        let note = self.scalar("oo_a223_checksum", "SELECT note FROM public.kept WHERE id = 1").await;
        let stored = self
            .scalar(
                "oo_a223_checksum",
                "SELECT checksum FROM _oo.schema_migrations WHERE version = 'target-bootstrap/1'",
            )
            .await;
        if note != "checksum-a223" || stored != BAD_CHECKSUM {
            return Err(format!("checksum preserved note={note} stored={stored}"));
        }
        Ok(())
    }

    async fn expect_import(&mut self, got: &(i32, String, String)) -> Result<(), String> {
        if got.0 != 0 {
            return Err(format!("import code={} err={}", got.0, got.2));
        }
        self.reject_secrets(&got.1, &got.2)?;
        let doc: Value = serde_json::from_str(got.1.trim()).map_err(|err| err.to_string())?;
        if doc["protocol"] != "oo.pipeline.import-pg-result/v1" || doc["state"] != "awaiting_publish" {
            return Err(format!("import document {doc}"));
        }
        if doc["source_generation"] != 11 || doc["table_count"] != 2 {
            return Err(format!("import counts {doc}"));
        }
        let tables = doc["tables"].as_array().ok_or("tables")?;
        self.compare_table(&tables[0], "crates", "label", &[("1", "spruce-keel"), ("2", "oak-rib")])
            .await?;
        self.compare_table(&tables[1], "tags", "name", &[("1", "hull"), ("2", "keel"), ("3", "deck")])
            .await?;
        let publications = self.scalar("oo_a223_dst", "SELECT count(*)::text FROM pg_catalog.pg_publication").await;
        if publications != "0" {
            return Err(format!("publications={publications}"));
        }
        let runs = self.scalar("oo_a223_dst", "SELECT count(*)::text FROM _oo.full_import_run").await;
        if runs != "1" {
            return Err(format!("runs={runs}"));
        }
        let state = self
            .scalar("oo_a223_dst", "SELECT state FROM _oo.full_import_run")
            .await;
        if state != "awaiting_publish" {
            return Err(format!("stored state {state}"));
        }
        let reader_writes = self
            .scalar(
                "oo_a223_src",
                "SELECT count(*)::text FROM app.write_audit WHERE who = 'oo_a223_reader'",
            )
            .await;
        if reader_writes != "0" {
            return Err(format!("reader writes={reader_writes}"));
        }
        let mut writer = dial(
            &self.host,
            self.port,
            "oo_a223_writer",
            &self.writer_password,
            "oo_a223_src",
            &self.ca,
        )
        .await;
        raw(&mut writer, "INSERT INTO app.crates (id, label) VALUES (3, 'after')").await;
        let reader_writes = self
            .scalar(
                "oo_a223_src",
                "SELECT count(*)::text FROM app.write_audit WHERE who = 'oo_a223_reader'",
            )
            .await;
        let writer_writes = self
            .scalar(
                "oo_a223_src",
                "SELECT count(*)::text FROM app.write_audit WHERE who = 'oo_a223_writer'",
            )
            .await;
        if reader_writes != "0" || writer_writes != "1" {
            return Err(format!("audit reader={reader_writes} writer={writer_writes}"));
        }
        Ok(())
    }

    async fn compare_table(
        &mut self,
        table: &Value,
        name: &str,
        label_column: &str,
        expect: &[(&str, &str)],
    ) -> Result<(), String> {
        if table["schema"] != "app" || table["name"] != name || table["rows"] != expect.len() {
            return Err(format!("table header {table}"));
        }
        let physical = table["physical_name"].as_str().ok_or("physical")?.to_string();
        let digest = table["canonical_digest"].as_str().ok_or("digest")?.to_string();
        if !safe_physical(&physical, "pt_") || digest.len() != 64 {
            return Err(format!("identity {physical} {digest}"));
        }
        let columns = table["columns"].as_array().ok_or("columns")?;
        let mut id_physical = String::new();
        let mut label_physical = String::new();
        for column in columns {
            let got = column["physical_name"].as_str().ok_or("column physical")?;
            if !safe_physical(got, "pc_") {
                return Err(format!("column physical {got}"));
            }
            if column["name"] == "id" {
                id_physical = got.to_string();
            }
            if column["name"] == label_column {
                label_physical = got.to_string();
            }
        }
        if id_physical.is_empty() || label_physical.is_empty() {
            return Err("missing id or label column".to_string());
        }
        let sql = format!(
            "SELECT {id_physical}::text || '=' || {label_physical}::text FROM _oo.\"{physical}\" ORDER BY \"_oo_pos\""
        );
        let got_rows = column_on(&mut self.target, &sql).await;
        let expect_rows: Vec<String> = expect.iter().map(|(id, label)| format!("{id}={label}")).collect();
        if got_rows != expect_rows {
            return Err(format!("values {got_rows:?}"));
        }
        Ok(())
    }

    fn expect_init(
        &self,
        got: &(i32, String, String),
        install_outcome: &str,
        full_outcome: &str,
        historical_v2: bool,
    ) -> Result<(), String> {
        if got.0 != 0 || !got.2.is_empty() {
            return Err(format!("init code={} err={}", got.0, got.2));
        }
        let doc: Value = serde_json::from_str(got.1.trim()).map_err(|err| err.to_string())?;
        let object = doc.as_object().ok_or("object")?;
        if object.len() != 6 || doc["protocol"] != "oo.pipeline.init-pg-result/v1" {
            return Err(format!("init document {doc}"));
        }
        if doc["install"] != install_outcome || doc["full_import"] != full_outcome || doc["historical_v2"] != historical_v2
        {
            return Err(format!("outcomes {doc}"));
        }
        let digests = compiled_digests();
        let seals = doc.get("seals").and_then(Value::as_object).ok_or("seals")?;
        if seals.len() != 4
            || doc["seals"]["bootstrap"] != digests.bootstrap()
            || doc["seals"]["fence"] != digests.fence()
            || doc["seals"]["preflight"] != digests.preflight()
            || doc["seals"]["mapping"] != digests.mapping()
            || doc["full_import_digest"] != compiled_full_import_digest()
        {
            return Err(format!("seals {doc}"));
        }
        if !got.1.ends_with('\n') || got.1.contains("awaiting_publish") {
            return Err("stdout shape".to_string());
        }
        Ok(())
    }

    async fn expect_no_publish(&mut self, database: &str) -> Result<(), String> {
        let publications = self
            .scalar(database, "SELECT count(*)::text FROM pg_catalog.pg_publication")
            .await;
        let owner = self
            .scalar(
                database,
                "SELECT pg_catalog.pg_has_role('oo_pipeline_runtime', 'oo_target_owner', 'MEMBER')::text",
            )
            .await;
        if publications != "0" || owner != "false" {
            return Err(format!("publish publications={publications} owner_member={owner}"));
        }
        Ok(())
    }

    async fn init_admin(&self, database: &str, ca_file: &Path) -> Result<(i32, String, String), String> {
        self.init_as(database, &self.admin_user, &self.admin_file, ca_file).await
    }

    async fn init_as(
        &self,
        database: &str,
        administrator: &str,
        password_file: &Path,
        ca_file: &Path,
    ) -> Result<(i32, String, String), String> {
        let body = json!({
            "protocol": "oo.pipeline.init-pg/v1",
            "connect_ms": 30000,
            "target": {
                "host": self.host,
                "port": self.port,
                "database": database,
                "administrator": administrator,
                "password_file": password_file.display().to_string(),
                "ca_file": ca_file.display().to_string()
            }
        });
        Ok(spawn(env!("CARGO_BIN_EXE_init-pg"), &[], body.to_string().as_bytes()))
    }

    fn spawn_import(&self) -> (i32, String, String) {
        let body = json!({
            "protocol": "oo.pipeline.import-pg/v1",
            "source": {
                "source_id": "a223src",
                "host": self.host,
                "port": self.port,
                "database": "oo_a223_src",
                "username": "oo_a223_reader",
                "secret_ref": "secret://pg/a223/reader",
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
                "database": "oo_a223_dst",
                "runtime_principal": "oo_a223_runtime",
                "password_file": self.runtime_file.display().to_string(),
                "ca_file": self.ca_file.display().to_string()
            },
            "shard": "a223main",
            "owner": OWNER,
            "source_generation": 11
        });
        spawn(env!("CARGO_BIN_EXE_oo-pipeline"), &["import-pg"], body.to_string().as_bytes())
    }

    fn save(&self, name: &str, got: &(i32, String, String)) {
        fs::write(self.evidence.join(format!("{name}.stdout")), &got.1).unwrap();
        fs::write(self.evidence.join(format!("{name}.stderr")), &got.2).unwrap();
        fs::write(self.evidence.join(format!("{name}.exit")), got.0.to_string()).unwrap();
    }

    fn reject_secrets(&self, out: &str, err: &str) -> Result<(), String> {
        for secret in [
            self.admin_password.as_str(),
            self.reader_password.as_str(),
            self.runtime_password.as_str(),
            self.writer_password.as_str(),
            self.limited_password.as_str(),
            SECRET,
            DECOY,
            "PRIVATE KEY",
            "BEGIN CERTIFICATE",
        ] {
            if out.contains(secret) || err.contains(secret) {
                return Err("secret appeared in process output".to_string());
            }
        }
        Ok(())
    }

    async fn migration_text(&mut self, database: &str) -> String {
        let mut conn = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            database,
            &self.ca,
        )
        .await;
        lines(
            &mut conn,
            "SELECT version || '=' || checksum || '=' || phase || '=' || filename || '=' || applied_at::text \
             FROM _oo.schema_migrations ORDER BY version",
        )
        .await
        .join("\n")
    }

    async fn scalar(&mut self, database: &str, sql: &str) -> String {
        if database == "oo_a223_dst" {
            return text(&mut self.target, sql).await;
        }
        if database == "oo_a223_src" {
            return text(&mut self.source, sql).await;
        }
        let mut conn = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            database,
            &self.ca,
        )
        .await;
        text(&mut conn, sql).await
    }

    async fn reset(&mut self) {
        for name in ["oo_a223_src", "oo_a223_dst", "oo_a223_second", "oo_a223_partial", "oo_a223_checksum"] {
            raw(&mut self.postgres, &format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await;
        }
        for role_name in ["oo_a223_reader", "oo_a223_writer", "oo_a223_runtime", "oo_a223_limited"] {
            raw(&mut self.postgres, &format!("DROP ROLE IF EXISTS {role_name}")).await;
        }
        raw(&mut self.postgres, "DROP ROLE IF EXISTS oo_pipeline_runtime").await;
        raw(&mut self.postgres, "DROP ROLE IF EXISTS oo_target_owner").await;
    }

    async fn cleanup(&mut self) {
        self.reset().await;
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
GRANT USAGE ON SCHEMA app TO oo_a223_reader, oo_a223_writer;\n\
GRANT SELECT ON app.crates, app.tags TO oo_a223_reader;\n\
GRANT INSERT ON app.crates, app.write_audit TO oo_a223_writer;\n\
";

fn body(host: &str, connect_ms: Option<u64>) -> Value {
    let mut document = json!({
        "protocol": "oo.pipeline.init-pg/v1",
        "target": {
            "host": host,
            "port": 5432,
            "database": "dstdb",
            "administrator": "admin",
            "password_file": "password",
            "ca_file": "ca.crt"
        }
    });
    if let Some(ms) = connect_ms {
        document["connect_ms"] = json!(ms);
    }
    document
}

fn assert_fail(args: &[&str], stdin: &[u8], code: i32, fact: &str) {
    let (got, out, err) = spawn(env!("CARGO_BIN_EXE_init-pg"), args, stdin);
    assert_eq!(got, code, "{err}");
    assert!(out.is_empty(), "{out}");
    assert_eq!(err, format!("init-pg: {fact}\n"));
    assert!(!err.contains(SECRET), "{err}");
    assert!(!out.contains(SECRET), "{out}");
    assert!(!err.contains(DECOY), "{err}");
    assert!(!out.contains(DECOY), "{out}");
}

fn spawn(bin: &str, args: &[&str], stdin: &[u8]) -> (i32, String, String) {
    let mut child = Command::new(bin)
        .args(args)
        .env_clear()
        .env("HOME", "/tmp")
        .env("PGPASSWORD", DECOY)
        .env("PGSSLROOTCERT", "/tmp/not-a-ca")
        .env("DATABASE_URL", format!("postgres://user:{DECOY}@db.example/app"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(stdin);
    }
    let output = child.wait_with_output().expect("wait");
    (
        output.status.code().unwrap_or(9),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn scratch(name: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!("oo-a223-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_file(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
}

fn write_secret(path: &Path, secret: &str) {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    writeln!(file, "{secret}").unwrap();
}

fn read_secret(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap();
    let text = if let Some(stripped) = text.strip_suffix("\r\n") {
        stripped.to_string()
    } else if let Some(stripped) = text.strip_suffix('\n') {
        stripped.to_string()
    } else {
        text
    };
    assert!(!text.is_empty());
    assert!(text.chars().all(|ch| ch.is_ascii_hexdigit()));
    text
}

async fn hex_secret(conn: &mut sqlx::postgres::PgConnection, label: &str) -> String {
    let sql = format!("SELECT encode(sha256(('{label}-' || clock_timestamp()::text)::bytea), 'hex')");
    let value = text(conn, &sql).await;
    assert!(value.chars().all(|ch| ch.is_ascii_hexdigit()) && value.len() == 64);
    value
}

async fn dial(
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
    database: &str,
    ca: &[u8],
) -> sqlx::postgres::PgConnection {
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
        .unwrap_or_else(|_| panic!("sql failed"));
}

async fn text(conn: &mut sqlx::postgres::PgConnection, sql: &str) -> String {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|_| panic!("scalar failed"))
}

async fn lines(conn: &mut sqlx::postgres::PgConnection, sql: &str) -> Vec<String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_all(&mut *conn)
        .await
        .unwrap_or_else(|_| panic!("lines failed"))
}

async fn column_on(conn: &mut sqlx::postgres::PgConnection, sql: &str) -> Vec<String> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_all(&mut *conn)
        .await
        .unwrap_or_else(|_| panic!("rows failed"));
    rows.into_iter().map(|row| row.try_get(0).unwrap()).collect()
}

async fn create_login(conn: &mut sqlx::postgres::PgConnection, name: &str, secret: &str) {
    assert!(name.starts_with("oo_a223_"));
    assert!(secret.chars().all(|ch| ch.is_ascii_hexdigit()) && !secret.is_empty());
    let sql = format!(
        "CREATE ROLE {name} LOGIN PASSWORD '{secret}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION"
    );
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|_| panic!("create role"));
}

async fn create_db(conn: &mut sqlx::postgres::PgConnection, name: &str) {
    assert!(name.starts_with("oo_a223_"));
    raw(
        conn,
        &format!(
            "CREATE DATABASE {name} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'"
        ),
    )
    .await;
}

fn safe_physical(name: &str, prefix: &str) -> bool {
    let bytes = name.as_bytes();
    let prefix = prefix.as_bytes();
    bytes.len() == 63
        && bytes.starts_with(prefix)
        && bytes[prefix.len()..].iter().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
