//! Independent review of compiled init-pg, import-pg, and analyze-pg.
//!
//! This file is copied onto the fixed archive only for the proof container.
//! It is not a product source. It does not write row values or passwords.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use oo_gateway::policy::catalog::{CatalogLimits, UnknownRows, load_policy_catalog};
use oo_gateway::query_executor::{
    AuditAdmission, Cell, CompletionStatus, DedicatedSession, ExecError, SelectRequest, SelectStream,
};
use oo_gateway::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, InstallEffect, LogicalTable, PiiClass, PolicyIdentity,
    TlsEndpoint, compile_column_acl, connect_require_tls, install_column_acl, rustls_connector,
};
use serde_json::{Value, json};
use tokio_postgres::Client;

const SOURCE: &str = "a247src";
const GENERATION: i64 = 11;
const OWNER: &str = "a247a247-a247-a247-a247-a247a247a247";
const LOGIN: &str = "oo_a247_login";
const ROLE: &str = "oo_r_clerk";
const SRC_DB: &str = "oo_a247_src";
const DST_DB: &str = "oo_a247_dst";

struct Probe<'a> {
    operator: &'a str,
    password: &'a Path,
    ca: &'a Path,
    generation: i64,
    table: &'a str,
    code: i32,
    fact: &'a str,
    label: &'a str,
}

#[tokio::test]
async fn compiled_chain_unknown_then_catalog_query() {
    assert_eq!(env::var("OO_A247_LIVE").ok().as_deref(), Some("1"), "live gate");
    let mut proof = Proof::connect().await;
    proof.prepare_source().await;
    proof.init_and_import().await;
    proof.refusals().await;
    proof.analyze().await;
    proof.source_quiet().await;
    proof.catalog_query().await;
    proof.finish();
}

struct Proof {
    host: String,
    port: u16,
    admin_user: String,
    admin_password: String,
    admin_file: PathBuf,
    ca_file: PathBuf,
    ca: Vec<u8>,
    wrong_ca: PathBuf,
    evidence: PathBuf,
    secrets: PathBuf,
    init_bin: PathBuf,
    pipeline_bin: PathBuf,
    analyze_bin: PathBuf,
    reader_password: String,
    reader_file: PathBuf,
    runtime_password: String,
    runtime_file: PathBuf,
    plain_password: String,
    plain_file: PathBuf,
    login_password: String,
    login_file: PathBuf,
    cluster: Client,
    source: Client,
    target: Client,
    digest: String,
    physical: String,
    audit_before: i64,
    bits: u32,
    estimated: u64,
    facts: String,
}

impl Proof {
    async fn connect() -> Self {
        let host = env::var("OO_A247_PG_HOST").expect("host");
        let port: u16 = env::var("OO_A247_PG_PORT").expect("port").parse().expect("port");
        assert!(!host.contains(':') && !host.contains('/') && !host.contains('@'));
        let admin_user = env::var("OO_A247_PG_ADMIN_USER").expect("admin user");
        let admin_file = PathBuf::from(env::var("OO_A247_PG_ADMIN_PASSWORD_FILE").expect("admin file"));
        let admin_password = read_secret(&admin_file);
        let ca_file = PathBuf::from(env::var("OO_A247_PG_CA_FILE").expect("ca file"));
        let ca = fs::read(&ca_file).expect("ca");
        let wrong_ca = PathBuf::from(env::var("OO_A247_PG_WRONG_CA_FILE").expect("wrong ca"));
        let evidence = PathBuf::from(env::var("OO_A247_EVIDENCE").expect("evidence"));
        let secrets = PathBuf::from("/tmp/oo-a247-secrets");
        fs::create_dir_all(&evidence).expect("evidence dir");
        fs::create_dir_all(&secrets).expect("secret dir");
        let init_bin = PathBuf::from(env::var("OO_A247_INIT_PG").expect("init bin"));
        let pipeline_bin = PathBuf::from(env::var("OO_A247_PIPELINE").expect("pipeline bin"));
        let analyze_bin = PathBuf::from(env::var("OO_A247_ANALYZE").expect("analyze bin"));
        assert!(init_bin.is_file(), "init-pg binary");
        assert!(pipeline_bin.is_file(), "oo-pipeline binary");
        assert!(analyze_bin.is_file(), "analyze-pg binary");
        let cluster = tls_client(&host, port, "postgres", &admin_user, &admin_password, &ca).await;
        let version = text(&cluster, "SELECT version()").await;
        assert!(version.contains("17.10"), "postgres version");
        assert_eq!(ssl_text(&cluster).await, "true");
        let reader_password = hex_secret(&cluster, "reader").await;
        let runtime_password = hex_secret(&cluster, "runtime").await;
        let plain_password = hex_secret(&cluster, "plain").await;
        let login_password = hex_secret(&cluster, "login").await;
        let reader_file = secrets.join("reader.pass");
        let runtime_file = secrets.join("runtime.pass");
        let plain_file = secrets.join("plain.pass");
        let login_file = secrets.join("login.pass");
        write_secret(&reader_file, &reader_password);
        write_secret(&runtime_file, &runtime_password);
        write_secret(&plain_file, &plain_password);
        write_secret(&login_file, &login_password);
        let source = tls_client(&host, port, "postgres", &admin_user, &admin_password, &ca).await;
        let target = tls_client(&host, port, "postgres", &admin_user, &admin_password, &ca).await;
        Self {
            host,
            port,
            admin_user,
            admin_password,
            admin_file,
            ca_file,
            ca,
            wrong_ca,
            evidence,
            secrets,
            init_bin,
            pipeline_bin,
            analyze_bin,
            reader_password,
            reader_file,
            runtime_password,
            runtime_file,
            plain_password,
            plain_file,
            login_password,
            login_file,
            cluster,
            source,
            target,
            digest: String::new(),
            physical: String::new(),
            audit_before: 0,
            bits: 0,
            estimated: 0,
            facts: String::new(),
        }
    }

    async fn prepare_source(&mut self) {
        exec_quiet(&self.cluster, &format!("DROP DATABASE IF EXISTS {SRC_DB} WITH (FORCE)"), "drop src").await;
        exec_quiet(&self.cluster, &format!("DROP DATABASE IF EXISTS {DST_DB} WITH (FORCE)"), "drop dst").await;
        for role in [
            "oo_a247_reader",
            "oo_a247_runtime",
            "oo_a247_plain",
            "oo_a247_login",
            "oo_pipeline_runtime",
            "oo_target_owner",
            ROLE,
        ] {
            exec_quiet(&self.cluster, &format!("DROP ROLE IF EXISTS {role}"), "drop role").await;
        }
        exec_quiet(
            &self.cluster,
            &format!("CREATE DATABASE {SRC_DB} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'"),
            "create src",
        )
        .await;
        exec_quiet(
            &self.cluster,
            &format!("CREATE DATABASE {DST_DB} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'"),
            "create dst",
        )
        .await;
        self.source = tls_client(&self.host, self.port, SRC_DB, &self.admin_user, &self.admin_password, &self.ca).await;
        self.target = tls_client(&self.host, self.port, DST_DB, &self.admin_user, &self.admin_password, &self.ca).await;
        assert_eq!(ssl_text(&self.source).await, "true");
        assert_eq!(ssl_text(&self.target).await, "true");
        exec_quiet(&self.source, SOURCE_SQL, "source sql").await;
        create_login(&self.cluster, "oo_a247_reader", &self.reader_password).await;
        exec_quiet(
            &self.cluster,
            "GRANT CONNECT ON DATABASE oo_a247_src TO oo_a247_reader",
            "reader connect",
        )
        .await;
        exec_quiet(
            &self.source,
            "GRANT USAGE ON SCHEMA app TO oo_a247_reader; GRANT SELECT ON app.party TO oo_a247_reader",
            "reader select",
        )
        .await;
        self.audit_before = i64_one(&self.source, "SELECT count(*)::int8 FROM app.write_audit").await;
        assert!(self.audit_before >= 2, "setup inserts wrote the audit");
        let audit_before = self.audit_before;
        self.note(&format!("audit_before={audit_before}\n"));
    }

    async fn init_and_import(&mut self) {
        let init = spawn(&self.init_bin, &[], self.init_document().as_bytes());
        assert_eq!(init.0, 0, "init exit");
        assert!(init.2.is_empty(), "init stderr");
        let init_value: Value = serde_json::from_str(init.1.trim_end()).expect("init json");
        assert_eq!(init_value["install"], json!("bootstrapped"));
        assert_eq!(init_value["full_import"], json!("installed"));
        create_login(&self.cluster, "oo_a247_runtime", &self.runtime_password).await;
        create_login(&self.cluster, "oo_a247_plain", &self.plain_password).await;
        exec_quiet(&self.cluster, "GRANT oo_pipeline_runtime TO oo_a247_runtime", "runtime member").await;
        exec_quiet(
            &self.cluster,
            "GRANT CONNECT ON DATABASE oo_a247_dst TO oo_a247_runtime, oo_a247_plain",
            "dst connect",
        )
        .await;
        exec_quiet(&self.target, "GRANT USAGE ON SCHEMA _oo TO oo_a247_plain", "plain usage").await;
        exec_quiet(&self.target, "CREATE TABLE public.sentinel (id int4)", "sentinel").await;
        let imported = spawn(&self.pipeline_bin, &["import-pg"], self.import_document().as_bytes());
        assert_eq!(imported.0, 0, "import exit");
        assert!(imported.2.is_empty(), "import stderr");
        let value: Value = serde_json::from_str(imported.1.trim_end()).expect("import json");
        assert_eq!(value["state"], json!("awaiting_publish"));
        assert_eq!(value["source_generation"], json!(GENERATION));
        assert_eq!(value["table_count"], json!(1));
        assert_eq!(value["tables"][0]["rows"], json!(2));
        self.physical = value["tables"][0]["physical_name"].as_str().expect("physical").to_string();
        assert!(closed_physical(&self.physical), "physical name");
        let row = self
            .target
            .query_one(
                "SELECT config_digest, run_generation FROM _oo.physical_mappings \
                 WHERE source = $1 AND level = 't' AND logical_namespace = 'app' AND logical_table = 'party'",
                &[&SOURCE],
            )
            .await
            .expect("mapping");
        self.digest = row.get(0);
        let generation: i64 = row.get(1);
        assert_eq!(generation, GENERATION);
        assert_eq!(self.digest.len(), 64);
        self.note("import_state=awaiting_publish\nimport_rows=2\n");
    }

    async fn refusals(&mut self) {
        let unknown = (-1.0f32).to_bits();
        assert_eq!(reltuples(&self.target, "_oo", &self.physical).await, unknown);
        assert_eq!(reltuples(&self.target, "public", "sentinel").await, unknown);
        self.note("reltuples_before=-1\nsentinel_before=-1\n");
        let admin_user = self.admin_user.clone();
        let admin_file = self.admin_file.clone();
        let ca_file = self.ca_file.clone();
        let wrong_ca = self.wrong_ca.clone();
        let runtime_file = self.runtime_file.clone();
        let plain_file = self.plain_file.clone();
        let mut refused = String::new();
        refused.push_str(&self.probe(Probe { operator: &admin_user, password: &admin_file, ca: &wrong_ca, generation: GENERATION, table: "party", code: 4, fact: "target connect", label: "wrong_ca" }));
        refused.push_str(&self.probe(Probe { operator: "oo_a247_runtime", password: &runtime_file, ca: &ca_file, generation: GENERATION, table: "party", code: 5, fact: "runtime caller", label: "runtime" }));
        refused.push_str(&self.probe(Probe { operator: "oo_a247_plain", password: &plain_file, ca: &ca_file, generation: GENERATION, table: "party", code: 5, fact: "target role", label: "plain" }));
        refused.push_str(&self.probe(Probe { operator: &admin_user, password: &admin_file, ca: &ca_file, generation: GENERATION + 1, table: "party", code: 6, fact: "stale generation", label: "stale" }));
        refused.push_str(&self.probe(Probe { operator: &admin_user, password: &admin_file, ca: &ca_file, generation: GENERATION, table: "absent", code: 6, fact: "mapping absent", label: "absent" }));
        self.note(&refused);
        assert_eq!(reltuples(&self.target, "_oo", &self.physical).await, unknown);
        assert_eq!(reltuples(&self.target, "public", "sentinel").await, unknown);
        self.note("reltuples_after_refusal=-1\n");
    }

    async fn analyze(&mut self) {
        let (code, out, err) = self.run_analyze(&self.admin_user, &self.admin_file, &self.ca_file, GENERATION, "party");
        assert_eq!(code, 0, "analyze exit");
        assert!(err.is_empty(), "analyze stderr");
        assert!(out.ends_with('\n') && out.chars().filter(|ch| *ch == '\n').count() == 1, "one receipt line");
        fs::write(self.evidence.join("analyze.stdout"), &out).expect("receipt file");
        let value: Value = serde_json::from_str(out.trim_end()).expect("receipt");
        assert_eq!(out.trim_end(), serde_json::to_string(&value).expect("canonical"));
        assert_eq!(value["protocol"], json!("oo.pipeline.analyze-pg-result/v1"));
        assert_eq!(value["published"], json!(false));
        assert_eq!(value["estimate_is_exact"], json!(false));
        assert_eq!(value["project"], json!(SOURCE));
        assert_eq!(value["source"], json!(SOURCE));
        assert_eq!(value["config_digest"], json!(self.digest));
        assert_eq!(value["source_generation"], json!(GENERATION));
        let disposition = value["disposition"].as_str().expect("disposition");
        assert!(disposition == "reusable" || disposition == "fresh_connection", "disposition");
        assert_eq!(value["tables"][0]["namespace"], json!("app"));
        assert_eq!(value["tables"][0]["name"], json!("party"));
        assert_eq!(value["tables"][0]["physical_name"], json!(self.physical));
        self.bits = u32::try_from(value["tables"][0]["reltuples_bits"].as_u64().expect("bits")).expect("bits fit");
        self.estimated = value["tables"][0]["estimated_rows"].as_u64().expect("rows");
        let measured = f32::from_bits(self.bits);
        assert!(measured.is_finite() && measured >= 0.0, "finite estimate");
        assert_ne!(self.bits, (-1.0f32).to_bits());
        assert_eq!(self.estimated, f64::from(measured).round() as u64);
        assert_eq!(reltuples(&self.target, "_oo", &self.physical).await, self.bits);
        assert_eq!(reltuples(&self.target, "public", "sentinel").await, (-1.0f32).to_bits());
        let count = i64_one(&self.target, &format!("SELECT count(*)::int8 FROM \"_oo\".\"{}\"", self.physical)).await;
        assert_eq!(count, 2);
        let again = self.run_analyze(&self.admin_user, &self.admin_file, &self.ca_file, GENERATION, "party");
        assert_eq!(again.0, 0, "second analyze exit");
        assert!(again.2.is_empty(), "second analyze stderr");
        let second: Value = serde_json::from_str(again.1.trim_end()).expect("second receipt");
        assert_eq!(second["published"], json!(false));
        assert_eq!(second["estimate_is_exact"], json!(false));
        assert_eq!(second["tables"][0]["reltuples_bits"], json!(self.bits));
        let idle = i64_one(
            &self.target,
            "SELECT count(*)::int8 FROM pg_stat_activity WHERE state = 'idle in transaction' AND usename IN ('oo_a247_admin', 'oo_a247_runtime', 'oo_a247_plain', 'oo_a247_reader')",
        )
        .await;
        assert_eq!(idle, 0, "cli left no idle transaction");
        let bits = self.bits;
        let estimated = self.estimated;
        self.note(&format!(
            "published=false\nestimate_is_exact=false\nreltuples_after_bits={bits}\nestimated_rows={estimated}\ncount_rows={count}\ndisposition={disposition}\nsecond_published=false\nsecond_exact=false\nsecond_bits=same\nsentinel_after=-1\nadmin_idle_in_transaction=0\nmaintenance=explicit\nopen_transaction_input=absent\ntimeout_campaign=not_run\nforged_relation_campaign=not_run\n"
        ));
    }

    async fn source_quiet(&mut self) {
        let audit = i64_one(&self.source, "SELECT count(*)::int8 FROM app.write_audit").await;
        let reader = i64_one(
            &self.source,
            "SELECT count(*)::int8 FROM app.write_audit WHERE who = 'oo_a247_reader'",
        )
        .await;
        let source_bits = reltuples(&self.source, "app", "party").await;
        assert_eq!(audit, self.audit_before);
        assert_eq!(reader, 0);
        assert_eq!(source_bits, (-1.0f32).to_bits());
        self.note("source_audit_delta=0\nsource_reader_writes=0\nsource_reltuples=-1\n");
    }

    async fn catalog_query(&mut self) {
        let login_password = read_secret(&self.login_file);
        assert_eq!(login_password, self.login_password);
        create_login(&self.cluster, LOGIN, &login_password).await;
        exec_quiet(&self.cluster, &format!("GRANT CONNECT ON DATABASE {DST_DB} TO {LOGIN}"), "login connect").await;
        let policy = ColumnAclPolicy {
            project: SOURCE.to_string(),
            config_digest: self.digest.clone(),
            generation: GENERATION,
            role: "clerk".to_string(),
            login: LOGIN.to_string(),
            table: LogicalTable {
                namespace: "app".to_string(),
                name: "party".to_string(),
                columns: vec![column("id"), column("label"), column("active")],
            },
            row_filter: None,
        };
        let compiled = compile_column_acl(&policy).expect("compile");
        let installed = install_column_acl(&mut self.target, &policy).await.expect("acl");
        assert_eq!(installed.effect, InstallEffect::Installed);
        assert_eq!(installed.role, ROLE);
        assert_eq!(installed.physical_table, self.physical);
        let identity = PolicyIdentity {
            project: SOURCE.to_string(),
            role: ROLE.to_string(),
            policy_id: self.digest.clone(),
        };
        let limits = CatalogLimits {
            max_tables: 4,
            max_columns: 64,
            max_functions: 1,
            max_operators: 64,
            large_table_row_threshold: 1_000_000,
            unknown_rows: UnknownRows::Refuse,
        };
        let catalog = load_policy_catalog(&mut self.target, &identity, &[compiled], &limits)
            .await
            .expect("catalog");
        assert_eq!(catalog.relations.len(), 1);
        assert_eq!(catalog.relations[0].physical_name, self.physical);
        assert_eq!(catalog.relations[0].estimated_rows, self.estimated);
        assert!(catalog.relations[0].estimated_rows < u64::MAX);
        let admin_count = i64_one(
            &self.target,
            &format!("SELECT count(*)::int8 FROM \"_oo\".\"{}\"", self.physical),
        )
        .await;
        let (client, connection) = connect_require_tls(
            &TlsEndpoint {
                host: self.host.clone(),
                port: self.port,
                database: DST_DB.to_string(),
                user: LOGIN.to_string(),
            },
            &login_password,
            &self.ca,
        )
        .await
        .expect("login tls");
        let tls = rustls_connector(&self.ca).expect("connector");
        let mut session = DedicatedSession::bind(client, connection, tls).await.expect("bind");
        let sql = "SELECT count(*) FROM app.party";
        let stream = session
            .execute_trusted_select(
                SelectRequest {
                    identity: &identity,
                    catalog: &catalog,
                    original_sql: sql,
                    role: ROLE,
                    max_rows: 4,
                    max_result_bytes: 1_024,
                    wall_clock: Duration::from_secs(30),
                },
                MemoryAudit,
            )
            .await
            .expect("query");
        let rows = finish_ok(&mut session, stream).await;
        assert_eq!(rows.len(), 1);
        match &rows[0][..] {
            [Cell::Int(text)] => assert_eq!(text, &admin_count.to_string()),
            _ => panic!("unexpected count cell"),
        }
        let estimated = catalog.relations[0].estimated_rows;
        self.note(&format!(
            "acl_install=fixture\npolicy_catalog=loaded\nunknown_rows=refuse\ncatalog_estimated_rows={estimated}\nquery_status=succeeded\nquery_count={admin_count}\nstatistics_published=false\n"
        ));
    }

    fn finish(&self) {
        assert!(!self.facts.contains(&self.admin_password));
        assert!(!self.facts.contains(&self.reader_password));
        assert!(!self.facts.contains(&self.runtime_password));
        assert!(!self.facts.contains(&self.plain_password));
        assert!(!self.facts.contains(&self.login_password));
        let _ = &self.secrets;
        fs::write(self.evidence.join("oracle.txt"), &self.facts).expect("oracle");
    }

    fn probe(&self, probe: Probe<'_>) -> String {
        let (got, out, err) = self.run_analyze(probe.operator, probe.password, probe.ca, probe.generation, probe.table);
        assert_eq!(got, probe.code, "{} exit", probe.label);
        assert!(out.is_empty(), "{} stdout", probe.label);
        assert_eq!(err, format!("analyze-pg: {}\n", probe.fact));
        assert!(!err.contains(&self.admin_password));
        assert!(!err.contains(&self.reader_password));
        assert!(!err.contains(&self.runtime_password));
        assert!(!err.contains(&self.plain_password));
        format!("{}={}\n{}_fact={}\n", probe.label, probe.code, probe.label, probe.fact)
    }

    fn run_analyze(&self, operator: &str, password: &Path, ca: &Path, generation: i64, table: &str) -> (i32, String, String) {
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
                "database": DST_DB,
                "operator": operator,
                "password_file": password.display().to_string(),
                "ca_file": ca.display().to_string(),
            }
        });
        let text = document.to_string();
        assert!(document.get("transaction").is_none());
        assert!(!text.contains("BEGIN"));
        spawn(&self.analyze_bin, &[], text.as_bytes())
    }

    fn init_document(&self) -> String {
        json!({
            "protocol": "oo.pipeline.init-pg/v1",
            "target": {
                "host": self.host,
                "port": self.port,
                "database": DST_DB,
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
                "database": SRC_DB,
                "username": "oo_a247_reader",
                "secret_ref": "secret://a247/reader",
                "password_file": self.reader_file.display().to_string(),
                "ca_file": self.ca_file.display().to_string(),
                "tables": [{"schema": "app", "table": "party"}]
            },
            "target": {
                "host": self.host,
                "port": self.port,
                "database": DST_DB,
                "runtime_principal": "oo_a247_runtime",
                "password_file": self.runtime_file.display().to_string(),
                "ca_file": self.ca_file.display().to_string(),
            },
            "shard": "a247main",
            "owner": OWNER,
            "source_generation": GENERATION
        })
        .to_string()
    }

    fn note(&mut self, line: &str) {
        self.facts.push_str(line);
        fs::write(self.evidence.join("oracle.txt"), &self.facts).expect("oracle");
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

fn column(name: &str) -> ColumnSpec {
    ColumnSpec {
        name: name.to_string(),
        status: ClassStatus::Confirmed,
        semantic_type: "operator_label".to_string(),
        pii: PiiClass::None,
    }
}

struct MemoryAudit;

impl AuditAdmission for MemoryAudit {
    async fn admit(&mut self, admission: &oo_gateway::query_executor::Admission<'_>) -> Result<(), ExecError> {
        let _ = admission.role();
        Ok(())
    }

    async fn complete(&mut self, completion: &oo_gateway::query_executor::CompletionRecord) -> Result<(), ExecError> {
        let _ = completion.status();
        Ok(())
    }
}

async fn finish_ok(session: &mut DedicatedSession, mut stream: SelectStream) -> Vec<Vec<Cell>> {
    let mut rows = Vec::new();
    loop {
        match stream.next_row().await {
            Ok(Some(row)) => rows.push(row),
            Ok(None) => break,
            Err(err) => panic!("row error: {err}"),
        }
    }
    drop(stream);
    session.wait_idle().await.expect("idle");
    let summary = session.summary().expect("summary");
    assert_eq!(summary.status(), CompletionStatus::Succeeded);
    assert!(!summary.truncated());
    assert!(summary.rollback_confirmed());
    rows
}

fn spawn(bin: &Path, args: &[&str], stdin: &[u8]) -> (i32, String, String) {
    let mut child = Command::new(bin)
        .args(args)
        .env_clear()
        .env("HOME", "/tmp")
        .env("PGPASSWORD", "decoy-a247-env")
        .env("PGSSLROOTCERT", "/tmp/not-a-ca")
        .env("DATABASE_URL", "postgres://user:decoy-a247-env@db.example/app")
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

fn read_secret(path: &Path) -> String {
    let text = fs::read_to_string(path).expect("password file");
    let trimmed = text.trim().to_string();
    assert!(!trimmed.is_empty());
    assert!(trimmed.chars().all(|ch| ch.is_ascii_hexdigit()));
    trimmed
}

fn write_secret(path: &Path, secret: &str) {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .expect("secret file");
    writeln!(file, "{secret}").expect("secret write");
}

async fn hex_secret(client: &Client, label: &str) -> String {
    let row = client
        .query_one(
            "SELECT encode(sha256(($1 || clock_timestamp()::text)::bytea), 'hex')",
            &[&label],
        )
        .await
        .expect("secret");
    let value: String = row.get(0);
    assert_eq!(value.len(), 64);
    assert!(value.chars().all(|ch| ch.is_ascii_hexdigit()));
    value
}

async fn tls_client(host: &str, port: u16, database: &str, user: &str, password: &str, ca: &[u8]) -> Client {
    let (client, connection) = connect_require_tls(
        &TlsEndpoint {
            host: host.to_string(),
            port,
            database: database.to_string(),
            user: user.to_string(),
        },
        password,
        ca,
    )
    .await
    .expect("tls");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn text(client: &Client, sql: &str) -> String {
    let row = client
        .query_one(sql, &[] as &[&(dyn tokio_postgres::types::ToSql + Sync)])
        .await
        .expect("text");
    row.get(0)
}

async fn ssl_text(client: &Client) -> String {
    text(client, "SELECT ssl::text FROM pg_stat_ssl WHERE pid = pg_backend_pid()").await
}

async fn i64_one(client: &Client, sql: &str) -> i64 {
    let row = client
        .query_one(sql, &[] as &[&(dyn tokio_postgres::types::ToSql + Sync)])
        .await
        .expect("count");
    row.get(0)
}

async fn reltuples(client: &Client, namespace: &str, name: &str) -> u32 {
    let row = client
        .query_one(
            "SELECT class.reltuples FROM pg_catalog.pg_class AS class \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace \
             WHERE namespace.nspname = $1 AND class.relname = $2",
            &[&namespace, &name],
        )
        .await
        .expect("reltuples");
    let value: f32 = row.get(0);
    value.to_bits()
}

async fn exec_quiet(client: &Client, sql: &str, label: &str) {
    if client.batch_execute(sql).await.is_err() {
        panic!("{label}");
    }
}

async fn create_login(client: &Client, name: &str, secret: &str) {
    assert!(name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_'));
    assert!(!secret.is_empty() && secret.chars().all(|ch| ch.is_ascii_hexdigit()));
    let sql = format!(
        "CREATE ROLE {name} LOGIN PASSWORD '{secret}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION"
    );
    if client.batch_execute(&sql).await.is_err() {
        panic!("create role failed");
    }
}

fn closed_physical(name: &str) -> bool {
    name.len() == 63 && name.starts_with("pt_") && name.bytes().skip(3).all(|byte| byte.is_ascii_hexdigit())
}
 
