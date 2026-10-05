//! Mapped-shadow statistics on one private PostgreSQL 17.
//!
//! The public import entry loads one source table. `reltuples` stays `-1`.
//! The explicit maintenance entry then analyzes that mapped shadow. A runtime
//! login, a read-only login, a stale generation, a missing table, a forged
//! relation, and a lock timeout are refused. The source audit does not grow.

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

const SOURCE: &str = "a219src";
const GENERATION: u64 = 11;

#[tokio::test]
#[ignore = "one private PostgreSQL 17.10; OO_TARGET_STATS_HOST must be set by the deciding run"]
async fn mapped_shadow_analyze_on_one_postgres() {
    let mut harness = Harness::connect().await;
    harness.setup().await;
    harness.import_one_table().await;
    harness.unknown_before_maintenance().await;
    harness.refuse_before_analyze().await;
    harness.maintain().await;
    harness.refuse_after_analyze().await;
    harness.source_audit().await;
    harness.write_facts();
    print!("{}", harness.facts);
}

struct Harness {
    host: String,
    port: u16,
    admin_user: String,
    admin_password: String,
    reader_password: String,
    runtime_password: String,
    readonly_password: String,
    ca: Vec<u8>,
    evidence: PathBuf,
    postgres: PgConnection,
    source: PgConnection,
    target: PgConnection,
    postmaster: String,
    in_recovery: bool,
    physical: String,
    digest: String,
    rows: i64,
    audit_before: i64,
    facts: String,
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
            ca,
            evidence,
            postgres,
            source,
            target,
            postmaster: String::new(),
            in_recovery: false,
            physical: String::new(),
            digest: String::new(),
            rows: 0,
            audit_before: 0,
            facts: "server_tls=true\nversion_17_10=true\n".to_string(),
        }
    }

    async fn setup(&mut self) {
        reset_cluster(&mut self.postgres).await;
        exec_role(&mut self.postgres, "oo_a219_reader", &self.reader_password).await;
        exec_role(&mut self.postgres, "oo_a219_runtime", &self.runtime_password).await;
        exec_role(&mut self.postgres, "oo_a219_readonly", &self.readonly_password).await;
        raw(
            &mut self.postgres,
            "ALTER ROLE oo_a219_readonly SET default_transaction_read_only = on",
        )
        .await;
        raw(&mut self.postgres, "CREATE DATABASE oo_a219_src TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
        raw(&mut self.postgres, "CREATE DATABASE oo_a219_dst TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
        self.source = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            "oo_a219_src",
            &self.ca,
        )
        .await;
        self.target = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            "oo_a219_dst",
            &self.ca,
        )
        .await;
        assert_eq!(ssl_on(&mut self.source).await, "true");
        assert_eq!(ssl_on(&mut self.target).await, "true");
        raw(&mut self.source, SOURCE_SQL).await;
        raw(
            &mut self.source,
            "GRANT USAGE ON SCHEMA app TO oo_a219_reader; GRANT SELECT ON app.party TO oo_a219_reader; GRANT CONNECT ON DATABASE oo_a219_src TO oo_a219_reader",
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
        raw(&mut self.target, "GRANT oo_pipeline_runtime TO oo_a219_runtime").await;
        raw(
            &mut self.target,
            "GRANT CONNECT ON DATABASE oo_a219_dst TO oo_a219_runtime, oo_a219_readonly",
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
        self.audit_before = scalar_i64(
            &mut self.source,
            "SELECT count(*)::int8 FROM app.write_audit",
        )
        .await;
        assert!(self.audit_before >= 2);
        let audit_before = self.audit_before;
        self.facts.push_str(&format!("audit_before={audit_before}\n"));
    }

    async fn import_one_table(&mut self) {
        let admitted = load_admitted(&mut self.source).await;
        let mut runtime = dial(
            &self.host,
            self.port,
            "oo_a219_runtime",
            &self.runtime_password,
            "oo_a219_dst",
            &self.ca,
        )
        .await;
        assert_eq!(ssl_on(&mut runtime).await, "true");
        let summary = import_unpublished(&mut runtime, self.request(&admitted))
            .await
            .expect("import");
        runtime.close().await.expect("close runtime");
        assert_eq!(summary.state, RunState::AwaitingPublish);
        assert_eq!(summary.source_id, SOURCE);
        assert_eq!(summary.source_generation, GENERATION);
        assert_eq!(summary.tables.len(), 1);
        assert_eq!(summary.tables[0].rows, 2);
        self.physical = summary.tables[0].physical_name.clone();
        self.rows = 2;
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
        assert_eq!(generation, i64::try_from(GENERATION).expect("generation"));
        assert_eq!(self.digest.len(), 64);
        self.facts.push_str("import_state=awaiting_publish\nimport_rows=2\n");
    }

    async fn unknown_before_maintenance(&mut self) {
        let bits = reltuples_bits(&mut self.target, &self.physical).await;
        assert_eq!(bits, (-1.0f32).to_bits(), "import left a known estimate");
        let sentinel = reltuples_bits(&mut self.target, "sentinel").await;
        assert_eq!(sentinel, (-1.0f32).to_bits());
        self.facts.push_str("reltuples_before=-1\nsentinel_before=-1\n");
    }

    async fn refuse_before_analyze(&mut self) {
        self.expect_open_transaction().await;
        let stale = self.identity(GENERATION + 1, "app", "party");
        expect_kind(&mut self.target, stale, &StatisticsError::StaleGeneration).await;
        let missing = self.identity(GENERATION, "app", "missing");
        expect_kind(&mut self.target, missing, &StatisticsError::MissingTable).await;
        let mut runtime = dial(
            &self.host,
            self.port,
            "oo_a219_runtime",
            &self.runtime_password,
            "oo_a219_dst",
            &self.ca,
        )
        .await;
        let runtime_request = self.identity(GENERATION, "app", "party");
        expect_kind(&mut runtime, runtime_request, &StatisticsError::RuntimeCaller).await;
        runtime.close().await.expect("close runtime refusal");
        let mut readonly = dial(
            &self.host,
            self.port,
            "oo_a219_readonly",
            &self.readonly_password,
            "oo_a219_dst",
            &self.ca,
        )
        .await;
        let readonly_request = self.identity(GENERATION, "app", "party");
        expect_kind(&mut readonly, readonly_request, &StatisticsError::ReadOnlyCaller).await;
        readonly.close().await.expect("close readonly");
        let bits = reltuples_bits(&mut self.target, &self.physical).await;
        assert_eq!(bits, (-1.0f32).to_bits());
        self.facts.push_str(
            "refused=open_transaction,stale,missing,runtime,readonly\nreltuples_after_refusal=-1\n",
        );
    }

    async fn maintain(&mut self) {
        let request = self.identity(GENERATION, "app", "party");
        let receipt = analyze_mapped_shadows(&mut self.target, &request)
            .await
            .expect("analyze");
        assert_eq!(receipt.protocol, PROTOCOL);
        assert!(!receipt.published);
        assert!(!receipt.estimate_is_exact);
        assert_eq!(receipt.disposition, ConnectionDisposition::Reusable);
        assert_eq!(receipt.tables.len(), 1);
        assert_eq!(receipt.tables[0].physical_name, self.physical);
        assert_ne!(receipt.tables[0].reltuples_bits, (-1.0f32).to_bits());
        let measured = f32::from_bits(receipt.tables[0].reltuples_bits);
        assert!(measured.is_finite() && measured >= 0.0);
        let bits = reltuples_bits(&mut self.target, &self.physical).await;
        assert_eq!(bits, receipt.tables[0].reltuples_bits);
        let sentinel = reltuples_bits(&mut self.target, "sentinel").await;
        assert_eq!(sentinel, (-1.0f32).to_bits());
        let reltuples_after_bits = receipt.tables[0].reltuples_bits;
        let estimated_rows = receipt.tables[0].estimated_rows;
        let count_rows = self.rows;
        self.facts.push_str(&format!(
            "published=false\nestimate_is_exact=false\nreltuples_after_bits={reltuples_after_bits}\nestimated_rows={estimated_rows}\ncount_rows={count_rows}\nconnection=reusable\n"
        ));
        self.facts.push_str("sentinel_after=-1\n");
    }

    async fn refuse_after_analyze(&mut self) {
        let quoted = quote_physical_name(&self.physical).expect("quote");
        let mut locker = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            "oo_a219_dst",
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
        let err = analyze_mapped_shadows(&mut self.target, &request)
            .await
            .expect_err("lock timeout");
        assert_eq!(err.kind, StatisticsError::LockTimeout);
        assert_eq!(err.disposition, ConnectionDisposition::Reusable);
        raw(&mut locker, "ROLLBACK").await;
        locker.close().await.expect("close locker");
        self.forged_relation(&quoted).await;
        let bits = reltuples_bits(&mut self.target, &self.physical).await;
        assert_ne!(bits, (-1.0f32).to_bits());
        self.facts.push_str("refused_after=lock_timeout,forged\nreltuples_restored=known\n");
    }

    async fn forged_relation(&mut self, quoted: &str) {
        raw(
            &mut self.target,
            &format!("ALTER TABLE \"_oo\".{quoted} RENAME TO pt_hold_a219"),
        )
        .await;
        raw(
            &mut self.target,
            &format!("CREATE VIEW \"_oo\".{quoted} AS SELECT 1 AS n"),
        )
        .await;
        let forged = self.identity(GENERATION, "app", "party");
        expect_kind(&mut self.target, forged, &StatisticsError::ForgedRelation).await;
        raw(&mut self.target, &format!("DROP VIEW \"_oo\".{quoted}")).await;
        raw(
            &mut self.target,
            &format!("ALTER TABLE \"_oo\".pt_hold_a219 RENAME TO {quoted}"),
        )
        .await;
    }

    async fn source_audit(&mut self) {
        let audit = scalar_i64(&mut self.source, "SELECT count(*)::int8 FROM app.write_audit").await;
        let reader = scalar_i64(
            &mut self.source,
            "SELECT count(*)::int8 FROM app.write_audit WHERE who = 'oo_a219_reader'",
        )
        .await;
        let source_bits = reltuples_named(&mut self.source, "app", "party").await;
        assert_eq!(audit, self.audit_before);
        assert_eq!(reader, 0);
        assert_eq!(source_bits, (-1.0f32).to_bits());
        self.facts.push_str("source_audit_delta=0\nsource_reader_writes=0\nsource_reltuples=-1\n");
    }

    fn write_facts(&self) {
        fs::write(self.evidence.join("facts.txt"), &self.facts).expect("facts");
    }

    async fn expect_open_transaction(&mut self) {
        raw(&mut self.target, "BEGIN").await;
        raw(&mut self.target, "SELECT pg_catalog.pg_sleep(0.01)").await;
        let request = self.identity(GENERATION, "app", "party");
        let err = analyze_mapped_shadows(&mut self.target, &request)
            .await
            .expect_err("open transaction");
        assert_eq!(err.kind, StatisticsError::OpenTransaction);
        assert_eq!(err.disposition, ConnectionDisposition::Reusable);
        raw(&mut self.target, "ROLLBACK").await;
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

    fn request(&self, admitted: &AdmittedCatalog) -> ImportRequest {
        let config = PgSourceConfig::try_new(
            SourceId::new(SOURCE).expect("source"),
            PgEndpoint::new(&self.host, self.port, "oo_a219_src").expect("endpoint"),
            PgCredentials::new("oo_a219_reader", &self.reader_password).expect("credentials"),
            SecretRef::new("secret://a219/reader").expect("secret"),
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
            shard: "a219main".to_string(),
            owner: String::new(),
            snapshot: SnapshotRequest {
                config_identity: config_identity(&config),
                type_policy_identity: type_policy_identity(oo_connectors::PgTypePolicy::PRESERVE_ROW),
                config,
                admitted: admitted.clone(),
                policy: oo_connectors::PgTypePolicy::PRESERVE_ROW,
                generation: GENERATION,
                deadline: Instant::now() + Duration::from_secs(60),
                expected_principal: "oo_a219_reader".to_string(),
                expected_database: "oo_a219_src".to_string(),
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
LANGUAGE plpgsql SECURITY DEFINER AS $audit$\n\
BEGIN\n\
  INSERT INTO app.write_audit (who) VALUES (current_user);\n\
  RETURN NEW;\n\
END;\n\
$audit$;\n\
CREATE TRIGGER party_audit BEFORE INSERT OR UPDATE OR DELETE ON app.party\n\
  FOR EACH ROW EXECUTE FUNCTION app.note_write();\n\
INSERT INTO app.party (id, label, active) VALUES (1, 'north', true), (2, 'south', false);\n\
";

async fn expect_kind(conn: &mut PgConnection, request: StatisticsRequest, kind: &StatisticsError) {
    let err = analyze_mapped_shadows(conn, &request).await.expect_err("refused");
    assert_eq!(&err.kind, kind, "{err}");
    assert_eq!(err.disposition, ConnectionDisposition::Reusable, "{err}");
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
    let value: f32 = sqlx::query_scalar(
        "SELECT class.reltuples \
         FROM pg_catalog.pg_class AS class \
         JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace \
         WHERE class.relname = $1 AND namespace.nspname IN ('_oo', 'public')",
    )
    .bind(name)
    .fetch_one(&mut *conn)
    .await
    .unwrap_or_else(|err| panic!("reltuples {name}: {err}"));
    value.to_bits()
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
    .expect("source reltuples");
    value.to_bits()
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
    for name in ["oo_a219_src", "oo_a219_dst"] {
        raw(admin, &format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await;
    }
    for role in ["oo_a219_reader", "oo_a219_runtime", "oo_a219_readonly", "oo_a219_login"] {
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
