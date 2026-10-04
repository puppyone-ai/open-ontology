//! Column ACL against a shadow built by the reviewed prepare function.
//!
//! The fixture administrator applies the frozen target scripts in the
//! installer's transaction shape. This test does not link `oo-pipeline`.
//! Passwords are read from files and are not written to the oracle.

use std::fs;
use std::path::Path;

use oo_gateway::{
    compile_column_acl, connect_require_tls, install_column_acl, ClassStatus, ColumnAclPolicy,
    ColumnSpec, InstallEffect, LogicalTable, PiiClass, PolicyError, RowFilter, TlsEndpoint,
};
use sha2::{Digest, Sha256};
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;

const HOST: &str = "oo-a171-pg";
const DATABASE: &str = "oo_a171";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a171_login";
const SECRET_DIR: &str = "/run/oo-a171/fixture";
const ORACLE_PATH: &str = "/evidence/live/oracle.txt";
const OWNER: &str = "a1710000-0000-4000-8000-000000000171";
const SHARD: &str = "shard_a171";
const SOURCE: &str = "orders";
const CONFIG: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const TYPE_DIGEST: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
const INPUT_DIGEST: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
const SCHEMA_DIGEST: &str = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";
const NAMESPACE: &str = "logistics";
const TABLE: &str = "shipment";
const STALE_GENERATION: i64 = 3;
const CURRENT_GENERATION: i64 = 7;

#[tokio::test]
async fn shadow_column_acl_installs_and_denies() {
    let admin_pass = read_hex("admin.pass");
    let login_pass = read_hex("login.pass");
    let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca.crt");
    let mut oracle = Oracle::new(&admin_pass, &login_pass);

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
    // CREATE ROLE is cluster-wide. Dropping the database leaves the previous
    // attempt's roles in place, and the frozen script has no IF NOT EXISTS.
    expect_sql(
        &maintenance,
        "drop fixture roles",
        "DO $$\n\
         DECLARE role_name text;\n\
         BEGIN\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a171_login') THEN\n\
             DROP ROLE oo_a171_login;\n\
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
    apply_frozen_schema(&mut admin).await;
    create_login(&admin, &login_pass).await;
    let shadow = prepare_shadow(&mut admin).await;
    oracle.fact("physical_table", &shadow.table);
    oracle.fact("order_column", &shadow.order_id);
    oracle.fact("region_column", &shadow.region);
    oracle.fact("secret_column", &shadow.secret);

    let before = role_count(&admin, "oo_r_").await;
    assert_eq!(
        install_column_acl(&mut admin, &sample("filtered", LOGIN, true, false)).await,
        Err(PolicyError::RowFilterRefused)
    );
    assert_eq!(
        install_column_acl(&mut admin, &unknown_pii()).await,
        Err(PolicyError::ClassificationRefused {
            column: "secret".to_string(),
        })
    );
    let mut invalid = sample("dispatcher", LOGIN, false, false);
    invalid.role = "dispatcher;drop".to_string();
    assert!(matches!(
        compile_column_acl(&invalid),
        Err(PolicyError::InvalidIdentifier { field: "role" })
    ));
    assert_eq!(
        install_column_acl(&mut admin, &sample("stale_reader", LOGIN, false, true)).await,
        Err(PolicyError::StaleGeneration {
            policy: STALE_GENERATION,
            current: Some(CURRENT_GENERATION),
        })
    );
    assert_eq!(
        install_column_acl(&mut admin, &sample("orphan", "oo_absent_login", false, false)).await,
        Err(PolicyError::LoginAbsent)
    );
    assert_eq!(role_count(&admin, "oo_r_").await, before);
    assert!(!role_present(&admin, "oo_r_orphan").await);
    assert!(!role_present(&admin, "oo_r_stale_reader").await);
    oracle.fact("refusal_roles", "unchanged");

    let dispatcher = sample("dispatcher", LOGIN, false, false);
    let installed = install_column_acl(&mut admin, &dispatcher)
        .await
        .expect("install dispatcher");
    assert_eq!(installed.effect, InstallEffect::Installed);
    assert_eq!(installed.grantor, ADMIN);
    assert_eq!(installed.role, "oo_r_dispatcher");
    assert_eq!(installed.physical_table, shadow.table);
    let replay = install_column_acl(&mut admin, &dispatcher)
        .await
        .expect("replay dispatcher");
    assert_eq!(replay.effect, InstallEffect::Replay);
    assert_eq!(replay.physical_columns, installed.physical_columns);
    let clerk = install_column_acl(&mut admin, &clerk_policy())
        .await
        .expect("install clerk");
    assert_eq!(clerk.effect, InstallEffect::Installed);
    assert_eq!(clerk.role, "oo_r_clerk");
    oracle.fact("dispatcher_effect", "installed_then_replay");
    oracle.fact("grantor", &installed.grantor);

    assert_catalog(&admin, &shadow, &mut oracle).await;
    admin
        .batch_execute(&format!(
            "ALTER TABLE \"_oo\".{} ADD COLUMN \"extra\" text",
            quote_physical(&shadow.table, "pt_")
        ))
        .await
        .expect("add column");

    let mut reader = connect_db(DATABASE, LOGIN, &login_pass, &ca).await;
    let reader_pid = backend_pid(&reader).await;
    let admin_view: bool = admin
        .query_one(
            "SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = $1",
            &[&reader_pid],
        )
        .await
        .expect("reader ssl")
        .get(0);
    assert!(admin_view);
    oracle.fact("reader_tls", "true");
    exercise_reader(&mut reader, &shadow, reader_pid, &mut oracle).await;
    oracle.flush();
}

struct Shadow {
    table: String,
    order_id: String,
    region: String,
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
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
    panic!("tls connect to {database} as {user} failed: {last}");
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
    for generation in [STALE_GENERATION, CURRENT_GENERATION] {
        register(client, generation, "namespace", "n", NAMESPACE, "", "").await;
        register(client, generation, "table", "t", NAMESPACE, TABLE, "").await;
        for column in ["order_id", "region", "secret"] {
            register(client, generation, "column", "c", NAMESPACE, TABLE, column).await;
        }
    }
    let table = lookup(client, "table", NAMESPACE, TABLE, "").await;
    let order_id = lookup(client, "column", NAMESPACE, TABLE, "order_id").await;
    let region = lookup(client, "column", NAMESPACE, TABLE, "region").await;
    let secret = lookup(client, "column", NAMESPACE, TABLE, "secret").await;
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
    let idents = vec![
        "order_id".to_string(),
        "region".to_string(),
        "secret".to_string(),
    ];
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
    let order_sql = quote_physical(&order_id, "pc_");
    let region_sql = quote_physical(&region, "pc_");
    let secret_sql = quote_physical(&secret, "pc_");
    let insert = format!(
        "INSERT INTO \"_oo\".{quoted_table} (\"_oo_pos\", {order_sql}, {region_sql}, {secret_sql}) \
         VALUES (($1::text)::numeric, $2, $3, $4)"
    );
    let tx = client.transaction().await.expect("insert begin");
    let params: [&(dyn ToSql + Sync); 4] = [&"1", &42_i32, &"west", &"hidden-value"];
    tx.execute(&insert, &params).await.expect("insert shadow row");
    tx.commit().await.expect("insert commit");
    Shadow {
        table,
        order_id,
        region,
        secret,
    }
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

async fn lookup(client: &Client, level: &str, namespace: &str, table: &str, column: &str) -> String {
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

fn sample(role: &str, login: &str, row_filter: bool, stale: bool) -> ColumnAclPolicy {
    let mut columns = vec![
        column("order_id", PiiClass::None),
        column("region", PiiClass::None),
    ];
    if role == "orphan" {
        columns.truncate(1);
    }
    ColumnAclPolicy {
        project: SOURCE.to_string(),
        config_digest: CONFIG.to_string(),
        generation: if stale {
            STALE_GENERATION
        } else {
            CURRENT_GENERATION
        },
        role: role.to_string(),
        login: login.to_string(),
        table: LogicalTable {
            namespace: NAMESPACE.to_string(),
            name: TABLE.to_string(),
            columns,
        },
        row_filter: row_filter.then(|| RowFilter::Claims {
            column: "region".to_string(),
            claim: "region".to_string(),
        }),
    }
}

fn unknown_pii() -> ColumnAclPolicy {
    let mut policy = sample("pii_secret", LOGIN, false, false);
    policy.table.columns = vec![column(
        "secret",
        PiiClass::Unknown,
    )];
    policy
}

fn clerk_policy() -> ColumnAclPolicy {
    let mut policy = sample("clerk", LOGIN, false, false);
    policy.table.columns.truncate(1);
    policy
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

async fn assert_catalog(client: &Client, shadow: &Shadow, oracle: &mut Oracle) {
    let row = client
        .query_one(
            "SELECT rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, rolbypassrls, rolreplication \
             FROM pg_catalog.pg_roles WHERE rolname = 'oo_r_dispatcher'",
            &[],
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
    assert_eq!(flags, [false; 7]);
    let member = client
        .query_one(
            "SELECT member_role.rolname, inherit_option, set_option, admin_option \
             FROM pg_catalog.pg_auth_members AS membership \
             JOIN pg_catalog.pg_roles AS granted ON granted.oid = membership.roleid \
             JOIN pg_catalog.pg_roles AS member_role ON member_role.oid = membership.member \
             JOIN pg_catalog.pg_roles AS grantor_role ON grantor_role.oid = membership.grantor \
             WHERE granted.rolname = 'oo_r_dispatcher' AND member_role.rolname = $1",
            &[&LOGIN],
        )
        .await
        .expect("membership");
    let member_name: String = member.get(0);
    let inherit: bool = member.get(1);
    let set_option: bool = member.get(2);
    let admin_option: bool = member.get(3);
    assert_eq!(member_name, LOGIN);
    assert!(!inherit);
    assert!(set_option);
    assert!(!admin_option);
    let grantor: String = client
        .query_one(
            "SELECT grantor_role.rolname \
             FROM pg_catalog.pg_auth_members AS membership \
             JOIN pg_catalog.pg_roles AS granted ON granted.oid = membership.roleid \
             JOIN pg_catalog.pg_roles AS member_role ON member_role.oid = membership.member \
             JOIN pg_catalog.pg_roles AS grantor_role ON grantor_role.oid = membership.grantor \
             WHERE granted.rolname = 'oo_r_dispatcher' AND member_role.rolname = $1",
            &[&LOGIN],
        )
        .await
        .expect("membership grantor")
        .get(0);
    assert_eq!(grantor, ADMIN);
    oracle.fact("membership_grantor", &grantor);
    oracle.fact("membership", "inherit=false set=true admin=false");

    let allowed = column_privileges(client, "oo_r_dispatcher", &shadow.table).await;
    assert_eq!(
        allowed,
        vec![shadow.order_id.clone(), shadow.region.clone()]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
    let clerk = column_privileges(client, "oo_r_clerk", &shadow.table).await;
    assert_eq!(clerk, std::collections::BTreeSet::from([shadow.order_id.clone()]));
    assert_eq!(table_privileges(client, "oo_r_dispatcher", &shadow.table).await, 0);
    assert_eq!(table_privileges(client, "PUBLIC", &shadow.table).await, 0);
    assert_eq!(table_privileges(client, LOGIN, &shadow.table).await, 0);
    assert_eq!(
        public_column_privileges(client, &shadow.table).await,
        0
    );
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
    assert!(!relacl.contains("oo_r_dispatcher"));
    assert!(!relacl.contains(LOGIN));
    assert!(!relacl.contains("{="), "{relacl}");
    assert!(!relacl.contains(",="), "{relacl}");
    oracle.fact("relacl", &relacl);
    let acls = client
        .query(
            "SELECT attribute.attname::text, COALESCE(attribute.attacl::text, 'null') \
             FROM pg_catalog.pg_attribute AS attribute \
             JOIN pg_catalog.pg_class AS relation ON relation.oid = attribute.attrelid \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = '_oo' AND relation.relname = $1 AND attribute.attnum > 0 \
               AND NOT attribute.attisdropped \
             ORDER BY attribute.attname",
            &[&shadow.table],
        )
        .await
        .expect("attacl");
    for row in acls {
        let name: String = row.get(0);
        let acl: String = row.get(1);
        oracle.fact(&format!("attacl.{name}"), &acl);
        if name == shadow.order_id || name == shadow.region {
            assert!(acl.contains("oo_r_dispatcher=r/"), "{name} {acl}");
        } else {
            assert!(!acl.contains("oo_r_dispatcher"), "{name} {acl}");
        }
        assert!(!acl.contains("{=") && !acl.contains(",="), "{name} public acl {acl}");
    }
    let session: String = client
        .query_one("SELECT session_user::text", &[])
        .await
        .expect("session")
        .get(0);
    assert_eq!(session, ADMIN);
    oracle.fact("installer_session_user", &session);
}

async fn column_privileges(
    client: &Client,
    role: &str,
    table: &str,
) -> std::collections::BTreeSet<String> {
    let rows = client
        .query(
            "SELECT column_name::text FROM information_schema.column_privileges \
             WHERE grantee = $1 AND table_schema = '_oo' AND table_name = $2 AND privilege_type = 'SELECT' \
             ORDER BY column_name",
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
    let order_sql = quote_physical(&shadow.order_id, "pc_");
    let region_sql = quote_physical(&shadow.region, "pc_");
    let secret_sql = quote_physical(&shadow.secret, "pc_");
    let selected = format!("SELECT {order_sql}::int4, {region_sql}::text FROM \"_oo\".{table}");
    let row = query_as_role(client, "oo_r_dispatcher", &selected).await;
    assert_eq!(row.get::<_, i32>(0), 42);
    assert_eq!(row.get::<_, String>(1), "west");
    oracle.fact("dispatcher_order_id", "42");
    oracle.fact("dispatcher_region", "west");
    assert_eq!(backend_pid(client).await, pid);

    expect_42501(
        client,
        Some("oo_r_dispatcher"),
        &format!("SELECT {secret_sql}::text FROM \"_oo\".{table}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_dispatcher"),
        &format!("SELECT \"_oo_pos\" FROM \"_oo\".{table}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_dispatcher"),
        &format!("SELECT \"extra\" FROM \"_oo\".{table}"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_dispatcher"),
        &format!("INSERT INTO \"_oo\".{table} (\"_oo_pos\") VALUES (2)"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_dispatcher"),
        &format!("UPDATE \"_oo\".{table} SET {region_sql} = 'east'"),
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_dispatcher"),
        "SELECT count(*) FROM _oo.physical_mappings",
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_dispatcher"),
        "SELECT count(*) FROM _oo.full_import_run",
    )
    .await;
    expect_42501(
        client,
        Some("oo_r_clerk"),
        &format!("SELECT {region_sql}::text FROM \"_oo\".{table}"),
    )
    .await;
    let clerk_row = query_as_role(
        client,
        "oo_r_clerk",
        &format!("SELECT {order_sql}::int4 FROM \"_oo\".{table}"),
    )
    .await;
    assert_eq!(clerk_row.get::<_, i32>(0), 42);
    expect_42501(
        client,
        None,
        &format!("SELECT {order_sql}::int4 FROM \"_oo\".{table}"),
    )
    .await;
    assert_eq!(backend_pid(client).await, pid);
    let current: String = client
        .query_one("SELECT current_user::text", &[])
        .await
        .expect("current user")
        .get(0);
    assert_eq!(current, LOGIN);
    oracle.fact("reader_pid", &pid.to_string());
    oracle.fact("hidden_secret", "42501");
    oracle.fact("hidden_pos", "42501");
    oracle.fact("hidden_extra", "42501");
    oracle.fact("write_insert", "42501");
    oracle.fact("write_update", "42501");
    oracle.fact("metadata_mappings", "42501");
    oracle.fact("metadata_run", "42501");
    oracle.fact("clerk_region", "42501");
    oracle.fact("login_without_role", "42501");
    oracle.fact("same_backend_after_rollback", "true");
}

async fn query_as_role(
    client: &mut Client,
    role: &str,
    sql: &str,
) -> tokio_postgres::Row {
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

fn quote_role(role: &str) -> String {
    assert!(
        role.bytes().all(|byte| {
            byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit()
        }) && !role.is_empty()
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
