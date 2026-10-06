//! Existing policy catalog and dedicated query after the analyze-pg receipt.
//!
//! This file is copied onto the fixed archive only for the proof container.
//! It is not a product source. It does not write row values.
//! Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a219/SP04/target-statistics/overlay/catalog_proof.rs
//! 4502a2b0f68dc7691c2c25821b5999578ce47dd098641e8460b914061976e23c

use std::env;
use std::fs;
use std::path::Path;
use std::time::Duration;

use oo_gateway::policy::catalog::{CatalogLimits, UnknownRows, load_policy_catalog};
use oo_gateway::query_executor::{
    AuditAdmission, Cell, CompletionStatus, DedicatedSession, ExecError, SelectRequest, SelectStream,
};
use oo_gateway::{
    compile_column_acl, connect_require_tls, install_column_acl, rustls_connector, ClassStatus,
    ColumnAclPolicy, ColumnSpec, LogicalTable, PiiClass, PolicyIdentity, TlsEndpoint,
};
use tokio_postgres::Client;

const SOURCE: &str = "a236src";
const LOGIN: &str = "oo_a236_login";
const ROLE: &str = "oo_r_clerk";

#[tokio::test]
#[ignore = "one private PostgreSQL 17.10 after the analyze-pg proof"]
async fn policy_catalog_query_after_analyze_pg() {
    let mut proof = Proof::connect().await;
    proof.read_mapping().await;
    proof.install_and_query().await;
    proof.write();
    println!("{}", proof.facts);
}

struct Proof {
    host: String,
    port: u16,
    admin_user: String,
    admin_password: String,
    login_password: String,
    ca: Vec<u8>,
    evidence: std::path::PathBuf,
    admin: Client,
    digest: String,
    generation: i64,
    physical: String,
    facts: String,
}

impl Proof {
    async fn connect() -> Self {
        let host = env::var("OO_ANALYZE_PG_HOST").expect("host");
        let port: u16 = env::var("OO_ANALYZE_PG_PORT").expect("port").parse().expect("port");
        let admin_user = env::var("OO_ANALYZE_PG_ADMIN_USER").expect("admin user");
        let admin_password = password("OO_ANALYZE_PG_ADMIN_PASSWORD_FILE");
        let login_password = password("OO_ANALYZE_PG_LOGIN_PASSWORD_FILE");
        let ca = fs::read(env::var("OO_ANALYZE_PG_CA_FILE").expect("ca")).expect("ca");
        let evidence = std::path::PathBuf::from(env::var("OO_ANALYZE_PG_EVIDENCE").expect("evidence"));
        let pipeline = fs::read_to_string(evidence.join("facts.txt")).expect("pipeline facts");
        assert!(pipeline.contains("reltuples_before=-1\n"), "{pipeline}");
        assert!(pipeline.contains("published=false\n"), "{pipeline}");
        assert!(pipeline.contains("source_reader_writes=0\n"), "{pipeline}");
        let (admin, connection) = connect_require_tls(
            &TlsEndpoint {
                host: host.clone(),
                port,
                database: "oo_a236_dst".to_string(),
                user: admin_user.clone(),
            },
            &admin_password,
            &ca,
        )
        .await
        .expect("admin tls");
        tokio::spawn(async move {
            let _ = connection.await;
        });
        Self {
            host,
            port,
            admin_user,
            admin_password,
            login_password,
            ca,
            evidence,
            admin,
            digest: String::new(),
            generation: 0,
            physical: String::new(),
            facts: "pipeline_unknown_before=true\npipeline_published=false\n".to_string(),
        }
    }

    async fn read_mapping(&mut self) {
        let row = self
            .admin
            .query_one(
                "SELECT mapping.config_digest, mapping.run_generation, mapping.physical_name, \
                        class.reltuples \
                 FROM _oo.physical_mappings AS mapping \
                 JOIN pg_catalog.pg_class AS class ON class.relname = mapping.physical_name \
                 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace \
                 WHERE mapping.source = $1 AND mapping.level = 't' \
                   AND mapping.logical_namespace = 'app' AND mapping.logical_table = 'party' \
                   AND namespace.nspname = '_oo' AND class.relkind = 'r'",
                &[&SOURCE],
            )
            .await
            .expect("mapping");
        self.digest = row.get(0);
        self.generation = row.get(1);
        self.physical = row.get(2);
        let reltuples: f32 = row.get(3);
        assert!(reltuples.is_finite() && reltuples >= 0.0);
        assert_ne!(reltuples.to_bits(), (-1.0f32).to_bits());
        let rounded = f64::from(reltuples).round() as u64;
        self.facts.push_str(&format!(
            "catalog_reltuples_known=true\nrounded_estimate={rounded}\n"
        ));
    }

    async fn install_and_query(&mut self) {
        assert!(!self.admin_user.is_empty());
        let secret = self.login_password.replace('\'', "");
        assert!(secret.chars().all(|ch| ch.is_ascii_hexdigit()));
        self.admin
            .batch_execute(&format!(
                "DROP ROLE IF EXISTS {LOGIN}; \
                 CREATE ROLE {LOGIN} LOGIN PASSWORD '{secret}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION; \
                 GRANT CONNECT ON DATABASE oo_a236_dst TO {LOGIN}"
            ))
            .await
            .expect("login");
        let policy = ColumnAclPolicy {
            project: SOURCE.to_string(),
            config_digest: self.digest.clone(),
            generation: self.generation,
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
        let installed = install_column_acl(&mut self.admin, &policy).await.expect("acl");
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
        let catalog = load_policy_catalog(&mut self.admin, &identity, &[compiled], &limits)
            .await
            .expect("catalog");
        assert_eq!(catalog.relations.len(), 1);
        assert_eq!(catalog.relations[0].physical_name, self.physical);
        assert!(catalog.relations[0].estimated_rows < u64::MAX);
        let admin_count: i64 = self
            .admin
            .query_one(
                &format!(
                    "SELECT count(*)::int8 FROM \"_oo\".{}",
                    quote_physical(&self.physical)
                ),
                &[],
            )
            .await
            .expect("admin count")
            .get(0);
        let (client, connection) = connect_require_tls(
            &TlsEndpoint {
                host: self.host.clone(),
                port: self.port,
                database: "oo_a236_dst".to_string(),
                user: LOGIN.to_string(),
            },
            &self.login_password,
            &self.ca,
        )
        .await
        .expect("login tls");
        let tls = rustls_connector(&self.ca).expect("connector");
        let mut session = DedicatedSession::bind(client, connection, tls)
            .await
            .expect("bind");
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
            other => panic!("unexpected count cell {other:?}"),
        }
        let estimated = catalog.relations[0].estimated_rows;
        self.facts.push_str(&format!(
            "policy_catalog=loaded\nunknown_rows=refuse\nestimated_rows={estimated}\nquery_status=succeeded\nquery_count={admin_count}\nclassification=operator_label\nstatistics_published=false\n"
        ));
        let _ = self.admin_password;
    }

    fn write(&self) {
        fs::write(self.evidence.join("catalog-facts.txt"), &self.facts).expect("catalog facts");
    }
}

fn column(name: &str) -> ColumnSpec {
    ColumnSpec {
        name: name.to_string(),
        status: ClassStatus::Confirmed,
        semantic_type: "operator_label".to_string(),
        pii: PiiClass::None,
    }
}

fn quote_physical(name: &str) -> String {
    assert!(name.len() == 63 && name.starts_with("pt_") && name.bytes().skip(3).all(|byte| byte.is_ascii_hexdigit()));
    format!("\"{name}\"")
}

struct MemoryAudit;

impl AuditAdmission for MemoryAudit {
    async fn admit(
        &mut self,
        admission: &oo_gateway::query_executor::Admission<'_>,
    ) -> Result<(), ExecError> {
        let _ = admission.role();
        Ok(())
    }

    async fn complete(
        &mut self,
        completion: &oo_gateway::query_executor::CompletionRecord,
    ) -> Result<(), ExecError> {
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

fn password(var: &str) -> String {
    let path = env::var(var).unwrap_or_else(|_| panic!("{var}"));
    let text = fs::read_to_string(Path::new(&path)).expect("password file");
    let trimmed = text.trim().to_string();
    assert!(trimmed.chars().all(|ch| ch.is_ascii_hexdigit()));
    trimmed
}
