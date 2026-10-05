//! Independent review oracle for frozen `init-pg`.
//!
//! This file is copied onto an archive of commit 6e1e6e57. It is not product
//! source. It drives the compiled `init-pg` and `oo-pipeline import-pg`
//! binaries against one private PostgreSQL 17.10 cluster.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use oo_pipeline::target_installer::{
    compiled_digests, compiled_full_import_digest, VERSION_BOOTSTRAP, VERSION_FENCE,
    VERSION_FULL_IMPORT, VERSION_MAPPING, VERSION_PREFLIGHT, VERSION_SEGMENT_V2,
};
use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Connection};

const MARKER: &str = "s3cret-a232-marker";
const DECOY: &str = "decoy-a232-env";
const OWNER: &str = "a232a232-a232-4a23-8a23-2a232a232a23";
const BAD_CHECKSUM: &str = "abababababababababababababababababababababababababababababababab";

#[tokio::test(flavor = "current_thread")]
#[ignore = "one private PostgreSQL 17.10; OO_A232_PG_HOST must be set by the review run"]
async fn init_pg_a232_review_on_private_cluster() {
    let mut harness = Harness::connect().await;
    let result = harness.exercise().await;
    let cleanup = harness.cleanup().await;
    match (result, cleanup) {
        (Err(err), _) => panic!("{err}"),
        (Ok(()), Err(err)) => panic!("cleanup {err}"),
        (Ok(()), Ok(())) => {}
    }
}

struct Harness {
    host: String,
    port: u16,
    admin_user: String,
    admin_password: String,
    admin_file: PathBuf,
    ca: Vec<u8>,
    ca_file: PathBuf,
    wrong_ca: PathBuf,
    evidence: PathBuf,
    reader_password: String,
    reader_file: PathBuf,
    runtime_password: String,
    runtime_file: PathBuf,
    writer_password: String,
    limited_password: String,
    limited_file: PathBuf,
    postgres: sqlx::postgres::PgConnection,
    checks: Vec<String>,
}

impl Harness {
    async fn connect() -> Self {
        let host = env::var("OO_A232_PG_HOST").expect("OO_A232_PG_HOST");
        let port: u16 = env::var("OO_A232_PG_PORT").expect("OO_A232_PG_PORT").parse().unwrap();
        let admin_user = env::var("OO_A232_ADMIN_USER").expect("OO_A232_ADMIN_USER");
        let admin_file = PathBuf::from(env::var("OO_A232_ADMIN_PASSWORD_FILE").unwrap());
        let admin_password = read_secret(&admin_file);
        let ca_file = PathBuf::from(env::var("OO_A232_CA_FILE").unwrap());
        let wrong_ca = PathBuf::from(env::var("OO_A232_WRONG_CA_FILE").unwrap());
        let ca = fs::read(&ca_file).expect("ca");
        let secrets = PathBuf::from(env::var("OO_A232_SECRET_DIR").unwrap());
        let evidence = PathBuf::from(env::var("OO_A232_EVIDENCE").unwrap());
        fs::create_dir_all(&secrets).unwrap();
        fs::create_dir_all(&evidence).unwrap();
        let mut postgres = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
        let version = text(&mut postgres, "SELECT version()").await.expect("version");
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
        Self {
            host,
            port,
            admin_user,
            admin_password,
            admin_file,
            ca,
            ca_file,
            wrong_ca,
            evidence,
            reader_password,
            reader_file,
            runtime_password,
            runtime_file,
            writer_password,
            limited_password,
            limited_file,
            postgres,
            checks: Vec::new(),
        }
    }

    async fn exercise(&mut self) -> Result<(), String> {
        self.setup().await?;
        self.import_before_init().await?;
        let first = self.init_admin("oo_a232_dst", &self.ca_file).await?;
        self.save("init-first", &first);
        self.expect_init(&first, "bootstrapped", "installed", false)?;
        self.reject_secrets(&first.1, &first.2)?;
        self.expect_stored_checksums().await?;
        self.expect_runtime_not_owner("after_init").await?;
        let sealed = self.migration_text("oo_a232_dst").await?;
        fs::write(self.evidence.join("migrations-sealed.txt"), &sealed).unwrap();
        self.expect_runs("oo_a232_dst", "0").await?;
        let replay = self.init_admin("oo_a232_dst", &self.ca_file).await?;
        self.save("init-replay", &replay);
        self.expect_init(&replay, "replay", "replay", false)?;
        let replay_rows = self.migration_text("oo_a232_dst").await?;
        if replay_rows != sealed {
            return Err("replay changed schema_migrations".to_string());
        }
        self.checks.push("replay_applied_at=unchanged".to_string());
        self.grant_runtime().await?;
        self.expect_runtime_not_owner("after_grant").await?;
        let imported = self.spawn_import();
        self.save("import", &imported);
        self.expect_import(&imported).await?;
        if self.migration_text("oo_a232_dst").await? != sealed {
            return Err("import changed schema_migrations".to_string());
        }
        let again = self.init_admin("oo_a232_dst", &self.ca_file).await?;
        self.save("init-after-import", &again);
        self.expect_init(&again, "replay", "replay", false)?;
        if self.migration_text("oo_a232_dst").await? != sealed {
            return Err("post-import replay changed schema_migrations".to_string());
        }
        self.expect_runs("oo_a232_dst", "1").await?;
        self.wrong_ca(&sealed).await?;
        self.privilege(&sealed).await?;
        self.second_empty(&sealed).await?;
        self.partial_schema().await?;
        self.bad_checksum().await?;
        if self.migration_text("oo_a232_dst").await? != sealed {
            return Err("negative cases changed the installed database".to_string());
        }
        self.checks.push("first_database=unchanged".to_string());
        fs::write(self.evidence.join("checks.txt"), self.checks.join("\n") + "\n").unwrap();
        fs::write(self.evidence.join("outcome.txt"), "init_pg_review_ok\n").unwrap();
        Ok(())
    }

    async fn setup(&mut self) -> Result<(), String> {
        self.reset().await?;
        create_login(&mut self.postgres, "oo_a232_reader", &self.reader_password).await?;
        create_login(&mut self.postgres, "oo_a232_writer", &self.writer_password).await?;
        create_login(&mut self.postgres, "oo_a232_runtime", &self.runtime_password).await?;
        create_login(&mut self.postgres, "oo_a232_limited", &self.limited_password).await?;
        for name in [
            "oo_a232_src",
            "oo_a232_dst",
            "oo_a232_second",
            "oo_a232_partial",
            "oo_a232_checksum",
        ] {
            create_db(&mut self.postgres, name).await?;
        }
        let names = text(
            &mut self.postgres,
            "SELECT string_agg(datname, ',' ORDER BY datname) FROM pg_catalog.pg_database WHERE datname LIKE 'oo_a232_%'",
        )
        .await?;
        fs::write(self.evidence.join("cluster-databases.txt"), names.clone() + "\n").unwrap();
        if names != "oo_a232_checksum,oo_a232_dst,oo_a232_partial,oo_a232_second,oo_a232_src" {
            return Err(format!("cluster databases {names}"));
        }
        self.checks.push("cluster_databases=5".to_string());
        let mut source = self.admin_on("oo_a232_src").await;
        raw(&mut source, SOURCE_SQL).await?;
        Ok(())
    }

    async fn import_before_init(&mut self) -> Result<(), String> {
        let got = self.spawn_import();
        self.save("import-not-ready", &got);
        if got.0 != 1 || !got.1.is_empty() || got.2 != "oo-pipeline: target schema is not ready\n" {
            return Err(format!("not ready code={} err={}", got.0, got.2.trim()));
        }
        self.reject_secrets(&got.1, &got.2)?;
        let schema = self.scalar("oo_a232_dst", "SELECT count(*)::text FROM pg_catalog.pg_namespace WHERE nspname = '_oo'").await?;
        if schema != "0" {
            return Err("import-pg installed a schema".to_string());
        }
        self.checks.push("import_before_init=refused".to_string());
        Ok(())
    }

    async fn expect_stored_checksums(&mut self) -> Result<(), String> {
        let digests = compiled_digests();
        let full = compiled_full_import_digest();
        let rows = self
            .lines(
                "oo_a232_dst",
                "SELECT version || '=' || checksum FROM _oo.schema_migrations ORDER BY version",
            )
            .await?;
        fs::write(self.evidence.join("stored-checksums.txt"), rows.join("\n") + "\n").unwrap();
        let mut stored = std::collections::BTreeMap::new();
        for row in &rows {
            let (version, checksum) = row.split_once('=').ok_or("checksum row")?;
            stored.insert(version, checksum);
        }
        let expect = [
            (VERSION_BOOTSTRAP, digests.bootstrap()),
            (VERSION_FENCE, digests.fence()),
            (VERSION_PREFLIGHT, digests.preflight()),
            (VERSION_MAPPING, digests.mapping()),
            (VERSION_FULL_IMPORT, full.as_str()),
        ];
        if stored.len() != expect.len() {
            return Err(format!("stored checksum count {}", stored.len()));
        }
        for (version, checksum) in expect {
            if stored.get(version) != Some(&checksum) {
                return Err(format!("stored checksum {version}"));
            }
        }
        let historical_sql = format!(
            "SELECT count(*)::text FROM _oo.schema_migrations WHERE version = '{VERSION_SEGMENT_V2}'"
        );
        let historical = self.scalar("oo_a232_dst", &historical_sql).await?;
        if historical != "0" {
            return Err(format!("historical v2 rows {historical}"));
        }
        self.checks.push("stored_checksums=match".to_string());
        Ok(())
    }

    async fn expect_runtime_not_owner(&mut self, label: &str) -> Result<(), String> {
        let owner = self
            .scalar(
                "oo_a232_dst",
                "SELECT pg_catalog.pg_has_role('oo_pipeline_runtime', 'oo_target_owner', 'MEMBER')::text",
            )
            .await?;
        let login = self
            .scalar(
                "oo_a232_dst",
                "SELECT pg_catalog.pg_has_role('oo_a232_runtime', 'oo_target_owner', 'MEMBER')::text",
            )
            .await?;
        if owner != "false" || login != "false" {
            return Err(format!("runtime owner member {label} role={owner} login={login}"));
        }
        self.checks.push(format!("runtime_owner_member_{label}=false"));
        Ok(())
    }

    async fn grant_runtime(&mut self) -> Result<(), String> {
        raw(&mut self.postgres, "GRANT CONNECT ON DATABASE oo_a232_dst TO oo_a232_runtime").await?;
        raw(&mut self.postgres, "GRANT oo_pipeline_runtime TO oo_a232_runtime").await?;
        let member = self
            .scalar(
                "oo_a232_dst",
                "SELECT pg_catalog.pg_has_role('oo_a232_runtime', 'oo_pipeline_runtime', 'MEMBER')::text",
            )
            .await?;
        if member != "true" {
            return Err(format!("runtime grant {member}"));
        }
        self.checks.push("harness_granted_runtime=true".to_string());
        Ok(())
    }

    async fn wrong_ca(&mut self, sealed: &str) -> Result<(), String> {
        let got = self
            .init_as("oo_a232_dst", &self.admin_user, &self.admin_file, &self.wrong_ca)
            .await?;
        self.save("wrong-ca", &got);
        if got.0 != 4 || !got.1.is_empty() || got.2 != "init-pg: target connect\n" {
            return Err(format!("wrong ca code={} err={}", got.0, got.2.trim()));
        }
        self.reject_secrets(&got.1, &got.2)?;
        if self.migration_text("oo_a232_dst").await? != sealed {
            return Err("wrong ca changed migrations".to_string());
        }
        self.checks.push("wrong_ca=4".to_string());
        Ok(())
    }

    async fn privilege(&mut self, sealed: &str) -> Result<(), String> {
        raw(&mut self.postgres, "GRANT CONNECT ON DATABASE oo_a232_dst TO oo_a232_limited").await?;
        let got = self
            .init_as("oo_a232_dst", "oo_a232_limited", &self.limited_file, &self.ca_file)
            .await?;
        self.save("privilege", &got);
        if got.0 != 5 || !got.1.is_empty() || got.2 != "init-pg: target administrator\n" {
            return Err(format!("privilege code={} err={}", got.0, got.2.trim()));
        }
        self.reject_secrets(&got.1, &got.2)?;
        if self.migration_text("oo_a232_dst").await? != sealed {
            return Err("privilege changed migrations".to_string());
        }
        self.expect_runtime_not_owner("after_nonadmin").await?;
        self.checks.push("nonadmin=5".to_string());
        Ok(())
    }

    async fn second_empty(&mut self, sealed: &str) -> Result<(), String> {
        let limited = self
            .init_as("oo_a232_second", "oo_a232_limited", &self.limited_file, &self.ca_file)
            .await?;
        self.save("second-privilege", &limited);
        if limited.0 != 5 || limited.2 != "init-pg: target administrator\n" {
            return Err(format!("second privilege code={} err={}", limited.0, limited.2.trim()));
        }
        raw(&mut self.postgres, "GRANT CONNECT ON DATABASE oo_a232_second TO oo_a232_limited").await?;
        let limited = self
            .init_as("oo_a232_second", "oo_a232_limited", &self.limited_file, &self.ca_file)
            .await?;
        self.save("second-privilege-connect", &limited);
        if limited.0 != 5 || limited.2 != "init-pg: target administrator\n" {
            return Err(format!("second connect privilege code={} err={}", limited.0, limited.2.trim()));
        }
        let got = self.init_admin("oo_a232_second", &self.ca_file).await?;
        self.save("second-empty", &got);
        if got.0 != 6 || !got.1.is_empty() || got.2 != "init-pg: target catalog refused\n" {
            return Err(format!("second code={} err={}", got.0, got.2.trim()));
        }
        self.reject_secrets(&got.1, &got.2)?;
        let schema = self
            .scalar(
                "oo_a232_second",
                "SELECT count(*)::text FROM pg_catalog.pg_namespace WHERE nspname = '_oo'",
            )
            .await?;
        if schema != "0" {
            return Err(format!("second schema count {schema}"));
        }
        if self.migration_text("oo_a232_dst").await? != sealed {
            return Err("second empty changed the first database".to_string());
        }
        self.checks.push("second_empty=6".to_string());
        Ok(())
    }

    async fn partial_schema(&mut self) -> Result<(), String> {
        let mut partial = self.admin_on("oo_a232_partial").await;
        raw(
            &mut partial,
            "CREATE SCHEMA _oo AUTHORIZATION oo_target_owner; \
             REVOKE ALL ON SCHEMA _oo FROM PUBLIC; \
             CREATE TABLE public.kept (id int PRIMARY KEY, note text NOT NULL); \
             INSERT INTO public.kept (id, note) VALUES (1, 'partial-a232');",
        )
        .await?;
        drop(partial);
        let got = self.init_admin("oo_a232_partial", &self.ca_file).await?;
        self.save("partial", &got);
        if got.0 != 6 || got.2 != "init-pg: target catalog refused\n" {
            return Err(format!("partial code={} err={}", got.0, got.2.trim()));
        }
        let note = self.scalar("oo_a232_partial", "SELECT note FROM public.kept WHERE id = 1").await?;
        let migrations = self
            .scalar(
                "oo_a232_partial",
                "SELECT (pg_catalog.to_regclass('_oo.schema_migrations') IS NULL)::text",
            )
            .await?;
        if note != "partial-a232" || migrations != "true" {
            return Err(format!("partial preserved note={note} migrations_absent={migrations}"));
        }
        self.checks.push("partial_schema=preserved_exit_6".to_string());
        Ok(())
    }

    async fn bad_checksum(&mut self) -> Result<(), String> {
        let mut db = self.admin_on("oo_a232_checksum").await;
        let sql = format!(
            "CREATE SCHEMA _oo; \
             CREATE TABLE public.kept (id int PRIMARY KEY, note text NOT NULL); \
             INSERT INTO public.kept (id, note) VALUES (1, 'checksum-a232'); \
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
        raw(&mut db, &sql).await?;
        drop(db);
        let got = self.init_admin("oo_a232_checksum", &self.ca_file).await?;
        self.save("checksum", &got);
        if got.0 != 7 || got.2 != "init-pg: migration checksum\n" {
            return Err(format!("checksum code={} err={}", got.0, got.2.trim()));
        }
        if got.1.contains(BAD_CHECKSUM) || got.2.contains(BAD_CHECKSUM) || got.2.contains("OO001") {
            return Err("checksum output echoed the stored digest".to_string());
        }
        let note = self.scalar("oo_a232_checksum", "SELECT note FROM public.kept WHERE id = 1").await?;
        let stored = self
            .scalar(
                "oo_a232_checksum",
                "SELECT checksum FROM _oo.schema_migrations WHERE version = 'target-bootstrap/1'",
            )
            .await?;
        if note != "checksum-a232" || stored != BAD_CHECKSUM {
            return Err("checksum row was not preserved".to_string());
        }
        self.checks.push("bad_checksum=7_digest_hidden".to_string());
        Ok(())
    }

    async fn expect_import(&mut self, got: &(i32, String, String)) -> Result<(), String> {
        if got.0 != 0 {
            return Err(format!("import code={} err={}", got.0, one_line(&got.2)));
        }
        self.reject_secrets(&got.1, &got.2)?;
        let doc: Value = serde_json::from_str(got.1.trim()).map_err(|err| err.to_string())?;
        if doc["protocol"] != "oo.pipeline.import-pg-result/v1" || doc["state"] != "awaiting_publish" {
            return Err("import document state".to_string());
        }
        if doc["source_generation"] != 23 || doc["table_count"] != 2 {
            return Err("import counts".to_string());
        }
        let tables = doc["tables"].as_array().ok_or("tables")?;
        self.compare_table(&tables[0], "boats", "label", &[("1", "cedar-stem"), ("2", "ash-frame")])
            .await?;
        self.compare_table(&tables[1], "parts", "name", &[("1", "stem"), ("2", "frame"), ("3", "keel")])
            .await?;
        let publications = self.scalar("oo_a232_dst", "SELECT count(*)::text FROM pg_catalog.pg_publication").await?;
        if publications != "0" {
            return Err(format!("publications={publications}"));
        }
        let state = self.scalar("oo_a232_dst", "SELECT state FROM _oo.full_import_run").await?;
        if state != "awaiting_publish" {
            return Err("stored import state".to_string());
        }
        let reader_writes = self
            .scalar(
                "oo_a232_src",
                "SELECT count(*)::text FROM yard.write_audit WHERE who = 'oo_a232_reader'",
            )
            .await?;
        if reader_writes != "0" {
            return Err(format!("reader writes={reader_writes}"));
        }
        let mut writer = dial(
            &self.host,
            self.port,
            "oo_a232_writer",
            &self.writer_password,
            "oo_a232_src",
            &self.ca,
        )
        .await;
        raw(&mut writer, "INSERT INTO yard.boats (id, label) VALUES (3, 'after')").await?;
        let reader_writes = self
            .scalar(
                "oo_a232_src",
                "SELECT count(*)::text FROM yard.write_audit WHERE who = 'oo_a232_reader'",
            )
            .await?;
        let writer_writes = self
            .scalar(
                "oo_a232_src",
                "SELECT count(*)::text FROM yard.write_audit WHERE who = 'oo_a232_writer'",
            )
            .await?;
        if reader_writes != "0" || writer_writes != "1" {
            return Err(format!("audit reader={reader_writes} writer={writer_writes}"));
        }
        self.checks.push("import=awaiting_publish".to_string());
        self.checks.push("publications=0".to_string());
        Ok(())
    }

    async fn compare_table(
        &mut self,
        table: &Value,
        name: &str,
        label_column: &str,
        expect: &[(&str, &str)],
    ) -> Result<(), String> {
        if table["schema"] != "yard" || table["name"] != name || table["rows"] != expect.len() {
            return Err(format!("table header {name}"));
        }
        let physical = table["physical_name"].as_str().ok_or("physical")?.to_string();
        let digest = table["canonical_digest"].as_str().ok_or("digest")?.to_string();
        if !safe_physical(&physical, "pt_") || digest.len() != 64 {
            return Err(format!("identity {name}"));
        }
        let columns = table["columns"].as_array().ok_or("columns")?;
        let mut id_physical = String::new();
        let mut label_physical = String::new();
        for column in columns {
            let got = column["physical_name"].as_str().ok_or("column physical")?;
            if !safe_physical(got, "pc_") {
                return Err("column physical".to_string());
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
        let got_rows = self.lines("oo_a232_dst", &sql).await?;
        let expect_rows: Vec<String> = expect.iter().map(|(id, label)| format!("{id}={label}")).collect();
        if got_rows != expect_rows {
            return Err(format!("values {name}"));
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
            return Err(format!("init code={} err={}", got.0, one_line(&got.2)));
        }
        let doc: Value = serde_json::from_str(got.1.trim()).map_err(|err| err.to_string())?;
        let object = doc.as_object().ok_or("object")?;
        if object.len() != 6 || doc["protocol"] != "oo.pipeline.init-pg-result/v1" {
            return Err("init document".to_string());
        }
        if doc["install"] != install_outcome || doc["full_import"] != full_outcome || doc["historical_v2"] != historical_v2
        {
            return Err(format!("outcomes {install_outcome}/{full_outcome}"));
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
            return Err("seals differ from compiled digests".to_string());
        }
        if !got.1.ends_with('\n') || got.1.contains("awaiting_publish") {
            return Err("stdout shape".to_string());
        }
        Ok(())
    }

    async fn expect_runs(&mut self, database: &str, expect: &str) -> Result<(), String> {
        let runs = self.scalar(database, "SELECT count(*)::text FROM _oo.full_import_run").await?;
        if runs != expect {
            return Err(format!("import rows {runs} expected {expect}"));
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
                "source_id": "a232src",
                "host": self.host,
                "port": self.port,
                "database": "oo_a232_src",
                "username": "oo_a232_reader",
                "secret_ref": "secret://pg/a232/reader",
                "password_file": self.reader_file.display().to_string(),
                "ca_file": self.ca_file.display().to_string(),
                "tables": [
                    {"schema": "yard", "table": "boats"},
                    {"schema": "yard", "table": "parts"}
                ]
            },
            "target": {
                "host": self.host,
                "port": self.port,
                "database": "oo_a232_dst",
                "runtime_principal": "oo_a232_runtime",
                "password_file": self.runtime_file.display().to_string(),
                "ca_file": self.ca_file.display().to_string()
            },
            "shard": "a232main",
            "owner": OWNER,
            "source_generation": 23
        });
        spawn(env!("CARGO_BIN_EXE_oo-pipeline"), &["import-pg"], body.to_string().as_bytes())
    }

    fn save(&self, name: &str, got: &(i32, String, String)) {
        fs::write(self.evidence.join(format!("{name}.stdout")), &got.1).unwrap();
        fs::write(self.evidence.join(format!("{name}.stderr")), &got.2).unwrap();
        fs::write(self.evidence.join(format!("{name}.exit")), format!("{}\n", got.0)).unwrap();
    }

    fn reject_secrets(&self, out: &str, err: &str) -> Result<(), String> {
        for secret in [
            self.admin_password.as_str(),
            self.reader_password.as_str(),
            self.runtime_password.as_str(),
            self.writer_password.as_str(),
            self.limited_password.as_str(),
            MARKER,
            DECOY,
            "PRIVATE KEY",
            "BEGIN CERTIFICATE",
            BAD_CHECKSUM,
        ] {
            if out.contains(secret) || err.contains(secret) {
                return Err("secret appeared in process output".to_string());
            }
        }
        Ok(())
    }

    async fn migration_text(&mut self, database: &str) -> Result<String, String> {
        let rows = self
            .lines(
                database,
                "SELECT version || '=' || checksum || '=' || phase || '=' || filename || '=' || applied_at::text \
                 FROM _oo.schema_migrations ORDER BY version",
            )
            .await?;
        Ok(rows.join("\n"))
    }

    async fn scalar(&mut self, database: &str, sql: &str) -> Result<String, String> {
        let mut conn = self.admin_on(database).await;
        text(&mut conn, sql).await
    }

    async fn lines(&mut self, database: &str, sql: &str) -> Result<Vec<String>, String> {
        let mut conn = self.admin_on(database).await;
        lines(&mut conn, sql).await
    }

    async fn admin_on(&self, database: &str) -> sqlx::postgres::PgConnection {
        dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            database,
            &self.ca,
        )
        .await
    }

    async fn reset(&mut self) -> Result<(), String> {
        for name in [
            "oo_a232_src",
            "oo_a232_dst",
            "oo_a232_second",
            "oo_a232_partial",
            "oo_a232_checksum",
        ] {
            raw(&mut self.postgres, &format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await?;
        }
        for role_name in ["oo_a232_reader", "oo_a232_writer", "oo_a232_runtime", "oo_a232_limited"] {
            raw(&mut self.postgres, &format!("DROP ROLE IF EXISTS {role_name}")).await?;
        }
        raw(&mut self.postgres, "DROP ROLE IF EXISTS oo_pipeline_runtime").await?;
        raw(&mut self.postgres, "DROP ROLE IF EXISTS oo_target_owner").await?;
        Ok(())
    }

    async fn cleanup(&mut self) -> Result<(), String> {
        self.reset().await
    }
}

const SOURCE_SQL: &str = "\
CREATE SCHEMA yard;\n\
CREATE TABLE yard.boats (id int4 PRIMARY KEY, label text NOT NULL);\n\
CREATE TABLE yard.parts (id int4 PRIMARY KEY, name text NOT NULL);\n\
CREATE TABLE yard.write_audit (who text NOT NULL);\n\
CREATE FUNCTION yard.note_write() RETURNS trigger LANGUAGE plpgsql AS $audit$\n\
BEGIN\n\
  INSERT INTO yard.write_audit (who) VALUES (current_user);\n\
  RETURN NEW;\n\
END;\n\
$audit$;\n\
CREATE TRIGGER boats_audit BEFORE INSERT OR UPDATE OR DELETE ON yard.boats\n\
  FOR EACH ROW EXECUTE FUNCTION yard.note_write();\n\
CREATE TRIGGER parts_audit BEFORE INSERT OR UPDATE OR DELETE ON yard.parts\n\
  FOR EACH ROW EXECUTE FUNCTION yard.note_write();\n\
INSERT INTO yard.boats (id, label) VALUES (1, 'cedar-stem'), (2, 'ash-frame');\n\
INSERT INTO yard.parts (id, name) VALUES (1, 'stem'), (2, 'frame'), (3, 'keel');\n\
GRANT USAGE ON SCHEMA yard TO oo_a232_reader, oo_a232_writer;\n\
GRANT SELECT ON yard.boats, yard.parts TO oo_a232_reader;\n\
GRANT INSERT ON yard.boats, yard.write_audit TO oo_a232_writer;\n\
";

fn spawn(bin: &str, args: &[&str], stdin: &[u8]) -> (i32, String, String) {
    let mut child = Command::new(bin)
        .args(args)
        .env_clear()
        .env("HOME", "/tmp")
        .env("PGPASSWORD", DECOY)
        .env("PGSSLROOTCERT", "/tmp/not-a-ca")
        .env("DATABASE_URL", format!("postgres://user:{MARKER}@db.example/app"))
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
    let value = text(conn, &sql).await.expect("hex secret");
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

async fn raw(conn: &mut sqlx::postgres::PgConnection, sql: &str) -> Result<(), String> {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(|_| "sql failed".to_string())
}

async fn text(conn: &mut sqlx::postgres::PgConnection, sql: &str) -> Result<String, String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| "scalar failed".to_string())
}

async fn lines(conn: &mut sqlx::postgres::PgConnection, sql: &str) -> Result<Vec<String>, String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_all(&mut *conn)
        .await
        .map_err(|_| "lines failed".to_string())
}

async fn create_login(conn: &mut sqlx::postgres::PgConnection, name: &str, secret: &str) -> Result<(), String> {
    assert!(name.starts_with("oo_a232_"));
    assert!(secret.chars().all(|ch| ch.is_ascii_hexdigit()) && !secret.is_empty());
    let sql = format!(
        "CREATE ROLE {name} LOGIN PASSWORD '{secret}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION"
    );
    raw(conn, &sql).await
}

async fn create_db(conn: &mut sqlx::postgres::PgConnection, name: &str) -> Result<(), String> {
    assert!(name.starts_with("oo_a232_"));
    raw(
        conn,
        &format!(
            "CREATE DATABASE {name} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'"
        ),
    )
    .await
}

fn safe_physical(name: &str, prefix: &str) -> bool {
    let bytes = name.as_bytes();
    let prefix = prefix.as_bytes();
    bytes.len() == 63
        && bytes.starts_with(prefix)
        && bytes[prefix.len()..].iter().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn one_line(text: &str) -> String {
    text.trim().replace('\n', " ")
}
