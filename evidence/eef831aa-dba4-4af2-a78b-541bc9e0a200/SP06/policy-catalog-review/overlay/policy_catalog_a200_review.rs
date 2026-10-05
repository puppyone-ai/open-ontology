//! Independent live review of `load_policy_catalog` on fixed 78a60fde.
//!
//! Runs only when `OO_A200_LIVE=1`. The fixture is `warehouse.parcel` on
//! database `oo_a200`, not the author fixture. Passwords stay in files.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Duration;

use oo_gateway::policy::catalog::{
    CatalogError, CatalogLimits, UnknownRows, load_policy_catalog,
};
use oo_gateway::query_executor::{
    Admission, AuditAdmission, Cell, CompletionRecord, CompletionStatus, DedicatedSession,
    ExecError, SelectRequest, SelectStream,
};
use oo_gateway::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, CompiledPolicy, GuardError, InstallEffect,
    LogicalTable, PiiClass, PolicyCatalog, PolicyIdentity, TlsEndpoint, compile_column_acl,
    connect_require_tls, install_column_acl, rustls_connector,
};
use sha2::{Digest, Sha256};
use tokio_postgres::Client;
use tokio_postgres::types::ToSql;

const HOST: &str = "oo-a200-pg";
const DATABASE: &str = "oo_a200";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a200_login";
const SECRET_DIR: &str = "/run/oo-a200/fixture";
const ORACLE_PATH: &str = "/evidence/live/oracle.txt";
const OWNER: &str = "a2000000-0000-4000-8000-000000000200";
const SHARD: &str = "shard_a200";
const SOURCE: &str = "depot";
const NAMESPACE: &str = "warehouse";
const TABLE: &str = "parcel";
const CONFIG: &str = "a200a200a200a200a200a200a200a200a200a200a200a200a200a200a200a200";
const OTHER: &str = "b200b200b200b200b200b200b200b200b200b200b200b200b200b200b200b200";
const TYPE_DIGEST: &str = "c200c200c200c200c200c200c200c200c200c200c200c200c200c200c200c200";
const INPUT_DIGEST: &str = "d200d200d200d200d200d200d200d200d200d200d200d200d200d200d200d200";
const SCHEMA_DIGEST: &str = "e200e200e200e200e200e200e200e200e200e200e200e200e200e200e200e200";
const STALE_GENERATION: i64 = 3;
const CURRENT_GENERATION: i64 = 9;
const OTHER_GENERATION: i64 = 14;
const ADVISORY_LOCK: i64 = 64_042_006_200;
const PICKER_POLICY: &[&str] = &["sealed", "note", "units", "serial", "weight"];
const PICKER_ORDER: &[&str] = &["units", "weight", "serial", "note", "sealed"];
const PHYSICAL_COLUMNS: &[&str] = &["units", "weight", "serial", "note", "sealed", "token"];

#[tokio::test]
async fn policy_catalog_review_on_a_different_fixture() {
    if std::env::var("OO_A200_LIVE").ok().as_deref() != Some("1") {
        eprintln!("OO_A200_LIVE is unset; the policy catalog review was not run");
        return;
    }
    let mut live = Live::setup().await;
    live.predicate_gap().await;
    live.unknown_rows_and_client().await;
    live.generation_across_configs().await;
    live.estimate_edges().await;
    live.analyze_and_identify().await;
    live.positive_queries().await;
    live.grant_without_classification().await;
    live.revoke_sealed().await;
    live.bounds_and_missing().await;
    live.column_added_and_write().await;
    live.mapping_replay_and_conflict().await;
    live.domain_is_unsupported().await;
    live.finish_counts().await;
}

struct MemoryAudit;

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
        Ok(())
    }

    async fn complete(&mut self, completion: &CompletionRecord) -> Result<(), ExecError> {
        let _ = completion.status();
        Ok(())
    }
}

struct Shadow {
    table: String,
    columns: BTreeMap<String, String>,
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

struct SessionView {
    xid_null: bool,
    xact_idle: bool,
    search_path: String,
}

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
    observer: Client,
    login_pass: String,
    ca: Vec<u8>,
    shadow: Shadow,
    picker: CompiledPolicy,
    sorter: CompiledPolicy,
    picker_role: String,
    sorter_role: String,
    picker_identity: PolicyIdentity,
    sorter_identity: PolicyIdentity,
    picker_catalog: PolicyCatalog,
    sorter_catalog: PolicyCatalog,
    mappings: i64,
}

impl Live {
    async fn setup() -> Self {
        let admin_pass = read_hex("admin.pass");
        let login_pass = read_hex("login.pass");
        let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca.crt");
        let mut oracle = Oracle::new(&admin_pass, &login_pass);
        reset_database(&admin_pass, &ca).await;
        let mut admin = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
        let observer = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
        apply_frozen_schema(&mut admin).await;
        create_login(&admin, &login_pass).await;
        let shadow = prepare_shadow(&mut admin).await;
        let distinct = lookup_at(&admin, OTHER, OTHER_GENERATION, "table", NAMESPACE, TABLE, "")
            .await
            != shadow.table;
        assert!(distinct, "cross-config physical names were not distinct");
        oracle.fact("cross_config_physical", "distinct");
        let picker_input = acl("picker", PICKER_POLICY, CURRENT_GENERATION, CONFIG);
        let sorter_input = acl("sorter", &["weight"], CURRENT_GENERATION, CONFIG);
        let picker_installed = install_column_acl(&mut admin, &picker_input)
            .await
            .expect("install picker");
        let sorter_installed = install_column_acl(&mut admin, &sorter_input)
            .await
            .expect("install sorter");
        assert_eq!(picker_installed.effect, InstallEffect::Installed);
        assert_eq!(sorter_installed.effect, InstallEffect::Installed);
        assert_eq!(picker_installed.grantor, ADMIN);
        assert_eq!(picker_installed.physical_table, shadow.table);
        assert_eq!(sorter_installed.physical_table, shadow.table);
        oracle.fact("grantor", &picker_installed.grantor);
        oracle.fact("install", "both_roles");
        let picker = compile_column_acl(&picker_input).expect("compile picker");
        let sorter = compile_column_acl(&sorter_input).expect("compile sorter");
        let mappings = mapping_count(&admin).await;
        let picker_role = picker_installed.role;
        let sorter_role = sorter_installed.role;
        let picker_identity = identity(&picker_role, CONFIG);
        let sorter_identity = identity(&sorter_role, CONFIG);
        Self {
            oracle,
            admin,
            observer,
            login_pass,
            ca,
            shadow,
            picker_role,
            sorter_role,
            picker_identity,
            sorter_identity,
            picker,
            sorter,
            picker_catalog: empty_catalog(),
            sorter_catalog: empty_catalog(),
            mappings,
        }
    }

    async fn predicate_gap(&mut self) {
        self.admin
            .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .await
            .expect("open read only");
        let open = session_view(&self.admin, &self.observer).await;
        self.oracle.fact(
            "open_rr_xid_null",
            if open.xid_null { "true" } else { "false" },
        );
        self.oracle.fact(
            "open_rr_xact_idle",
            if open.xact_idle { "true" } else { "false" },
        );
        assert!(open.xid_null, "open read-only assigned an xid");
        assert!(!open.xact_idle, "open read-only had no xact_start");
        self.admin.batch_execute("ROLLBACK").await.expect("rollback gap");
        self.expect_returned("after_gap_rollback").await;
    }

    async fn unknown_rows_and_client(&mut self) {
        let bits = self.reltuples().await.to_bits();
        assert_eq!(bits, (-1.0f32).to_bits(), "fresh reltuples was not -1");
        self.oracle.fact("reltuples_before", "-1");
        let before = self.stats().await;
        let err = self
            .load_role(true, UnknownRows::Refuse, 1_000_000)
            .await
            .expect_err("refuse");
        assert_eq!(err, CatalogError::UnknownRows);
        self.expect_returned("after_refuse").await;
        self.assert_quiet(before, "refuse").await;
        let zero = self
            .load_role(true, UnknownRows::Zero, 1_000_000)
            .await
            .expect("zero");
        assert_eq!(zero.relations[0].estimated_rows, 0);
        assert_eq!(names(&zero), PICKER_ORDER);
        assert_eq!(zero.relations[0].pending_unclassified, 2);
        self.expect_returned("after_zero").await;
        let saturated = self
            .load_role(true, UnknownRows::Saturate, 10)
            .await
            .expect("saturate");
        assert_eq!(saturated.relations[0].estimated_rows, u64::MAX);
        self.expect_returned("after_saturate").await;
        self.oracle.fact("unknown_rows", "refuse_zero_saturate");
        let mut session = self.open_session().await;
        assert_eq!(session.bound_role(), None);
        let star = self
            .run(
                &mut session,
                &saturated,
                &self.picker_identity,
                &self.picker_role,
                "SELECT * FROM warehouse.parcel",
            )
            .await;
        assert!(
            matches!(star, Err(ExecError::Guard(err)) if matches!(err.as_ref(), GuardError::CostTooHigh { .. })),
            "saturate star was not CostTooHigh"
        );
        let bound = session.bound_role();
        assert_eq!(bound.as_deref(), Some(self.picker_role.as_str()));
        self.oracle.fact("bound_role_after_guard", "picker");
        let write = self
            .run(
                &mut session,
                &saturated,
                &self.picker_identity,
                &self.picker_role,
                "INSERT INTO warehouse.parcel (weight) VALUES (1)",
            )
            .await;
        assert!(matches!(
            write,
            Err(ExecError::Guard(err)) if matches!(err.as_ref(), GuardError::Write)
        ));
        assert_eq!(session.bound_role().as_deref(), Some(self.picker_role.as_str()));
        let reuse = self
            .run(
                &mut session,
                &saturated,
                &self.sorter_identity,
                &self.sorter_role,
                "SELECT weight FROM warehouse.parcel",
            )
            .await;
        assert!(matches!(reuse, Err(ExecError::RoleReuse)));
        self.oracle.fact("role_reuse_after_guard", "refused");
        session.close();
    }

    async fn generation_across_configs(&mut self) {
        let picker_role = self.picker_role.clone();
        let stale = compile_column_acl(&acl("picker", PICKER_POLICY, STALE_GENERATION, CONFIG))
            .expect("stale compile");
        let err = self
            .load_compiled(&identity(&picker_role, CONFIG), &[stale], UnknownRows::Zero, 1_000_000)
            .await
            .expect_err("same digest stale");
        assert_eq!(err, CatalogError::StaleGeneration);
        self.expect_returned("after_same_digest_stale").await;
        self.oracle.fact("same_digest_generation_3", "stale");

        let ahead = compile_column_acl(&acl("picker", PICKER_POLICY, OTHER_GENERATION, CONFIG))
            .expect("ahead compile");
        let err = self
            .load_compiled(&identity(&picker_role, CONFIG), &[ahead], UnknownRows::Zero, 1_000_000)
            .await
            .expect_err("same digest higher");
        assert_eq!(err, CatalogError::StaleGeneration);
        self.oracle.fact("same_digest_generation_14", "stale");

        let other_low = compile_column_acl(&acl("picker", &["weight"], CURRENT_GENERATION, OTHER))
            .expect("other low");
        let err = self
            .load_compiled(
                &identity(&picker_role, OTHER),
                &[other_low],
                UnknownRows::Zero,
                1_000_000,
            )
            .await
            .expect_err("other digest at config generation");
        assert_eq!(err, CatalogError::StaleGeneration);
        self.oracle.fact("other_digest_generation_9", "stale");

        let other_now = compile_column_acl(&acl("picker", &["weight"], OTHER_GENERATION, OTHER))
            .expect("other now");
        let err = self
            .load_compiled(
                &identity(&picker_role, OTHER),
                &[other_now],
                UnknownRows::Zero,
                1_000_000,
            )
            .await
            .expect_err("other current");
        assert_eq!(err, CatalogError::MappingAbsent);
        self.expect_returned("after_other_current").await;
        self.oracle.fact("other_digest_generation_14", "mapping_absent");

        let kept = self
            .load_role(true, UnknownRows::Zero, 1_000_000)
            .await
            .expect("config 9 still current");
        assert_eq!(kept.identity.policy_id, CONFIG);
        assert_eq!(kept.relations[0].physical_name, self.shadow.table);
        self.oracle.fact("config_generation_9_after_other", "loaded");
        self.oracle.fact("max_generation_scope", "source_and_digest");
    }

    async fn estimate_edges(&mut self) {
        self.set_reltuples(2.5).await;
        let half = self
            .load_role(true, UnknownRows::Refuse, 1_000_000)
            .await
            .expect("half");
        assert_eq!(half.relations[0].estimated_rows, 3);
        self.oracle.fact("reltuples_2_5", "3");
        self.set_reltuples(-2.0).await;
        let err = self
            .load_role(true, UnknownRows::Zero, 1_000_000)
            .await
            .expect_err("negative");
        assert_eq!(err, CatalogError::EstimateUnusable);
        self.expect_returned("after_negative_estimate").await;
        self.oracle.fact("reltuples_neg2", "unusable");
        self.set_reltuples(f32::NAN).await;
        let err = self
            .load_role(true, UnknownRows::Saturate, 1_000_000)
            .await
            .expect_err("nan");
        assert_eq!(err, CatalogError::EstimateUnusable);
        self.oracle.fact("reltuples_nan", "unusable");
        self.set_reltuples(f32::INFINITY).await;
        let err = self
            .load_role(true, UnknownRows::Zero, 1_000_000)
            .await
            .expect_err("infinity");
        assert_eq!(err, CatalogError::EstimateUnusable);
        self.oracle.fact("reltuples_infinity", "unusable");
        self.set_reltuples(-1.0).await;
        let again = self.reltuples().await.to_bits();
        assert_eq!(again, (-1.0f32).to_bits());
        let err = self
            .load_role(true, UnknownRows::Refuse, 1_000_000)
            .await
            .expect_err("refuse again");
        assert_eq!(err, CatalogError::UnknownRows);
        let zero = self
            .load_role(false, UnknownRows::Zero, 1_000_000)
            .await
            .expect("sorter zero");
        assert_eq!(zero.relations[0].estimated_rows, 0);
        assert_eq!(names(&zero), vec!["weight".to_string()]);
        assert_eq!(zero.relations[0].pending_unclassified, 6);
        self.oracle.fact("sorter_pending_unknown", "6");
    }

    async fn analyze_and_identify(&mut self) {
        let table = quote_physical(&self.shadow.table, "pt_");
        let analyze = format!("ANALYZE \"_oo\".{table}");
        self.admin.batch_execute(&analyze).await.expect("analyze");
        let estimate = self.reltuples().await;
        let before = self.stats().await;
        let picker = self
            .load_role(true, UnknownRows::Refuse, 1_000_000)
            .await
            .expect("picker analyzed");
        let sorter = self
            .load_role(false, UnknownRows::Refuse, 1_000_000)
            .await
            .expect("sorter analyzed");
        self.assert_quiet(before, "analyze_load").await;
        self.expect_returned("after_analyzed_load").await;
        let rounded = round_rows(estimate);
        assert_eq!(picker.relations[0].estimated_rows, rounded);
        assert_eq!(sorter.relations[0].estimated_rows, rounded);
        self.oracle.fact("estimated_rows", &rounded.to_string());
        assert_eq!(names(&picker), PICKER_ORDER);
        assert_ne!(names(&picker), PICKER_POLICY);
        assert_eq!(picker.relations[0].pending_unclassified, 2);
        assert_eq!(names(&sorter), vec!["weight".to_string()]);
        assert_eq!(sorter.relations[0].pending_unclassified, 6);
        assert!(picker.null_test_allowed);
        assert_eq!(picker.functions.len(), 1);
        assert!(picker.functions[0].aggregate_star);
        assert!(picker.functions[0].argument_type_oids.is_empty());
        assert_eq!(picker.operators.len(), 54);
        self.assert_mapping(&picker).await;
        self.assert_types(&picker).await;
        self.assert_operators(&picker).await;
        self.assert_absent(&picker).await;
        self.assert_role_flags().await;
        self.picker_catalog = picker;
        self.sorter_catalog = sorter;
        self.oracle.fact("identity_order_oids", "matched");
    }

    async fn positive_queries(&mut self) {
        let admin = self.admin_star().await;
        let mut picker = self.open_session().await;
        let star = self
            .query(
                &mut picker,
                &self.picker_catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT * FROM warehouse.parcel ORDER BY weight",
            )
            .await;
        assert_eq!(star, admin);
        assert_eq!(star.len(), 3);
        let summary = picker.summary().expect("summary");
        assert_eq!(summary.status(), CompletionStatus::Succeeded);
        assert!(summary.rollback_confirmed());
        assert!(!summary.socket_dropped());
        assert!(summary.end_audit_ok());
        self.oracle.fact("star_matches_admin", "3");
        self.oracle.fact("query_rollback_confirmed", "true");
        self.oracle.fact("end_audit", "memory_ok");
        let count_true = self
            .query(
                &mut picker,
                &self.picker_catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT count(*) FROM warehouse.parcel WHERE sealed = true",
            )
            .await;
        assert_eq!(count_true, vec![vec![Cell::Int("1".to_string())]]);
        let count_ne = self
            .query(
                &mut picker,
                &self.picker_catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT count(*) FROM warehouse.parcel WHERE sealed <> true",
            )
            .await;
        assert_eq!(count_ne, vec![vec![Cell::Int("2".to_string())]]);
        let count_ge = self
            .query(
                &mut picker,
                &self.picker_catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT count(*) FROM warehouse.parcel WHERE weight >= 0",
            )
            .await;
        assert_eq!(count_ge, vec![vec![Cell::Int("2".to_string())]]);
        let count_le = self
            .query(
                &mut picker,
                &self.picker_catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT count(*) FROM warehouse.parcel WHERE weight <= 0",
            )
            .await;
        assert_eq!(count_le, vec![vec![Cell::Int("2".to_string())]]);
        let by_weight = self
            .query(
                &mut picker,
                &self.picker_catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT units FROM warehouse.parcel WHERE weight = 15",
            )
            .await;
        assert_eq!(by_weight, vec![vec![Cell::Int("0".to_string())]]);
        let by_units = self
            .query(
                &mut picker,
                &self.picker_catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT weight FROM warehouse.parcel WHERE units > 1",
            )
            .await;
        assert_eq!(by_units, vec![vec![Cell::Int("0".to_string())]]);
        let by_low_units = self
            .query(
                &mut picker,
                &self.picker_catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT weight FROM warehouse.parcel WHERE units = -32768",
            )
            .await;
        assert_eq!(by_low_units, vec![vec![Cell::Int("-40".to_string())]]);
        let by_serial = self
            .query(
                &mut picker,
                &self.picker_catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT serial FROM warehouse.parcel WHERE serial < 0",
            )
            .await;
        assert_eq!(
            by_serial,
            vec![vec![Cell::Int(i64::MIN.to_string())]]
        );
        let by_note = self
            .query(
                &mut picker,
                &self.picker_catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT note FROM warehouse.parcel WHERE note = 'lane'",
            )
            .await;
        assert_eq!(by_note, vec![vec![Cell::Text("lane".to_string())]]);
        let by_null = self
            .query(
                &mut picker,
                &self.picker_catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT sealed FROM warehouse.parcel WHERE note IS NULL",
            )
            .await;
        assert_eq!(by_null, vec![vec![Cell::Bool(false)]]);
        picker.close();
        let mut sorter = self.open_session().await;
        let weights = self
            .query(
                &mut sorter,
                &self.sorter_catalog,
                &self.sorter_identity,
                &self.sorter_role,
                "SELECT weight FROM warehouse.parcel ORDER BY weight",
            )
            .await;
        assert_eq!(
            weights,
            vec![
                vec![Cell::Int("-40".to_string())],
                vec![Cell::Int("0".to_string())],
                vec![Cell::Int("15".to_string())],
            ]
        );
        let hidden = self
            .run(
                &mut sorter,
                &self.sorter_catalog,
                &self.sorter_identity,
                &self.sorter_role,
                "SELECT units FROM warehouse.parcel",
            )
            .await
            .expect_err("sorter units");
        let text = hidden.to_string();
        assert!(matches!(
            hidden,
            ExecError::Guard(err) if matches!(err.as_ref(), GuardError::ColumnNotAllowed { .. })
        ));
        assert!(!text.contains("units"));
        sorter.close();
        self.oracle.fact("positive_both_roles", "matched");
    }

    async fn grant_without_classification(&mut self) {
        let token = self.shadow.columns.get("token").expect("token").clone();
        let before = self.privilege(&self.picker_role, &token).await;
        assert!(!before, "token was granted by install");
        let role = self.picker_role.clone();
        self.grant_column(&token, &role, true).await;
        assert!(self.privilege(&self.picker_role, &token).await);
        let before_stats = self.stats().await;
        let catalog = self
            .load_role(true, UnknownRows::Refuse, 1_000_000)
            .await
            .expect("load after token grant");
        self.assert_quiet(before_stats, "token_grant_load").await;
        assert_eq!(catalog.relations[0].pending_unclassified, 2);
        assert!(!names(&catalog).iter().any(|name| name == "token"));
        let mut session = self.open_session().await;
        let err = self
            .run(
                &mut session,
                &catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT token FROM warehouse.parcel",
            )
            .await
            .expect_err("token guard");
        let text = err.to_string();
        assert!(matches!(
            err,
            ExecError::Guard(inner) if matches!(inner.as_ref(), GuardError::ColumnNotAllowed { .. })
        ));
        assert!(!text.contains("token"));
        assert!(!text.contains(&token));
        session.close();
        let role = self.picker_role.clone();
        self.grant_column(&token, &role, false).await;
        assert!(!self.privilege(&self.picker_role, &token).await);
        self.direct_denial(&token, "direct_token").await;
        self.oracle.fact("grant_without_classification", "omitted");
    }

    async fn revoke_sealed(&mut self) {
        let sealed = self.shadow.columns.get("sealed").expect("sealed").clone();
        let snapshot = self.picker_catalog.clone();
        let role = self.picker_role.clone();
        self.grant_column(&sealed, &role, false).await;
        assert!(!self.privilege(&self.picker_role, &sealed).await);
        let before = self.stats().await;
        let catalog = self
            .load_role(true, UnknownRows::Refuse, 1_000_000)
            .await
            .expect("load revoked");
        self.assert_quiet(before, "revoke_load").await;
        assert_eq!(catalog.relations[0].pending_unclassified, 3);
        assert!(!names(&catalog).iter().any(|name| name == "sealed"));
        let mut session = self.open_session().await;
        let err = self
            .run(
                &mut session,
                &catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT sealed FROM warehouse.parcel",
            )
            .await
            .expect_err("sealed guard");
        assert!(!err.to_string().contains("sealed"));
        session.close();
        self.direct_denial(&sealed, "direct_sealed").await;
        let mut stale = self.open_session().await;
        let db = self
            .run(
                &mut stale,
                &snapshot,
                &self.picker_identity,
                &self.picker_role,
                "SELECT sealed FROM warehouse.parcel",
            )
            .await
            .expect_err("stale catalog sealed");
        assert_eq!(db.sqlstate(), Some("42501"));
        assert!(!db.to_string().contains("sealed"));
        drop_stream_idle(&mut stale).await;
        let summary = stale.summary().expect("revoke summary");
        assert_eq!(summary.status(), CompletionStatus::Failed);
        self.oracle.fact(
            "revoke_executor_rollback",
            if summary.rollback_confirmed() { "true" } else { "false" },
        );
        self.oracle.fact(
            "revoke_executor_open",
            if stale.is_open() { "true" } else { "false" },
        );
        stale.close();
        let role = self.picker_role.clone();
        self.grant_column(&sealed, &role, true).await;
        let restored = self
            .load_role(true, UnknownRows::Refuse, 1_000_000)
            .await
            .expect("restore sealed");
        assert_eq!(names(&restored), PICKER_ORDER);
        assert_eq!(restored.relations[0].pending_unclassified, 2);
        self.picker_catalog = restored;
        self.oracle.fact("revoke_sealed", "42501");
    }

    async fn bounds_and_missing(&mut self) {
        let narrow = CatalogLimits {
            max_tables: 4,
            max_columns: 6,
            max_functions: 1,
            max_operators: 54,
            large_table_row_threshold: 1_000_000,
            unknown_rows: UnknownRows::Refuse,
        };
        let err = load_policy_catalog(
            &mut self.admin,
            &self.picker_identity,
            std::slice::from_ref(&self.picker),
            &narrow,
        )
        .await
        .expect_err("column bound");
        assert_eq!(err, CatalogError::LimitExceeded);
        self.expect_returned("after_column_bound").await;
        let exact = CatalogLimits {
            max_columns: 7,
            ..narrow
        };
        let ok = load_policy_catalog(
            &mut self.admin,
            &self.picker_identity,
            std::slice::from_ref(&self.picker),
            &exact,
        )
        .await
        .expect("exact column bound");
        assert_eq!(ok.relations[0].columns.len(), 5);
        let operators = CatalogLimits {
            max_operators: 53,
            max_columns: 64,
            ..narrow
        };
        let err = load_policy_catalog(
            &mut self.admin,
            &self.picker_identity,
            std::slice::from_ref(&self.picker),
            &operators,
        )
        .await
        .expect_err("operator bound");
        assert_eq!(err, CatalogError::InvalidLimit);
        self.expect_returned("after_invalid_limit").await;
        let picker_identity = self.picker_identity.clone();
        let empty = self
            .load_compiled(&picker_identity, &[], UnknownRows::Refuse, 1_000_000)
            .await
            .expect_err("empty");
        assert_eq!(empty, CatalogError::EmptyInput);
        let both = [self.picker.clone(), self.sorter.clone()];
        let mixed = self
            .load_compiled(&picker_identity, &both, UnknownRows::Refuse, 1_000_000)
            .await
            .expect_err("mixed");
        assert_eq!(mixed, CatalogError::MixedIdentity);
        let sorter_identity = self.sorter_identity.clone();
        let picker_policy = self.picker.clone();
        let mismatch = self
            .load_compiled(
                &sorter_identity,
                std::slice::from_ref(&picker_policy),
                UnknownRows::Refuse,
                1_000_000,
            )
            .await
            .expect_err("mismatch");
        assert_eq!(mismatch, CatalogError::IdentityMismatch);
        let ghost = compile_column_acl(&acl("ghost", &["weight"], CURRENT_GENERATION, CONFIG))
            .expect("ghost");
        let missing = self
            .load_compiled(
                &identity("oo_r_ghost", CONFIG),
                &[ghost],
                UnknownRows::Refuse,
                1_000_000,
            )
            .await
            .expect_err("missing role");
        assert_eq!(missing, CatalogError::MissingRole);
        self.expect_returned("after_missing_role").await;
        assert!(!self.role_present("oo_r_ghost").await);
        let absent = compile_column_acl(&acl("picker", &["ghostcol"], CURRENT_GENERATION, CONFIG))
            .expect("ghost column");
        let picker_identity = self.picker_identity.clone();
        let err = self
            .load_compiled(
                &picker_identity,
                &[absent],
                UnknownRows::Refuse,
                1_000_000,
            )
            .await
            .expect_err("missing mapping");
        assert_eq!(err, CatalogError::MappingAbsent);
        assert!(!err.to_string().contains("ghostcol"));
        self.expect_returned("after_missing_mapping").await;
        self.oracle.fact("bounds_missing_mixed", "refused");
    }

    async fn column_added_and_write(&mut self) {
        let table = quote_physical(&self.shadow.table, "pt_");
        let add = format!("ALTER TABLE \"_oo\".{table} ADD COLUMN \"extra\" text");
        self.admin.batch_execute(&add).await.expect("add column");
        let before = self.stats().await;
        let catalog = self
            .load_role(true, UnknownRows::Refuse, 1_000_000)
            .await
            .expect("load extra");
        self.assert_quiet(before, "extra_load").await;
        assert_eq!(catalog.relations[0].pending_unclassified, 3);
        assert!(!names(&catalog).iter().any(|name| name == "extra"));
        let mut session = self.open_session().await;
        let err = self
            .run(
                &mut session,
                &catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT extra FROM warehouse.parcel",
            )
            .await
            .expect_err("extra guard");
        assert!(!err.to_string().contains("extra"));
        let sleep = self
            .run(
                &mut session,
                &catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT pg_catalog.pg_sleep(0)",
            )
            .await
            .expect_err("sleep");
        assert!(matches!(
            sleep,
            ExecError::Guard(inner) if matches!(inner.as_ref(), GuardError::FunctionNotAllowed)
        ));
        let count_col = self
            .run(
                &mut session,
                &catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT count(weight) FROM warehouse.parcel",
            )
            .await
            .expect_err("count column");
        assert!(matches!(
            count_col,
            ExecError::Guard(inner) if matches!(inner.as_ref(), GuardError::FunctionNotAllowed)
        ));
        let plus = self
            .run(
                &mut session,
                &catalog,
                &self.picker_identity,
                &self.picker_role,
                "SELECT weight + 1 FROM warehouse.parcel",
            )
            .await
            .expect_err("plus");
        assert!(matches!(
            plus,
            ExecError::Guard(inner) if matches!(inner.as_ref(), GuardError::FunctionNotAllowed)
        ));
        session.close();
        self.direct_sql(
            &format!("SELECT \"extra\" FROM \"_oo\".{table}"),
            "direct_extra",
        )
        .await;
        self.direct_sql(
            &format!("INSERT INTO \"_oo\".{table} (\"_oo_pos\") VALUES (9)"),
            "direct_insert",
        )
        .await;
        self.oracle.fact("extra_and_write", "42501");
        self.picker_catalog = catalog;
    }

    async fn mapping_replay_and_conflict(&mut self) {
        let replay = register_again(&self.admin).await;
        assert_eq!(replay, "replay");
        let state = conflict_register(&self.admin).await;
        assert_eq!(state, "OO022");
        self.expect_returned("after_mapping_conflict").await;
        let count = mapping_count(&self.admin).await;
        assert_eq!(count, self.mappings);
        self.oracle.fact("mapping_replay", "replay");
        self.oracle.fact("mapping_conflict", "OO022");
    }

    async fn domain_is_unsupported(&mut self) {
        let table = quote_physical(&self.shadow.table, "pt_");
        let note = quote_physical(self.shadow.columns.get("note").expect("note"), "pc_");
        let alter = format!(
            "CREATE DOMAIN public.a200_note AS text; ALTER TABLE \"_oo\".{table} ALTER COLUMN {note} TYPE public.a200_note"
        );
        self.admin.batch_execute(&alter).await.expect("domain");
        let before = self.stats().await;
        let err = self
            .load_role(true, UnknownRows::Refuse, 1_000_000)
            .await
            .expect_err("domain load");
        self.assert_quiet(before, "domain_load").await;
        assert_eq!(err, CatalogError::UnsupportedType);
        let text = err.to_string();
        assert!(!text.contains("note"));
        assert!(!text.contains("a200_note"));
        self.expect_returned("after_domain").await;
        self.oracle.fact("domain_note", "unsupported");
    }

    async fn finish_counts(&mut self) {
        let table = quote_physical(&self.shadow.table, "pt_");
        let sql = format!("SELECT count(*)::int8 FROM \"_oo\".{table}");
        let count: i64 = self.admin.query_one(&sql, &[]).await.expect("row count").get(0);
        assert_eq!(count, 3);
        assert_eq!(mapping_count(&self.admin).await, self.mappings);
        self.oracle.fact("user_rows", "3");
        self.oracle.fact("mapping_count_delta", "0");
        self.oracle.fact("done", "ok");
    }

    async fn load_role(
        &mut self,
        picker: bool,
        unknown: UnknownRows,
        threshold: u64,
    ) -> Result<PolicyCatalog, CatalogError> {
        let identity = if picker {
            self.picker_identity.clone()
        } else {
            self.sorter_identity.clone()
        };
        let policy = if picker {
            self.picker.clone()
        } else {
            self.sorter.clone()
        };
        self.load_compiled(&identity, &[policy], unknown, threshold).await
    }

    async fn load_compiled(
        &mut self,
        identity: &PolicyIdentity,
        policies: &[CompiledPolicy],
        unknown: UnknownRows,
        threshold: u64,
    ) -> Result<PolicyCatalog, CatalogError> {
        let limits = CatalogLimits {
            max_tables: 4,
            max_columns: 64,
            max_functions: 1,
            max_operators: 54,
            large_table_row_threshold: threshold,
            unknown_rows: unknown,
        };
        load_policy_catalog(&mut self.admin, identity, policies, &limits).await
    }

    async fn expect_returned(&mut self, key: &str) {
        let view = session_view(&self.admin, &self.observer).await;
        assert!(view.xid_null, "{key} xid");
        assert!(view.xact_idle, "{key} xact");
        assert_ne!(view.search_path, "pg_catalog");
        assert_ne!(view.search_path, "\"pg_catalog\"");
        self.oracle.fact(key, "idle");
    }

    async fn set_reltuples(&mut self, value: f32) {
        let oid = self.relation_oid().await;
        self.admin
            .execute(
                "UPDATE pg_catalog.pg_class SET reltuples = $1 WHERE oid = ($2::int4)::oid",
                &[&value, &oid],
            )
            .await
            .expect("set reltuples");
    }

    async fn reltuples(&self) -> f32 {
        let oid = self.relation_oid().await;
        self.admin
            .query_one(
                "SELECT reltuples FROM pg_catalog.pg_class WHERE oid = ($1::int4)::oid",
                &[&oid],
            )
            .await
            .expect("reltuples")
            .get(0)
    }

    async fn relation_oid(&self) -> i32 {
        self.admin
            .query_one(
                "SELECT relation.oid::int4 FROM pg_catalog.pg_class AS relation \
                 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
                 WHERE namespace.nspname = '_oo' AND relation.relname = $1",
                &[&self.shadow.table],
            )
            .await
            .expect("relation oid")
            .get(0)
    }

    async fn assert_quiet(&mut self, before: Stats, key: &str) {
        let after = self.stats().await;
        assert_eq!(after.seq_scan - before.seq_scan, 0, "{key} seq");
        assert_eq!(after.idx_scan - before.idx_scan, 0, "{key} idx");
        assert_eq!(after.inserted - before.inserted, 0, "{key} ins");
        assert_eq!(after.updated - before.updated, 0, "{key} upd");
        assert_eq!(after.deleted - before.deleted, 0, "{key} del");
        assert_eq!(after.hot_updated - before.hot_updated, 0, "{key} hot");
        assert_eq!(mapping_count(&self.admin).await, self.mappings, "{key} map");
        self.oracle.fact(&format!("{key}_scan"), "0");
    }

    async fn stats(&self) -> Stats {
        let rows = self
            .admin
            .query(
                "SELECT COALESCE(seq_scan, 0)::int8, COALESCE(idx_scan, 0)::int8, \
                        COALESCE(n_tup_ins, 0)::int8, COALESCE(n_tup_upd, 0)::int8, \
                        COALESCE(n_tup_del, 0)::int8, COALESCE(n_tup_hot_upd, 0)::int8 \
                 FROM pg_catalog.pg_stat_user_tables WHERE relname = $1",
                &[&self.shadow.table],
            )
            .await
            .expect("stats");
        if rows.is_empty() {
            return Stats {
                seq_scan: 0,
                idx_scan: 0,
                inserted: 0,
                updated: 0,
                deleted: 0,
                hot_updated: 0,
            };
        }
        let row = &rows[0];
        Stats {
            seq_scan: row.get(0),
            idx_scan: row.get(1),
            inserted: row.get(2),
            updated: row.get(3),
            deleted: row.get(4),
            hot_updated: row.get(5),
        }
    }

    async fn assert_mapping(&self, catalog: &PolicyCatalog) {
        let relation = &catalog.relations[0];
        assert_eq!(relation.schema, NAMESPACE);
        assert_eq!(relation.name, TABLE);
        assert_eq!(relation.physical_schema, "_oo");
        assert_eq!(relation.physical_name, self.shadow.table);
        let looked = lookup_at(
            &self.admin,
            CONFIG,
            CURRENT_GENERATION,
            "table",
            NAMESPACE,
            TABLE,
            "",
        )
        .await;
        assert_eq!(relation.physical_name, looked);
        for column in &relation.columns {
            let physical = self.shadow.columns.get(&column.name).expect("column map");
            assert_eq!(&column.physical_name, physical);
            let stored = lookup_at(
                &self.admin,
                CONFIG,
                CURRENT_GENERATION,
                "column",
                NAMESPACE,
                TABLE,
                &column.name,
            )
            .await;
            assert_eq!(column.physical_name, stored);
            assert_ne!(column.type_oid, 0);
        }
    }

    async fn assert_types(&mut self, catalog: &PolicyCatalog) {
        let expected = ["int2", "int4", "int8", "text", "bool"];
        assert_eq!(catalog.relations[0].columns.len(), expected.len());
        for (column, type_name) in catalog.relations[0].columns.iter().zip(expected) {
            let physical = &column.physical_name;
            let attribute = self.attribute_type(physical).await;
            let named = self.type_oid(type_name).await;
            assert_eq!(column.type_oid, attribute);
            assert_eq!(column.type_oid, named);
            self.oracle.fact(&format!("oid_{type_name}"), &named.to_string());
        }
        assert_eq!(catalog.literal_types.integer, Some(self.type_oid("int4").await));
        assert_eq!(catalog.literal_types.text, Some(self.type_oid("text").await));
        assert_eq!(catalog.literal_types.boolean, Some(self.type_oid("bool").await));
    }

    async fn assert_operators(&mut self, catalog: &PolicyCatalog) {
        let mut seen = BTreeSet::new();
        for operator in &catalog.operators {
            let left = self.type_name(operator.left_type_oid).await;
            let right = self.type_name(operator.right_type_oid).await;
            let looked = self.operator_oids(&operator.name, &left, &right).await;
            assert_eq!(looked, (operator.oid, operator.procedure_oid));
            assert_eq!(operator.schema, "pg_catalog");
            seen.insert((operator.name.clone(), left, right));
        }
        let signatures = [
            ("bool", "bool"),
            ("int2", "int2"),
            ("int2", "int4"),
            ("int4", "int2"),
            ("int4", "int4"),
            ("int4", "int8"),
            ("int8", "int4"),
            ("int8", "int8"),
            ("text", "text"),
        ];
        let ops = ["<", "<=", "<>", "=", ">", ">="];
        for (left, right) in signatures {
            for name in ops {
                assert!(seen.contains(&(name.to_string(), left.to_string(), right.to_string())));
            }
        }
        assert_eq!(seen.len(), 54);
        let count_oid = self.count_star_oid().await;
        assert_eq!(catalog.functions[0].oid, count_oid);
        assert_eq!(catalog.functions[0].name, "count");
        self.oracle.fact("operator_count", "54");
        self.oracle.fact("count_star_oid", &count_oid.to_string());
    }

    async fn assert_absent(&mut self, catalog: &PolicyCatalog) {
        let sleep = self.proc_oids("pg_sleep").await;
        assert!(!sleep.is_empty(), "pg_sleep was not present to reject");
        for oid in sleep {
            assert!(catalog.functions.iter().all(|function| function.oid != oid));
        }
        let count_any = self.count_any_oid().await;
        assert!(catalog.functions.iter().all(|function| function.oid != count_any));
        let plus = self.operator_oids("+", "int4", "int4").await.0;
        assert!(catalog.operators.iter().all(|operator| operator.oid != plus));
        let int2 = self.type_oid("int2").await;
        let int8 = self.type_oid("int8").await;
        assert!(catalog.operators.iter().all(|operator| {
            (operator.left_type_oid, operator.right_type_oid) != (int2, int8)
                && (operator.left_type_oid, operator.right_type_oid) != (int8, int2)
        }));
        self.oracle.fact("pg_sleep_admitted", "no");
        self.oracle.fact("count_any_admitted", "no");
        self.oracle.fact("int4_plus_admitted", "no");
        self.oracle.fact("int2_int8_pair", "absent");
    }

    async fn assert_role_flags(&mut self) {
        for role in [self.picker_role.as_str(), self.sorter_role.as_str()] {
            let row = self
                .admin
                .query_one(
                    "SELECT rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, \
                            rolbypassrls, rolreplication \
                     FROM pg_catalog.pg_roles WHERE rolname = $1",
                    &[&role],
                )
                .await
                .expect("role flags");
            let flags: [bool; 7] = [
                row.get(0),
                row.get(1),
                row.get(2),
                row.get(3),
                row.get(4),
                row.get(5),
                row.get(6),
            ];
            assert!(flags.iter().all(|flag| !flag), "role flag set");
        }
        self.oracle.fact("role_flags", "nologin_shape");
    }

    async fn privilege(&self, role: &str, column: &str) -> bool {
        let oid = self.relation_oid().await;
        let attnum = self.attnum(column).await;
        self.admin
            .query_one(
                "SELECT pg_catalog.has_column_privilege(($1::text)::name, ($2::int4)::oid, $3::int2, 'SELECT')",
                &[&role, &oid, &attnum],
            )
            .await
            .expect("privilege")
            .get::<_, Option<bool>>(0)
            .unwrap_or(false)
    }

    async fn attnum(&self, column: &str) -> i16 {
        let oid = self.relation_oid().await;
        self.admin
            .query_one(
                "SELECT attnum FROM pg_catalog.pg_attribute \
                 WHERE attrelid = ($1::int4)::oid AND attname = $2",
                &[&oid, &column],
            )
            .await
            .expect("attnum")
            .get(0)
    }

    fn grant_sql(column: &str, role: &str, grant: bool, table: &str) -> String {
        let verb = if grant { "GRANT" } else { "REVOKE" };
        let direction = if grant { "TO" } else { "FROM" };
        format!(
            "{verb} SELECT ({column}) ON TABLE \"_oo\".{table} {direction} {role}"
        )
    }

    async fn grant_column(&mut self, column: &str, role: &str, grant: bool) {
        let column_sql = quote_physical(column, "pc_");
        let role_sql = quote_role(role);
        let table_sql = quote_physical(&self.shadow.table, "pt_");
        let sql = Self::grant_sql(&column_sql, &role_sql, grant, &table_sql);
        self.admin.batch_execute(&sql).await.expect("column privilege");
    }

    async fn direct_denial(&mut self, column: &str, key: &str) {
        let column_sql = quote_physical(column, "pc_");
        let table_sql = quote_physical(&self.shadow.table, "pt_");
        let sql = format!("SELECT {column_sql} FROM \"_oo\".{table_sql}");
        self.direct_sql(&sql, key).await;
    }

    async fn direct_sql(&mut self, sql: &str, key: &str) {
        let client = connect_db(DATABASE, LOGIN, &self.login_pass, &self.ca).await;
        let set_role = format!("SET ROLE {}", quote_role(&self.picker_role));
        client.batch_execute(&set_role).await.expect("set role");
        let err = client.batch_execute(sql).await.expect_err("direct allowed");
        let state = err.code().map(|code| code.code().to_string()).unwrap_or_default();
        assert_eq!(state, "42501");
        self.oracle.fact(key, &state);
    }

    async fn admin_star(&self) -> Vec<Vec<Cell>> {
        let units = quote_physical(self.shadow.columns.get("units").expect("u"), "pc_");
        let weight = quote_physical(self.shadow.columns.get("weight").expect("w"), "pc_");
        let serial = quote_physical(self.shadow.columns.get("serial").expect("s"), "pc_");
        let note = quote_physical(self.shadow.columns.get("note").expect("n"), "pc_");
        let sealed = quote_physical(self.shadow.columns.get("sealed").expect("b"), "pc_");
        let table = quote_physical(&self.shadow.table, "pt_");
        let sql = format!(
            "SELECT {units}, {weight}, {serial}, {note}, {sealed} FROM \"_oo\".{table} ORDER BY {weight}"
        );
        let rows = self.admin.query(&sql, &[]).await.expect("admin star");
        rows.into_iter().map(admin_star_row).collect()
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

    async fn run(
        &self,
        session: &mut DedicatedSession,
        catalog: &PolicyCatalog,
        identity: &PolicyIdentity,
        role: &str,
        sql: &str,
    ) -> Result<(), ExecError> {
        match session
            .execute_trusted_select(request(identity, catalog, sql, role), MemoryAudit)
            .await
        {
            Err(err) => Err(err),
            Ok(mut stream) => {
                let found = loop {
                    match stream.next_row().await {
                        Ok(Some(_)) => {}
                        Ok(None) => panic!("statement was executed"),
                        Err(err) => break err,
                    }
                };
                drop(stream);
                let _ = session.wait_idle().await;
                Err(found)
            }
        }
    }

    async fn attribute_type(&self, physical: &str) -> u32 {
        let oid = self.relation_oid().await;
        let value: i32 = self
            .admin
            .query_one(
                "SELECT atttypid::int4 FROM pg_catalog.pg_attribute \
                 WHERE attrelid = ($1::int4)::oid AND attname = $2",
                &[&oid, &physical],
            )
            .await
            .expect("attribute type")
            .get(0);
        u32::try_from(value).expect("oid")
    }

    async fn type_oid(&self, name: &str) -> u32 {
        let value: i32 = self
            .admin
            .query_one(
                "SELECT type_row.oid::int4 FROM pg_catalog.pg_type AS type_row \
                 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type_row.typnamespace \
                 WHERE namespace.nspname = 'pg_catalog' AND type_row.typname = $1 AND type_row.typtype = 'b'",
                &[&name],
            )
            .await
            .expect("type oid")
            .get(0);
        u32::try_from(value).expect("oid")
    }

    async fn type_name(&self, oid: u32) -> String {
        let value = i32::try_from(oid).expect("oid fits");
        self.admin
            .query_one(
                "SELECT typname::text FROM pg_catalog.pg_type WHERE oid = ($1::int4)::oid",
                &[&value],
            )
            .await
            .expect("type name")
            .get(0)
    }

    async fn operator_oids(&self, name: &str, left: &str, right: &str) -> (u32, u32) {
        let row = self
            .admin
            .query_one(
                "SELECT operator.oid::int4, operator.oprcode::oid::int4 \
                 FROM pg_catalog.pg_operator AS operator \
                 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = operator.oprnamespace \
                 JOIN pg_catalog.pg_type AS left_type ON left_type.oid = operator.oprleft \
                 JOIN pg_catalog.pg_type AS right_type ON right_type.oid = operator.oprright \
                 JOIN pg_catalog.pg_namespace AS left_ns ON left_ns.oid = left_type.typnamespace \
                 JOIN pg_catalog.pg_namespace AS right_ns ON right_ns.oid = right_type.typnamespace \
                 WHERE namespace.nspname = 'pg_catalog' AND left_ns.nspname = 'pg_catalog' \
                   AND right_ns.nspname = 'pg_catalog' AND operator.oprname = $1 \
                   AND left_type.typname = $2 AND right_type.typname = $3 AND operator.oprkind = 'b'",
                &[&name, &left, &right],
            )
            .await
            .unwrap_or_else(|err| panic!("operator lookup failed: {err}"));
        let oid: i32 = row.get(0);
        let procedure: i32 = row.get(1);
        (
            u32::try_from(oid).expect("operator oid"),
            u32::try_from(procedure).expect("procedure oid"),
        )
    }

    async fn count_star_oid(&self) -> u32 {
        let value: i32 = self
            .admin
            .query_one(
                "SELECT procedure.oid::int4 FROM pg_catalog.pg_proc AS procedure \
                 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = procedure.pronamespace \
                 WHERE namespace.nspname = 'pg_catalog' AND procedure.proname = 'count' \
                   AND procedure.pronargs = 0 AND procedure.prokind = 'a'",
                &[],
            )
            .await
            .expect("count star")
            .get(0);
        u32::try_from(value).expect("oid")
    }

    async fn count_any_oid(&self) -> u32 {
        let rows = self
            .admin
            .query(
                "SELECT procedure.oid::int4 FROM pg_catalog.pg_proc AS procedure \
                 JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = procedure.pronamespace \
                 JOIN pg_catalog.pg_type AS argument ON argument.oid = procedure.proargtypes[0] \
                 WHERE namespace.nspname = 'pg_catalog' AND procedure.proname = 'count' \
                   AND procedure.pronargs = 1 AND procedure.prokind = 'a' \
                   AND argument.typname = 'any'",
                &[],
            )
            .await
            .expect("count any");
        assert_eq!(rows.len(), 1, "count(any) oid");
        u32::try_from(rows[0].get::<_, i32>(0)).expect("oid")
    }

    async fn proc_oids(&self, name: &str) -> Vec<u32> {
        let rows = self
            .admin
            .query(
                "SELECT procedure.oid::int4 FROM pg_catalog.pg_proc AS procedure \
                 WHERE procedure.proname = $1",
                &[&name],
            )
            .await
            .expect("proc oids");
        rows.into_iter()
            .map(|row| u32::try_from(row.get::<_, i32>(0)).expect("oid"))
            .collect()
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

async fn drop_stream_idle(session: &mut DedicatedSession) {
    let _ = session.wait_idle().await;
}

fn names(catalog: &PolicyCatalog) -> Vec<String> {
    catalog.relations[0]
        .columns
        .iter()
        .map(|column| column.name.clone())
        .collect()
}

fn empty_catalog() -> PolicyCatalog {
    PolicyCatalog {
        identity: identity("oo_r_unset", CONFIG),
        relations: Vec::new(),
        functions: Vec::new(),
        operators: Vec::new(),
        literal_types: oo_gateway::LiteralTypes::default(),
        null_test_allowed: false,
        large_table_row_threshold: 0,
    }
}

fn identity(role: &str, policy_id: &str) -> PolicyIdentity {
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
        max_result_bytes: 16_384,
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
    assert!(summary.rollback_confirmed());
    rows
}

fn admin_star_row(row: tokio_postgres::Row) -> Vec<Cell> {
    let units: i16 = row.get(0);
    let weight: i32 = row.get(1);
    let serial: i64 = row.get(2);
    let note: Option<String> = row.get(3);
    let sealed: bool = row.get(4);
    vec![
        Cell::Int(units.to_string()),
        Cell::Int(weight.to_string()),
        Cell::Int(serial.to_string()),
        match note {
            Some(text) => Cell::Text(text),
            None => Cell::Null,
        },
        Cell::Bool(sealed),
    ]
}

fn round_rows(value: f32) -> u64 {
    let rounded = f64::from(value).round();
    assert!(rounded.is_finite() && rounded >= 0.0);
    rounded as u64
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

async fn session_view(target: &Client, observer: &Client) -> SessionView {
    let row = target
        .query_one(
            "SELECT pg_catalog.pg_backend_pid(), \
                    pg_catalog.pg_current_xact_id_if_assigned() IS NULL, \
                    current_setting('search_path')",
            &[],
        )
        .await
        .expect("session probe");
    let pid: i32 = row.get(0);
    let xid_null: bool = row.get(1);
    let search_path: String = row.get(2);
    let seen = observer
        .query_one(
            "SELECT activity.xact_start IS NULL \
             FROM pg_catalog.pg_stat_activity AS activity \
             WHERE activity.pid = $1",
            &[&pid],
        )
        .await
        .expect("observer view");
    SessionView {
        xid_null,
        xact_idle: seen.get(0),
        search_path,
    }
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
    panic!("tls connect failed: {last}");
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
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a200_login') THEN\n\
             DROP ROLE oo_a200_login;\n\
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
    exec_named(&tx, "0003_target_segment_preflight_expand.sql", &preflight.0).await;
    exec_named(&tx, "0004_physical_mapping.sql", &mapping.0).await;
    exec_named(
        &tx,
        "markers-target",
        &marker_sql(&[
            ("target-bootstrap/1", &bootstrap.1, "0000_bootstrap_roles.sql"),
            ("target-fence-receipts/1", &fence.1, "0001_target_fence_expand.sql"),
            (
                "target-fence-receipts/3",
                &preflight.1,
                "0003_target_segment_preflight_expand.sql",
            ),
            ("physical-mapping/v1", &mapping.1, "0004_physical_mapping.sql"),
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
        &marker_sql(&[("full-import/v1", &full_import.1, "0005_full_import_expand.sql")]),
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
        values.push_str(&format!("  ('{version}', '{checksum}', 'expand', '{filename}')"));
    }
    format!("INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values}")
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
    exec_named(&tx, "statement-timeout", "SET LOCAL statement_timeout = '60s'").await;
    let lock = format!("SELECT pg_catalog.pg_advisory_xact_lock({ADVISORY_LOCK})");
    exec_named(&tx, "advisory-lock", &lock).await;
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
    let sql = format!(
        "CREATE ROLE \"{LOGIN}\" LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOBYPASSRLS NOREPLICATION PASSWORD '{password}';\n\
         GRANT CONNECT ON DATABASE \"{DATABASE}\" TO \"{LOGIN}\""
    );
    client.batch_execute(&sql).await.expect("create login");
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
    for generation in [STALE_GENERATION, CURRENT_GENERATION] {
        register(client, CONFIG, generation, "namespace", "n", NAMESPACE, "", "").await;
        register(client, CONFIG, generation, "table", "t", NAMESPACE, TABLE, "").await;
        for column in PHYSICAL_COLUMNS {
            register(client, CONFIG, generation, "column", "c", NAMESPACE, TABLE, column).await;
        }
    }
    register(client, OTHER, OTHER_GENERATION, "namespace", "n", NAMESPACE, "", "").await;
    register(client, OTHER, OTHER_GENERATION, "table", "t", NAMESPACE, TABLE, "").await;
    for column in PHYSICAL_COLUMNS {
        register(client, OTHER, OTHER_GENERATION, "column", "c", NAMESPACE, TABLE, column).await;
    }
    let table = lookup_at(client, CONFIG, CURRENT_GENERATION, "table", NAMESPACE, TABLE, "").await;
    let mut columns = BTreeMap::new();
    for column in PHYSICAL_COLUMNS {
        let physical = lookup_at(
            client,
            CONFIG,
            CURRENT_GENERATION,
            "column",
            NAMESPACE,
            TABLE,
            column,
        )
        .await;
        columns.insert((*column).to_string(), physical);
    }
    let oids = vec![
        type_oid_i64(client, "int2").await,
        type_oid_i64(client, "int4").await,
        type_oid_i64(client, "int8").await,
        type_oid_i64(client, "text").await,
        type_oid_i64(client, "bool").await,
        type_oid_i64(client, "text").await,
    ];
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
    let idents: Vec<String> = PHYSICAL_COLUMNS.iter().map(|name| (*name).to_string()).collect();
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
    insert_rows(client, &table, &columns).await;
    Shadow { table, columns }
}

async fn type_oid_i64(client: &Client, name: &str) -> i64 {
    client
        .query_one(
            "SELECT type_row.oid::int8 FROM pg_catalog.pg_type AS type_row \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type_row.typnamespace \
             WHERE namespace.nspname = 'pg_catalog' AND type_row.typname = $1 AND type_row.typtype = 'b'",
            &[&name],
        )
        .await
        .expect("fixture type")
        .get(0)
}

async fn insert_rows(client: &mut Client, table: &str, columns: &BTreeMap<String, String>) {
    let sql = format!(
        "INSERT INTO \"_oo\".{table} (\"_oo_pos\", {units}, {weight}, {serial}, {note}, {sealed}, {token}) \
         VALUES (($1::text)::numeric, $2, $3, $4, $5, $6, $7)",
        table = quote_physical(table, "pt_"),
        units = quote_physical(columns.get("units").expect("u"), "pc_"),
        weight = quote_physical(columns.get("weight").expect("w"), "pc_"),
        serial = quote_physical(columns.get("serial").expect("s"), "pc_"),
        note = quote_physical(columns.get("note").expect("n"), "pc_"),
        sealed = quote_physical(columns.get("sealed").expect("b"), "pc_"),
        token = quote_physical(columns.get("token").expect("t"), "pc_"),
    );
    let rows = [
        FixtureRow {
            pos: "1",
            units: -32_768,
            weight: -40,
            serial: i64::MIN,
            note: Some("crate"),
            sealed: false,
            token: "crate-key",
        },
        FixtureRow {
            pos: "2",
            units: 0,
            weight: 15,
            serial: 1,
            note: None,
            sealed: false,
            token: "dock-key",
        },
        FixtureRow {
            pos: "3",
            units: 32_767,
            weight: 0,
            serial: 99,
            note: Some("lane"),
            sealed: true,
            token: "lane-key",
        },
    ];
    let tx = client.transaction().await.expect("insert begin");
    for row in rows {
        let params: [&(dyn ToSql + Sync); 7] = [
            &row.pos,
            &row.units,
            &row.weight,
            &row.serial,
            &row.note,
            &row.sealed,
            &row.token,
        ];
        tx.execute(&sql, &params).await.expect("insert shadow row");
    }
    tx.commit().await.expect("insert commit");
}

struct FixtureRow<'a> {
    pos: &'a str,
    units: i16,
    weight: i32,
    serial: i64,
    note: Option<&'a str>,
    sealed: bool,
    token: &'a str,
}

async fn register(
    client: &Client,
    digest: &str,
    generation: i64,
    level: &str,
    letter: &str,
    namespace: &str,
    table: &str,
    column: &str,
) {
    let payload: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &digest,
        &generation,
        &letter,
        &namespace,
        &table,
        &column,
    ];
    let digest_bytes: Vec<u8> = client
        .query_one(
            "SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, $3, $4, $5, $6, $7))",
            &payload,
        )
        .await
        .expect("digest")
        .get(0);
    let bind: [&(dyn ToSql + Sync); 8] = [
        &SOURCE,
        &digest,
        &generation,
        &level,
        &namespace,
        &table,
        &column,
        &digest_bytes,
    ];
    client
        .execute(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &bind,
        )
        .await
        .expect("register");
}

async fn register_again(client: &Client) -> String {
    let payload: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &CONFIG,
        &CURRENT_GENERATION,
        &"t",
        &NAMESPACE,
        &TABLE,
        &"",
    ];
    let digest_bytes: Vec<u8> = client
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
        &CURRENT_GENERATION,
        &"table",
        &NAMESPACE,
        &TABLE,
        &"",
        &digest_bytes,
    ];
    client
        .query_one(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &bind,
        )
        .await
        .expect("replay")
        .get(0)
}

async fn conflict_register(client: &Client) -> String {
    let digest_bytes = vec![0x11_u8; 32];
    let bind: [&(dyn ToSql + Sync); 8] = [
        &SOURCE,
        &CONFIG,
        &CURRENT_GENERATION,
        &"table",
        &NAMESPACE,
        &TABLE,
        &"",
        &digest_bytes,
    ];
    match client
        .query_one(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &bind,
        )
        .await
    {
        Ok(_) => "accepted".to_string(),
        Err(err) => err.code().map(|code| code.code().to_string()).unwrap_or_default(),
    }
}

async fn lookup_at(
    client: &Client,
    digest: &str,
    generation: i64,
    level: &str,
    namespace: &str,
    table: &str,
    column: &str,
) -> String {
    let params: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &digest,
        &generation,
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
    assert!(name.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_' || byte.is_ascii_digit()));
    format!("\"{name}\"")
}
