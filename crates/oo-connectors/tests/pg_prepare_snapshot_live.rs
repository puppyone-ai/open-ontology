//! Public `prepare_snapshot` against one private PostgreSQL 17.10.
//!
//! The connector is the read-only login. Admin SQL is fixture setup and the
//! comparison oracle only.

use std::env;
use std::fs;
use std::time::{Duration, Instant};

use oo_connectors::{
    PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, PgTypePolicy,
    SecretRef, SelectedTable, SnapshotOwner, SourceId, TableSelection,
};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};

#[tokio::test]
#[ignore = "one private PostgreSQL 17.10; OO_IMPORT_PG_HOST must be set by the deciding run"]
async fn prepare_snapshot_reads_the_public_session() {
    let mut harness = Harness::connect().await;
    let result = harness.exercise().await;
    harness.cleanup().await;
    result.expect("prepare_snapshot live");
}

struct Harness {
    host: String,
    port: u16,
    admin_user: String,
    admin_password: String,
    reader_password: String,
    ca: Vec<u8>,
    postgres: sqlx::postgres::PgConnection,
    source: sqlx::postgres::PgConnection,
}

impl Harness {
    async fn connect() -> Self {
        let host = env::var("OO_IMPORT_PG_HOST").expect("OO_IMPORT_PG_HOST");
        let port: u16 = env::var("OO_IMPORT_PG_PORT").expect("OO_IMPORT_PG_PORT").parse().unwrap();
        let admin_user = env::var("OO_IMPORT_PG_ADMIN_USER").expect("OO_IMPORT_PG_ADMIN_USER");
        let admin_password = password("OO_IMPORT_PG_ADMIN_PASSWORD_FILE");
        let reader_password = password("OO_IMPORT_PG_READER_PASSWORD_FILE");
        let ca = fs::read(env::var("OO_IMPORT_PG_CA_FILE").expect("OO_IMPORT_PG_CA_FILE")).expect("ca");
        let mut postgres = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
        let version = text(&mut postgres, "SELECT version()").await;
        assert!(version.contains("17.10"), "{version}");
        let source = dial(&host, port, &admin_user, &admin_password, "postgres", &ca).await;
        Self {
            host,
            port,
            admin_user,
            admin_password,
            reader_password,
            ca,
            postgres,
            source,
        }
    }

    async fn exercise(&mut self) -> Result<(), String> {
        self.setup().await;
        self.reader_prepares().await?;
        self.writer_is_refused().await?;
        self.drift_is_rejected().await?;
        Ok(())
    }

    async fn setup(&mut self) {
        raw(&mut self.postgres, "DROP DATABASE IF EXISTS oo_a162_prep WITH (FORCE)").await;
        for role_name in ["oo_a162_prep_reader", "oo_a162_prep_writer"] {
            raw(&mut self.postgres, &format!("DROP ROLE IF EXISTS {role_name}")).await;
        }
        role(&mut self.postgres, "oo_a162_prep_reader", &self.reader_password).await;
        let writer_password = text(
            &mut self.postgres,
            "SELECT encode(sha256((clock_timestamp()::text || 'prep-writer')::bytea), 'hex')",
        )
        .await;
        role(&mut self.postgres, "oo_a162_prep_writer", &writer_password).await;
        raw(&mut self.postgres, "CREATE DATABASE oo_a162_prep TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
        self.source = dial(&self.host, self.port, &self.admin_user, &self.admin_password, "oo_a162_prep", &self.ca).await;
        raw(&mut self.source, SOURCE_SQL).await;
        fs::write("/tmp/oo-a162-prep-writer", format!("{writer_password}\n")).expect("writer file");
    }

    async fn reader_prepares(&mut self) -> Result<(), String> {
        let connector = reader_connector(&self.host, self.port, &self.reader_password, &self.ca, "a162prep")?;
        let deadline = Instant::now() + Duration::from_secs(45);
        let prepared = connector
            .prepare_snapshot(PgTypePolicy::PRESERVE_ROW, 1, deadline)
            .await
            .map_err(|err| format!("prepare {}", err.fact().unwrap_or("none")))?;
        self.quiet().await?;
        if prepared.expected_principal != "oo_a162_prep_reader" || prepared.expected_principal == self.admin_user {
            return Err("principal was not the reader".to_string());
        }
        if prepared.expected_database != "oo_a162_prep" {
            return Err("database mismatch".to_string());
        }
        let row = sqlx::query("SELECT pg_catalog.pg_is_in_recovery(), pg_catalog.pg_postmaster_start_time()::text")
            .fetch_one(&mut self.source)
            .await
            .map_err(|err| err.to_string())?;
        let recovery: bool = row.try_get(0).map_err(|err| err.to_string())?;
        let postmaster: String = row.try_get(1).map_err(|err| err.to_string())?;
        if prepared.expected_in_recovery != recovery || prepared.expected_postmaster_start != postmaster {
            return Err("node facts mismatch".to_string());
        }
        let columns = column(
            &mut self.source,
            "SELECT a.attnum::text || ':' || a.attname || ':' || a.atttypid::text || ':' || a.atttypmod::text || ':' || a.attnotnull::text FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'app' AND c.relname = 'item' AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
        )
        .await;
        let admitted = &prepared.admitted.tables[0];
        let got: Vec<String> = admitted
            .columns
            .iter()
            .map(|column| {
                format!(
                    "{}:{}:{}:{}:{}",
                    column.attnum, column.name, column.type_oid, column.typmod, column.not_null
                )
            })
            .collect();
        if admitted.schema != "app" || admitted.name != "item" || got != columns || admitted.primary_key != ["id"] {
            return Err(format!("catalog {got:?} pk {:?}", admitted.primary_key));
        }
        let rendered = format!("{prepared:?}");
        if rendered.contains(&self.reader_password) || rendered.contains("BEGIN CERTIFICATE") {
            return Err("prepare debug leaked".to_string());
        }
        let owner = SnapshotOwner::open(prepared.clone())
            .await
            .map_err(|err| format!("open {}", err.fact().unwrap_or("none")))?;
        let binding = owner.binding().clone();
        owner.abort().await.map_err(|err| format!("abort {}", err.fact().unwrap_or("none")))?;
        self.quiet().await?;
        if binding.principal != "oo_a162_prep_reader" || binding.database != "oo_a162_prep" || binding.postmaster_start != postmaster {
            return Err("binding mismatch".to_string());
        }
        Ok(())
    }

    async fn writer_is_refused(&mut self) -> Result<(), String> {
        let writer_password = fs::read_to_string("/tmp/oo-a162-prep-writer").map_err(|err| err.to_string())?;
        let connector = PgConnector::new(config(
            &self.host,
            self.port,
            "oo_a162_prep",
            "oo_a162_prep_writer",
            writer_password.trim(),
            &self.ca,
            "a162prepw",
        )?);
        let err = connector
            .prepare_snapshot(PgTypePolicy::PRESERVE_ROW, 1, Instant::now() + Duration::from_secs(30))
            .await
            .expect_err("writer must be refused");
        if err.fact() != Some("registration_refused") {
            return Err(format!("writer fact {}", err.fact().unwrap_or("none")));
        }
        self.quiet().await?;
        Ok(())
    }

    async fn drift_is_rejected(&mut self) -> Result<(), String> {
        let connector = reader_connector(&self.host, self.port, &self.reader_password, &self.ca, "a162prep")?;
        let prepared = connector
            .prepare_snapshot(PgTypePolicy::PRESERVE_ROW, 2, Instant::now() + Duration::from_secs(45))
            .await
            .map_err(|err| format!("second prepare {}", err.fact().unwrap_or("none")))?;
        self.quiet().await?;
        raw(&mut self.source, "ALTER TABLE app.item ADD COLUMN extra int4").await;
        let mut drifted = prepared;
        drifted.generation = 3;
        let err = SnapshotOwner::open(drifted)
            .await
            .expect_err("changed catalog must fail");
        if err.fact() != Some("catalog_identity") {
            return Err(format!("drift fact {}", err.fact().unwrap_or("none")));
        }
        self.quiet().await?;
        Ok(())
    }

    async fn quiet(&mut self) -> Result<(), String> {
        for _ in 0..20 {
            let n = text(
                &mut self.postgres,
                "SELECT count(*)::text FROM pg_catalog.pg_stat_activity WHERE application_name = 'oo-pg-source'",
            )
            .await;
            if n == "0" {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err("source session remained".to_string())
    }

    async fn cleanup(&mut self) {
        raw(&mut self.postgres, "DROP DATABASE IF EXISTS oo_a162_prep WITH (FORCE)").await;
        raw(&mut self.postgres, "DROP ROLE IF EXISTS oo_a162_prep_reader").await;
        raw(&mut self.postgres, "DROP ROLE IF EXISTS oo_a162_prep_writer").await;
        let _ = fs::remove_file("/tmp/oo-a162-prep-writer");
    }
}

const SOURCE_SQL: &str = "\
CREATE SCHEMA app;\n\
CREATE TABLE app.item (id int4 PRIMARY KEY, label text NOT NULL);\n\
INSERT INTO app.item (id, label) VALUES (1, 'spruce-keel');\n\
GRANT USAGE ON SCHEMA app TO oo_a162_prep_reader, oo_a162_prep_writer;\n\
GRANT SELECT ON app.item TO oo_a162_prep_reader;\n\
GRANT INSERT ON app.item TO oo_a162_prep_writer;\n\
";

fn reader_connector(
    host: &str,
    port: u16,
    password: &str,
    ca: &[u8],
    source_id: &str,
) -> Result<PgConnector, String> {
    Ok(PgConnector::new(config(
        host,
        port,
        "oo_a162_prep",
        "oo_a162_prep_reader",
        password,
        ca,
        source_id,
    )?))
}

fn config(
    host: &str,
    port: u16,
    database: &str,
    username: &str,
    password: &str,
    ca: &[u8],
    source_id: &str,
) -> Result<PgSourceConfig, String> {
    PgSourceConfig::try_new(
        SourceId::new(source_id).map_err(|err| err.fact().unwrap_or("source").to_string())?,
        PgEndpoint::new(host, port, database).map_err(|err| err.fact().unwrap_or("endpoint").to_string())?,
        PgCredentials::new(username, password).map_err(|err| err.fact().unwrap_or("credentials").to_string())?,
        SecretRef::new("secret://pg/a162/prep").map_err(|err| err.fact().unwrap_or("secret").to_string())?,
        PgTlsConfig::from_ca_pem(ca).map_err(|err| err.fact().unwrap_or("ca").to_string())?,
        PgBudget::new(
            Duration::from_secs(30),
            Duration::from_secs(30),
            Duration::from_secs(15),
            1000,
            1024 * 1024,
        )
        .map_err(|err| err.fact().unwrap_or("budget").to_string())?,
        TableSelection::new(vec![SelectedTable::new("app", "item").map_err(|err| err.to_string())?])
            .map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())
}

fn password(var: &str) -> String {
    let text = fs::read_to_string(env::var(var).unwrap()).unwrap();
    let trimmed = text.trim().to_string();
    assert!(trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) && !trimmed.is_empty());
    trimmed
}

async fn dial(host: &str, port: u16, user: &str, pass: &str, database: &str, ca: &[u8]) -> sqlx::postgres::PgConnection {
    let options = PgConnectOptions::from_explicit()
        .host(host)
        .port(port)
        .username(user)
        .password(pass)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    assert!(options.get_options().is_none());
    let options = options.disable_statement_logging();
    sqlx::postgres::PgConnection::connect_with(&options).await.expect("dial")
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

async fn column(conn: &mut sqlx::postgres::PgConnection, sql: &str) -> Vec<String> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_all(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("rows failed: {err}"));
    rows.into_iter().map(|row| row.try_get(0).unwrap()).collect()
}

async fn role(conn: &mut sqlx::postgres::PgConnection, name: &str, secret: &str) {
    assert!(secret.chars().all(|ch| ch.is_ascii_hexdigit()));
    raw(
        conn,
        &format!(
            "CREATE ROLE {name} LOGIN PASSWORD '{secret}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION"
        ),
    )
    .await;
}
