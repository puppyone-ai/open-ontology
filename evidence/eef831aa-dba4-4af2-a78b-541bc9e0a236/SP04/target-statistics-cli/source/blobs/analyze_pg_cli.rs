//! Process checks for `analyze-pg`, plus one private PostgreSQL run.
//!
//! The ignored test uses compiled `init-pg`, `import-pg`, and `analyze-pg`.
//! Error cases that reject the document do not open a database.

#![forbid(unsafe_code)]

use std::env;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Map, Value, json};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{Connection, Row};

const SECRET: &str = "s3cret-a236-marker";
const DECOY: &str = "decoy-a236-env";
const SOURCE: &str = "a236src";
const GENERATION: i64 = 11;
const OWNER: &str = "a236a236-a236-a236-a236-a236a236a236";
const DIGEST: &str = "abababababababababababababababababababababababababababababababab";

#[test]
fn usage_exits_2_without_echoing_arguments() {
    assert_fail(&[], b"", 3, "invalid input");
    assert_fail(&[SECRET], SECRET.as_bytes(), 2, "usage");
    assert_fail(&["--help"], b"", 2, "usage");
    assert_fail(&["analyze-pg"], b"", 2, "usage");
}

#[test]
fn invalid_json_version_and_secret_field() {
    assert_fail(&[], b"{", 3, "invalid input");
    assert_fail(&[], b"\xff", 3, "invalid input");
    assert_fail(
        &[],
        br#"{"protocol":"oo.pipeline.analyze-pg/v1","protocol":"oo.pipeline.analyze-pg/v1"}"#,
        3,
        "invalid input",
    );
    let mut document = body();
    document["protocol"] = json!("oo.pipeline.analyze-pg/v9");
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid version");
    document = body();
    document["password"] = json!(SECRET);
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid input");
    document = body();
    document["target"]["password"] = json!(SECRET);
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid input");
    document = body();
    document["connect_ms"] = Value::Null;
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid input");
    document = body();
    document["source_generation"] = json!(1.5);
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid input");
    let mut big = vec![b' '; 1_048_577];
    big.extend_from_slice(SECRET.as_bytes());
    assert_fail(&[], &big, 3, "invalid input");
}

#[test]
fn closed_fields_fail_before_files() {
    let missing = format!("/tmp/oo-a236-absent-{SECRET}");
    let mut document = body();
    document["target"]["password_file"] = json!(&missing);
    document["target"]["ca_file"] = json!(&missing);
    document["project"] = json!("other");
    assert_fail(&[], document.to_string().as_bytes(), 3, "identity");

    document = body();
    document["target"]["password_file"] = json!(&missing);
    document["config_digest"] = json!("AB".repeat(32));
    assert_fail(&[], document.to_string().as_bytes(), 3, "identity");
    document["source_generation"] = json!(0);
    document["config_digest"] = json!(DIGEST);
    assert_fail(&[], document.to_string().as_bytes(), 3, "identity");

    document = body();
    document["target"]["password_file"] = json!(&missing);
    document["tables"] = json!([]);
    assert_fail(&[], document.to_string().as_bytes(), 3, "bound");
    document["tables"] = json!([
        {"namespace": "app", "name": "party"},
        {"namespace": "app", "name": "party"}
    ]);
    assert_fail(&[], document.to_string().as_bytes(), 3, "bound");
    let mut many = Vec::new();
    for index in 0..9 {
        many.push(json!({"namespace": "app", "name": format!("party{index}")}));
    }
    document["tables"] = Value::Array(many);
    assert_fail(&[], document.to_string().as_bytes(), 3, "bound");
    document = body();
    document["target"]["password_file"] = json!(&missing);
    document["statement_timeout_ms"] = json!(0);
    assert_fail(&[], document.to_string().as_bytes(), 3, "bound");
    document["statement_timeout_ms"] = json!(30_000);
    document["lock_timeout_ms"] = json!(30_001);
    assert_fail(&[], document.to_string().as_bytes(), 3, "bound");
    document = body();
    document["target"]["password_file"] = json!(&missing);
    document["tables"][0]["name"] = json!("party\"; ANALYZE pg_catalog.pg_authid");
    assert_fail(&[], document.to_string().as_bytes(), 3, "bound");

    document = body();
    document["connect_ms"] = json!(0);
    document["target"]["password_file"] = json!(&missing);
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid input");
    document = body();
    document["target"]["host"] = json!(format!("{SECRET}:5432"));
    document["target"]["password_file"] = json!(&missing);
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid input");
    document = body();
    document["target"]["operator"] = json!("has-hyphen");
    document["target"]["password_file"] = json!(&missing);
    assert_fail(&[], document.to_string().as_bytes(), 3, "invalid input");
}

#[test]
fn password_and_ca_files_fail_without_echoing_secrets() {
    let dir = scratch("files");
    let missing = dir.join(format!("zz-absent-{SECRET}"));
    let mut document = body();
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
#[ignore = "one private PostgreSQL 17.10; OO_ANALYZE_PG_HOST must be set by the deciding run"]
async fn analyze_pg_binary_on_private_postgres() {
    let mut harness = Harness::connect().await;
    harness.setup().await;
    harness.install_and_import().await;
    harness.unknown_and_refusals().await;
    harness.analyze().await;
    harness.source_audit().await;
    harness.write_facts();
}

struct Harness {
    host: String,
    port: u16,
    admin_user: String,
    admin_password: String,
    admin_file: PathBuf,
    ca_file: PathBuf,
    wrong_ca: PathBuf,
    evidence: PathBuf,
    reader_password: String,
    reader_file: PathBuf,
    runtime_password: String,
    runtime_file: PathBuf,
    plain_password: String,
    plain_file: PathBuf,
    postgres: sqlx::postgres::PgConnection,
    source: sqlx::postgres::PgConnection,
    target: sqlx::postgres::PgConnection,
    digest: String,
    physical: String,
    audit_before: i64,
    facts: String,
}

impl Harness {
    async fn connect() -> Self {
        let host = env::var("OO_ANALYZE_PG_HOST").expect("OO_ANALYZE_PG_HOST");
        let port: u16 = env::var("OO_ANALYZE_PG_PORT")
            .expect("OO_ANALYZE_PG_PORT")
            .parse()
            .expect("port");
        assert!(!host.contains(':') && !host.contains('/') && !host.contains('@'));
        let admin_user = env::var("OO_ANALYZE_PG_ADMIN_USER").expect("OO_ANALYZE_PG_ADMIN_USER");
        let admin_file = PathBuf::from(
            env::var("OO_ANALYZE_PG_ADMIN_PASSWORD_FILE").expect("OO_ANALYZE_PG_ADMIN_PASSWORD_FILE"),
        );
        let admin_password = read_secret(&admin_file);
        let ca_file = PathBuf::from(env::var("OO_ANALYZE_PG_CA_FILE").expect("OO_ANALYZE_PG_CA_FILE"));
        let ca = fs::read(&ca_file).expect("ca");
        let wrong_ca = PathBuf::from(env::var("OO_ANALYZE_PG_WRONG_CA_FILE").expect("OO_ANALYZE_PG_WRONG_CA_FILE"));
        let secrets = PathBuf::from(env::var("OO_ANALYZE_PG_SECRET_DIR").expect("OO_ANALYZE_PG_SECRET_DIR"));
        let evidence = PathBuf::from(env::var("OO_ANALYZE_PG_EVIDENCE").expect("OO_ANALYZE_PG_EVIDENCE"));
        fs::create_dir_all(&secrets).expect("secret dir");
        fs::create_dir_all(&evidence).expect("evidence dir");
        let mut postgres = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
        let version = text(&mut postgres, "SELECT version()").await;
        assert!(version.contains("17.10"), "{version}");
        assert_eq!(ssl_on(&mut postgres).await, "true");
        let source = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
        let target = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
        let reader_password = hex_secret(&mut postgres, "reader").await;
        let runtime_password = hex_secret(&mut postgres, "runtime").await;
        let plain_password = hex_secret(&mut postgres, "plain").await;
        let reader_file = secrets.join("reader.pass");
        let runtime_file = secrets.join("runtime.pass");
        let plain_file = secrets.join("plain.pass");
        write_secret(&reader_file, &reader_password);
        write_secret(&runtime_file, &runtime_password);
        write_secret(&plain_file, &plain_password);
        Self {
            host,
            port,
            admin_user,
            admin_password,
            admin_file,
            ca_file,
            wrong_ca,
            evidence,
            reader_password,
            reader_file,
            runtime_password,
            runtime_file,
            plain_password,
            plain_file,
            postgres,
            source,
            target,
            digest: String::new(),
            physical: String::new(),
            audit_before: 0,
            facts: String::new(),
        }
    }

    async fn setup(&mut self) {
        reset_cluster(&mut self.postgres).await;
        raw(&mut self.postgres, "CREATE DATABASE oo_a236_src TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
        raw(&mut self.postgres, "CREATE DATABASE oo_a236_dst TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
        let ca = fs::read(&self.ca_file).expect("ca");
        self.source = dial(&self.host, self.port, &self.admin_user, &self.admin_password, "oo_a236_src", &ca).await;
        self.target = dial(&self.host, self.port, &self.admin_user, &self.admin_password, "oo_a236_dst", &ca).await;
        assert_eq!(ssl_on(&mut self.source).await, "true");
        assert_eq!(ssl_on(&mut self.target).await, "true");
        raw(&mut self.source, SOURCE_SQL).await;
        exec_role(&mut self.postgres, "oo_a236_reader", &self.reader_password).await;
        raw(
            &mut self.source,
            "GRANT CONNECT ON DATABASE oo_a236_src TO oo_a236_reader; GRANT USAGE ON SCHEMA app TO oo_a236_reader; GRANT SELECT ON app.party TO oo_a236_reader",
        )
        .await;
        self.audit_before = scalar_i64(&mut self.source, "SELECT count(*)::int8 FROM app.write_audit").await;
        assert!(self.audit_before >= 2);
        let audit_before = self.audit_before;
        self.facts.push_str(&format!("audit_before={audit_before}\n"));
    }

    async fn install_and_import(&mut self) {
        let init = spawn(
            env!("CARGO_BIN_EXE_init-pg"),
            &[],
            self.init_document().as_bytes(),
        );
        assert_eq!(init.0, 0, "init stderr={}", init.2);
        let init_value: Value = serde_json::from_str(init.1.trim_end()).expect("init json");
        assert_eq!(init_value["install"], json!("bootstrapped"));
        assert_eq!(init_value["full_import"], json!("installed"));
        assert_eq!(init.2, "");
        exec_role(&mut self.postgres, "oo_a236_runtime", &self.runtime_password).await;
        exec_role(&mut self.postgres, "oo_a236_plain", &self.plain_password).await;
        raw(&mut self.postgres, "GRANT oo_pipeline_runtime TO oo_a236_runtime").await;
        raw(&mut self.postgres, "GRANT CONNECT ON DATABASE oo_a236_dst TO oo_a236_runtime, oo_a236_plain").await;
        raw(&mut self.target, "GRANT USAGE ON SCHEMA _oo TO oo_a236_plain").await;
        raw(&mut self.target, "CREATE TABLE public.sentinel (id int4)").await;
        let imported = spawn(
            env!("CARGO_BIN_EXE_oo-pipeline"),
            &["import-pg"],
            self.import_document().as_bytes(),
        );
        assert_eq!(imported.0, 0, "import stderr={}", imported.2);
        let value: Value = serde_json::from_str(imported.1.trim_end()).expect("import json");
        assert_eq!(value["state"], json!("awaiting_publish"));
        assert_eq!(value["source_generation"], json!(GENERATION));
        assert_eq!(value["table_count"], json!(1));
        assert_eq!(value["tables"][0]["rows"], json!(2));
        self.physical = value["tables"][0]["physical_name"]
            .as_str()
            .expect("physical")
            .to_string();
        assert!(self.physical.starts_with("pt_") && self.physical.len() == 63);
        let row = sqlx::query(
            "SELECT config_digest, run_generation \
             FROM _oo.physical_mappings \
             WHERE source = $1 AND level = 't' AND logical_namespace = 'app' AND logical_table = 'party'",
        )
        .bind(SOURCE)
        .fetch_one(&mut self.target)
        .await
        .expect("mapping");
        self.digest = row.try_get(0).expect("digest");
        let generation: i64 = row.try_get(1).expect("generation");
        assert_eq!(generation, GENERATION);
        assert_eq!(self.digest.len(), 64);
        let stored: String = row_physical(&mut self.target).await;
        assert_eq!(stored, self.physical);
        self.facts.push_str("import_state=awaiting_publish\nimport_rows=2\n");
    }

    async fn unknown_and_refusals(&mut self) {
        let unknown = (-1.0f32).to_bits();
        assert_eq!(reltuples_bits(&mut self.target, "_oo", &self.physical).await, unknown);
        assert_eq!(reltuples_bits(&mut self.target, "public", "sentinel").await, unknown);
        self.facts.push_str("reltuples_before=-1\nsentinel_before=-1\n");
        self.expect_analyze(&self.admin_user, &self.admin_file, &self.wrong_ca, GENERATION, "party", 4, "target connect");
        self.expect_analyze("oo_a236_runtime", &self.runtime_file, &self.ca_file, GENERATION, "party", 5, "runtime caller");
        self.expect_analyze("oo_a236_plain", &self.plain_file, &self.ca_file, GENERATION, "party", 5, "target role");
        self.expect_analyze(&self.admin_user, &self.admin_file, &self.ca_file, GENERATION + 1, "party", 6, "stale generation");
        self.expect_analyze(&self.admin_user, &self.admin_file, &self.ca_file, GENERATION, "absent", 6, "mapping absent");
        assert_eq!(reltuples_bits(&mut self.target, "_oo", &self.physical).await, unknown);
        assert_eq!(reltuples_bits(&mut self.target, "public", "sentinel").await, unknown);
        self.facts
            .push_str("refused=wrong_ca,runtime,role,stale_generation,mapping_absent\nreltuples_after_refusal=-1\n");
    }

    async fn analyze(&mut self) {
        let (code, out, err) = self.run_analyze(&self.admin_user, &self.admin_file, &self.ca_file, GENERATION, "party");
        assert_eq!(code, 0, "{err}");
        assert_eq!(err, "");
        assert!(out.ends_with('\n'));
        assert_eq!(out.chars().filter(|ch| *ch == '\n').count(), 1);
        let value: Value = serde_json::from_str(out.trim_end()).expect("receipt");
        assert_eq!(out.trim_end(), &canonical_json(&value));
        assert_eq!(value["protocol"], json!("oo.pipeline.analyze-pg-result/v1"));
        assert_eq!(value["published"], json!(false));
        assert_eq!(value["estimate_is_exact"], json!(false));
        assert_eq!(value["project"], json!(SOURCE));
        assert_eq!(value["source"], json!(SOURCE));
        assert_eq!(value["config_digest"], json!(self.digest));
        assert_eq!(value["source_generation"], json!(GENERATION));
        assert_eq!(value["disposition"], json!("reusable"));
        assert_eq!(value["tables"][0]["namespace"], json!("app"));
        assert_eq!(value["tables"][0]["name"], json!("party"));
        assert_eq!(value["tables"][0]["physical_name"], json!(self.physical));
        let bits = value["tables"][0]["reltuples_bits"].as_u64().expect("bits") as u32;
        let estimated = value["tables"][0]["estimated_rows"].as_u64().expect("rows");
        let measured = f32::from_bits(bits);
        assert!(measured.is_finite() && measured >= 0.0);
        assert_ne!(bits, (-1.0f32).to_bits());
        assert_eq!(estimated, f64::from(measured).round() as u64);
        assert_eq!(reltuples_bits(&mut self.target, "_oo", &self.physical).await, bits);
        assert_eq!(reltuples_bits(&mut self.target, "public", "sentinel").await, (-1.0f32).to_bits());
        assert!(
            self.physical.len() == 63
                && self.physical.starts_with("pt_")
                && self.physical.bytes().skip(3).all(|byte| byte.is_ascii_hexdigit())
        );
        let count = scalar_i64(
            &mut self.target,
            &format!("SELECT count(*)::int8 FROM \"_oo\".\"{}\"", self.physical),
        )
        .await;
        assert_eq!(count, 2);
        let again = self.run_analyze(&self.admin_user, &self.admin_file, &self.ca_file, GENERATION, "party");
        assert_eq!(again.0, 0, "{}", again.2);
        let second: Value = serde_json::from_str(again.1.trim_end()).expect("second receipt");
        assert_eq!(second["published"], json!(false));
        assert_eq!(second["estimate_is_exact"], json!(false));
        assert_eq!(second["tables"][0]["reltuples_bits"], json!(bits));
        self.facts.push_str(&format!(
            "published=false\nestimate_is_exact=false\nreltuples_after_bits={bits}\nestimated_rows={estimated}\ncount_rows={count}\nconnection=reusable\nsentinel_after=-1\nmaintenance=explicit\n"
        ));
    }

    async fn source_audit(&mut self) {
        let audit = scalar_i64(&mut self.source, "SELECT count(*)::int8 FROM app.write_audit").await;
        let reader = scalar_i64(
            &mut self.source,
            "SELECT count(*)::int8 FROM app.write_audit WHERE who = 'oo_a236_reader'",
        )
        .await;
        let source_bits = reltuples_bits(&mut self.source, "app", "party").await;
        assert_eq!(audit, self.audit_before);
        assert_eq!(reader, 0);
        assert_eq!(source_bits, (-1.0f32).to_bits());
        self.facts
            .push_str("source_audit_delta=0\nsource_reader_writes=0\nsource_reltuples=-1\n");
    }

    fn write_facts(&self) {
        fs::write(self.evidence.join("facts.txt"), &self.facts).expect("facts");
    }

    #[allow(clippy::too_many_arguments)]
    fn expect_analyze(
        &self,
        operator: &str,
        password: &Path,
        ca: &Path,
        generation: i64,
        table: &str,
        code: i32,
        fact: &str,
    ) {
        let (got, out, err) = self.run_analyze(operator, password, ca, generation, table);
        assert_eq!(got, code, "{err}");
        assert!(out.is_empty(), "{out}");
        assert_eq!(err, format!("analyze-pg: {fact}\n"));
        assert!(!err.contains(&self.admin_password));
        assert!(!err.contains(&self.reader_password));
        assert!(!err.contains(&self.runtime_password));
        assert!(!err.contains(&self.plain_password));
    }

    fn run_analyze(
        &self,
        operator: &str,
        password: &Path,
        ca: &Path,
        generation: i64,
        table: &str,
    ) -> (i32, String, String) {
        let document = json!({
            "protocol": "oo.pipeline.analyze-pg/v1",
            "project": SOURCE,
            "source": SOURCE,
            "config_digest": self.digest,
            "source_generation": generation,
            "tables": [{"namespace": "app", "name": table}],
            "statement_timeout_ms": 5000,
            "lock_timeout_ms": 5000,
            "target": {
                "host": self.host,
                "port": self.port,
                "database": "oo_a236_dst",
                "operator": operator,
                "password_file": password.display().to_string(),
                "ca_file": ca.display().to_string(),
            }
        });
        spawn(env!("CARGO_BIN_EXE_analyze-pg"), &[], document.to_string().as_bytes())
    }

    fn init_document(&self) -> String {
        json!({
            "protocol": "oo.pipeline.init-pg/v1",
            "target": {
                "host": self.host,
                "port": self.port,
                "database": "oo_a236_dst",
                "administrator": self.admin_user,
                "password_file": self.admin_file.display().to_string(),
                "ca_file": self.ca_file.display().to_string(),
            }
        })
        .to_string()
    }

    fn import_document(&self) -> String {
        json!({
            "protocol": "oo.pipeline.import-pg/v1",
            "source": {
                "source_id": SOURCE,
                "host": self.host,
                "port": self.port,
                "database": "oo_a236_src",
                "username": "oo_a236_reader",
                "secret_ref": "secret://a236/reader",
                "password_file": self.reader_file.display().to_string(),
                "ca_file": self.ca_file.display().to_string(),
                "tables": [{"schema": "app", "table": "party"}]
            },
            "target": {
                "host": self.host,
                "port": self.port,
                "database": "oo_a236_dst",
                "runtime_principal": "oo_a236_runtime",
                "password_file": self.runtime_file.display().to_string(),
                "ca_file": self.ca_file.display().to_string(),
            },
            "shard": "a236main",
            "owner": OWNER,
            "source_generation": GENERATION
        })
        .to_string()
    }
}

const SOURCE_SQL: &str = "\
CREATE SCHEMA app;\n\
CREATE TABLE app.party (\n\
  id int4 PRIMARY KEY,\n\
  label text NOT NULL,\n\
  active bool NOT NULL\n\
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
CREATE TRIGGER party_audit BEFORE INSERT OR UPDATE OR DELETE ON app.party\n\
  FOR EACH ROW EXECUTE FUNCTION app.note_write();\n\
INSERT INTO app.party (id, label, active) VALUES (1, 'north', true), (2, 'south', false);\n\
";

fn body() -> Value {
    json!({
        "protocol": "oo.pipeline.analyze-pg/v1",
        "project": SOURCE,
        "source": SOURCE,
        "config_digest": DIGEST,
        "source_generation": GENERATION,
        "tables": [{"namespace": "app", "name": "party"}],
        "statement_timeout_ms": 5000,
        "lock_timeout_ms": 5000,
        "target": {
            "host": "db.example",
            "port": 5432,
            "database": "dstdb",
            "operator": "admin",
            "password_file": "password",
            "ca_file": "ca.crt"
        }
    })
}

fn assert_fail(args: &[&str], stdin: &[u8], code: i32, fact: &str) {
    let (got, out, err) = spawn(env!("CARGO_BIN_EXE_analyze-pg"), args, stdin);
    assert_eq!(got, code, "{err}");
    assert!(out.is_empty(), "{out}");
    assert_eq!(err, format!("analyze-pg: {fact}\n"));
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
    let dir = env::temp_dir().join(format!("oo-a236-{name}-{}", std::process::id()));
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
    let options = PgConnectOptions::new()
        .host(host)
        .port(port)
        .username(user)
        .password(pass)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    sqlx::postgres::PgConnection::connect_with(&options)
        .await
        .expect("dial")
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

async fn scalar_i64(conn: &mut sqlx::postgres::PgConnection, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("scalar failed: {err}"))
}

async fn ssl_on(conn: &mut sqlx::postgres::PgConnection) -> String {
    text(conn, "SELECT ssl::text FROM pg_stat_ssl WHERE pid = pg_backend_pid()").await
}

async fn reltuples_bits(conn: &mut sqlx::postgres::PgConnection, namespace: &str, name: &str) -> u32 {
    let value: f32 = sqlx::query_scalar(
        "SELECT class.reltuples \
         FROM pg_catalog.pg_class AS class \
         JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace \
         WHERE namespace.nspname = $1 AND class.relname = $2",
    )
    .bind(namespace)
    .bind(name)
    .fetch_one(&mut *conn)
    .await
    .expect("reltuples");
    value.to_bits()
}

async fn row_physical(conn: &mut sqlx::postgres::PgConnection) -> String {
    let value: String = sqlx::query_scalar(
        "SELECT physical_name FROM _oo.physical_mappings \
         WHERE source = $1 AND level = 't' AND logical_namespace = 'app' AND logical_table = 'party'",
    )
    .bind(SOURCE)
    .fetch_one(&mut *conn)
    .await
    .expect("physical");
    value
}

async fn reset_cluster(admin: &mut sqlx::postgres::PgConnection) {
    for name in ["oo_a236_src", "oo_a236_dst"] {
        raw(admin, &format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await;
    }
    for role in ["oo_a236_reader", "oo_a236_runtime", "oo_a236_plain", "oo_a236_login"] {
        raw(admin, &format!("DROP ROLE IF EXISTS {role}")).await;
    }
    raw(admin, "DROP ROLE IF EXISTS oo_pipeline_runtime").await;
    raw(admin, "DROP ROLE IF EXISTS oo_target_owner").await;
}

async fn exec_role(conn: &mut sqlx::postgres::PgConnection, name: &str, secret: &str) {
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

fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => push_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => write_object(out, map),
    }
}

fn write_object(out: &mut String, map: &Map<String, Value>) {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    out.push('{');
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_string(out, key);
        out.push(':');
        write_value(out, &map[*key]);
    }
    out.push('}');
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if u32::from(ch) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(ch));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}
