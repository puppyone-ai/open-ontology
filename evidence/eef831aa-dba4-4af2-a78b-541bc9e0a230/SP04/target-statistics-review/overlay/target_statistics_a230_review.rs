//! Independent review of explicit mapped-shadow statistics.
//!
//! Copied into a throwaway archive. Not a product source. The fixture and the
//! typed gold are this review's. `estimated_rows` is recorded and is not
//! required to equal the gold cardinality.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use oo_connectors::{
    config_identity, type_policy_identity, AdmittedCatalog, AdmittedColumn, AdmittedTable,
    PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef, SelectedTable,
    SnapshotRequest, SourceId, StreamBudget, TableSelection,
};
use oo_pipeline::copy_encoder::CopyBudgets;
use oo_pipeline::full_import::{import_unpublished, ImportRequest, RunState};
use oo_pipeline::physical_mapping::quote_physical_name;
use oo_pipeline::target_installer::{
    install, install_full_import, FullImportInstallOutcome, InstallOutcome,
};
use oo_pipeline::target_statistics::{
    analyze_mapped_shadows, ConnectionDisposition, LogicalTableRef, StatisticsError,
    StatisticsRequest, PROTOCOL,
};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{Connection, Row};

const SOURCE: &str = "a230src";
const OTHER_SOURCE: &str = "a230alt";
const GENERATION: u64 = 4;
const GOLD: [(i32, &str, bool); 3] = [(7, "keel", true), (8, "helm", false), (9, "mast", true)];

#[tokio::test]
#[ignore = "one private PostgreSQL 17.10; OO_TARGET_STATS_HOST is set by the review launch"]
async fn mapped_shadow_review_on_one_postgres() {
    let mut harness = Harness::connect().await;
    harness.setup().await;
    harness.import_gold().await;
    harness.unknown_and_sentinel().await;
    harness.refuse_identity().await;
    harness.raw_idle_probe().await;
    harness.nosleep_begin().await;
    harness.refuse_mapping_scope().await;
    harness.refuse_roles().await;
    harness.refuse_owner_and_ordinary().await;
    harness.still_unknown().await;
    harness.explicit_analyze().await;
    harness.lock_timeout().await;
    harness.source_audit().await;
    harness.finish();
}

struct Harness {
    host: String,
    port: u16,
    admin_user: String,
    admin_password: String,
    reader_password: String,
    runtime_password: String,
    readonly_password: String,
    plain_password: String,
    ca: Vec<u8>,
    evidence: PathBuf,
    postgres: PgConnection,
    source: PgConnection,
    target: PgConnection,
    postmaster: String,
    in_recovery: bool,
    physical: String,
    digest: String,
    audit_before: i64,
    bits_before_explicit: u32,
    facts: String,
    failures: Vec<String>,
}

impl Harness {
    async fn connect() -> Self {
        let host = env::var("OO_TARGET_STATS_HOST").expect("OO_TARGET_STATS_HOST");
        let port: u16 = env::var("OO_TARGET_STATS_PORT")
            .expect("OO_TARGET_STATS_PORT")
            .parse()
            .expect("port");
        assert!(!host.contains(':') && !host.contains('/') && !host.contains('@'));
        let admin_user = env::var("OO_TARGET_STATS_ADMIN_USER").expect("OO_TARGET_STATS_ADMIN_USER");
        let admin_password = password("OO_TARGET_STATS_ADMIN_PASSWORD_FILE");
        let reader_password = password("OO_TARGET_STATS_READER_PASSWORD_FILE");
        let runtime_password = password("OO_TARGET_STATS_RUNTIME_PASSWORD_FILE");
        let readonly_password = password("OO_TARGET_STATS_READONLY_PASSWORD_FILE");
        let plain_password = password("OO_TARGET_STATS_PLAIN_PASSWORD_FILE");
        let ca = fs::read(env::var("OO_TARGET_STATS_CA_FILE").expect("OO_TARGET_STATS_CA_FILE")).expect("ca");
        let evidence = PathBuf::from(env::var("OO_TARGET_STATS_EVIDENCE").expect("OO_TARGET_STATS_EVIDENCE"));
        fs::create_dir_all(&evidence).expect("evidence dir");
        let mut postgres = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
        let version = text(&mut postgres, "SELECT version()").await;
        assert!(version.contains("17.10"), "{version}");
        assert_eq!(ssl_on(&mut postgres).await, "true");
        let source = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
        let target = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
        Self {
            host,
            port,
            admin_user,
            admin_password,
            reader_password,
            runtime_password,
            readonly_password,
            plain_password,
            ca,
            evidence,
            postgres,
            source,
            target,
            postmaster: String::new(),
            in_recovery: true,
            physical: String::new(),
            digest: String::new(),
            audit_before: -1,
            bits_before_explicit: 0,
            facts: "server_tls=true\nversion_17_10=true\n".to_string(),
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

    fn identity(&self, generation: u64, namespace: &str, table: &str) -> StatisticsRequest {
        StatisticsRequest {
            project: SOURCE.to_string(),
            source: SOURCE.to_string(),
            config_digest: self.digest.clone(),
            source_generation: i64::try_from(generation).expect("generation"),
            tables: vec![LogicalTableRef {
                namespace: namespace.to_string(),
                name: table.to_string(),
            }],
            statement_timeout_ms: 5_000,
            lock_timeout_ms: 5_000,
        }
    }

    async fn setup(&mut self) {
        reset_cluster(&mut self.postgres).await;
        exec_role(&mut self.postgres, "oo_a230_reader", &self.reader_password).await;
        exec_role(&mut self.postgres, "oo_a230_runtime", &self.runtime_password).await;
        exec_role(&mut self.postgres, "oo_a230_readonly", &self.readonly_password).await;
        exec_role(&mut self.postgres, "oo_a230_plain", &self.plain_password).await;
        raw(
            &mut self.postgres,
            "ALTER ROLE oo_a230_readonly SET default_transaction_read_only = on",
        )
        .await;
        raw(&mut self.postgres, "CREATE DATABASE oo_a230_src TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
        raw(&mut self.postgres, "CREATE DATABASE oo_a230_dst TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
        self.source = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            "oo_a230_src",
            &self.ca,
        )
        .await;
        self.target = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            "oo_a230_dst",
            &self.ca,
        )
        .await;
        assert_eq!(ssl_on(&mut self.source).await, "true");
        assert_eq!(ssl_on(&mut self.target).await, "true");
        raw(&mut self.source, &source_sql()).await;
        raw(
            &mut self.source,
            "GRANT USAGE ON SCHEMA app TO oo_a230_reader; GRANT SELECT ON app.party TO oo_a230_reader; GRANT CONNECT ON DATABASE oo_a230_src TO oo_a230_reader",
        )
        .await;
        assert_eq!(
            install(&mut self.target).await.expect("install"),
            InstallOutcome::Bootstrapped
        );
        assert_eq!(
            install_full_import(&mut self.target).await.expect("0005"),
            FullImportInstallOutcome::Installed
        );
        raw(&mut self.target, "GRANT oo_pipeline_runtime TO oo_a230_runtime").await;
        raw(
            &mut self.target,
            "GRANT CONNECT ON DATABASE oo_a230_dst TO oo_a230_runtime, oo_a230_readonly, oo_a230_plain",
        )
        .await;
        raw(&mut self.target, "CREATE TABLE public.sentinel (id int4)").await;
        let row = sqlx::query(
            "SELECT pg_catalog.pg_postmaster_start_time()::text, pg_catalog.pg_is_in_recovery()",
        )
        .fetch_one(&mut self.source)
        .await
        .expect("node");
        self.postmaster = row.try_get(0).expect("postmaster");
        self.in_recovery = row.try_get(1).expect("recovery");
        self.observe("in_recovery", if self.in_recovery { "true" } else { "false" }, !self.in_recovery);
        self.audit_before = scalar_i64(&mut self.source, "SELECT count(*)::int8 FROM app.write_audit").await;
        let audit_before = self.audit_before;
        self.observe("audit_before", &audit_before.to_string(), audit_before == 3);
    }

    async fn import_gold(&mut self) {
        let admitted = load_admitted(&mut self.source).await;
        let mut runtime = dial(
            &self.host,
            self.port,
            "oo_a230_runtime",
            &self.runtime_password,
            "oo_a230_dst",
            &self.ca,
        )
        .await;
        assert_eq!(ssl_on(&mut runtime).await, "true");
        let summary = import_unpublished(&mut runtime, self.request(&admitted))
            .await
            .expect("import");
        runtime.close().await.expect("close runtime");
        self.observe(
            "import_state",
            "awaiting_publish",
            summary.state == RunState::AwaitingPublish,
        );
        self.observe("import_source", SOURCE, summary.source_id == SOURCE);
        self.observe(
            "import_generation",
            &summary.source_generation.to_string(),
            summary.source_generation == GENERATION,
        );
        self.observe("import_tables", &summary.tables.len().to_string(), summary.tables.len() == 1);
        if summary.tables.len() == 1 {
            let rows = summary.tables[0].rows;
            self.observe("import_rows", &rows.to_string(), rows == 3);
            self.physical = summary.tables[0].physical_name.clone();
        } else {
            self.observe("import_rows", "absent", false);
        }
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
        let digest_ok = self.digest.len() == 64
            && self.digest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
        self.observe("config_digest_len", &self.digest.len().to_string(), digest_ok);
        self.observe(
            "mapping_generation",
            &generation.to_string(),
            generation == i64::try_from(GENERATION).expect("generation"),
        );
        self.observe(
            "mapping_physical_present",
            "true",
            self.physical.starts_with("pt_") && self.physical.len() == 63,
        );
    }

    async fn unknown_and_sentinel(&mut self) {
        let bits = reltuples_bits(&mut self.target, &self.physical).await;
        self.observe(
            "reltuples_before",
            "-1",
            bits == (-1.0f32).to_bits(),
        );
        let sentinel = reltuples_named(&mut self.target, "public", "sentinel").await;
        self.observe("sentinel_before", "-1", sentinel == (-1.0f32).to_bits());
    }

    async fn refuse_identity(&mut self) {
        let mut mismatched = self.identity(GENERATION, "app", "party");
        mismatched.project = OTHER_SOURCE.to_string();
        let outcome = refusal(&mut self.target, &mismatched).await;
        self.note("identity_project", &outcome, "Identity");
        let mut closed_generation = self.identity(GENERATION, "app", "party");
        closed_generation.source_generation = 0;
        let outcome = refusal(&mut self.target, &closed_generation).await;
        self.note("identity_generation", &outcome, "Identity");
        let idle = raw_open(&mut self.target).await;
        self.observe("identity_left_idle", "true", !idle);
    }

    async fn raw_idle_probe(&mut self) {
        let open = raw_open(&mut self.target).await;
        self.observe("raw_simple_query_idle", "false", !open);
    }

    async fn nosleep_begin(&mut self) {
        let mut side = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            "oo_a230_dst",
            &self.ca,
        )
        .await;
        let before = reltuples_bits(&mut self.target, &self.physical).await;
        raw(&mut side, "BEGIN").await;
        let request = self.identity(GENERATION, "app", "party");
        let started = Instant::now();
        let outcome = refusal(&mut side, &request).await;
        let elapsed_ms = u128_ms(started.elapsed());
        let still_open = raw_open(&mut side).await;
        raw(&mut side, "ROLLBACK").await;
        let idle_after = raw_open(&mut side).await;
        side.close().await.expect("close nosleep");
        let after = reltuples_bits(&mut self.target, &self.physical).await;
        self.observe("nosleep_elapsed_ms", &elapsed_ms.to_string(), true);
        self.observe(
            "nosleep_kind",
            &outcome.kind,
            outcome.kind == "OpenTransaction" && outcome.disposition == "Reusable",
        );
        self.observe("nosleep_disposition", &outcome.disposition, outcome.disposition == "Reusable");
        self.observe("nosleep_caller_transaction_kept", "true", still_open);
        self.observe("nosleep_rollback_idle", "true", !idle_after);
        self.observe(
            "nosleep_reltuples_unchanged",
            "true",
            before == after && after == (-1.0f32).to_bits(),
        );
        self.observe_display("nosleep", &outcome.display);
        self.facts.push_str("statement_timeout_provoked=false\n");
    }

    async fn refuse_mapping_scope(&mut self) {
        let mut wrong_source = self.identity(GENERATION, "app", "party");
        wrong_source.project = OTHER_SOURCE.to_string();
        wrong_source.source = OTHER_SOURCE.to_string();
        let outcome = refusal(&mut self.target, &wrong_source).await;
        self.note("source", &outcome, "StaleGeneration");
        let mut wrong_digest = self.identity(GENERATION, "app", "party");
        wrong_digest.config_digest = alternate_digest(&self.digest);
        let outcome = refusal(&mut self.target, &wrong_digest).await;
        self.note("config_digest", &outcome, "StaleGeneration");
        let stale = self.identity(GENERATION + 1, "app", "party");
        let outcome = refusal(&mut self.target, &stale).await;
        self.note("current_generation", &outcome, "StaleGeneration");
        let missing = self.identity(GENERATION, "app", "hold");
        let outcome = refusal(&mut self.target, &missing).await;
        self.note("mapping_absent", &outcome, "MissingTable");
    }

    async fn refuse_roles(&mut self) {
        let request = self.identity(GENERATION, "app", "party");
        let mut runtime = dial(
            &self.host,
            self.port,
            "oo_a230_runtime",
            &self.runtime_password,
            "oo_a230_dst",
            &self.ca,
        )
        .await;
        let outcome = refusal(&mut runtime, &request).await;
        self.note("runtime_role", &outcome, "RuntimeCaller");
        runtime.close().await.expect("close runtime");
        let readonly_request = self.identity(GENERATION, "app", "party");
        let mut readonly = dial(
            &self.host,
            self.port,
            "oo_a230_readonly",
            &self.readonly_password,
            "oo_a230_dst",
            &self.ca,
        )
        .await;
        let outcome = refusal(&mut readonly, &readonly_request).await;
        self.note("readonly", &outcome, "ReadOnlyCaller");
        readonly.close().await.expect("close readonly");
        raw(
            &mut self.target,
            "GRANT USAGE ON SCHEMA _oo TO oo_a230_plain; \
             GRANT SELECT ON TABLE _oo.physical_mappings TO oo_a230_plain; \
             GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO oo_a230_plain",
        )
        .await;
        let mut plain = dial(
            &self.host,
            self.port,
            "oo_a230_plain",
            &self.plain_password,
            "oo_a230_dst",
            &self.ca,
        )
        .await;
        let visible: Option<i64> = sqlx::query_scalar(
            "SELECT max(run_generation) FROM _oo.physical_mappings WHERE source = $1 AND config_digest = $2",
        )
        .bind(SOURCE)
        .bind(&self.digest)
        .fetch_one(&mut plain)
        .await
        .expect("plain generation");
        let looked: String = sqlx::query_scalar(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, 'table', 'app', 'party', '')",
        )
        .bind(SOURCE)
        .bind(&self.digest)
        .bind(i64::try_from(GENERATION).expect("generation"))
        .fetch_one(&mut plain)
        .await
        .expect("plain lookup");
        self.observe(
            "plain_sees_current_generation",
            "true",
            visible == Some(i64::try_from(GENERATION).expect("generation")),
        );
        self.observe("plain_lookup", "stored", looked == self.physical);
        let allowed: bool = sqlx::query_scalar(
            "SELECT pg_catalog.has_table_privilege($1::text, class.oid, 'MAINTAIN') \
             FROM pg_catalog.pg_class AS class \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace \
             WHERE namespace.nspname = '_oo' AND class.relname = $2",
        )
        .bind("oo_a230_plain")
        .bind(&self.physical)
        .fetch_one(&mut self.target)
        .await
        .expect("maintain privilege");
        self.observe("plain_maintain", "false", !allowed);
        let maintain_request = self.identity(GENERATION, "app", "party");
        let outcome = refusal(&mut plain, &maintain_request).await;
        self.note("maintain", &outcome, "RoleDenied");
        plain.close().await.expect("close plain");
    }

    async fn refuse_owner_and_ordinary(&mut self) {
        let quoted = quote_physical_name(&self.physical).expect("quote");
        raw(
            &mut self.target,
            &format!("ALTER TABLE \"_oo\".{quoted} OWNER TO oo_a230_plain"),
        )
        .await;
        let owner_request = self.identity(GENERATION, "app", "party");
        let outcome = refusal(&mut self.target, &owner_request).await;
        self.note("ordinary_owner", &outcome, "ForgedRelation");
        raw(
            &mut self.target,
            &format!("ALTER TABLE \"_oo\".{quoted} OWNER TO oo_target_owner"),
        )
        .await;
        let owner: String = sqlx::query_scalar(
            "SELECT pg_catalog.pg_get_userbyid(class.relowner) \
             FROM pg_catalog.pg_class AS class \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace \
             WHERE namespace.nspname = '_oo' AND class.relname = $1",
        )
        .bind(&self.physical)
        .fetch_one(&mut self.target)
        .await
        .expect("owner restored");
        self.observe("owner_restored", "oo_target_owner", owner == "oo_target_owner");
        raw(
            &mut self.target,
            &format!("ALTER TABLE \"_oo\".{quoted} RENAME TO pt_hold_a230"),
        )
        .await;
        raw(
            &mut self.target,
            &format!("CREATE VIEW \"_oo\".{quoted} AS SELECT 1::int4 AS n"),
        )
        .await;
        let view_request = self.identity(GENERATION, "app", "party");
        let outcome = refusal(&mut self.target, &view_request).await;
        self.note("ordinary_relkind", &outcome, "ForgedRelation");
        raw(&mut self.target, &format!("DROP VIEW \"_oo\".{quoted}")).await;
        raw(
            &mut self.target,
            &format!("ALTER TABLE \"_oo\".pt_hold_a230 RENAME TO {quoted}"),
        )
        .await;
        let relkind: String = sqlx::query_scalar(
            "SELECT class.relkind::text \
             FROM pg_catalog.pg_class AS class \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace \
             WHERE namespace.nspname = '_oo' AND class.relname = $1",
        )
        .bind(&self.physical)
        .fetch_one(&mut self.target)
        .await
        .expect("relkind restored");
        self.observe("relkind_restored", "r", relkind == "r");
    }

    async fn still_unknown(&mut self) {
        let bits = reltuples_bits(&mut self.target, &self.physical).await;
        self.bits_before_explicit = bits;
        self.observe(
            "reltuples_after_refusal",
            "-1",
            bits == (-1.0f32).to_bits(),
        );
    }

    async fn explicit_analyze(&mut self) {
        let request = self.identity(GENERATION, "app", "party");
        let first_unknown = self.bits_before_explicit == (-1.0f32).to_bits();
        match analyze_mapped_shadows(&mut self.target, &request).await {
            Ok(receipt) => {
                let table = receipt.tables.first();
                let bits = table.map(|item| item.reltuples_bits).unwrap_or(u32::MAX);
                let estimated = table.map(|item| item.estimated_rows).unwrap_or(0);
                let measured = f32::from_bits(bits);
                let physical_ok = receipt.tables.len() == 1
                    && receipt.tables[0].physical_name == self.physical;
                let known = receipt.tables.len() == 1
                    && measured.is_finite()
                    && measured >= 0.0
                    && bits != (-1.0f32).to_bits();
                self.observe("explicit_protocol", receipt.protocol, receipt.protocol == PROTOCOL);
                self.observe("published", "false", !receipt.published);
                self.observe("estimate_is_exact", "false", !receipt.estimate_is_exact);
                self.observe(
                    "explicit_disposition",
                    disposition_label(receipt.disposition),
                    receipt.disposition == ConnectionDisposition::Reusable,
                );
                self.observe("explicit_physical", "stored", physical_ok);
                self.observe("reltuples_after_bits", &bits.to_string(), known);
                self.observe("estimated_rows", &estimated.to_string(), true);
                self.observe(
                    "estimate_equals_gold",
                    if estimated == 3 { "true" } else { "false" },
                    true,
                );
                self.observe("explicit_was_first_known", "true", first_unknown && known);
                let stored = reltuples_bits(&mut self.target, &self.physical).await;
                self.observe("reltuples_stored_matches_receipt", "true", stored == bits);
                let sentinel = reltuples_named(&mut self.target, "public", "sentinel").await;
                self.observe("sentinel_after", "-1", sentinel == (-1.0f32).to_bits());
                let idle = raw_open(&mut self.target).await;
                self.observe("explicit_probe_idle", "true", !idle);
            }
            Err(err) => {
                self.observe(
                    "explicit_analyze",
                    &format!("{}/{}", kind_label(&err.kind), disposition_label(err.disposition)),
                    false,
                );
                self.observe("explicit_was_first_known", "false", false);
            }
        }
    }

    async fn lock_timeout(&mut self) {
        let quoted = quote_physical_name(&self.physical).expect("quote");
        let before = reltuples_bits(&mut self.target, &self.physical).await;
        let mut locker = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            "oo_a230_dst",
            &self.ca,
        )
        .await;
        raw(&mut locker, "BEGIN").await;
        raw(
            &mut locker,
            &format!("LOCK TABLE \"_oo\".{quoted} IN ACCESS EXCLUSIVE MODE"),
        )
        .await;
        let mut request = self.identity(GENERATION, "app", "party");
        request.lock_timeout_ms = 400;
        request.statement_timeout_ms = 5_000;
        let outcome = refusal(&mut self.target, &request).await;
        let idle = raw_open(&mut self.target).await;
        raw(&mut locker, "ROLLBACK").await;
        locker.close().await.expect("close locker");
        let after = reltuples_bits(&mut self.target, &self.physical).await;
        self.observe(
            "lock_timeout",
            &format!("{}/{}", outcome.kind, outcome.disposition),
            outcome.kind == "LockTimeout" && outcome.disposition == "Reusable",
        );
        self.observe("lock_probe_idle", "true", !idle);
        self.observe("lock_left_estimate", "unchanged", before == after && after != (-1.0f32).to_bits());
        self.observe_display("lock", &outcome.display);
    }

    async fn source_audit(&mut self) {
        let audit = scalar_i64(&mut self.source, "SELECT count(*)::int8 FROM app.write_audit").await;
        let reader = scalar_i64(
            &mut self.source,
            "SELECT count(*)::int8 FROM app.write_audit WHERE who = 'oo_a230_reader'",
        )
        .await;
        let source_bits = reltuples_named(&mut self.source, "app", "party").await;
        let state: String = sqlx::query_scalar(
            "SELECT state FROM _oo.full_import_run WHERE source_id = $1",
        )
        .bind(SOURCE)
        .fetch_one(&mut self.target)
        .await
        .unwrap_or_else(|_| "absent".to_string());
        self.observe("source_audit_delta", "0", audit == self.audit_before);
        self.observe("source_reader_writes", "0", reader == 0);
        self.observe("source_reltuples", "-1", source_bits == (-1.0f32).to_bits());
        self.observe("import_state_after", &state, state == "awaiting_publish");
        self.observe("gold_rows", "3", true);
    }

    fn note(&mut self, key: &str, outcome: &RefusalOutcome, expect_kind: &str) {
        self.observe(
            key,
            &format!("{}/{}", outcome.kind, outcome.disposition),
            outcome.kind == expect_kind && outcome.disposition == "Reusable",
        );
        self.observe_display(key, &outcome.display);
    }

    fn observe_display(&mut self, key: &str, shown: &str) {
        let hides = !shown.contains("keel")
            && !shown.contains("helm")
            && !shown.contains("mast")
            && !shown.contains(&self.admin_password)
            && !shown.contains(&self.reader_password)
            && !shown.contains(&self.plain_password);
        self.observe(&format!("{key}_display_hides_values"), "true", hides);
    }

    fn request(&self, admitted: &AdmittedCatalog) -> ImportRequest {
        let config = PgSourceConfig::try_new(
            SourceId::new(SOURCE).expect("source"),
            PgEndpoint::new(&self.host, self.port, "oo_a230_src").expect("endpoint"),
            PgCredentials::new("oo_a230_reader", &self.reader_password).expect("credentials"),
            SecretRef::new("secret://a230/reader").expect("secret"),
            PgTlsConfig::from_ca_pem(&self.ca).expect("tls"),
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(60),
                Duration::from_secs(30),
                1000,
                8 * 1024 * 1024,
            )
            .expect("budget"),
            TableSelection::new(vec![SelectedTable::new("app", "party").expect("table")]).expect("selection"),
        )
        .expect("config");
        ImportRequest {
            shard: "a230main".to_string(),
            owner: String::new(),
            snapshot: SnapshotRequest {
                config_identity: config_identity(&config),
                type_policy_identity: type_policy_identity(oo_connectors::PgTypePolicy::PRESERVE_ROW),
                config,
                admitted: admitted.clone(),
                policy: oo_connectors::PgTypePolicy::PRESERVE_ROW,
                generation: GENERATION,
                deadline: Instant::now() + Duration::from_secs(60),
                expected_principal: "oo_a230_reader".to_string(),
                expected_database: "oo_a230_src".to_string(),
                expected_in_recovery: self.in_recovery,
                expected_postmaster_start: self.postmaster.clone(),
            },
            stream: StreamBudget::try_new(100, 1_048_576, 1_048_576, 2_097_152).expect("stream"),
            copy: CopyBudgets {
                max_value_bytes: 1_048_576,
                max_row_bytes: 4_194_304,
                max_chunk_bytes: 8_388_608,
                max_rows_per_chunk: 1024,
            },
        }
    }

    fn finish(&mut self) {
        let failure_count = self.failures.len();
        self.observe("failure_count", &failure_count.to_string(), failure_count == 0);
        fs::write(self.evidence.join("facts.txt"), &self.facts).expect("facts");
        if !self.failures.is_empty() {
            panic!("review failures:\n{}", self.failures.join("\n"));
        }
    }
}

struct RefusalOutcome {
    kind: String,
    disposition: String,
    display: String,
}

async fn refusal(conn: &mut PgConnection, request: &StatisticsRequest) -> RefusalOutcome {
    match analyze_mapped_shadows(conn, request).await {
        Ok(receipt) => RefusalOutcome {
            kind: "Ok".to_string(),
            disposition: disposition_label(receipt.disposition).to_string(),
            display: String::new(),
        },
        Err(err) => RefusalOutcome {
            kind: kind_label(&err.kind),
            disposition: disposition_label(err.disposition).to_string(),
            display: err.to_string(),
        },
    }
}

fn kind_label(kind: &StatisticsError) -> String {
    match kind {
        StatisticsError::Identity => "Identity".to_string(),
        StatisticsError::Bound => "Bound".to_string(),
        StatisticsError::TimeoutBound => "TimeoutBound".to_string(),
        StatisticsError::EmptyTables => "EmptyTables".to_string(),
        StatisticsError::DuplicateTable => "DuplicateTable".to_string(),
        StatisticsError::OpenTransaction => "OpenTransaction".to_string(),
        StatisticsError::RuntimeCaller => "RuntimeCaller".to_string(),
        StatisticsError::ReadOnlyCaller => "ReadOnlyCaller".to_string(),
        StatisticsError::RoleDenied => "RoleDenied".to_string(),
        StatisticsError::LockTimeout => "LockTimeout".to_string(),
        StatisticsError::StatementTimeout => "StatementTimeout".to_string(),
        StatisticsError::StaleGeneration => "StaleGeneration".to_string(),
        StatisticsError::MissingTable => "MissingTable".to_string(),
        StatisticsError::MissingRelation => "MissingRelation".to_string(),
        StatisticsError::ForgedRelation => "ForgedRelation".to_string(),
        StatisticsError::EstimateUnknown => "EstimateUnknown".to_string(),
        StatisticsError::EstimateUnusable => "EstimateUnusable".to_string(),
        StatisticsError::TransactionIncomplete => "TransactionIncomplete".to_string(),
        StatisticsError::Database { sqlstate } => format!("Database:{sqlstate}"),
    }
}

fn disposition_label(disposition: ConnectionDisposition) -> &'static str {
    match disposition {
        ConnectionDisposition::Reusable => "Reusable",
        ConnectionDisposition::FreshConnection => "FreshConnection",
    }
}

fn alternate_digest(actual: &str) -> String {
    let candidate = "ab".repeat(32);
    if candidate == actual {
        "cd".repeat(32)
    } else {
        candidate
    }
}

fn source_sql() -> String {
    let mut values = String::new();
    for (index, (id, label, active)) in GOLD.iter().enumerate() {
        if index > 0 {
            values.push_str(", ");
        }
        let flag = if *active { "true" } else { "false" };
        values.push_str(&format!("({id}, '{label}', {flag})"));
    }
    format!(
        "CREATE SCHEMA app;\n\
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
LANGUAGE plpgsql SECURITY DEFINER AS $audit$\n\
BEGIN\n\
  INSERT INTO app.write_audit (who) VALUES (current_user);\n\
  RETURN NEW;\n\
END;\n\
$audit$;\n\
CREATE TRIGGER party_audit BEFORE INSERT OR UPDATE OR DELETE ON app.party\n\
  FOR EACH ROW EXECUTE FUNCTION app.note_write();\n\
INSERT INTO app.party (id, label, active) VALUES {values};\n"
    )
}

fn u128_ms(duration: Duration) -> u128 {
    duration.as_millis()
}

async fn load_admitted(admin: &mut PgConnection) -> AdmittedCatalog {
    let rows = sqlx::query(
        "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull \
         FROM pg_catalog.pg_attribute a \
         JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = 'app' AND c.relname = 'party' AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum",
    )
    .fetch_all(&mut *admin)
    .await
    .expect("admitted columns");
    let mut columns = Vec::new();
    for row in &rows {
        columns.push(AdmittedColumn {
            attnum: row.try_get(0).unwrap(),
            name: row.try_get(1).unwrap(),
            type_oid: row.try_get(2).unwrap(),
            typmod: row.try_get(3).unwrap(),
            not_null: row.try_get(4).unwrap(),
        });
    }
    let keys = sqlx::query(
        "SELECT a.attname::text \
         FROM pg_catalog.pg_index i \
         JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS u(attnum, ord) ON true \
         JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = u.attnum \
         WHERE n.nspname = 'app' AND c.relname = 'party' AND i.indisprimary \
         ORDER BY u.ord",
    )
    .fetch_all(&mut *admin)
    .await
    .expect("keys");
    AdmittedCatalog {
        tables: vec![AdmittedTable {
            schema: "app".to_string(),
            name: "party".to_string(),
            columns,
            primary_key: keys.iter().map(|row| row.try_get(0).unwrap()).collect(),
        }],
    }
}

async fn reltuples_bits(conn: &mut PgConnection, name: &str) -> u32 {
    reltuples_named(conn, "_oo", name).await
}

async fn reltuples_named(conn: &mut PgConnection, namespace: &str, name: &str) -> u32 {
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
    .unwrap_or_else(|err| panic!("reltuples {namespace}.{name}: {err}"));
    value.to_bits()
}

async fn raw_open(conn: &mut PgConnection) -> bool {
    let row = sqlx::raw_sql(sqlx::AssertSqlSafe(
        "SELECT pg_catalog.statement_timestamp() > pg_catalog.transaction_timestamp()".to_string(),
    ))
    .fetch_one(&mut *conn)
    .await
    .expect("raw simple query");
    row.try_get(0).expect("bool")
}

async fn scalar_i64(conn: &mut PgConnection, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .expect("scalar")
}

async fn ssl_on(conn: &mut PgConnection) -> String {
    text(conn, "SELECT ssl::text FROM pg_stat_ssl WHERE pid = pg_backend_pid()").await
}

async fn reset_cluster(admin: &mut PgConnection) {
    for name in ["oo_a230_src", "oo_a230_dst"] {
        raw(admin, &format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await;
    }
    raw(
        admin,
        "DO $$ BEGIN \
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_r_clerk') \
              AND EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a230_login') THEN \
             EXECUTE 'REVOKE oo_r_clerk FROM oo_a230_login'; \
           END IF; \
         END $$",
    )
    .await;
    for role in [
        "oo_a230_reader",
        "oo_a230_runtime",
        "oo_a230_readonly",
        "oo_a230_plain",
        "oo_a230_login",
        "oo_r_clerk",
    ] {
        raw(admin, &format!("DROP ROLE IF EXISTS {role}")).await;
    }
    raw(admin, "DROP ROLE IF EXISTS oo_pipeline_runtime").await;
    raw(admin, "DROP ROLE IF EXISTS oo_target_owner").await;
}

async fn exec_role(conn: &mut PgConnection, name: &str, secret: &str) {
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

fn password(var: &str) -> String {
    let path = env::var(var).unwrap_or_else(|_| panic!("{var}"));
    let text = fs::read_to_string(path).expect("password file");
    let trimmed = text.trim().to_string();
    assert!(trimmed.chars().all(|ch| ch.is_ascii_hexdigit()));
    trimmed
}

async fn dial(host: &str, port: u16, user: &str, pass: &str, database: &str, ca: &[u8]) -> PgConnection {
    let options = PgConnectOptions::new()
        .host(host)
        .port(port)
        .username(user)
        .password(pass)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    PgConnection::connect_with(&options).await.expect("dial")
}

async fn raw(conn: &mut PgConnection, sql: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("sql failed: {err}"));
}

async fn text(conn: &mut PgConnection, sql: &str) -> String {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("scalar failed: {err}"))
}
