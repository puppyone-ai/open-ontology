//! Independent review of the public column-ACL install.
//!
//! The fixture is `clinic.ward.visit`, not the author shipment fixture.
//! The library is given a trusted client. This file does not authenticate a principal.

#![forbid(unsafe_code)]
#![allow(clippy::cognitive_complexity, clippy::too_many_lines)]

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use oo_gateway::{
    compile_column_acl, connect_require_tls, install_column_acl, ClassStatus, ColumnAclPolicy,
    ColumnSpec, InstallEffect, LogicalTable, PiiClass, PolicyError, RowFilter, TlsEndpoint,
};
use sha2::{Digest, Sha256};
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;

const HOST: &str = "oo-a173-pg";
const ALIAS: &str = "oo-a173-other";
const DATABASE: &str = "oo_a173";
const ADMIN: &str = "oo_a173_grantor";
const LOGIN: &str = "oo_a173_session";
const SECRET_DIR: &str = "/run/oo-a173/fixture";
const ORACLE_PATH: &str = "/evidence/live/oracle.txt";
const OWNER: &str = "a1730000-0000-4000-8000-000000000173";
const SHARD: &str = "shard_a173";
const SOURCE: &str = "clinic";
const DECOY_SOURCE: &str = "pharmacy";
const CONFIG: &str = "c173c173c173c173c173c173c173c173c173c173c173c173c173c173c173c173";
const DECOY_DIGEST: &str = "d173d173d173d173d173d173d173d173d173d173d173d173d173d173d173d173";
const TYPE_DIGEST: &str = "a173a173a173a173a173a173a173a173a173a173a173a173a173a173a173a173";
const INPUT_DIGEST: &str = "b173b173b173b173b173b173b173b173b173b173b173b173b173b173b173b173";
const SCHEMA_DIGEST: &str = "e173e173e173e173e173e173e173e173e173e173e173e173e173e173e173e173";
const NAMESPACE: &str = "ward";
const TABLE: &str = "visit";
const STALE_GENERATION: i64 = 4;
const CURRENT_GENERATION: i64 = 11;
const HIDDEN: &str = "ward-token";

const MIGRATION_SHA: &[(&str, &str)] = &[
    (
        "0000_bootstrap_roles.sql",
        "37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208",
    ),
    (
        "0001_target_fence_expand.sql",
        "97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9",
    ),
    (
        "0003_target_segment_preflight_expand.sql",
        "d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3",
    ),
    (
        "0004_physical_mapping.sql",
        "844de6f645f8f1ebf29d05118ae25d4669d8dc870eed1fa7697d4b98456eaf95",
    ),
    (
        "0005_full_import_expand.sql",
        "a9cef2013019784857353ff4fe060350d71a4e9ac4ab18551b992a7a9b52ecb6",
    ),
];

#[tokio::test]
async fn clinic_visit_column_acl_review() {
    assert_library_source();
    let admin_pass = read_hex("admin.pass");
    let login_pass = read_hex("login.pass");
    let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca.crt");
    let other_ca = fs::read(Path::new(SECRET_DIR).join("other-ca.crt")).expect("other-ca.crt");
    let mut oracle = Oracle::new(&admin_pass, &login_pass);

    let wrong_ca = connect_once(HOST, "postgres", ADMIN, &admin_pass, &other_ca).await;
    assert!(wrong_ca.is_err(), "other CA must not authenticate the server");
    let wrong_ca_text = wrong_ca.err().unwrap_or_default();
    assert!(!wrong_ca_text.contains(&admin_pass), "tls error leaked the password");
    assert!(
        public_tls_refusal(&wrong_ca_text),
        "other CA error was not a TLS handshake refusal: {wrong_ca_text}"
    );
    oracle.fact("tls_other_ca", &tls_refusal_class(&wrong_ca_text));

    let wrong_name = connect_once(ALIAS, "postgres", ADMIN, &admin_pass, &ca).await;
    assert!(wrong_name.is_err(), "alias outside the SAN must fail");
    let wrong_name_text = wrong_name.err().unwrap_or_default();
    assert!(!wrong_name_text.contains(&admin_pass), "name error leaked the password");
    assert!(
        !wrong_name_text.contains("failed to lookup")
            && !wrong_name_text.contains("Name or service not known"),
        "alias did not resolve: {wrong_name_text}"
    );
    assert!(
        public_tls_refusal(&wrong_name_text),
        "alias error was not a TLS handshake refusal: {wrong_name_text}"
    );
    oracle.fact("tls_other_name", &tls_refusal_class(&wrong_name_text));

    let maintenance = connect_db("postgres", ADMIN, &admin_pass, &ca).await;
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
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a173_session') THEN\n\
             DROP ROLE oo_a173_session;\n\
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
    drop(maintenance);

    let mut admin = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
    let admin_ssl: bool = admin
        .query_one(
            "SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()",
            &[],
        )
        .await
        .expect("admin ssl")
        .get(0);
    assert!(admin_ssl);
    oracle.fact("admin_tls", "true");
    apply_frozen_schema(&mut admin).await;
    create_login(&admin, &login_pass).await;
    let login_inherit: bool = admin
        .query_one(
            "SELECT rolinherit FROM pg_catalog.pg_roles WHERE rolname = $1",
            &[&LOGIN],
        )
        .await
        .expect("login inherit")
        .get(0);
    assert!(login_inherit, "the login role itself inherits; membership must not");
    oracle.fact("login_rolinherit", "true");

    let shadow = prepare_shadow(&mut admin).await;
    oracle.fact("physical_table", &shadow.table);
    oracle.fact("stale_physical_table", &shadow.stale_table);
    assert_ne!(shadow.table, shadow.stale_table);
    assert_ne!(shadow.visit_id, shadow.stale_visit);
    let scoped: i64 = admin
        .query_one(
            "SELECT max(run_generation) FROM _oo.physical_mappings WHERE source = $1 AND config_digest = $2",
            &[&SOURCE, &CONFIG],
        )
        .await
        .expect("scoped generation")
        .get(0);
    let source_only: i64 = admin
        .query_one(
            "SELECT max(run_generation) FROM _oo.physical_mappings WHERE source = $1",
            &[&SOURCE],
        )
        .await
        .expect("source generation")
        .get(0);
    let digest_only: i64 = admin
        .query_one(
            "SELECT max(run_generation) FROM _oo.physical_mappings WHERE config_digest = $1",
            &[&CONFIG],
        )
        .await
        .expect("digest generation")
        .get(0);
    assert_eq!(scoped, CURRENT_GENERATION);
    assert_eq!(source_only, 99);
    assert_eq!(digest_only, 50);
    oracle.fact("generation_scoped", &scoped.to_string());
    oracle.fact("generation_source_only", &source_only.to_string());
    oracle.fact("generation_digest_only", &digest_only.to_string());

    let before_roles = role_count(&admin, "oo_r_").await;
    let pure = xact_shape(&mut admin, XactShape::Pure).await;
    let pure_again = xact_shape(&mut admin, XactShape::Pure).await;
    assert_eq!(pure, pure_again, "no-SQL xact measurement changed between reads");
    assert_eq!(pure.1, 0, "no-SQL measurement rolled back");
    let rolled = xact_shape(&mut admin, XactShape::ReadThenRollback).await;
    let rolled_again = xact_shape(&mut admin, XactShape::ReadThenRollback).await;
    assert_eq!(rolled, rolled_again, "rollback xact measurement changed between reads");
    assert_eq!(rolled.1, pure.1 + 1, "calibration did not record one rollback");
    assert!(rolled.0 > pure.0, "calibration did not record the extra read");
    oracle.fact("xact_pure", &format!("commit={} rollback={}", pure.0, pure.1));
    oracle.fact(
        "xact_read_rollback",
        &format!("commit={} rollback={}", rolled.0, rolled.1),
    );
    let filtered = policy("filtered", LOGIN, CURRENT_GENERATION, true, "both");
    assert_eq!(
        compile_column_acl(&filtered).unwrap_err(),
        PolicyError::RowFilterRefused
    );
    expect_refusal(&mut admin, filtered, Err(PolicyError::RowFilterRefused), pure).await;
    expect_refusal(
        &mut admin,
        unknown_pii(),
        Err(PolicyError::ClassificationRefused {
            column: "mrn".to_string(),
        }),
        pure,
    )
    .await;
    expect_refusal(
        &mut admin,
        policy("old_reader", LOGIN, STALE_GENERATION, false, "both"),
        Err(PolicyError::StaleGeneration {
            policy: STALE_GENERATION,
            current: Some(CURRENT_GENERATION),
        }),
        rolled,
    )
    .await;
    expect_refusal(
        &mut admin,
        policy("gap", "oo_a173_missing", CURRENT_GENERATION, false, "id"),
        Err(PolicyError::LoginAbsent),
        rolled,
    )
    .await;
    expect_refusal(
        &mut admin,
        policy("half", ADMIN, CURRENT_GENERATION, false, "id"),
        Err(PolicyError::LoginAbsent),
        rolled,
    )
    .await;
    assert_eq!(role_count(&admin, "oo_r_").await, before_roles);
    for name in ["oo_r_filtered", "oo_r_mrn_only", "oo_r_old_reader", "oo_r_gap", "oo_r_half"] {
        assert!(!role_present(&admin, name).await, "{name} survived a refusal");
    }
    oracle.fact("refusal_roles", "absent");

    let picker_input = policy("picker", LOGIN, CURRENT_GENERATION, false, "both");
    let installed = install_column_acl(&mut admin, &picker_input)
        .await
        .expect("install picker");
    assert_eq!(installed.effect, InstallEffect::Installed);
    assert_eq!(installed.grantor, ADMIN);
    assert_ne!(installed.grantor, "oo_target_owner");
    assert_eq!(installed.role, "oo_r_picker");
    assert_eq!(installed.physical_table, shadow.table);
    assert_eq!(
        installed.physical_columns,
        vec![shadow.visit_id.clone(), shadow.wing.clone()]
    );
    let replay = install_column_acl(&mut admin, &picker_input)
        .await
        .expect("replay picker");
    assert_eq!(replay.effect, InstallEffect::Replay);
    assert_eq!(replay.physical_columns, installed.physical_columns);
    let members_after_replay = member_count(&admin, "oo_r_picker").await;
    assert_eq!(members_after_replay, 1);
    let scribe = install_column_acl(&mut admin, &policy("scribe", LOGIN, CURRENT_GENERATION, false, "id"))
        .await
        .expect("install scribe");
    assert_eq!(scribe.effect, InstallEffect::Installed);
    assert_eq!(scribe.physical_columns, vec![shadow.visit_id.clone()]);
    oracle.fact("picker_effect", "installed_then_replay");
    oracle.fact("grantor", &installed.grantor);

    let before_shrink = attacl_snapshot(&admin, &shadow.table).await;
    let shrink = install_column_acl(&mut admin, &policy("picker", LOGIN, CURRENT_GENERATION, false, "id")).await;
    assert_eq!(shrink, Err(PolicyError::GrantMismatch));
    let after_shrink = attacl_snapshot(&admin, &shadow.table).await;
    assert_eq!(before_shrink, after_shrink, "shrink changed column acl");
    assert_eq!(member_count(&admin, "oo_r_picker").await, 1);
    let expand = install_column_acl(&mut admin, &explicit_mrn()).await;
    assert_eq!(expand, Err(PolicyError::GrantMismatch));
    assert_eq!(attacl_snapshot(&admin, &shadow.table).await, before_shrink);
    oracle.fact("conflict_shrink", "grant_mismatch_acl_unchanged");
    oracle.fact("conflict_expand", "grant_mismatch_acl_unchanged");

    assert_catalog(&admin, &shadow, &mut oracle).await;
    admin
        .batch_execute(&format!(
            "ALTER TABLE \"_oo\".{} ADD COLUMN \"note_after\" text",
            quote_physical(&shadow.table, "pt_")
        ))
        .await
        .expect("add column");

    let mut reader = connect_db(DATABASE, LOGIN, &login_pass, &ca).await;
    let reader_pid = backend_pid(&reader).await;
    let reader_ssl: bool = admin
        .query_one(
            "SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = $1",
            &[&reader_pid],
        )
        .await
        .expect("reader ssl")
        .get(0);
    assert!(reader_ssl);
    oracle.fact("reader_tls", "true");
    exercise_reader(&mut reader, &shadow, reader_pid, &mut oracle).await;
    let session: String = admin
        .query_one("SELECT session_user::text", &[])
        .await
        .expect("admin still grantor")
        .get(0);
    assert_eq!(session, ADMIN);
    oracle.fact("installer_still_grantor", &session);
    oracle.flush();
}

fn assert_library_source() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/policy.rs");
    let src = fs::read_to_string(&path).expect("policy.rs");
    let mut code = String::new();
    for line in src.lines() {
        let kept = line.split("//").next().unwrap_or("");
        code.push_str(kept);
        code.push('\n');
    }
    assert!(!code.contains("unsafe"));
    assert!(!code.contains("SET ROLE"), "install must not SET ROLE");
    assert!(!code.to_ascii_lowercase().contains("set_config"));
    assert!(!code.contains("oo.claims"));
    assert!(code.contains("SslMode::Require"));
    assert!(code.contains("with_no_client_auth"));
    assert!(!code.contains("NoTls"));
    let install = code
        .split("fn install_column_acl(")
        .nth(1)
        .expect("install fn");
    let body = install.split("fn install_in_transaction(").next().unwrap_or(install);
    assert!(!body.contains("password"), "install_column_acl must not take a password");
}

struct Shadow {
    table: String,
    visit_id: String,
    wing: String,
    mrn: String,
    stale_table: String,
    stale_visit: String,
}

struct Oracle {
    lines: Vec<String>,
    secrets: Vec<String>,
}

impl Oracle {
    fn new(admin_pass: &str, login_pass: &str) -> Self {
        Self {
            lines: Vec::new(),
            secrets: vec![
                admin_pass.to_string(),
                login_pass.to_string(),
                HIDDEN.to_string(),
            ],
        }
    }

    fn fact(&mut self, key: &str, value: &str) {
        assert!(!key.contains("pass"), "oracle key");
        for secret in &self.secrets {
            assert!(!value.contains(secret), "oracle value contained a secret");
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

async fn connect_once(
    host: &str,
    database: &str,
    user: &str,
    password: &str,
    ca: &[u8],
) -> Result<Client, String> {
    let endpoint = TlsEndpoint {
        host: host.to_string(),
        port: 5432,
        database: database.to_string(),
        user: user.to_string(),
    };
    match connect_require_tls(&endpoint, password, ca).await {
        Ok((client, connection)) => {
            tokio::spawn(async move {
                let _ = connection.await;
            });
            Ok(client)
        }
        Err(err) => Err(err.to_string()),
    }
}

async fn connect_db(database: &str, user: &str, password: &str, ca: &[u8]) -> Client {
    let mut last = String::new();
    for _ in 0..20 {
        match connect_once(HOST, database, user, password, ca).await {
            Ok(client) => return client,
            Err(err) => {
                last = err;
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
    panic!("tls connect to {database} as {user} failed: {last}");
}

/// Public `PolicyError` display. `db_error` keeps `Error::to_string()` and drops the rustls source.
fn public_tls_refusal(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("error performing tls handshake")
        || lower.contains("certificate")
        || text.contains("UnknownIssuer")
        || text.contains("NotValidForName")
        || text.contains("CertificateRejected")
}

fn tls_refusal_class(text: &str) -> String {
    if text.contains("UnknownIssuer")
        || text.contains("NotValidForName")
        || text.to_ascii_lowercase().contains("certificate")
    {
        "named_certificate_error".to_string()
    } else {
        "handshake_reason_omitted".to_string()
    }
}

async fn xacts(client: &Client) -> (i64, i64) {
    let row = client
        .query_one(
            "SELECT xact_commit, xact_rollback FROM pg_stat_database WHERE datname = current_database()",
            &[],
        )
        .await
        .expect("xact counts");
    (row.get(0), row.get(1))
}

#[derive(Clone, Copy)]
enum XactShape {
    Pure,
    ReadThenRollback,
}

async fn xact_shape(client: &mut Client, shape: XactShape) -> (i64, i64) {
    // Publish counters before the baseline. PostgreSQL 17 does not show this
    // backend's latest commit on the very next statement. tokio-postgres also
    // prepares each `&str` statement, so absolute deltas are compared with a
    // same-session calibration rather than with a simple-query count.
    client
        .execute("SELECT pg_stat_force_next_flush()", &[])
        .await
        .expect("flush before");
    let before = xacts(client).await;
    match shape {
        XactShape::Pure => {}
        XactShape::ReadThenRollback => {
            client.query_one("SELECT 1", &[]).await.expect("calibration read");
            let tx = client.transaction().await.expect("calibration begin");
            tx.rollback().await.expect("calibration rollback");
        }
    }
    client
        .execute("SELECT pg_stat_force_next_flush()", &[])
        .await
        .expect("flush after");
    let after = xacts(client).await;
    (after.0 - before.0, after.1 - before.1)
}

async fn expect_refusal(
    client: &mut Client,
    input: ColumnAclPolicy,
    expect: Result<oo_gateway::InstallOutcome, PolicyError>,
    xact: (i64, i64),
) {
    client
        .execute("SELECT pg_stat_force_next_flush()", &[])
        .await
        .expect("flush before");
    let before = xacts(client).await;
    let got = install_column_acl(client, &input).await;
    assert_eq!(got, expect);
    client
        .execute("SELECT pg_stat_force_next_flush()", &[])
        .await
        .expect("flush after");
    let after = xacts(client).await;
    assert_eq!(
        (after.0 - before.0, after.1 - before.1),
        xact,
        "xact delta {expect:?}"
    );
}

async fn apply_frozen_schema(client: &mut Client) {
    let bootstrap = migration("0000_bootstrap_roles.sql");
    let fence = migration("0001_target_fence_expand.sql");
    let preflight = migration("0003_target_segment_preflight_expand.sql");
    let mapping = migration("0004_physical_mapping.sql");
    let full_import = migration("0005_full_import_expand.sql");
    for (name, expected) in MIGRATION_SHA {
        let actual = match *name {
            "0000_bootstrap_roles.sql" => &bootstrap.1,
            "0001_target_fence_expand.sql" => &fence.1,
            "0003_target_segment_preflight_expand.sql" => &preflight.1,
            "0004_physical_mapping.sql" => &mapping.1,
            "0005_full_import_expand.sql" => &full_import.1,
            _ => panic!("unknown migration"),
        };
        assert_eq!(actual, expected, "{name} bytes changed");
    }
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
        &marker_sql(&[(
            "full-import/v1",
            &full_import.1,
            "0005_full_import_expand.sql",
        )]),
    )
    .await;
    tx.commit().await.expect("commit full import");

    let versions: Vec<String> = client
        .query(
            "SELECT version FROM _oo.schema_migrations ORDER BY version",
            &[],
        )
        .await
        .expect("versions")
        .into_iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(
        versions,
        vec![
            "full-import/v1".to_string(),
            "physical-mapping/v1".to_string(),
            "target-bootstrap/1".to_string(),
            "target-fence-receipts/1".to_string(),
            "target-fence-receipts/3".to_string(),
        ]
    );
    let applied_by: i64 = client
        .query_one(
            "SELECT count(*)::int8 FROM _oo.schema_migrations WHERE applied_by = 'oo_target_owner'",
            &[],
        )
        .await
        .expect("applied_by")
        .get(0);
    assert_eq!(applied_by, 5);
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
        assert!(
            checksum
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "checksum hex"
        );
        if index > 0 {
            values.push_str(",\n");
        }
        values.push_str(&format!(
            "  ('{version}', '{checksum}', 'expand', '{filename}')"
        ));
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
    exec_named(
        &tx,
        "advisory-lock",
        "SELECT pg_catalog.pg_advisory_xact_lock(6404202605)",
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
            "CREATE ROLE \"{LOGIN}\" LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE \
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
    register_tree(client, DECOY_SOURCE, CONFIG, 50).await;
    register_tree(client, SOURCE, DECOY_DIGEST, 99).await;
    register_tree(client, SOURCE, CONFIG, STALE_GENERATION).await;
    register_tree(client, SOURCE, CONFIG, CURRENT_GENERATION).await;
    let stale_table = lookup(client, SOURCE, CONFIG, STALE_GENERATION, "table", NAMESPACE, TABLE, "").await;
    let stale_visit = lookup(
        client,
        SOURCE,
        CONFIG,
        STALE_GENERATION,
        "column",
        NAMESPACE,
        TABLE,
        "visit_id",
    )
    .await;
    let table = lookup(client, SOURCE, CONFIG, CURRENT_GENERATION, "table", NAMESPACE, TABLE, "").await;
    let visit_id = lookup(
        client,
        SOURCE,
        CONFIG,
        CURRENT_GENERATION,
        "column",
        NAMESPACE,
        TABLE,
        "visit_id",
    )
    .await;
    let wing = lookup(client, SOURCE, CONFIG, CURRENT_GENERATION, "column", NAMESPACE, TABLE, "wing").await;
    let mrn = lookup(client, SOURCE, CONFIG, CURRENT_GENERATION, "column", NAMESPACE, TABLE, "mrn").await;
    let quoted_table = quote_physical(&table, "pt_");
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
    let idents = vec!["visit_id".to_string(), "wing".to_string(), "mrn".to_string()];
    let oids = vec![23_i64, 25, 25];
    let typmods = vec![-1_i32, -1, -1];
    let required = vec![true, true, true];
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
    let insert = format!(
        "INSERT INTO \"_oo\".{quoted_table} (\"_oo_pos\", {}, {}, {}) \
         VALUES (($1::text)::numeric, $2, $3, $4)",
        quote_physical(&visit_id, "pc_"),
        quote_physical(&wing, "pc_"),
        quote_physical(&mrn, "pc_")
    );
    let tx = client.transaction().await.expect("insert begin");
    let params: [&(dyn ToSql + Sync); 4] = [&"7", &8801_i32, &"north", &HIDDEN];
    tx.execute(&insert, &params).await.expect("insert shadow row");
    tx.commit().await.expect("insert commit");
    Shadow {
        table,
        visit_id,
        wing,
        mrn,
        stale_table,
        stale_visit,
    }
}

async fn register_tree(client: &Client, source: &str, digest: &str, generation: i64) {
    register(client, source, digest, generation, "namespace", "n", NAMESPACE, "", "").await;
    register(client, source, digest, generation, "table", "t", NAMESPACE, TABLE, "").await;
    for column in ["visit_id", "wing", "mrn"] {
        register(client, source, digest, generation, "column", "c", NAMESPACE, TABLE, column).await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn register(
    client: &Client,
    source: &str,
    digest: &str,
    generation: i64,
    level: &str,
    letter: &str,
    namespace: &str,
    table: &str,
    column: &str,
) {
    let payload: [&(dyn ToSql + Sync); 7] = [
        &source,
        &digest,
        &generation,
        &letter,
        &namespace,
        &table,
        &column,
    ];
    let identity: Vec<u8> = client
        .query_one(
            "SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, $3, $4, $5, $6, $7))",
            &payload,
        )
        .await
        .expect("digest")
        .get(0);
    let bind: [&(dyn ToSql + Sync); 8] = [
        &source,
        &digest,
        &generation,
        &level,
        &namespace,
        &table,
        &column,
        &identity,
    ];
    client
        .execute(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &bind,
        )
        .await
        .expect("register");
}

#[allow(clippy::too_many_arguments)]
async fn lookup(
    client: &Client,
    source: &str,
    digest: &str,
    generation: i64,
    level: &str,
    namespace: &str,
    table: &str,
    column: &str,
) -> String {
    let params: [&(dyn ToSql + Sync); 7] = [
        &source,
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

fn policy(role: &str, login: &str, generation: i64, row_filter: bool, shape: &str) -> ColumnAclPolicy {
    let columns = match shape {
        "id" => vec![column("visit_id", PiiClass::None)],
        "both" => vec![
            column("visit_id", PiiClass::None),
            column("wing", PiiClass::None),
        ],
        _ => panic!("shape"),
    };
    ColumnAclPolicy {
        project: SOURCE.to_string(),
        config_digest: CONFIG.to_string(),
        generation,
        role: role.to_string(),
        login: login.to_string(),
        table: LogicalTable {
            namespace: NAMESPACE.to_string(),
            name: TABLE.to_string(),
            columns,
        },
        row_filter: row_filter.then(|| RowFilter::Claims {
            column: "wing".to_string(),
            claim: "wing".to_string(),
        }),
    }
}

fn unknown_pii() -> ColumnAclPolicy {
    let mut input = policy("mrn_only", LOGIN, CURRENT_GENERATION, false, "id");
    input.table.columns = vec![column("mrn", PiiClass::Unknown)];
    input
}

fn explicit_mrn() -> ColumnAclPolicy {
    let mut input = policy("picker", LOGIN, CURRENT_GENERATION, false, "both");
    input.table.columns.push(column(
        "mrn",
        PiiClass::Declared {
            explicit_allow: true,
        },
    ));
    input
}

fn column(name: &str, pii: PiiClass) -> ColumnSpec {
    ColumnSpec {
        name: name.to_string(),
        status: ClassStatus::Confirmed,
        semantic_type: "identifier".to_string(),
        pii,
    }
}

async fn role_count(client: &Client, prefix: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM pg_catalog.pg_roles WHERE left(rolname, length($1)) = $1",
            &[&prefix],
        )
        .await
        .expect("role count")
        .get(0)
}

async fn role_present(client: &Client, name: &str) -> bool {
    client
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = $1)",
            &[&name],
        )
        .await
        .expect("role present")
        .get(0)
}

async fn member_count(client: &Client, role: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM pg_catalog.pg_auth_members AS membership \
             JOIN pg_catalog.pg_roles AS granted ON granted.oid = membership.roleid \
             WHERE granted.rolname = $1",
            &[&role],
        )
        .await
        .expect("member count")
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

async fn attacl_snapshot(client: &Client, table: &str) -> String {
    let rows = client
        .query(
            "SELECT attribute.attname::text, COALESCE(attribute.attacl::text, 'null') \
             FROM pg_catalog.pg_attribute AS attribute \
             JOIN pg_catalog.pg_class AS relation ON relation.oid = attribute.attrelid \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = '_oo' AND relation.relname = $1 AND attribute.attnum > 0 \
               AND NOT attribute.attisdropped \
             ORDER BY attribute.attname",
            &[&table],
        )
        .await
        .expect("attacl snapshot");
    rows.into_iter()
        .map(|row| {
            let name: String = row.get(0);
            let acl: String = row.get(1);
            format!("{name}={acl}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn assert_catalog(client: &Client, shadow: &Shadow, oracle: &mut Oracle) {
    for role in ["oo_r_picker", "oo_r_scribe"] {
        let row = client
            .query_one(
                "SELECT rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, rolbypassrls, rolreplication \
                 FROM pg_catalog.pg_roles WHERE rolname = $1",
                &[&role],
            )
            .await
            .expect("role flags");
        let flags = [
            row.get::<_, bool>(0),
            row.get::<_, bool>(1),
            row.get::<_, bool>(2),
            row.get::<_, bool>(3),
            row.get::<_, bool>(4),
            row.get::<_, bool>(5),
            row.get::<_, bool>(6),
        ];
        assert_eq!(flags, [false; 7], "{role}");
    }
    let login = client
        .query_one(
            "SELECT rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, rolbypassrls, rolreplication \
             FROM pg_catalog.pg_roles WHERE rolname = $1",
            &[&LOGIN],
        )
        .await
        .expect("login flags");
    assert!(login.get::<_, bool>(0));
    assert!(login.get::<_, bool>(4));
    assert!(!login.get::<_, bool>(1));
    assert!(!login.get::<_, bool>(5));
    let member = client
        .query_one(
            "SELECT member_role.rolname, inherit_option, set_option, admin_option, grantor_role.rolname \
             FROM pg_catalog.pg_auth_members AS membership \
             JOIN pg_catalog.pg_roles AS granted ON granted.oid = membership.roleid \
             JOIN pg_catalog.pg_roles AS member_role ON member_role.oid = membership.member \
             JOIN pg_catalog.pg_roles AS grantor_role ON grantor_role.oid = membership.grantor \
             WHERE granted.rolname = 'oo_r_picker' AND member_role.rolname = $1",
            &[&LOGIN],
        )
        .await
        .expect("membership");
    assert_eq!(member.get::<_, String>(0), LOGIN);
    assert!(!member.get::<_, bool>(1));
    assert!(member.get::<_, bool>(2));
    assert!(!member.get::<_, bool>(3));
    assert_eq!(member.get::<_, String>(4), ADMIN);
    oracle.fact("membership_grantor", ADMIN);
    oracle.fact("membership", "inherit=false set=true admin=false");

    let picker = column_privileges(client, "oo_r_picker", &shadow.table).await;
    assert_eq!(
        picker,
        BTreeSet::from([shadow.visit_id.clone(), shadow.wing.clone()])
    );
    let scribe = column_privileges(client, "oo_r_scribe", &shadow.table).await;
    assert_eq!(scribe, BTreeSet::from([shadow.visit_id.clone()]));
    assert_eq!(table_privileges(client, "oo_r_picker", &shadow.table).await, 0);
    assert_eq!(table_privileges(client, "oo_r_scribe", &shadow.table).await, 0);
    assert_eq!(table_privileges(client, "PUBLIC", &shadow.table).await, 0);
    assert_eq!(table_privileges(client, LOGIN, &shadow.table).await, 0);
    assert_eq!(public_column_privileges(client, &shadow.table).await, 0);
    let relacl: String = client
        .query_one(
            "SELECT COALESCE(relation.relacl::text, 'null') \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = '_oo' AND relation.relname = $1",
            &[&shadow.table],
        )
        .await
        .expect("relacl")
        .get(0);
    assert!(!relacl.contains("oo_r_picker"), "{relacl}");
    assert!(!relacl.contains("oo_r_scribe"), "{relacl}");
    assert!(!relacl.contains(LOGIN), "{relacl}");
    assert!(!relacl.contains("{="), "{relacl}");
    assert!(!relacl.contains(",="), "{relacl}");
    oracle.fact("relacl", &relacl);
    let snapshot = attacl_snapshot(client, &shadow.table).await;
    for line in snapshot.lines() {
        let (name, acl) = line.split_once('=').expect("attacl line");
        oracle.fact(&format!("attacl.{name}"), acl);
        if name == shadow.visit_id {
            assert!(acl.contains("oo_r_picker=r/"), "{acl}");
            assert!(acl.contains("oo_r_scribe=r/"), "{acl}");
            assert!(acl.contains("/oo_target_owner"), "{acl}");
        } else if name == shadow.wing {
            assert!(acl.contains("oo_r_picker=r/"), "{acl}");
            assert!(!acl.contains("oo_r_scribe"), "{acl}");
        } else {
            assert!(!acl.contains("oo_r_picker"), "{name} {acl}");
            assert!(!acl.contains("oo_r_scribe"), "{name} {acl}");
        }
        assert!(!acl.contains("{=") && !acl.contains(",="), "{name} public {acl}");
    }
}

async fn column_privileges(client: &Client, role: &str, table: &str) -> BTreeSet<String> {
    let rows = client
        .query(
            "SELECT column_name::text FROM information_schema.column_privileges \
             WHERE grantee = $1 AND table_schema = '_oo' AND table_name = $2 AND privilege_type = 'SELECT'",
            &[&role, &table],
        )
        .await
        .expect("column privileges");
    rows.into_iter().map(|row| row.get(0)).collect()
}

async fn table_privileges(client: &Client, role: &str, table: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.table_privileges \
             WHERE grantee = $1 AND table_schema = '_oo' AND table_name = $2",
            &[&role, &table],
        )
        .await
        .expect("table privileges")
        .get(0)
}

async fn public_column_privileges(client: &Client, table: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.column_privileges \
             WHERE grantee = 'PUBLIC' AND table_schema = '_oo' AND table_name = $1",
            &[&table],
        )
        .await
        .expect("public columns")
        .get(0)
}

async fn exercise_reader(client: &mut Client, shadow: &Shadow, pid: i32, oracle: &mut Oracle) {
    let table = quote_physical(&shadow.table, "pt_");
    let visit_sql = quote_physical(&shadow.visit_id, "pc_");
    let wing_sql = quote_physical(&shadow.wing, "pc_");
    let mrn_sql = quote_physical(&shadow.mrn, "pc_");
    let selected = format!("SELECT {visit_sql}::int4, {wing_sql}::text FROM \"_oo\".{table}");
    let row = query_as_role(client, "oo_r_picker", &selected).await;
    assert_eq!(row.get::<_, i32>(0), 8801);
    assert_eq!(row.get::<_, String>(1), "north");
    oracle.fact("picker_visit_id", "8801");
    oracle.fact("picker_wing", "north");
    assert_eq!(backend_pid(client).await, pid);
    expect_42501(client, None, &format!("SELECT {visit_sql}::int4 FROM \"_oo\".{table}")).await;
    oracle.fact("after_rollback_visit", "42501");

    let scribe_row = query_as_role(
        client,
        "oo_r_scribe",
        &format!("SELECT {visit_sql}::int4 FROM \"_oo\".{table}"),
    )
    .await;
    assert_eq!(scribe_row.get::<_, i32>(0), 8801);
    expect_42501(
        client,
        Some("oo_r_scribe"),
        &format!("SELECT {wing_sql}::text FROM \"_oo\".{table}"),
    )
    .await;
    oracle.fact("scribe_visit_id", "8801");
    oracle.fact("scribe_wing", "42501");

    client
        .batch_execute("SET ROLE \"oo_r_picker\"")
        .await
        .expect("session set picker");
    let wing: String = client
        .query_one(&format!("SELECT {wing_sql}::text FROM \"_oo\".{table}"), &[])
        .await
        .expect("session picker wing")
        .get(0);
    assert_eq!(wing, "north");
    client
        .batch_execute("SET ROLE \"oo_r_scribe\"")
        .await
        .expect("session set scribe");
    expect_42501_now(client, &format!("SELECT {wing_sql}::text FROM \"_oo\".{table}")).await;
    client.batch_execute("RESET ROLE").await.expect("reset role");
    expect_42501_now(client, &format!("SELECT {visit_sql}::int4 FROM \"_oo\".{table}")).await;
    client.batch_execute("RESET ALL").await.expect("reset all");
    assert_eq!(backend_pid(client).await, pid);
    let current: String = client
        .query_one("SELECT current_user::text", &[])
        .await
        .expect("current user")
        .get(0);
    assert_eq!(current, LOGIN);
    oracle.fact("after_reset_role_visit", "42501");
    oracle.fact("same_backend", "true");

    let claims = claims_then_mrn(client, &format!("SELECT {mrn_sql}::text FROM \"_oo\".{table}")).await;
    assert_eq!(claims.0, "00000");
    assert_eq!(claims.1, "42501");
    oracle.fact("claims_set_sqlstate", "00000");
    oracle.fact("claims_mrn", "42501");

    expect_42501(
        client,
        Some("oo_r_picker"),
        &format!("SELECT \"_oo_pos\" FROM \"_oo\".{table}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_picker"),
        &format!("SELECT \"note_after\" FROM \"_oo\".{table}"),
    )
    .await;
    let mode = read_write_mode(client, "oo_r_picker").await;
    assert_eq!(mode, "off");
    oracle.fact("transaction_read_only", "off");
    expect_42501(
        client,
        Some("oo_r_picker"),
        &format!("INSERT INTO \"_oo\".{table} (\"_oo_pos\") VALUES (8)"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_picker"),
        &format!("UPDATE \"_oo\".{table} SET {wing_sql} = 'south'"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_picker"),
        &format!("DELETE FROM \"_oo\".{table}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_picker"),
        "SELECT count(*) FROM _oo.physical_mappings",
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_picker"),
        "SELECT count(*) FROM _oo.full_import_run",
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_picker"),
        &format!(
            "SELECT _oo.lookup_physical_mapping('{SOURCE}', '{CONFIG}', {CURRENT_GENERATION}, 'table', '{NAMESPACE}', '{TABLE}', '')"
        ),
    )
    .await;
    assert_eq!(backend_pid(client).await, pid);
    oracle.fact("hidden_mrn", "42501");
    oracle.fact("hidden_pos", "42501");
    oracle.fact("hidden_note_after", "42501");
    oracle.fact("write_insert", "42501");
    oracle.fact("write_update", "42501");
    oracle.fact("write_delete", "42501");
    oracle.fact("metadata_mappings", "42501");
    oracle.fact("metadata_run", "42501");
    oracle.fact("metadata_lookup", "42501");
    oracle.fact("reader_pid", &pid.to_string());
}

async fn query_as_role(client: &mut Client, role: &str, sql: &str) -> tokio_postgres::Row {
    let tx = client.transaction().await.expect("begin read");
    tx.batch_execute(&format!("SET LOCAL ROLE {}", quote_role(role)))
        .await
        .expect("set role");
    let row = tx.query_one(sql, &[]).await.expect("selected read");
    tx.rollback().await.expect("rollback read");
    row
}

async fn expect_42501(client: &mut Client, role: Option<&str>, sql: &str) {
    let tx = client.transaction().await.expect("begin denial");
    if let Some(role) = role {
        tx.batch_execute(&format!("SET LOCAL ROLE {}", quote_role(role)))
            .await
            .expect("set role");
    }
    let err = tx.query_one(sql, &[]).await.expect_err("expected denial");
    let state = err.code().map(|code| code.code().to_string()).unwrap_or_default();
    assert_eq!(state, "42501", "{err}");
    tx.rollback().await.expect("rollback denial");
}

async fn expect_42501_now(client: &Client, sql: &str) {
    let err = client.query_one(sql, &[]).await.expect_err("expected denial");
    let state = err.code().map(|code| code.code().to_string()).unwrap_or_default();
    assert_eq!(state, "42501", "{err}");
}

async fn read_write_mode(client: &mut Client, role: &str) -> String {
    let tx = client.transaction().await.expect("begin mode");
    tx.batch_execute(&format!("SET LOCAL ROLE {}", quote_role(role)))
        .await
        .expect("set role");
    let mode: String = tx
        .query_one("SHOW transaction_read_only", &[])
        .await
        .expect("show")
        .get(0);
    tx.rollback().await.expect("rollback mode");
    mode
}

async fn claims_then_mrn(client: &mut Client, sql: &str) -> (String, String) {
    let tx = client.transaction().await.expect("begin claims");
    tx.batch_execute("SET LOCAL ROLE \"oo_r_picker\"")
        .await
        .expect("set picker");
    tx.batch_execute("SET LOCAL oo.claims = 'wing=north'")
        .await
        .expect("set claims");
    let set_state = "00000".to_string();
    let err = tx.query_one(sql, &[]).await.expect_err("mrn must stay denied");
    let state = err.code().map(|code| code.code().to_string()).unwrap_or_default();
    tx.rollback().await.expect("rollback claims");
    (set_state, state)
}

fn quote_role(role: &str) -> String {
    assert!(
        !role.is_empty()
            && role.bytes().all(|byte| {
                byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit()
            })
    );
    format!("\"{role}\"")
}

async fn backend_pid(client: &Client) -> i32 {
    client
        .query_one("SELECT pg_catalog.pg_backend_pid()", &[])
        .await
        .expect("pid")
        .get(0)
}
