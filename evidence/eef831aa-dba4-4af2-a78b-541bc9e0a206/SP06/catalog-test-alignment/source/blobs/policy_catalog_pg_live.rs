//! Live policy catalog against a fresh shadow, then one guarded read.
//!
//! Runs only when `OO_A190_LIVE=1`. Without that variable the file compiles
//! and returns. The fixture applies frozen target scripts itself. It does not
//! link `oo-pipeline` and it does not change those scripts. Passwords are read
//! from files and are not written to the oracle.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use oo_gateway::policy::catalog::{
    AGGREGATE_STAR_NAME, COMPARISON_OPERATORS, COMPARISON_SIGNATURES, CatalogError, CatalogLimits,
    UnknownRows, load_policy_catalog,
};
use oo_gateway::query_executor::{
    Admission, AuditAdmission, Cell, CompletionRecord, CompletionStatus, DedicatedSession,
    ExecError, SelectRequest, SelectStream,
};
use oo_gateway::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, CompiledPolicy, GuardError, InstallEffect,
    LogicalTable, PiiClass, PolicyCatalog, PolicyIdentity, TlsEndpoint, compile_column_acl,
    connect_require_tls, guard_select, install_column_acl, rustls_connector,
};
use sha2::{Digest, Sha256};
use tokio_postgres::Client;
use tokio_postgres::types::ToSql;

const HOST: &str = "oo-a190-pg";
const DATABASE: &str = "oo_a190";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a190_login";
const SECRET_DIR: &str = "/run/oo-a190/fixture";
const ORACLE_PATH: &str = "/evidence/live/oracle.txt";
const OWNER: &str = "a1900000-0000-4000-8000-000000000190";
const SHARD: &str = "shard_a190";
const SOURCE: &str = "ledger";
const CONFIG: &str = "a190a190a190a190a190a190a190a190a190a190a190a190a190a190a190a190";
const BAD_DIGEST: &str = "e190e190e190e190e190e190e190e190e190e190e190e190e190e190e190e190";
const TYPE_DIGEST: &str = "b190b190b190b190b190b190b190b190b190b190b190b190b190b190b190b190";
const INPUT_DIGEST: &str = "c190c190c190c190c190c190c190c190c190c190c190c190c190c190c190c190";
const SCHEMA_DIGEST: &str = "d190d190d190d190d190d190d190d190d190d190d190d190d190d190d190d190";
const NAMESPACE: &str = "billing";
const TABLE: &str = "invoice";
const STALE_GENERATION: i64 = 4;
const CURRENT_GENERATION: i64 = 11;
const ADVISORY_LOCK: i64 = 64_042_026_190;

#[tokio::test]
async fn policy_catalog_loads_from_postgres_and_executes() {
    if std::env::var("OO_A190_LIVE").ok().as_deref() != Some("1") {
        eprintln!("OO_A190_LIVE is unset; the policy catalog comparison was not run");
        return;
    }
    let mut live = Live::setup().await;
    live.refuse_before_user_rows().await;
    live.revoke_then_restore_paid().await;
    live.analyze_and_check_oids().await;
    live.execute_visible_queries().await;
    live.deny_hidden_write_and_injection().await;
    live.extra_column_stays_pending().await;
    live.user_function_is_not_resolved().await;
    live.domain_column_is_unsupported().await;
    live.oracle
        .fact("mapping_count", &live.mapping_count().await.to_string());
    live.oracle.fact("done", "ok");
}

struct MemoryAudit;

static AUDIT_ADMITS: AtomicU64 = AtomicU64::new(0);
static AUDIT_COMPLETES: AtomicU64 = AtomicU64::new(0);

fn audit_admits() -> u64 {
    AUDIT_ADMITS.load(Ordering::SeqCst)
}

fn audit_completes() -> u64 {
    AUDIT_COMPLETES.load(Ordering::SeqCst)
}

impl AuditAdmission for MemoryAudit {
    async fn admit(&mut self, admission: &Admission<'_>) -> Result<(), ExecError> {
        let _ = (
            admission.original_sha256(),
            admission.executed_sha256(),
            admission.fingerprint_hex(),
            admission.role(),
            admission.project(),
            admission.policy_id(),
            admission.backend_pid(),
            admission.backend_start(),
        );
        let _ = AUDIT_ADMITS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn complete(&mut self, completion: &CompletionRecord) -> Result<(), ExecError> {
        let _ = completion.status();
        let _ = AUDIT_COMPLETES.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct Shadow {
    table: String,
    qty: String,
    amount: String,
    seq: String,
    memo: String,
    paid: String,
    secret: String,
}

struct Oracle {
    lines: Vec<String>,
    secrets: Vec<String>,
}

impl Oracle {
    fn new(admin_pass: &str, login_pass: &str) -> Self {
        Self {
            lines: Vec::new(),
            secrets: vec![admin_pass.to_string(), login_pass.to_string()],
        }
    }

    fn fact(&mut self, key: &str, value: &str) {
        assert!(!key.contains("pass"), "oracle key");
        assert!(!value.contains("pt_"), "oracle physical table");
        assert!(!value.contains("pc_"), "oracle physical column");
        for secret in &self.secrets {
            assert!(!value.contains(secret), "oracle value contained a password");
        }
        self.lines.push(format!("{key}={value}"));
        self.flush();
    }

    fn flush(&self) {
        let path = Path::new(ORACLE_PATH);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("oracle dir");
        }
        fs::write(path, self.lines.join("\n") + "\n").expect("oracle write");
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stats {
    seq_scan: i64,
    idx_scan: i64,
    inserted: i64,
    updated: i64,
    deleted: i64,
    hot_updated: i64,
}

struct Live {
    oracle: Oracle,
    admin: Client,
    login_pass: String,
    ca: Vec<u8>,
    shadow: Shadow,
    clerk: CompiledPolicy,
    auditor: CompiledPolicy,
    clerk_role: String,
    auditor_role: String,
    clerk_identity: PolicyIdentity,
    auditor_identity: PolicyIdentity,
    clerk_catalog: PolicyCatalog,
    auditor_catalog: PolicyCatalog,
    mappings: i64,
}

impl Live {
    async fn setup() -> Self {
        let admin_pass = read_hex("admin.pass");
        let login_pass = read_hex("login.pass");
        let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca.crt");
        let oracle = Oracle::new(&admin_pass, &login_pass);
        reset_database(&admin_pass, &ca).await;
        let mut admin = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
        apply_frozen_schema(&mut admin).await;
        create_login(&admin, &login_pass).await;
        let shadow = prepare_shadow(&mut admin).await;
        let clerk_policy = acl("clerk", CLERK_ORDER, CURRENT_GENERATION, CONFIG);
        let auditor_policy = acl("auditor", &["amount"], CURRENT_GENERATION, CONFIG);
        let clerk_installed = install_column_acl(&mut admin, &clerk_policy)
            .await
            .expect("install clerk");
        let auditor_installed = install_column_acl(&mut admin, &auditor_policy)
            .await
            .expect("install auditor");
        assert_eq!(clerk_installed.effect, InstallEffect::Installed);
        assert_eq!(auditor_installed.effect, InstallEffect::Installed);
        assert_eq!(clerk_installed.physical_table, shadow.table);
        let clerk = compile_column_acl(&clerk_policy).expect("compile clerk");
        let auditor = compile_column_acl(&auditor_policy).expect("compile auditor");
        let clerk_identity = identity(&clerk_installed.role);
        let auditor_identity = identity(&auditor_installed.role);
        let mappings = mapping_count(&admin).await;
        Self {
            oracle,
            admin,
            login_pass,
            ca,
            shadow,
            clerk,
            auditor,
            clerk_role: clerk_installed.role,
            auditor_role: auditor_installed.role,
            clerk_identity,
            auditor_identity,
            clerk_catalog: empty_catalog(),
            auditor_catalog: empty_catalog(),
            mappings,
        }
    }

    async fn refuse_before_user_rows(&mut self) {
        assert_eq!(self.reltuples().await.to_bits(), (-1.0f32).to_bits());
        self.oracle.fact("reltuples_before", "-1");
        let before = self.stats().await;
        let clerk_identity = self.clerk_identity.clone();
        let auditor_identity = self.auditor_identity.clone();
        let clerk = self.clerk.clone();
        let auditor = self.auditor.clone();
        let clerk_role = self.clerk_role.clone();
        assert_eq!(
            self.load(&clerk_identity, &[], UnknownRows::Refuse).await,
            Err(CatalogError::EmptyInput)
        );
        assert_eq!(
            self.load(
                &clerk_identity,
                &[clerk.clone(), auditor],
                UnknownRows::Refuse
            )
            .await,
            Err(CatalogError::MixedIdentity)
        );
        assert_eq!(
            self.load(
                &auditor_identity,
                std::slice::from_ref(&clerk),
                UnknownRows::Refuse,
            )
            .await,
            Err(CatalogError::IdentityMismatch)
        );
        let stale = compile_column_acl(&acl("clerk", CLERK_ORDER, STALE_GENERATION, CONFIG))
            .expect("compile stale");
        assert_eq!(
            self.load(&identity(&clerk_role), &[stale], UnknownRows::Refuse)
                .await,
            Err(CatalogError::StaleGeneration)
        );
        let bad = compile_column_acl(&acl("clerk", CLERK_ORDER, CURRENT_GENERATION, BAD_DIGEST))
            .expect("compile bad digest");
        assert_eq!(
            self.load(
                &identity_for(&clerk_role, BAD_DIGEST),
                &[bad],
                UnknownRows::Refuse
            )
            .await,
            Err(CatalogError::StaleGeneration)
        );
        let ghost = compile_column_acl(&acl("ghost", &["amount"], CURRENT_GENERATION, CONFIG))
            .expect("compile ghost");
        assert_eq!(
            self.load(&identity("oo_r_ghost"), &[ghost], UnknownRows::Refuse)
                .await,
            Err(CatalogError::MissingRole)
        );
        assert!(!self.role_present("oo_r_ghost").await);
        let zero = self
            .load(
                &clerk_identity,
                std::slice::from_ref(&clerk),
                UnknownRows::Zero,
            )
            .await
            .expect("zero estimate");
        assert_eq!(zero.relations.len(), 1);
        assert_eq!(zero.relations[0].estimated_rows, 0);
        assert_eq!(column_names(&zero).as_slice(), CLERK_ORDER_SORTED);
        assert_eq!(zero.relations[0].pending_unclassified, 2);
        let saturated = self
            .load(&clerk_identity, &[clerk], UnknownRows::Saturate)
            .await
            .expect("saturated estimate");
        assert_eq!(saturated.relations[0].estimated_rows, u64::MAX);
        let clerk_again = self.clerk.clone();
        assert_eq!(
            self.load(&clerk_identity, &[clerk_again], UnknownRows::Refuse)
                .await,
            Err(CatalogError::UnknownRows)
        );
        self.assert_quiet(before).await;
        self.oracle.fact("estimate_zero", "0");
        self.oracle.fact("estimate_saturate", "max");
        self.oracle.fact("estimate_refuse", "unknown");
        self.oracle.fact("pending_before_analyze", "2");
        assert_idle(&self.admin).await;
    }

    async fn revoke_then_restore_paid(&mut self) {
        self.admin
            .batch_execute(&format!(
                "REVOKE SELECT ({paid}) ON TABLE \"_oo\".{table} FROM {role}",
                paid = quote_physical(&self.shadow.paid, "pc_"),
                table = quote_physical(&self.shadow.table, "pt_"),
                role = quote_role(&self.clerk_role),
            ))
            .await
            .expect("revoke paid");
        let before = self.stats().await;
        let identity = self.clerk_identity.clone();
        let policy = self.clerk.clone();
        let hidden = self
            .load(&identity, &[policy], UnknownRows::Zero)
            .await
            .expect("load after revoke");
        self.assert_quiet(before).await;
        assert!(!column_names(&hidden).contains(&"paid"));
        assert_eq!(hidden.relations[0].pending_unclassified, 3);
        self.oracle.fact("pending_revoked_paid", "3");
        self.admin
            .batch_execute(&format!(
                "GRANT SELECT ({paid}) ON TABLE \"_oo\".{table} TO {role}",
                paid = quote_physical(&self.shadow.paid, "pc_"),
                table = quote_physical(&self.shadow.table, "pt_"),
                role = quote_role(&self.clerk_role),
            ))
            .await
            .expect("grant paid");
    }

    async fn analyze_and_check_oids(&mut self) {
        self.admin
            .batch_execute(&format!(
                "ANALYZE \"_oo\".{table}",
                table = quote_physical(&self.shadow.table, "pt_")
            ))
            .await
            .expect("analyze");
        let reltuples = self.reltuples().await;
        assert!(
            reltuples.is_finite() && reltuples >= 0.0,
            "reltuples {reltuples}"
        );
        let expected = round_rows(reltuples);
        assert!(expected > 0 && expected < u64::MAX);
        let before = self.stats().await;
        let clerk_identity = self.clerk_identity.clone();
        let auditor_identity = self.auditor_identity.clone();
        let clerk_policy = self.clerk.clone();
        let auditor_policy = self.auditor.clone();
        let clerk = self
            .load(&clerk_identity, &[clerk_policy], UnknownRows::Refuse)
            .await
            .expect("clerk catalog");
        let auditor = self
            .load(&auditor_identity, &[auditor_policy], UnknownRows::Refuse)
            .await
            .expect("auditor catalog");
        self.assert_quiet(before).await;
        assert_eq!(clerk.relations[0].estimated_rows, expected);
        assert_eq!(auditor.relations[0].estimated_rows, expected);
        assert_eq!(column_names(&clerk).as_slice(), CLERK_ORDER_SORTED);
        assert_eq!(column_names(&auditor).as_slice(), &["amount"][..]);
        assert_eq!(clerk.relations[0].pending_unclassified, 2);
        assert_eq!(auditor.relations[0].pending_unclassified, 6);
        assert_eq!(clerk.relations[0].physical_schema, "_oo");
        assert_eq!(clerk.relations[0].physical_name, self.shadow.table);
        assert!(clerk.null_test_allowed);
        assert_eq!(clerk.functions.len(), 1);
        assert_eq!(clerk.functions[0].name, AGGREGATE_STAR_NAME);
        assert!(clerk.functions[0].argument_type_oids.is_empty());
        assert!(clerk.functions[0].aggregate_star);
        assert_eq!(
            clerk.operators.len(),
            COMPARISON_OPERATORS.len() * COMPARISON_SIGNATURES.len()
        );
        self.assert_type_oids(&clerk).await;
        self.assert_operator_oids(&clerk).await;
        self.assert_count_oid(&clerk).await;
        self.assert_absent_objects(&clerk).await;
        let plan = guard_select(
            &self.clerk_identity,
            &clerk,
            "SELECT * FROM billing.invoice ORDER BY amount",
        )
        .expect("guard star");
        assert_eq!(plan.star_expansions.len(), 1);
        let expanded: Vec<&str> = plan.star_expansions[0]
            .columns
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(expanded.as_slice(), CLERK_ORDER_SORTED);
        assert!(plan.executed_sql.contains(&self.shadow.table));
        assert!(!plan.executed_sql.contains("secret"));
        self.oracle.fact("estimated_rows", &expected.to_string());
        self.oracle.fact("pending_clerk", "2");
        self.oracle.fact("pending_auditor", "6");
        self.oracle
            .fact("operator_count", &clerk.operators.len().to_string());
        self.oracle
            .fact("function_oid", &clerk.functions[0].oid.to_string());
        self.clerk_catalog = clerk;
        self.auditor_catalog = auditor;
    }

    async fn execute_visible_queries(&mut self) {
        let expected = expected_star();
        let admin_rows = self.admin_star().await;
        assert_eq!(admin_rows, expected);
        let mut clerk = self.open_session().await;
        let star = self
            .query(
                &mut clerk,
                &self.clerk_catalog,
                &self.clerk_identity,
                &self.clerk_role,
                STAR_SQL,
            )
            .await;
        assert_eq!(star, expected);
        assert_eq!(
            self.query(
                &mut clerk,
                &self.clerk_catalog,
                &self.clerk_identity,
                &self.clerk_role,
                "SELECT count(*) FROM billing.invoice WHERE paid = true"
            )
            .await,
            vec![vec![Cell::Int("1".to_string())]]
        );
        assert_eq!(
            self.query(
                &mut clerk,
                &self.clerk_catalog,
                &self.clerk_identity,
                &self.clerk_role,
                "SELECT amount FROM billing.invoice WHERE amount = 10"
            )
            .await,
            vec![vec![Cell::Int("10".to_string())]]
        );
        assert_eq!(
            self.query(
                &mut clerk,
                &self.clerk_catalog,
                &self.clerk_identity,
                &self.clerk_role,
                "SELECT qty FROM billing.invoice WHERE qty = 2"
            )
            .await,
            vec![vec![Cell::Int("2".to_string())]]
        );
        assert_eq!(
            self.query(
                &mut clerk,
                &self.clerk_catalog,
                &self.clerk_identity,
                &self.clerk_role,
                "SELECT seq FROM billing.invoice WHERE seq = 100"
            )
            .await,
            vec![vec![Cell::Int("100".to_string())]]
        );
        assert_eq!(
            self.query(
                &mut clerk,
                &self.clerk_catalog,
                &self.clerk_identity,
                &self.clerk_role,
                "SELECT memo FROM billing.invoice WHERE memo = 'alpha'"
            )
            .await,
            vec![vec![Cell::Text("alpha".to_string())]]
        );
        assert_eq!(
            self.query(
                &mut clerk,
                &self.clerk_catalog,
                &self.clerk_identity,
                &self.clerk_role,
                "SELECT qty FROM billing.invoice WHERE paid = false ORDER BY qty"
            )
            .await,
            vec![
                vec![Cell::Int("-3".to_string())],
                vec![Cell::Int("2".to_string())]
            ]
        );
        assert_eq!(
            self.query(
                &mut clerk,
                &self.clerk_catalog,
                &self.clerk_identity,
                &self.clerk_role,
                "SELECT memo FROM billing.invoice WHERE memo IS NULL"
            )
            .await,
            vec![vec![Cell::Null]]
        );
        let mut auditor = self.open_session().await;
        assert_eq!(
            self.query(
                &mut auditor,
                &self.auditor_catalog,
                &self.auditor_identity,
                &self.auditor_role,
                "SELECT amount FROM billing.invoice ORDER BY amount"
            )
            .await,
            vec![
                vec![Cell::Int("-4".to_string())],
                vec![Cell::Int("10".to_string())],
                vec![Cell::Int("20".to_string())],
            ]
        );
        let hidden = self
            .guard_only(
                &mut auditor,
                &self.auditor_catalog,
                &self.auditor_identity,
                &self.auditor_role,
                "SELECT qty FROM billing.invoice",
            )
            .await;
        let hidden_text = hidden.to_string();
        assert!(matches!(
            hidden,
            ExecError::Guard(err) if matches!(err.as_ref(), GuardError::ColumnNotAllowed { .. })
        ));
        assert!(!hidden_text.contains("qty"));
        self.oracle.fact("star_rows", "3");
        self.oracle.fact("count_paid_true", "1");
        self.oracle.fact("auditor_rows", "3");
        clerk.close();
        auditor.close();
    }

    async fn deny_hidden_write_and_injection(&mut self) {
        let mut session = self.open_session().await;
        assert_eq!(session.bound_role(), None);
        let admits = audit_admits();
        let completes = audit_completes();
        for (sql, kind) in [
            ("SELECT pg_catalog.pg_sleep(0)", "function"),
            ("SELECT count(amount) FROM billing.invoice", "function"),
            ("SELECT amount + 1 FROM billing.invoice", "function"),
            ("INSERT INTO billing.invoice (amount) VALUES (1)", "write"),
            ("SELECT secret FROM billing.invoice", "column"),
        ] {
            let err = self
                .guard_only(
                    &mut session,
                    &self.clerk_catalog,
                    &self.clerk_identity,
                    &self.clerk_role,
                    sql,
                )
                .await;
            match kind {
                "function" => assert!(
                    matches!(err, ExecError::Guard(inner) if matches!(inner.as_ref(), GuardError::FunctionNotAllowed)),
                    "{sql}"
                ),
                "write" => assert!(
                    matches!(err, ExecError::Guard(inner) if matches!(inner.as_ref(), GuardError::Write)),
                    "{sql}"
                ),
                "column" => {
                    let text = err.to_string();
                    assert!(
                        matches!(err, ExecError::Guard(inner) if matches!(inner.as_ref(), GuardError::ColumnNotAllowed { .. }))
                    );
                    assert!(!text.contains("secret"));
                    assert!(!text.contains(&self.shadow.secret));
                    assert!(!text.contains(&self.shadow.table));
                }
                _ => unreachable!(),
            }
            assert_eq!(
                session.bound_role().as_deref(),
                Some(self.clerk_role.as_str())
            );
            assert!(session.is_open());
            assert_eq!(audit_admits(), admits);
            assert_eq!(audit_completes(), completes);
            self.oracle
                .fact(&format!("guard_{kind}_{}", sql.len()), kind);
        }
        let reuse = self
            .guard_only(
                &mut session,
                &self.auditor_catalog,
                &self.auditor_identity,
                &self.auditor_role,
                "SELECT amount FROM billing.invoice ORDER BY amount",
            )
            .await;
        assert!(matches!(reuse, ExecError::RoleReuse));
        assert_eq!(
            session.bound_role().as_deref(),
            Some(self.clerk_role.as_str())
        );
        assert!(session.is_open());
        assert_eq!(audit_admits(), admits);
        assert_eq!(audit_completes(), completes);
        let mut other = self.open_session().await;
        assert_eq!(other.bound_role(), None);
        let hidden = self
            .guard_only(
                &mut other,
                &self.auditor_catalog,
                &self.auditor_identity,
                &self.auditor_role,
                "SELECT qty FROM billing.invoice",
            )
            .await;
        let hidden_text = hidden.to_string();
        assert!(matches!(
            hidden,
            ExecError::Guard(err) if matches!(err.as_ref(), GuardError::ColumnNotAllowed { .. })
        ));
        assert!(!hidden_text.contains("qty"));
        assert_eq!(
            other.bound_role().as_deref(),
            Some(self.auditor_role.as_str())
        );
        assert!(other.is_open());
        assert_eq!(audit_admits(), admits);
        assert_eq!(audit_completes(), completes);
        assert_eq!(
            self.query(
                &mut other,
                &self.auditor_catalog,
                &self.auditor_identity,
                &self.auditor_role,
                "SELECT amount FROM billing.invoice ORDER BY amount"
            )
            .await,
            vec![
                vec![Cell::Int("-4".to_string())],
                vec![Cell::Int("10".to_string())],
                vec![Cell::Int("20".to_string())],
            ]
        );
        assert_eq!(
            other.bound_role().as_deref(),
            Some(self.auditor_role.as_str())
        );
        assert_eq!(audit_admits(), admits + 1);
        assert_eq!(audit_completes(), completes + 1);
        assert_eq!(
            session.bound_role().as_deref(),
            Some(self.clerk_role.as_str())
        );
        other.close();
        session.close();
        self.oracle.fact("guard_bound_role", "clerk");
        self.oracle.fact("guard_audit_delta", "0");
        self.oracle.fact("role_reuse", "refused");
        self.oracle.fact("fresh_other_role", "auditor");
        self.oracle.fact("fresh_read_audit", "1");
        self.assert_direct_denial(
            &format!(
                "SELECT {secret} FROM \"_oo\".{table}",
                secret = quote_physical(&self.shadow.secret, "pc_"),
                table = quote_physical(&self.shadow.table, "pt_"),
            ),
            "direct_secret",
        )
        .await;
        self.assert_direct_denial(
            &format!(
                "INSERT INTO \"_oo\".{table} (\"_oo_pos\") VALUES (9)",
                table = quote_physical(&self.shadow.table, "pt_"),
            ),
            "direct_insert",
        )
        .await;
    }

    async fn extra_column_stays_pending(&mut self) {
        self.admin
            .batch_execute(&format!(
                "ALTER TABLE \"_oo\".{table} ADD COLUMN \"extra\" text",
                table = quote_physical(&self.shadow.table, "pt_")
            ))
            .await
            .expect("add column");
        let before = self.stats().await;
        let identity = self.clerk_identity.clone();
        let policy = self.clerk.clone();
        let catalog = self
            .load(&identity, &[policy], UnknownRows::Refuse)
            .await
            .expect("load extra");
        self.assert_quiet(before).await;
        assert_eq!(catalog.relations[0].pending_unclassified, 3);
        assert!(!column_names(&catalog).contains(&"extra"));
        let mut session = self.open_session().await;
        let err = self
            .guard_only(
                &mut session,
                &catalog,
                &self.clerk_identity,
                &self.clerk_role,
                "SELECT extra FROM billing.invoice",
            )
            .await;
        let text = err.to_string();
        assert!(
            matches!(err, ExecError::Guard(inner) if matches!(inner.as_ref(), GuardError::ColumnNotAllowed { .. }))
        );
        assert!(!text.contains("extra"));
        session.close();
        self.assert_direct_denial(
            &format!(
                "SELECT \"extra\" FROM \"_oo\".{table}",
                table = quote_physical(&self.shadow.table, "pt_"),
            ),
            "direct_extra",
        )
        .await;
        self.oracle.fact("pending_extra", "3");
        self.clerk_catalog = catalog;
    }

    async fn user_function_is_not_resolved(&mut self) {
        self.admin
            .batch_execute(
                "CREATE SCHEMA fixture; \
                 CREATE FUNCTION fixture.touch() RETURNS integer LANGUAGE sql AS 'SELECT 1'",
            )
            .await
            .expect("create touch");
        let touch_oid = self.function_oid("fixture", "touch").await;
        let before = self.stats().await;
        let identity = self.clerk_identity.clone();
        let policy = self.clerk.clone();
        let catalog = self
            .load(&identity, &[policy], UnknownRows::Refuse)
            .await
            .expect("load after touch");
        self.assert_quiet(before).await;
        assert_eq!(catalog.functions.len(), 1);
        assert_eq!(catalog.functions[0].name, "count");
        assert!(
            catalog
                .functions
                .iter()
                .all(|function| function.oid != touch_oid)
        );
        assert!(
            catalog
                .operators
                .iter()
                .all(|operator| operator.oid != touch_oid)
        );
        let mut session = self.open_session().await;
        let err = self
            .guard_only(
                &mut session,
                &catalog,
                &self.clerk_identity,
                &self.clerk_role,
                "SELECT fixture.touch()",
            )
            .await;
        assert!(
            matches!(err, ExecError::Guard(inner) if matches!(inner.as_ref(), GuardError::FunctionNotAllowed))
        );
        session.close();
        self.oracle.fact("touch_admitted", "no");
    }

    async fn domain_column_is_unsupported(&mut self) {
        self.admin
            .batch_execute(&format!(
                "CREATE DOMAIN public.a190_memo AS text; \
                 ALTER TABLE \"_oo\".{table} ALTER COLUMN {memo} TYPE public.a190_memo",
                table = quote_physical(&self.shadow.table, "pt_"),
                memo = quote_physical(&self.shadow.memo, "pc_"),
            ))
            .await
            .expect("alter memo domain");
        let before = self.stats().await;
        let identity = self.clerk_identity.clone();
        let policy = self.clerk.clone();
        let err = self
            .load(&identity, &[policy], UnknownRows::Refuse)
            .await
            .expect_err("domain column");
        self.assert_quiet(before).await;
        assert_eq!(err, CatalogError::UnsupportedType);
        let text = err.to_string();
        assert!(!text.contains("memo"));
        assert!(!text.contains(&self.shadow.memo));
        assert!(!text.contains("SELECT"));
        self.oracle.fact("domain_memo", "unsupported");
        assert_eq!(self.mapping_count().await, self.mappings);
    }

    async fn load(
        &mut self,
        identity: &PolicyIdentity,
        policies: &[CompiledPolicy],
        unknown: UnknownRows,
    ) -> Result<PolicyCatalog, CatalogError> {
        load_policy_catalog(&mut self.admin, identity, policies, &limits(unknown)).await
    }

    async fn query(
        &self,
        session: &mut DedicatedSession,
        catalog: &PolicyCatalog,
        identity: &PolicyIdentity,
        role: &str,
        sql: &str,
    ) -> Vec<Vec<Cell>> {
        let stream = session
            .execute_trusted_select(request(identity, catalog, sql, role), MemoryAudit)
            .await
            .unwrap_or_else(|err| panic!("query failed: {err}"));
        finish_ok(session, stream).await
    }

    async fn guard_only(
        &self,
        session: &mut DedicatedSession,
        catalog: &PolicyCatalog,
        identity: &PolicyIdentity,
        role: &str,
        sql: &str,
    ) -> ExecError {
        match session
            .execute_trusted_select(request(identity, catalog, sql, role), MemoryAudit)
            .await
        {
            Err(err) => err,
            Ok(stream) => {
                drop(stream);
                let _ = session.wait_idle().await;
                panic!("statement was executed");
            }
        }
    }

    async fn open_session(&self) -> DedicatedSession {
        let endpoint = TlsEndpoint {
            host: HOST.to_string(),
            port: 5432,
            database: DATABASE.to_string(),
            user: LOGIN.to_string(),
        };
        let (client, connection) = connect_require_tls(&endpoint, &self.login_pass, &self.ca)
            .await
            .expect("session connect");
        let tls = rustls_connector(&self.ca).expect("connector");
        DedicatedSession::bind(client, connection, tls)
            .await
            .expect("bind")
    }

    async fn assert_direct_denial(&mut self, sql: &str, key: &str) {
        let client = connect_db(DATABASE, LOGIN, &self.login_pass, &self.ca).await;
        client
            .batch_execute(&format!("SET ROLE {}", quote_role(&self.clerk_role)))
            .await
            .expect("set role");
        let err = client.batch_execute(sql).await.expect_err("direct denial");
        let state = err
            .code()
            .map(|code| code.code().to_string())
            .unwrap_or_default();
        assert_eq!(state, "42501");
        self.oracle.fact(key, &state);
    }

    async fn admin_star(&self) -> Vec<Vec<Cell>> {
        let sql = format!(
            "SELECT {qty}, {amount}, {seq}, {memo}, {paid} FROM \"_oo\".{table} ORDER BY {amount}",
            qty = quote_physical(&self.shadow.qty, "pc_"),
            amount = quote_physical(&self.shadow.amount, "pc_"),
            seq = quote_physical(&self.shadow.seq, "pc_"),
            memo = quote_physical(&self.shadow.memo, "pc_"),
            paid = quote_physical(&self.shadow.paid, "pc_"),
            table = quote_physical(&self.shadow.table, "pt_"),
        );
        let rows = self.admin.query(&sql, &[]).await.expect("admin star");
        rows.into_iter().map(admin_star_row).collect()
    }

    async fn assert_type_oids(&mut self, catalog: &PolicyCatalog) {
        let relation = &catalog.relations[0];
        let expected = [
            ("qty", &self.shadow.qty, "int2"),
            ("amount", &self.shadow.amount, "int4"),
            ("seq", &self.shadow.seq, "int8"),
            ("memo", &self.shadow.memo, "text"),
            ("paid", &self.shadow.paid, "bool"),
        ];
        assert_eq!(relation.columns.len(), expected.len());
        for (column, (name, physical, type_name)) in relation.columns.iter().zip(expected) {
            assert_eq!(column.name, name);
            assert_eq!(column.physical_name, *physical);
            let looked = self.attribute_type(physical).await;
            let named = self.type_oid(type_name).await;
            assert_eq!(column.type_oid, looked);
            assert_eq!(column.type_oid, named);
            assert_ne!(column.type_oid, 0);
            self.oracle
                .fact(&format!("oid_{type_name}"), &named.to_string());
        }
        assert_eq!(
            catalog.literal_types.integer,
            Some(self.type_oid("int4").await)
        );
        assert_eq!(
            catalog.literal_types.text,
            Some(self.type_oid("text").await)
        );
        assert_eq!(
            catalog.literal_types.boolean,
            Some(self.type_oid("bool").await)
        );
    }

    async fn assert_operator_oids(&self, catalog: &PolicyCatalog) {
        let names = self.type_name_by_oid().await;
        let mut seen = BTreeSet::new();
        for operator in &catalog.operators {
            let left = names
                .get(&operator.left_type_oid)
                .expect("left type")
                .clone();
            let right = names
                .get(&operator.right_type_oid)
                .expect("right type")
                .clone();
            let looked = self.operator_oids(&operator.name, &left, &right).await;
            assert_eq!(looked, (operator.oid, operator.procedure_oid));
            assert_eq!(operator.schema, "pg_catalog");
            seen.insert((operator.name.clone(), left, right));
        }
        for (left, right) in COMPARISON_SIGNATURES {
            for name in COMPARISON_OPERATORS {
                assert!(seen.contains(&(
                    name.to_string(),
                    (*left).to_string(),
                    (*right).to_string()
                )));
            }
        }
    }

    async fn assert_count_oid(&self, catalog: &PolicyCatalog) {
        let oid = self.count_star_oid().await;
        assert_eq!(catalog.functions[0].oid, oid);
        assert_eq!(catalog.functions[0].schema, "pg_catalog");
    }

    async fn assert_absent_objects(&mut self, catalog: &PolicyCatalog) {
        let sleep = self.proc_oids("pg_sleep").await;
        assert!(!sleep.is_empty());
        for oid in sleep {
            assert!(catalog.functions.iter().all(|function| function.oid != oid));
        }
        let int4 = self.type_oid("int4").await;
        let count_arg = self.count_any_oid().await;
        assert!(
            catalog
                .functions
                .iter()
                .all(|function| function.oid != count_arg)
        );
        let plus = self.plus_oid(int4).await;
        assert!(
            catalog
                .operators
                .iter()
                .all(|operator| operator.oid != plus)
        );
        self.oracle.fact("pg_sleep_admitted", "no");
        self.oracle.fact("count_arg_admitted", "no");
        self.oracle.fact("plus_admitted", "no");
    }

    async fn attribute_type(&self, physical: &str) -> u32 {
        let row = self
            .admin
            .query_one(
                "SELECT attribute.atttypid::int4 \
                 FROM pg_catalog.pg_attribute AS attribute \
                 JOIN pg_catalog.pg_class AS relation ON relation.oid = attribute.attrelid \
                 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
                 WHERE namespace.nspname = '_oo' AND relation.relname = $1 \
                   AND attribute.attname = $2 AND NOT attribute.attisdropped",
                &[&self.shadow.table, &physical],
            )
            .await
            .expect("attribute type");
        oid_u32(row.get(0))
    }

    async fn type_oid(&self, name: &str) -> u32 {
        let row = self
            .admin
            .query_one(
                "SELECT type_row.oid::int4 \
                 FROM pg_catalog.pg_type AS type_row \
                 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type_row.typnamespace \
                 WHERE namespace.nspname = 'pg_catalog' AND type_row.typname = $1 AND type_row.typtype = 'b'",
                &[&name],
            )
            .await
            .expect("type oid");
        oid_u32(row.get(0))
    }

    async fn type_name_by_oid(&self) -> std::collections::BTreeMap<u32, String> {
        let type_names: &[&str] = &["bool", "int2", "int4", "int8", "text"];
        let rows = self
            .admin
            .query(
                "SELECT type_row.oid::int4, type_row.typname::text \
                 FROM pg_catalog.pg_type AS type_row \
                 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type_row.typnamespace \
                 WHERE namespace.nspname = 'pg_catalog' AND type_row.typtype = 'b' \
                   AND type_row.typname = ANY($1::text[])",
                &[&type_names],
            )
            .await
            .expect("type names");
        rows.into_iter()
            .map(|row| (oid_u32(row.get(0)), row.get(1)))
            .collect()
    }

    async fn operator_oids(&self, name: &str, left: &str, right: &str) -> (u32, u32) {
        let rows = self
            .admin
            .query(
                "SELECT operator.oid::int4, operator.oprcode::oid::int4 \
                 FROM pg_catalog.pg_operator AS operator \
                 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = operator.oprnamespace \
                 JOIN pg_catalog.pg_type AS left_type ON left_type.oid = operator.oprleft \
                 JOIN pg_catalog.pg_type AS right_type ON right_type.oid = operator.oprright \
                 WHERE namespace.nspname = 'pg_catalog' AND operator.oprname = $1 \
                   AND operator.oprkind = 'b' AND left_type.typname = $2 AND right_type.typname = $3",
                &[&name, &left, &right],
            )
            .await
            .expect("operator oid");
        assert_eq!(rows.len(), 1, "operator {name} {left} {right}");
        (oid_u32(rows[0].get(0)), oid_u32(rows[0].get(1)))
    }

    async fn count_star_oid(&self) -> u32 {
        let rows = self
            .admin
            .query(
                "SELECT procedure.oid::int4 \
                 FROM pg_catalog.pg_proc AS procedure \
                 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = procedure.pronamespace \
                 WHERE namespace.nspname = 'pg_catalog' AND procedure.proname = 'count' \
                   AND procedure.prokind = 'a' AND procedure.pronargs = 0",
                &[],
            )
            .await
            .expect("count star");
        assert_eq!(rows.len(), 1);
        oid_u32(rows[0].get(0))
    }

    async fn proc_oids(&self, name: &str) -> Vec<u32> {
        let rows = self
            .admin
            .query(
                "SELECT procedure.oid::int4 FROM pg_catalog.pg_proc AS procedure WHERE procedure.proname = $1",
                &[&name],
            )
            .await
            .expect("proc oids");
        rows.into_iter().map(|row| oid_u32(row.get(0))).collect()
    }

    async fn count_any_oid(&self) -> u32 {
        let rows = self
            .admin
            .query(
                "SELECT procedure.oid::int4 \
                 FROM pg_catalog.pg_proc AS procedure \
                 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = procedure.pronamespace \
                 JOIN pg_catalog.pg_type AS argument ON argument.oid = procedure.proargtypes[0] \
                 WHERE namespace.nspname = 'pg_catalog' AND procedure.proname = 'count' \
                   AND procedure.prokind = 'a' AND procedure.pronargs = 1 \
                   AND argument.typname = 'any'",
                &[],
            )
            .await
            .expect("count any");
        assert_eq!(rows.len(), 1);
        oid_u32(rows[0].get(0))
    }

    async fn plus_oid(&self, int4_oid: u32) -> u32 {
        let oid = i32::try_from(int4_oid).expect("int4 oid");
        let rows = self
            .admin
            .query(
                "SELECT operator.oid::int4 \
                 FROM pg_catalog.pg_operator AS operator \
                 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = operator.oprnamespace \
                 WHERE namespace.nspname = 'pg_catalog' AND operator.oprname = '+' \
                   AND operator.oprleft = ($1::int4)::oid AND operator.oprright = ($1::int4)::oid",
                &[&oid],
            )
            .await
            .expect("plus");
        assert_eq!(rows.len(), 1);
        oid_u32(rows[0].get(0))
    }

    async fn function_oid(&self, schema: &str, name: &str) -> u32 {
        let row = self
            .admin
            .query_one(
                "SELECT procedure.oid::int4 \
                 FROM pg_catalog.pg_proc AS procedure \
                 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = procedure.pronamespace \
                 WHERE namespace.nspname = $1 AND procedure.proname = $2",
                &[&schema, &name],
            )
            .await
            .expect("function oid");
        oid_u32(row.get(0))
    }

    async fn reltuples(&self) -> f32 {
        self.admin
            .query_one(
                "SELECT relation.reltuples \
                 FROM pg_catalog.pg_class AS relation \
                 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
                 WHERE namespace.nspname = '_oo' AND relation.relname = $1",
                &[&self.shadow.table],
            )
            .await
            .expect("reltuples")
            .get(0)
    }

    async fn stats(&self) -> Stats {
        let row = self
            .admin
            .query_one(
                "SELECT COALESCE(stats.seq_scan, 0), COALESCE(stats.idx_scan, 0), \
                        COALESCE(stats.n_tup_ins, 0), COALESCE(stats.n_tup_upd, 0), \
                        COALESCE(stats.n_tup_del, 0), COALESCE(stats.n_tup_hot_upd, 0) \
                 FROM pg_catalog.pg_class AS relation \
                 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
                 LEFT JOIN pg_catalog.pg_stat_user_tables AS stats ON stats.relid = relation.oid \
                 WHERE namespace.nspname = '_oo' AND relation.relname = $1",
                &[&self.shadow.table],
            )
            .await
            .expect("stats");
        Stats {
            seq_scan: row.get(0),
            idx_scan: row.get(1),
            inserted: row.get(2),
            updated: row.get(3),
            deleted: row.get(4),
            hot_updated: row.get(5),
        }
    }

    async fn assert_quiet(&mut self, before: Stats) {
        let after = self.stats().await;
        assert_eq!(after, before, "loader touched user-table stats");
        assert_eq!(self.mapping_count().await, self.mappings);
        self.oracle.fact("scan_delta", "0");
    }

    async fn mapping_count(&self) -> i64 {
        mapping_count(&self.admin).await
    }

    async fn role_present(&self, name: &str) -> bool {
        self.admin
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = $1)",
                &[&name],
            )
            .await
            .expect("role")
            .get(0)
    }
}

const CLERK_ORDER: &[&str] = &["paid", "qty", "memo", "seq", "amount"];
const CLERK_ORDER_SORTED: &[&str] = &["qty", "amount", "seq", "memo", "paid"];
const STAR_SQL: &str = "SELECT * FROM billing.invoice ORDER BY amount";

fn empty_catalog() -> PolicyCatalog {
    PolicyCatalog {
        identity: identity("oo_r_unset"),
        relations: Vec::new(),
        functions: Vec::new(),
        operators: Vec::new(),
        literal_types: oo_gateway::LiteralTypes::default(),
        null_test_allowed: false,
        large_table_row_threshold: 0,
    }
}

fn limits(unknown_rows: UnknownRows) -> CatalogLimits {
    CatalogLimits {
        max_tables: 4,
        max_columns: 64,
        max_functions: 1,
        max_operators: 64,
        large_table_row_threshold: 1_000_000,
        unknown_rows,
    }
}

fn identity(role: &str) -> PolicyIdentity {
    identity_for(role, CONFIG)
}

fn identity_for(role: &str, policy_id: &str) -> PolicyIdentity {
    PolicyIdentity {
        project: SOURCE.to_string(),
        role: role.to_string(),
        policy_id: policy_id.to_string(),
    }
}

fn acl(role: &str, columns: &[&str], generation: i64, digest: &str) -> ColumnAclPolicy {
    ColumnAclPolicy {
        project: SOURCE.to_string(),
        config_digest: digest.to_string(),
        generation,
        role: role.to_string(),
        login: LOGIN.to_string(),
        table: LogicalTable {
            namespace: NAMESPACE.to_string(),
            name: TABLE.to_string(),
            columns: columns.iter().copied().map(column_spec).collect(),
        },
        row_filter: None,
    }
}

fn column_spec(name: &str) -> ColumnSpec {
    ColumnSpec {
        name: name.to_string(),
        status: ClassStatus::Confirmed,
        semantic_type: "value".to_string(),
        pii: PiiClass::None,
    }
}

fn column_names(catalog: &PolicyCatalog) -> Vec<&str> {
    catalog.relations[0]
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect()
}

fn expected_star() -> Vec<Vec<Cell>> {
    vec![
        vec![
            Cell::Int("-3".to_string()),
            Cell::Int("-4".to_string()),
            Cell::Int("-5".to_string()),
            Cell::Text("beta".to_string()),
            Cell::Bool(false),
        ],
        vec![
            Cell::Int("2".to_string()),
            Cell::Int("10".to_string()),
            Cell::Int("100".to_string()),
            Cell::Null,
            Cell::Bool(false),
        ],
        vec![
            Cell::Int("5".to_string()),
            Cell::Int("20".to_string()),
            Cell::Int("200".to_string()),
            Cell::Text("alpha".to_string()),
            Cell::Bool(true),
        ],
    ]
}

fn admin_star_row(row: tokio_postgres::Row) -> Vec<Cell> {
    let qty: i16 = row.get(0);
    let amount: i32 = row.get(1);
    let seq: i64 = row.get(2);
    let memo: Option<String> = row.get(3);
    let paid: bool = row.get(4);
    vec![
        Cell::Int(qty.to_string()),
        Cell::Int(amount.to_string()),
        Cell::Int(seq.to_string()),
        match memo {
            Some(text) => Cell::Text(text),
            None => Cell::Null,
        },
        Cell::Bool(paid),
    ]
}

fn request<'a>(
    identity: &'a PolicyIdentity,
    catalog: &'a PolicyCatalog,
    sql: &'a str,
    role: &'a str,
) -> SelectRequest<'a> {
    SelectRequest {
        identity,
        catalog,
        original_sql: sql,
        role,
        max_rows: 20,
        max_result_bytes: 4_096,
        wall_clock: Duration::from_millis(30_000),
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
    rows
}

fn round_rows(value: f32) -> u64 {
    let rounded = f64::from(value).round();
    assert!(rounded.is_finite() && rounded >= 0.0);
    rounded as u64
}

fn oid_u32(value: i32) -> u32 {
    u32::try_from(value).expect("oid")
}

fn read_hex(name: &str) -> String {
    let raw = fs::read_to_string(Path::new(SECRET_DIR).join(name)).expect("secret file");
    let value = raw.trim();
    let hex = (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    assert!(hex, "{name} is not lowercase hex");
    value.to_string()
}

async fn connect_db(database: &str, user: &str, password: &str, ca: &[u8]) -> Client {
    let endpoint = TlsEndpoint {
        host: HOST.to_string(),
        port: 5432,
        database: database.to_string(),
        user: user.to_string(),
    };
    let mut last = String::new();
    for _ in 0..20 {
        match connect_require_tls(&endpoint, password, ca).await {
            Ok((client, connection)) => {
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                return client;
            }
            Err(err) => {
                last = err.to_string();
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    panic!("tls connect to {database} as {user} failed: {last}");
}

async fn reset_database(admin_pass: &str, ca: &[u8]) {
    let maintenance = connect_db("postgres", ADMIN, admin_pass, ca).await;
    expect_sql(
        &maintenance,
        "terminate database sessions",
        &format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{DATABASE}' AND pid <> pg_backend_pid()"
        ),
    )
    .await;
    expect_sql(
        &maintenance,
        "drop database",
        &format!("DROP DATABASE IF EXISTS \"{DATABASE}\""),
    )
    .await;
    expect_sql(
        &maintenance,
        "drop fixture roles",
        "DO $$\n\
         DECLARE role_name text;\n\
         BEGIN\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a190_login') THEN\n\
             DROP ROLE oo_a190_login;\n\
           END IF;\n\
           FOR role_name IN\n\
             SELECT rolname FROM pg_catalog.pg_roles\n\
             WHERE rolname LIKE 'oo\\_r\\_%' ESCAPE '\\'\n\
           LOOP\n\
             EXECUTE format('DROP ROLE %I', role_name);\n\
           END LOOP;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_pipeline_runtime') THEN\n\
             DROP ROLE oo_pipeline_runtime;\n\
           END IF;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_target_owner') THEN\n\
             DROP ROLE oo_target_owner;\n\
           END IF;\n\
         END\n\
         $$",
    )
    .await;
    expect_sql(
        &maintenance,
        "create database",
        &format!("CREATE DATABASE \"{DATABASE}\" TEMPLATE template0 ENCODING 'UTF8'"),
    )
    .await;
}

async fn assert_idle(client: &Client) {
    let idle: bool = client
        .query_one(
            "SELECT pg_catalog.pg_current_xact_id_if_assigned() IS NULL",
            &[],
        )
        .await
        .expect("idle")
        .get(0);
    assert!(idle);
}

async fn mapping_count(client: &Client) -> i64 {
    client
        .query_one("SELECT count(*)::int8 FROM _oo.physical_mappings", &[])
        .await
        .expect("mapping count")
        .get(0)
}

async fn apply_frozen_schema(client: &mut Client) {
    let bootstrap = migration("0000_bootstrap_roles.sql");
    let fence = migration("0001_target_fence_expand.sql");
    let preflight = migration("0003_target_segment_preflight_expand.sql");
    let mapping = migration("0004_physical_mapping.sql");
    let full_import = migration("0005_full_import_expand.sql");
    let tx = begin_locked(client).await;
    exec_named(&tx, "0000_bootstrap_roles.sql", &bootstrap.0).await;
    exec_named(&tx, "set-owner", "SET LOCAL ROLE oo_target_owner").await;
    exec_named(&tx, "0001_target_fence_expand.sql", &fence.0).await;
    exec_named(
        &tx,
        "row-lock",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE",
    )
    .await;
    exec_named(
        &tx,
        "0003_target_segment_preflight_expand.sql",
        &preflight.0,
    )
    .await;
    exec_named(&tx, "0004_physical_mapping.sql", &mapping.0).await;
    exec_named(
        &tx,
        "markers-target",
        &marker_sql(&[
            (
                "target-bootstrap/1",
                &bootstrap.1,
                "0000_bootstrap_roles.sql",
            ),
            (
                "target-fence-receipts/1",
                &fence.1,
                "0001_target_fence_expand.sql",
            ),
            (
                "target-fence-receipts/3",
                &preflight.1,
                "0003_target_segment_preflight_expand.sql",
            ),
            (
                "physical-mapping/v1",
                &mapping.1,
                "0004_physical_mapping.sql",
            ),
        ]),
    )
    .await;
    tx.commit().await.expect("commit target schema");

    let tx = begin_locked(client).await;
    exec_named(
        &tx,
        "row-lock-import",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE",
    )
    .await;
    exec_named(
        &tx,
        "stats-grant",
        "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE",
    )
    .await;
    exec_named(&tx, "set-owner-import", "SET LOCAL ROLE oo_target_owner").await;
    exec_named(&tx, "0005_full_import_expand.sql", &full_import.0).await;
    exec_named(
        &tx,
        "marker-import",
        &marker_sql(&[(
            "full-import/v1",
            &full_import.1,
            "0005_full_import_expand.sql",
        )]),
    )
    .await;
    tx.commit().await.expect("commit full import");
}

fn migration(name: &str) -> (String, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oo-pipeline/migrations")
        .join(name);
    let bytes = fs::read(&path).unwrap_or_else(|err| panic!("read {name}: {err}"));
    let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| panic!("{name} is not utf-8"));
    (text, sha256_hex(&bytes))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        hex.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    hex
}

fn marker_sql(rows: &[(&str, &str, &str)]) -> String {
    let mut values = String::new();
    for (index, (version, checksum, filename)) in rows.iter().enumerate() {
        assert!(closed_token(version), "version token");
        assert!(closed_token(filename), "filename token");
        assert_eq!(checksum.len(), 64, "checksum length");
        if index > 0 {
            values.push_str(",\n");
        }
        values.push_str(&format!(
            "  ('{version}', '{checksum}', 'expand', '{filename}')"
        ));
    }
    format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values}"
    )
}

fn closed_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '.' | '-'))
}

async fn begin_locked(client: &mut Client) -> tokio_postgres::Transaction<'_> {
    let tx = client.transaction().await.expect("begin");
    exec_named(&tx, "lock-timeout", "SET LOCAL lock_timeout = '15s'").await;
    exec_named(
        &tx,
        "statement-timeout",
        "SET LOCAL statement_timeout = '60s'",
    )
    .await;
    exec_named(
        &tx,
        "advisory-lock",
        &format!("SELECT pg_catalog.pg_advisory_xact_lock({ADVISORY_LOCK})"),
    )
    .await;
    tx
}

async fn expect_sql(client: &Client, name: &str, sql: &str) {
    client
        .batch_execute(sql)
        .await
        .unwrap_or_else(|err| panic!("{name} failed: {}", db_detail(&err)));
}

fn db_detail(err: &tokio_postgres::Error) -> String {
    err.as_db_error()
        .map(|db| db.to_string())
        .unwrap_or_else(|| err.to_string())
}

async fn exec_named(tx: &tokio_postgres::Transaction<'_>, name: &str, sql: &str) {
    tx.batch_execute(sql)
        .await
        .unwrap_or_else(|err| panic!("{name} failed: {}", db_detail(&err)));
}

async fn create_login(client: &Client, password: &str) {
    assert!(password.bytes().all(|byte| byte.is_ascii_hexdigit()));
    client
        .batch_execute(&format!(
            "CREATE ROLE \"{LOGIN}\" LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOBYPASSRLS NOREPLICATION PASSWORD '{password}';\n\
             GRANT CONNECT ON DATABASE \"{DATABASE}\" TO \"{LOGIN}\""
        ))
        .await
        .expect("create login");
}

async fn prepare_shadow(client: &mut Client) -> Shadow {
    let target: i64 = client
        .query_one(
            "SELECT _oo.adopt_target_generation($1, ($2::text)::uuid)",
            &[&SHARD, &OWNER],
        )
        .await
        .expect("adopt")
        .get(0);
    client
        .execute(
            "SELECT _oo.full_import_bind_session($1, ($2::text)::uuid, $3)",
            &[&SHARD, &OWNER, &target],
        )
        .await
        .expect("bind");
    let columns = ["qty", "amount", "seq", "memo", "paid", "secret"];
    for generation in [STALE_GENERATION, CURRENT_GENERATION] {
        register(client, generation, "namespace", "n", NAMESPACE, "", "").await;
        register(client, generation, "table", "t", NAMESPACE, TABLE, "").await;
        for column in columns {
            register(client, generation, "column", "c", NAMESPACE, TABLE, column).await;
        }
    }
    let table = lookup(client, "table", NAMESPACE, TABLE, "").await;
    let qty = lookup(client, "column", NAMESPACE, TABLE, "qty").await;
    let amount = lookup(client, "column", NAMESPACE, TABLE, "amount").await;
    let seq = lookup(client, "column", NAMESPACE, TABLE, "seq").await;
    let memo = lookup(client, "column", NAMESPACE, TABLE, "memo").await;
    let paid = lookup(client, "column", NAMESPACE, TABLE, "paid").await;
    let secret = lookup(client, "column", NAMESPACE, TABLE, "secret").await;
    let schemas = vec![NAMESPACE.to_string()];
    let tables = vec![TABLE.to_string()];
    let physical = vec![table.clone()];
    let open_params: [&(dyn ToSql + Sync); 12] = [
        &SHARD,
        &OWNER,
        &target,
        &SOURCE,
        &CURRENT_GENERATION,
        &CONFIG,
        &TYPE_DIGEST,
        &INPUT_DIGEST,
        &SCHEMA_DIGEST,
        &schemas,
        &tables,
        &physical,
    ];
    let run_id: String = client
        .query_one(
            "SELECT _oo.full_import_open_run($1, ($2::text)::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
            &open_params,
        )
        .await
        .expect("open run")
        .get(0);
    let idents: Vec<String> = columns.into_iter().map(str::to_string).collect();
    let oids = vec![21_i64, 23, 20, 25, 16, 25];
    let typmods = vec![-1_i32, -1, -1, -1, -1, -1];
    let required = vec![true, true, true, false, true, true];
    let index = 0_i32;
    let prepare_params: [&(dyn ToSql + Sync); 14] = [
        &SHARD,
        &OWNER,
        &target,
        &run_id,
        &index,
        &SOURCE,
        &CONFIG,
        &CURRENT_GENERATION,
        &NAMESPACE,
        &TABLE,
        &idents,
        &oids,
        &typmods,
        &required,
    ];
    let prepared: String = client
        .query_one(
            "SELECT _oo.full_import_prepare_shadow($1, ($2::text)::uuid, $3, ($4::text)::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
            &prepare_params,
        )
        .await
        .expect("prepare shadow")
        .get(0);
    assert_eq!(prepared, table);
    insert_rows(
        client,
        &RowNames {
            table: &table,
            qty: &qty,
            amount: &amount,
            seq: &seq,
            memo: &memo,
            paid: &paid,
            secret: &secret,
        },
    )
    .await;
    Shadow {
        table,
        qty,
        amount,
        seq,
        memo,
        paid,
        secret,
    }
}

struct FixtureRow<'a> {
    pos: &'a str,
    qty: i16,
    amount: i32,
    seq: i64,
    memo: Option<&'a str>,
    paid: bool,
    secret: &'a str,
}

struct RowNames<'a> {
    table: &'a str,
    qty: &'a str,
    amount: &'a str,
    seq: &'a str,
    memo: &'a str,
    paid: &'a str,
    secret: &'a str,
}

async fn insert_rows(client: &mut Client, names: &RowNames<'_>) {
    let sql = format!(
        "INSERT INTO \"_oo\".{table} (\"_oo_pos\", {qty}, {amount}, {seq}, {memo}, {paid}, {secret}) \
         VALUES (($1::text)::numeric, $2, $3, $4, $5, $6, $7)",
        table = quote_physical(names.table, "pt_"),
        qty = quote_physical(names.qty, "pc_"),
        amount = quote_physical(names.amount, "pc_"),
        seq = quote_physical(names.seq, "pc_"),
        memo = quote_physical(names.memo, "pc_"),
        paid = quote_physical(names.paid, "pc_"),
        secret = quote_physical(names.secret, "pc_"),
    );
    let rows = [
        FixtureRow {
            pos: "1",
            qty: 2,
            amount: 10,
            seq: 100,
            memo: None,
            paid: false,
            secret: "seal-a",
        },
        FixtureRow {
            pos: "2",
            qty: 5,
            amount: 20,
            seq: 200,
            memo: Some("alpha"),
            paid: true,
            secret: "seal-b",
        },
        FixtureRow {
            pos: "3",
            qty: -3,
            amount: -4,
            seq: -5,
            memo: Some("beta"),
            paid: false,
            secret: "seal-c",
        },
    ];
    let tx = client.transaction().await.expect("insert begin");
    for row in rows {
        let params: [&(dyn ToSql + Sync); 7] = [
            &row.pos,
            &row.qty,
            &row.amount,
            &row.seq,
            &row.memo,
            &row.paid,
            &row.secret,
        ];
        tx.execute(&sql, &params).await.expect("insert shadow row");
    }
    tx.commit().await.expect("insert commit");
}

async fn register(
    client: &Client,
    generation: i64,
    level: &str,
    letter: &str,
    namespace: &str,
    table: &str,
    column: &str,
) {
    let payload: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &CONFIG,
        &generation,
        &letter,
        &namespace,
        &table,
        &column,
    ];
    let digest: Vec<u8> = client
        .query_one(
            "SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, $3, $4, $5, $6, $7))",
            &payload,
        )
        .await
        .expect("digest")
        .get(0);
    let bind: [&(dyn ToSql + Sync); 8] = [
        &SOURCE,
        &CONFIG,
        &generation,
        &level,
        &namespace,
        &table,
        &column,
        &digest,
    ];
    client
        .execute(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &bind,
        )
        .await
        .expect("register");
}

async fn lookup(
    client: &Client,
    level: &str,
    namespace: &str,
    table: &str,
    column: &str,
) -> String {
    let params: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &CONFIG,
        &CURRENT_GENERATION,
        &level,
        &namespace,
        &table,
        &column,
    ];
    client
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &params,
        )
        .await
        .expect("lookup")
        .get(0)
}

fn quote_physical(name: &str, prefix: &str) -> String {
    let rest = name.strip_prefix(prefix).unwrap_or("");
    assert_eq!(name.len(), 63, "physical length");
    assert_eq!(rest.len(), 60, "physical body");
    assert!(
        rest.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "physical hex"
    );
    format!("\"{name}\"")
}

fn quote_role(name: &str) -> String {
    assert!(name.starts_with("oo_r_"));
    assert!(name.len() <= 63);
    assert!(
        name.bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_' || byte.is_ascii_digit())
    );
    format!("\"{name}\"")
}
