//! Process checks for `oo-pipeline catalog-pg`, plus one private PostgreSQL run.
//!
//! The ignored test uses the binary, two selected tables, and verify-full TLS.
//! It feeds that catalog to `oo-builder captured-pg-draft`. Error cases do not
//! open a database.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};

const SECRET: &str = "s3cret-a165-marker";
const TABLE_COMMENT: &str = "comment-body-a165-customer-table";
const EMAIL_COMMENT: &str = "comment-body-a165-email-keel";
const FK_COMMENT: &str = "comment-body-a165-fk-note";

#[test]
fn usage_exits_2_without_echoing_arguments() {
    let (code, out, err) = run(&[], b"");
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(err.trim(), "oo-pipeline: usage");

    let (code, out, err) = run(&["inspect"], b"");
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(err.trim(), "oo-pipeline: usage");

    let (code, out, err) = run(&["catalog-pg", SECRET], b"");
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(err.trim(), "oo-pipeline: usage");
    assert!(!err.contains(SECRET));
    assert!(!out.contains(SECRET));

    let (code, out, err) = run(&["import-pg", SECRET], b"");
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(err.trim(), "oo-pipeline: usage");
    assert!(!err.contains(SECRET));
}

#[test]
fn malformed_version_and_unknown_field_keep_the_secret() {
    assert_input(b"{", "invalid input");
    assert_input(b"\xff", "invalid input");
    assert_input(
        br#"{"protocol":"oo.pipeline.catalog-pg/v1","protocol":"oo.pipeline.catalog-pg/v1"}"#,
        "invalid input",
    );
    let mut body = minimal();
    body["protocol"] = json!("oo.pipeline.catalog-pg/v9");
    assert_body(&body, "invalid version");

    body = minimal();
    body["target"] = json!({"password": SECRET});
    let (code, out, err) = run(&["catalog-pg"], body.to_string().as_bytes());
    assert_eq!(code, 1, "{err}");
    assert!(out.is_empty());
    assert!(err.contains("invalid input"));
    assert!(!err.contains(SECRET));
    assert!(!out.contains(SECRET));

    let dir = scratch("files");
    let missing = dir.join("zz-absent-a165-path");
    body = minimal();
    body["source"]["password_file"] = json!(missing.display().to_string());
    let (code, out, err) = run(&["catalog-pg"], body.to_string().as_bytes());
    assert_eq!(code, 1, "{err}");
    assert!(out.is_empty());
    assert!(err.contains("source password file"));
    assert!(!err.contains("zz-absent-a165-path"));
    assert!(!err.contains(SECRET));
}

#[tokio::test]
#[ignore = "one private PostgreSQL 17.10; OO_CATALOG_PG_HOST must be set by the deciding run"]
async fn catalog_pg_binary_on_private_postgres() {
    let mut harness = Harness::connect().await;
    let result = harness.exercise().await;
    harness.cleanup().await;
    result.expect("catalog-pg live");
}

struct Harness {
    host: String,
    port: u16,
    admin_user: String,
    admin_password: String,
    reader_password: String,
    writer_password: String,
    ca: Vec<u8>,
    ca_file: PathBuf,
    reader_file: PathBuf,
    writer_file: PathBuf,
    evidence: PathBuf,
    postgres: sqlx::postgres::PgConnection,
    source: sqlx::postgres::PgConnection,
}

impl Harness {
    async fn connect() -> Self {
        let host = env::var("OO_CATALOG_PG_HOST").expect("OO_CATALOG_PG_HOST");
        let port: u16 = env::var("OO_CATALOG_PG_PORT").expect("OO_CATALOG_PG_PORT").parse().unwrap();
        let admin_user = env::var("OO_CATALOG_PG_ADMIN_USER").expect("OO_CATALOG_PG_ADMIN_USER");
        let admin_password = password("OO_CATALOG_PG_ADMIN_PASSWORD_FILE");
        let reader_password = password("OO_CATALOG_PG_READER_PASSWORD_FILE");
        let writer_password = password("OO_CATALOG_PG_WRITER_PASSWORD_FILE");
        let ca_file = PathBuf::from(env::var("OO_CATALOG_PG_CA_FILE").expect("OO_CATALOG_PG_CA_FILE"));
        let ca = fs::read(&ca_file).expect("ca");
        let evidence = PathBuf::from(
            env::var("OO_CATALOG_PG_EVIDENCE").unwrap_or_else(|_| "/tmp/oo-a165-catalog-evidence".to_string()),
        );
        fs::create_dir_all(&evidence).unwrap();
        let reader_file = PathBuf::from(env::var("OO_CATALOG_PG_READER_PASSWORD_FILE").unwrap());
        let writer_file = PathBuf::from(env::var("OO_CATALOG_PG_WRITER_PASSWORD_FILE").unwrap());
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
            writer_password,
            ca,
            ca_file,
            reader_file,
            writer_file,
            evidence,
            postgres,
            source,
        }
    }

    async fn exercise(&mut self) -> Result<(), String> {
        self.setup().await;
        self.reader_catalog().await?;
        self.writer_refused().await?;
        self.writer_can_insert().await?;
        self.quiet_source().await?;
        Ok(())
    }

    async fn setup(&mut self) {
        reset(&mut self.postgres).await;
        role(&mut self.postgres, "oo_a165_reader", &self.reader_password).await;
        role(&mut self.postgres, "oo_a165_writer", &self.writer_password).await;
        raw(
            &mut self.postgres,
            "CREATE DATABASE oo_a165_src TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'",
        )
        .await;
        self.source = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            "oo_a165_src",
            &self.ca,
        )
        .await;
        raw(&mut self.source, SOURCE_SQL).await;
        assert!(
            !flag(
                &mut self.source,
                "SELECT has_table_privilege('oo_a165_reader', 'app.order_row', 'INSERT')"
            )
            .await
        );
        assert!(
            flag(
                &mut self.source,
                "SELECT has_table_privilege('oo_a165_writer', 'app.order_row', 'INSERT')"
            )
            .await
        );
    }

    async fn reader_catalog(&mut self) -> Result<(), String> {
        let (code, out, err) = run(&["catalog-pg"], &self.request("oo_a165_reader", &self.reader_file));
        self.reject_secrets(&out, &err)?;
        self.reject_comments(&out, &err)?;
        fs::write(self.evidence.join("reader.stdout"), &out).unwrap();
        fs::write(self.evidence.join("reader.stderr"), &err).unwrap();
        fs::write(self.evidence.join("reader.exit"), code.to_string()).unwrap();
        if code != 0 {
            return Err(format!("reader code={code} err={err}"));
        }
        if out.ends_with('\n') || out.ends_with('\r') {
            return Err("catalog stdout has a trailing newline".to_string());
        }
        let doc: Value = serde_json::from_str(&out).map_err(|err| err.to_string())?;
        self.check_catalog_shape(&doc)?;
        self.compare_oracle(&doc).await?;
        self.compose_draft(out.as_bytes()).await?;
        self.quiet_source().await?;
        Ok(())
    }

    async fn writer_refused(&mut self) -> Result<(), String> {
        let (code, out, err) = run(&["catalog-pg"], &self.request("oo_a165_writer", &self.writer_file));
        self.reject_secrets(&out, &err)?;
        self.reject_comments(&out, &err)?;
        fs::write(self.evidence.join("writer.stdout"), &out).unwrap();
        fs::write(self.evidence.join("writer.stderr"), &err).unwrap();
        fs::write(self.evidence.join("writer.exit"), code.to_string()).unwrap();
        if code != 1 || !out.is_empty() || err.trim() != "oo-pipeline: registration refused" {
            return Err(format!("writer code={code} err={err}"));
        }
        self.quiet_source().await?;
        Ok(())
    }

    async fn writer_can_insert(&mut self) -> Result<(), String> {
        raw(&mut self.source, "INSERT INTO app.customer (id, email) VALUES (1, 'kept')").await;
        let mut writer = dial(
            &self.host,
            self.port,
            "oo_a165_writer",
            &self.writer_password,
            "oo_a165_src",
            &self.ca,
        )
        .await;
        raw(&mut writer, "INSERT INTO app.order_row (id, customer_id) VALUES (1, 1)").await;
        let mut reader = dial(
            &self.host,
            self.port,
            "oo_a165_reader",
            &self.reader_password,
            "oo_a165_src",
            &self.ca,
        )
        .await;
        let err = sqlx::query("INSERT INTO app.customer (id, email) VALUES (9, 'nope')")
            .execute(&mut reader)
            .await
            .expect_err("reader insert");
        let sqlx::Error::Database(db) = err else {
            return Err("reader insert error kind".to_string());
        };
        if db.code().as_deref() != Some("42501") {
            return Err(format!("reader insert sqlstate {:?}", db.code()));
        }
        let reader_writes = text(
            &mut self.source,
            "SELECT count(*)::text FROM app.write_audit WHERE who = 'oo_a165_reader'",
        )
        .await;
        let writer_writes = text(
            &mut self.source,
            "SELECT count(*)::text FROM app.write_audit WHERE who = 'oo_a165_writer'",
        )
        .await;
        if reader_writes != "0" || writer_writes != "1" {
            return Err(format!("audit reader={reader_writes} writer={writer_writes}"));
        }
        Ok(())
    }

    fn check_catalog_shape(&self, doc: &Value) -> Result<(), String> {
        let object = doc.as_object().ok_or("catalog is not an object")?;
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let expect = [
            "captured_at",
            "core_contract",
            "database",
            "hash_algorithm",
            "ident_algorithm",
            "protocol",
            "schema_hash",
            "source_id",
            "tables",
            "type_conversion",
        ];
        if keys != expect {
            return Err(format!("catalog keys {keys:?}"));
        }
        if doc["protocol"] != "oo.source.pgsql/v1"
            || doc["core_contract"] != "oo.config/v1"
            || doc["hash_algorithm"] != "pg-catalog-hash/v1"
            || doc["ident_algorithm"] != "pgident/v1"
            || doc["type_conversion"] != "unsupported"
            || doc["source_id"] != "a165src"
            || doc["database"] != "oo_a165_src"
        {
            return Err(format!("catalog header {doc}"));
        }
        let captured = doc["captured_at"].as_str().unwrap_or("");
        if captured.len() != 27 || !captured.ends_with('Z') {
            return Err(format!("captured_at {captured}"));
        }
        let hash = doc["schema_hash"].as_str().unwrap_or("");
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
            return Err(format!("schema_hash {hash}"));
        }
        Ok(())
    }

    async fn compare_oracle(&mut self, doc: &Value) -> Result<(), String> {
        let tables = doc["tables"].as_array().ok_or("tables")?;
        if tables.len() != 2 {
            return Err(format!("table count {}", tables.len()));
        }
        let oracle = self.oracle().await?;
        let oracle_bytes = serde_json::to_vec_pretty(&oracle).map_err(|err| err.to_string())?;
        let oracle_text = String::from_utf8(oracle_bytes.clone()).map_err(|err| err.to_string())?;
        self.reject_comments(&oracle_text, "")?;
        fs::write(self.evidence.join("oracle.json"), oracle_bytes).unwrap();
        for table in tables {
            let name = table["name"].as_str().ok_or("table name")?.to_string();
            let expect = oracle.get(&name).ok_or_else(|| format!("missing oracle {name}"))?;
            self.compare_table(table, expect)?;
        }
        Ok(())
    }

    fn compare_table(&self, table: &Value, expect: &OracleTable) -> Result<(), String> {
        if table["schema"] != "app" || table["relkind"] != "r" {
            return Err(format!("table identity {table}"));
        }
        assert_hash_only(&table["comment"], expect.comment.as_ref())?;
        let columns = table["columns"].as_array().ok_or("columns")?;
        if columns.len() != expect.columns.len() {
            return Err(format!("column count {}", columns.len()));
        }
        for (column, item) in columns.iter().zip(&expect.columns) {
            if column["attnum"] != item.attnum
                || column["name"] != item.name
                || column["type_oid"] != item.type_oid
                || column["type_name"] != item.type_name
                || column["not_null"] != item.not_null
            {
                return Err(format!("column {column}"));
            }
            if column["current_privileges"]["select"] != true || column["current_privileges"]["insert"] != false {
                return Err(format!("privileges {column}"));
            }
            assert_hash_only(&column["comment"], item.comment.as_ref())?;
        }
        let pk = table["primary_key"].as_array().ok_or("pk")?;
        if pk.len() != expect.primary_key.len() {
            return Err("primary key width".to_string());
        }
        for (got, item) in pk.iter().zip(&expect.primary_key) {
            if got["attnum"] != item.0 || got["name"] != item.1 {
                return Err(format!("primary key {got}"));
            }
        }
        if table["primary_key_nulls_not_distinct"] != false {
            return Err("nulls not distinct".to_string());
        }
        let fks = table["foreign_keys"].as_array().ok_or("fks")?;
        if fks.len() != expect.foreign_keys.len() {
            return Err(format!("fk count {}", fks.len()));
        }
        for (got, item) in fks.iter().zip(&expect.foreign_keys) {
            if got["name"] != item.name {
                return Err(format!("fk name {got}"));
            }
            let pairs = got["columns"].as_array().ok_or("fk columns")?;
            if pairs.len() != 1 {
                return Err("fk width".to_string());
            }
            let pair = &pairs[0];
            if pair["attnum"] != item.attnum
                || pair["name"] != item.column
                || pair["ref_schema"] != "app"
                || pair["ref_table"] != item.ref_table
                || pair["ref_attnum"] != item.ref_attnum
                || pair["ref_name"] != item.ref_name
            {
                return Err(format!("fk pair {pair}"));
            }
        }
        Ok(())
    }

    async fn oracle(&mut self) -> Result<std::collections::BTreeMap<String, OracleTable>, String> {
        let mut out = std::collections::BTreeMap::new();
        let columns = sqlx::query(
            "SELECT c.relname::text, a.attnum::int4, a.attname::text, a.atttypid::int8, t.typname::text, a.attnotnull, pg_catalog.col_description(a.attrelid, a.attnum) \
             FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_catalog.pg_type t ON t.oid = a.atttypid \
             WHERE n.nspname = 'app' AND c.relname IN ('customer', 'order_row') AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY c.relname, a.attnum",
        )
        .fetch_all(&mut self.source)
        .await
        .map_err(|err| err.to_string())?;
        for row in columns {
            let table: String = row.try_get(0).map_err(|err| err.to_string())?;
            let entry = out.entry(table).or_insert_with(OracleTable::default);
            let comment: Option<String> = row.try_get(6).map_err(|err| err.to_string())?;
            entry.columns.push(OracleColumn {
                attnum: row.try_get(1).map_err(|err| err.to_string())?,
                name: row.try_get(2).map_err(|err| err.to_string())?,
                type_oid: row.try_get(3).map_err(|err| err.to_string())?,
                type_name: row.try_get(4).map_err(|err| err.to_string())?,
                not_null: row.try_get(5).map_err(|err| err.to_string())?,
                comment: comment.map(|text| Evidence::from_text(&text)),
            });
        }
        let tables = sqlx::query(
            "SELECT c.relname::text, pg_catalog.obj_description(c.oid, 'pg_class'), i.indnullsnotdistinct \
             FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_catalog.pg_index i ON i.indrelid = c.oid AND i.indisprimary \
             WHERE n.nspname = 'app' AND c.relname IN ('customer', 'order_row')",
        )
        .fetch_all(&mut self.source)
        .await
        .map_err(|err| err.to_string())?;
        for row in tables {
            let name: String = row.try_get(0).map_err(|err| err.to_string())?;
            let comment: Option<String> = row.try_get(1).map_err(|err| err.to_string())?;
            let nulls: bool = row.try_get(2).map_err(|err| err.to_string())?;
            let entry = out.get_mut(&name).ok_or_else(|| format!("table {name}"))?;
            entry.comment = comment.map(|text| Evidence::from_text(&text));
            if nulls {
                return Err(format!("{name} nulls not distinct"));
            }
        }
        let keys = sqlx::query(
            "SELECT c.relname::text, a.attnum::int4, a.attname::text \
             FROM pg_catalog.pg_index i \
             JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS u(attnum, ord) ON true \
             JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = u.attnum \
             WHERE n.nspname = 'app' AND c.relname IN ('customer', 'order_row') AND i.indisprimary \
             ORDER BY c.relname, u.ord",
        )
        .fetch_all(&mut self.source)
        .await
        .map_err(|err| err.to_string())?;
        for row in keys {
            let name: String = row.try_get(0).map_err(|err| err.to_string())?;
            let entry = out.get_mut(&name).ok_or_else(|| format!("pk {name}"))?;
            entry.primary_key.push((
                row.try_get(1).map_err(|err| err.to_string())?,
                row.try_get(2).map_err(|err| err.to_string())?,
            ));
        }
        self.attach_foreign_keys(&mut out).await?;
        let customer = out.get("customer").ok_or("customer oracle")?;
        let order_row = out.get("order_row").ok_or("order oracle")?;
        if customer.comment.as_ref().map(|item| item.sha256.as_str()) != Some(sha256_hex(TABLE_COMMENT).as_str())
            || order_row.foreign_keys.len() != 1
            || !customer.foreign_keys.is_empty()
        {
            return Err("oracle shape".to_string());
        }
        Ok(out)
    }

    async fn attach_foreign_keys(
        &mut self,
        out: &mut std::collections::BTreeMap<String, OracleTable>,
    ) -> Result<(), String> {
        let fks = sqlx::query(
            "SELECT src.relname::text, con.conname::text, la.attnum::int4, la.attname::text, rt.relname::text, ra.attnum::int4, ra.attname::text \
             FROM pg_catalog.pg_constraint con \
             JOIN pg_catalog.pg_class src ON src.oid = con.conrelid \
             JOIN pg_catalog.pg_namespace sn ON sn.oid = src.relnamespace \
             JOIN pg_catalog.pg_class rt ON rt.oid = con.confrelid \
             JOIN LATERAL unnest(con.conkey, con.confkey) WITH ORDINALITY AS pair(local, remote, ord) ON true \
             JOIN pg_catalog.pg_attribute la ON la.attrelid = src.oid AND la.attnum = pair.local \
             JOIN pg_catalog.pg_attribute ra ON ra.attrelid = rt.oid AND ra.attnum = pair.remote \
             WHERE con.contype = 'f' AND sn.nspname = 'app' \
             ORDER BY src.relname, con.conname, pair.ord",
        )
        .fetch_all(&mut self.source)
        .await
        .map_err(|err| err.to_string())?;
        for row in fks {
            let name: String = row.try_get(0).map_err(|err| err.to_string())?;
            let entry = out.get_mut(&name).ok_or_else(|| format!("fk {name}"))?;
            entry.foreign_keys.push(OracleFk {
                name: row.try_get(1).map_err(|err| err.to_string())?,
                attnum: row.try_get(2).map_err(|err| err.to_string())?,
                column: row.try_get(3).map_err(|err| err.to_string())?,
                ref_table: row.try_get(4).map_err(|err| err.to_string())?,
                ref_attnum: row.try_get(5).map_err(|err| err.to_string())?,
                ref_name: row.try_get(6).map_err(|err| err.to_string())?,
            });
        }
        Ok(())
    }

    async fn compose_draft(&self, catalog: &[u8]) -> Result<(), String> {
        let builder = builder_bin();
        if !builder.is_file() {
            return Err(format!("oo-builder is not built at {}", builder.display()));
        }
        let (code, out, err) = run_at(&builder, &["captured-pg-draft"], catalog);
        self.reject_secrets(&out, &err)?;
        self.reject_comments(&out, &err)?;
        fs::write(self.evidence.join("draft.stdout"), &out).unwrap();
        fs::write(self.evidence.join("draft.stderr"), &err).unwrap();
        fs::write(self.evidence.join("draft.exit"), code.to_string()).unwrap();
        if code != 0 || !err.is_empty() {
            return Err(format!("draft code={code} err={err}"));
        }
        let (again, again_out, again_err) = run_at(&builder, &["captured-pg-draft"], catalog);
        self.reject_secrets(&again_out, &again_err)?;
        self.reject_comments(&again_out, &again_err)?;
        fs::write(self.evidence.join("draft-again.stdout"), &again_out).unwrap();
        fs::write(self.evidence.join("draft-again.exit"), again.to_string()).unwrap();
        if again != 0 || again_out != out || !again_err.is_empty() {
            return Err(format!("draft is not deterministic code={again} err={again_err}"));
        }
        let catalog_value: Value = serde_json::from_slice(catalog).map_err(|err| err.to_string())?;
        let review_in = serde_json::to_vec(&json!({"catalog": catalog_value})).map_err(|err| err.to_string())?;
        let (review_code, review_out, review_err) = run_at(&builder, &["review-pg-draft"], &review_in);
        self.reject_secrets(&review_out, &review_err)?;
        self.reject_comments(&review_out, &review_err)?;
        fs::write(self.evidence.join("review.stdout"), &review_out).unwrap();
        fs::write(self.evidence.join("review.stderr"), &review_err).unwrap();
        fs::write(self.evidence.join("review.exit"), review_code.to_string()).unwrap();
        if review_code != 0 || review_out != out || !review_err.is_empty() {
            return Err(format!("review code={review_code} err={review_err}"));
        }
        let doc: Value = serde_json::from_str(&out).map_err(|err| err.to_string())?;
        if doc["protocol"] != "oo.builder.captured-pg-draft-command/v1" {
            return Err("draft protocol".to_string());
        }
        if doc["wrapper"]["row_import"] != "not_proven"
            || doc["wrapper"]["publication"] != "unavailable"
            || doc["wrapper"]["grants"] != "not_authorized"
            || doc["wrapper"]["source_connection"] != "not_used"
            || doc["wrapper"]["live_catalog_review"] != "pending"
        {
            return Err(format!("wrapper {}", doc["wrapper"]));
        }
        if doc["draft"]["provenance"]["publication"] != "unavailable"
            || doc["draft"]["provenance"]["pg_catalog_integration"] != "not_accepted"
        {
            return Err("draft provenance".to_string());
        }
        let datasets = doc["draft"]["generated"]["datasets"].as_array().ok_or("datasets")?;
        let mut columns = 0_usize;
        for dataset in datasets {
            for column in dataset["columns"].as_array().ok_or("dataset columns")? {
                columns += 1;
                if !column["pii"].is_null() {
                    return Err(format!("pii {}", column["name"]));
                }
            }
        }
        let queue = doc["draft"]["review_queue"].as_array().ok_or("queue")?;
        let pii = queue.iter().filter(|item| item["kind"] == "pii_unknown").count();
        if pii != columns || columns == 0 {
            return Err(format!("pii queue {pii} columns {columns}"));
        }
        if queue.iter().any(|item| {
            item["kind"] == "pii_unknown"
                && (item["state"] != "pending"
                    || item["publishable"] != false
                    || item["detail"] != "pii_unreviewed")
        }) {
            return Err("pii queue state".to_string());
        }
        let relationships = doc["draft"]["generated"]["relationships"].as_array().ok_or("relationships")?;
        if relationships.len() != 1
            || relationships[0]["provenance"] != "declared_fk"
            || relationships[0]["join_hint"] != "left"
            || !relationships[0]["confidence"].is_null()
            || !relationships[0]["review"].is_null()
        {
            return Err(format!("relationship {}", relationships[0]));
        }
        let fk = queue.iter().filter(|item| item["kind"] == "candidate_relationship").count();
        if fk != 1 {
            return Err(format!("fk queue {fk}"));
        }
        if queue.iter().any(|item| {
            item["kind"] == "candidate_relationship"
                && (item["state"] != "pending"
                    || item["publishable"] != false
                    || item["detail"] != "join_hint_left_unverified")
        }) {
            return Err("fk queue state".to_string());
        }
        fs::write(self.evidence.join("outcome.txt"), "catalog_pg_cli_live_ok\n").unwrap();
        Ok(())
    }

    fn request(&self, username: &str, password_file: &Path) -> Vec<u8> {
        json!({
            "protocol": "oo.pipeline.catalog-pg/v1",
            "source": {
                "source_id": "a165src",
                "host": self.host,
                "port": self.port,
                "database": "oo_a165_src",
                "username": username,
                "secret_ref": "secret://pg/a165/reader",
                "password_file": password_file.display().to_string(),
                "ca_file": self.ca_file.display().to_string(),
                "tables": [
                    {"schema": "app", "table": "customer"},
                    {"schema": "app", "table": "order_row"}
                ]
            }
        })
        .to_string()
        .into_bytes()
    }

    fn reject_secrets(&self, out: &str, err: &str) -> Result<(), String> {
        for secret in [
            self.admin_password.as_str(),
            self.reader_password.as_str(),
            self.writer_password.as_str(),
            "PRIVATE KEY",
        ] {
            if out.contains(secret) || err.contains(secret) {
                return Err("secret appeared in process output".to_string());
            }
        }
        Ok(())
    }

    fn reject_comments(&self, out: &str, err: &str) -> Result<(), String> {
        for body in [TABLE_COMMENT, EMAIL_COMMENT, FK_COMMENT] {
            if out.contains(body) || err.contains(body) {
                return Err("comment body appeared in process output".to_string());
            }
        }
        Ok(())
    }

    async fn quiet_source(&mut self) -> Result<(), String> {
        for _ in 0..40 {
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
        reset(&mut self.postgres).await;
    }
}

#[derive(Default, serde::Serialize)]
struct OracleTable {
    comment: Option<Evidence>,
    columns: Vec<OracleColumn>,
    primary_key: Vec<(i32, String)>,
    foreign_keys: Vec<OracleFk>,
}

#[derive(serde::Serialize)]
struct OracleColumn {
    attnum: i32,
    name: String,
    type_oid: i64,
    type_name: String,
    not_null: bool,
    comment: Option<Evidence>,
}

#[derive(serde::Serialize)]
struct OracleFk {
    name: String,
    attnum: i32,
    column: String,
    ref_table: String,
    ref_attnum: i32,
    ref_name: String,
}

struct Evidence {
    byte_len: u64,
    sha256: String,
}

impl Evidence {
    fn from_text(text: &str) -> Self {
        Self {
            byte_len: u64::try_from(text.len()).unwrap_or(u64::MAX),
            sha256: sha256_hex(text),
        }
    }
}

impl serde::Serialize for Evidence {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Evidence", 2)?;
        state.serialize_field("byte_len", &self.byte_len)?;
        state.serialize_field("sha256", &self.sha256)?;
        state.end()
    }
}

fn assert_hash_only(value: &Value, expect: Option<&Evidence>) -> Result<(), String> {
    match expect {
        None => {
            if !value.is_null() {
                return Err(format!("unexpected comment {value}"));
            }
            Ok(())
        }
        Some(evidence) => {
            let object = value.as_object().ok_or_else(|| format!("comment {value}"))?;
            if object.len() != 2 || !object.contains_key("byte_len") || !object.contains_key("sha256") {
                return Err(format!("comment fields {value}"));
            }
            if value["byte_len"] != evidence.byte_len || value["sha256"] != evidence.sha256 {
                return Err(format!("comment hash {}", value["sha256"]));
            }
            Ok(())
        }
    }
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

const SOURCE_SQL: &str = "\
CREATE EXTENSION IF NOT EXISTS plpgsql;\n\
CREATE SCHEMA app;\n\
CREATE TABLE app.customer (id int4 PRIMARY KEY, email text NOT NULL);\n\
COMMENT ON TABLE app.customer IS 'comment-body-a165-customer-table';\n\
COMMENT ON COLUMN app.customer.email IS 'comment-body-a165-email-keel';\n\
CREATE TABLE app.order_row (\n\
  id int4 PRIMARY KEY,\n\
  customer_id int4 NOT NULL,\n\
  CONSTRAINT order_customer_fk FOREIGN KEY (customer_id) REFERENCES app.customer (id)\n\
);\n\
COMMENT ON COLUMN app.order_row.customer_id IS 'comment-body-a165-fk-note';\n\
CREATE TABLE app.write_audit (who text NOT NULL);\n\
CREATE FUNCTION app.note_write() RETURNS trigger LANGUAGE plpgsql AS $audit$\n\
BEGIN\n\
  INSERT INTO app.write_audit (who) VALUES (current_user);\n\
  RETURN NEW;\n\
END;\n\
$audit$;\n\
CREATE TRIGGER order_row_audit BEFORE INSERT OR UPDATE OR DELETE ON app.order_row\n\
  FOR EACH ROW EXECUTE FUNCTION app.note_write();\n\
GRANT USAGE ON SCHEMA app TO oo_a165_reader, oo_a165_writer;\n\
GRANT SELECT ON app.customer, app.order_row TO oo_a165_reader;\n\
GRANT INSERT ON app.order_row, app.write_audit TO oo_a165_writer;\n\
";

fn minimal() -> Value {
    json!({
        "protocol": "oo.pipeline.catalog-pg/v1",
        "source": {
            "source_id": "a165src",
            "host": "db.example",
            "port": 5432,
            "database": "srcdb",
            "username": "reader",
            "secret_ref": "secret://pg/a165/reader",
            "password_file": "reader-password",
            "ca_file": "ca.crt",
            "tables": [{"schema": "app", "table": "customer"}]
        }
    })
}

fn assert_body(body: &Value, needle: &str) {
    let (code, out, err) = run(&["catalog-pg"], body.to_string().as_bytes());
    assert_eq!(code, 1, "{err}");
    assert!(out.is_empty(), "{out}");
    assert!(err.contains(needle), "{err}");
    assert!(!err.contains(SECRET));
}

fn assert_input(bytes: &[u8], needle: &str) {
    let (code, out, err) = run(&["catalog-pg"], bytes);
    assert_eq!(code, 1, "{err}");
    assert!(out.is_empty());
    assert!(err.contains(needle), "{err}");
}

fn builder_bin() -> PathBuf {
    if let Ok(path) = env::var("OO_BUILDER_BIN") {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_BIN_EXE_oo-pipeline"))
        .parent()
        .expect("pipeline bin dir")
        .join("oo-builder")
}

fn run(args: &[&str], stdin: &[u8]) -> (i32, String, String) {
    run_at(Path::new(env!("CARGO_BIN_EXE_oo-pipeline")), args, stdin)
}

fn run_at(bin: &Path, args: &[&str], stdin: &[u8]) -> (i32, String, String) {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let output = child.wait_with_output().unwrap();
    (
        output.status.code().unwrap_or(9),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn scratch(name: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!("oo-a165-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn password(var: &str) -> String {
    let text = fs::read_to_string(env::var(var).unwrap()).unwrap();
    let trimmed = text.trim().to_string();
    assert!(trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) && !trimmed.is_empty());
    trimmed
}

async fn dial(
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
    database: &str,
    ca: &[u8],
) -> sqlx::postgres::PgConnection {
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

async fn flag(conn: &mut sqlx::postgres::PgConnection, sql: &str) -> bool {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("flag failed: {err}"))
}

async fn reset(admin: &mut sqlx::postgres::PgConnection) {
    raw(admin, "DROP DATABASE IF EXISTS oo_a165_src WITH (FORCE)").await;
    raw(admin, "DROP ROLE IF EXISTS oo_a165_reader").await;
    raw(admin, "DROP ROLE IF EXISTS oo_a165_writer").await;
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
