//! Live policy-catalog query after the review's explicit ANALYZE.
//!
//! Copied into a throwaway archive. Not a product source. The typed gold is
//! the three rows inserted by the pipeline review. This file does not treat
//! the planner estimate as that cardinality, and it does not publish.

use std::env;
use std::fs;
use std::path::Path;
use std::time::Duration;

use oo_gateway::policy::catalog::{load_policy_catalog, CatalogLimits, UnknownRows};
use oo_gateway::query_executor::{
    AuditAdmission, Cell, CompletionStatus, DedicatedSession, ExecError, SelectRequest, SelectStream,
};
use oo_gateway::{
    compile_column_acl, connect_require_tls, install_column_acl, rustls_connector, ClassStatus,
    ColumnAclPolicy, ColumnSpec, LogicalTable, PiiClass, PolicyIdentity, TlsEndpoint,
};

const SOURCE: &str = "a230src";
const LOGIN: &str = "oo_a230_login";
const ROLE: &str = "oo_r_clerk";
const GOLD: [(i32, &str, bool); 3] = [(7, "keel", true), (8, "helm", false), (9, "mast", true)];

#[tokio::test]
#[ignore = "one private PostgreSQL 17.10 after the pipeline review left the fixture"]
async fn policy_catalog_reads_typed_gold() {
    let mut proof = Proof::connect().await;
    proof.read_mapping().await;
    proof.install_and_query().await;
    proof.finish();
}

struct Proof {
    host: String,
    port: u16,
    admin_password: String,
    login_password: String,
    ca: Vec<u8>,
    evidence: std::path::PathBuf,
    admin: tokio_postgres::Client,
    digest: String,
    generation: i64,
    physical: String,
    facts: String,
    failures: Vec<String>,
}

impl Proof {
    async fn connect() -> Self {
        let host = env::var("OO_TARGET_STATS_HOST").expect("host");
        let port: u16 = env::var("OO_TARGET_STATS_PORT").expect("port").parse().expect("port");
        let admin_user = env::var("OO_TARGET_STATS_ADMIN_USER").expect("admin user");
        let admin_password = password("OO_TARGET_STATS_ADMIN_PASSWORD_FILE");
        let login_password = password("OO_TARGET_STATS_LOGIN_PASSWORD_FILE");
        let ca = fs::read(env::var("OO_TARGET_STATS_CA_FILE").expect("ca")).expect("ca");
        let evidence = std::path::PathBuf::from(env::var("OO_TARGET_STATS_EVIDENCE").expect("evidence"));
        let pipeline = fs::read_to_string(evidence.join("facts.txt")).unwrap_or_default();
        let mut facts = String::new();
        for marker in [
            "reltuples_before=-1\n",
            "explicit_was_first_known=true\n",
            "published=false\n",
            "estimate_is_exact=false\n",
            "source_audit_delta=0\n",
            "sentinel_after=-1\n",
            "source_reltuples=-1\n",
            "nosleep_kind=OpenTransaction\n",
            "nosleep_disposition=Reusable\n",
            "nosleep_caller_transaction_kept=true\n",
            "nosleep_reltuples_unchanged=true\n",
            "failure_count=0\n",
        ] {
            let present = pipeline.contains(marker);
            facts.push_str(&format!(
                "pipeline_{}={}",
                marker.trim().replace('=', "_"),
                if present { "true" } else { "false" }
            ));
            facts.push('\n');
        }
        let (admin, connection) = connect_require_tls(
            &TlsEndpoint {
                host: host.clone(),
                port,
                database: "oo_a230_dst".to_string(),
                user: admin_user,
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
            admin_password,
            login_password,
            ca,
            evidence,
            admin,
            digest: String::new(),
            generation: 0,
            physical: String::new(),
            facts,
            failures: Vec::new(),
        }
    }

    fn observe(&mut self, key: &str, value: &str, ok: bool) {
        let line = format!("{key}={value}");
        self.facts.push_str(&line);
        self.facts.push('\n');
        if !ok {
            self.failures.push(line);
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
        let known = reltuples.is_finite() && reltuples >= 0.0 && reltuples.to_bits() != (-1.0f32).to_bits();
        self.observe("catalog_reltuples_known", "true", known);
        self.observe(
            "catalog_reltuples_bits",
            &reltuples.to_bits().to_string(),
            known,
        );
        let rounded = if known {
            u64::try_from(f64::from(reltuples).round() as i64).unwrap_or(u64::MAX)
        } else {
            u64::MAX
        };
        self.observe("catalog_rounded_estimate", &rounded.to_string(), true);
        self.observe("estimate_treated_as_exact", "false", true);
    }

    async fn install_and_query(&mut self) {
        let secret = self.login_password.clone();
        assert!(secret.chars().all(|ch| ch.is_ascii_hexdigit()));
        self.admin
            .batch_execute(&format!(
                "DROP ROLE IF EXISTS {LOGIN}; \
                 CREATE ROLE {LOGIN} LOGIN PASSWORD '{secret}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION; \
                 GRANT CONNECT ON DATABASE oo_a230_dst TO {LOGIN}"
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
        self.observe("installed_role", &installed.role, installed.role == ROLE);
        self.observe(
            "installed_physical",
            "stored",
            installed.physical_table == self.physical,
        );
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
        let catalog = match load_policy_catalog(&mut self.admin, &identity, &[compiled], &limits).await {
            Ok(catalog) => catalog,
            Err(err) => {
                self.observe("policy_catalog", "refused", false);
                self.observe("catalog_error_hides_gold", "true", hides_values(&err.to_string()));
                return;
            }
        };
        self.observe("policy_catalog", "loaded", catalog.relations.len() == 1);
        self.observe("unknown_rows", "refuse", true);
        let estimated = catalog.relations.first().map(|relation| relation.estimated_rows).unwrap_or(u64::MAX);
        self.observe("catalog_estimated_rows", &estimated.to_string(), estimated != u64::MAX);
        self.observe("statistics_published", "false", true);
        let (client, connection) = connect_require_tls(
            &TlsEndpoint {
                host: self.host.clone(),
                port: self.port,
                database: "oo_a230_dst".to_string(),
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
        let stream = session
            .execute_trusted_select(
                SelectRequest {
                    identity: &identity,
                    catalog: &catalog,
                    original_sql: "SELECT id, label, active FROM app.party",
                    role: ROLE,
                    max_rows: 8,
                    max_result_bytes: 4_096,
                    wall_clock: Duration::from_secs(30),
                },
                MemoryAudit,
            )
            .await
            .expect("query");
        let rows = finish_ok(&mut session, stream).await;
        let mut got = Vec::new();
        for row in &rows {
            match &row[..] {
                [Cell::Int(id), Cell::Text(label), Cell::Bool(active)] => {
                    got.push((id.clone(), label.clone(), *active));
                }
                _ => self.failures.push("gold_cell_type".to_string()),
            }
        }
        got.sort();
        let mut expect: Vec<(String, String, bool)> = GOLD
            .iter()
            .map(|(id, label, active)| (id.to_string(), (*label).to_string(), *active))
            .collect();
        expect.sort();
        self.observe("query_status", "succeeded", true);
        self.observe("gold_match", "true", got == expect);
        self.observe("gold_rows", &got.len().to_string(), got.len() == 3);
        let _ = self.admin_password;
    }

    fn finish(&mut self) {
        let pipeline_ok = self.facts.lines().filter(|line| line.starts_with("pipeline_")).all(|line| line.ends_with("=true"));
        self.observe("pipeline_chain", "true", pipeline_ok);
        let failure_count = self.failures.len();
        self.observe("failure_count", &failure_count.to_string(), failure_count == 0);
        fs::write(self.evidence.join("catalog-facts.txt"), &self.facts).expect("catalog facts");
        if !self.failures.is_empty() {
            panic!("catalog review failures:\n{}", self.failures.join("\n"));
        }
    }
}

fn column(name: &str) -> ColumnSpec {
    ColumnSpec {
        name: name.to_string(),
        status: ClassStatus::Confirmed,
        semantic_type: "text".to_string(),
        pii: PiiClass::None,
    }
}

fn hides_values(shown: &str) -> bool {
    !shown.contains("keel") && !shown.contains("helm") && !shown.contains("mast")
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
