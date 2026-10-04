//! Independent review of `oo-pipeline catalog-pg` on one private PostgreSQL 17.10.
//!
//! The fixture is `pier.berth` and `pier.mooring`. Admin SQL is setup and the
//! oracle. The catalog itself is the `oo-pipeline` process. Its stdout is then
//! passed, unchanged, to the fixed `oo-builder` commands. This file does not
//! open a target and does not install a schema.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
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

const MARKER: &str = "s3cret-a166-marker";
const TABLE_COMMENT: &str = "a166-keel-comment-berth-body";
const LABEL_COMMENT: &str = "a166-keel-comment-label-body";
const MOOR_COMMENT: &str = "a166-keel-comment-moor-body";

#[test]
fn narrow_invalid_strict_and_noargs_exits() {
    let (code, out, err) = run(&[], b"");
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(err.trim(), "oo-pipeline: usage");

    let (code, out, err) = run(&["inspect"], b"");
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(err.trim(), "oo-pipeline: usage");

    let (code, out, err) = run(&["catalog-pg", MARKER], b"");
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(err.trim(), "oo-pipeline: usage");
    assert!(!err.contains(MARKER));

    let (code, out, err) = run(&["import-pg", MARKER], b"");
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(err.trim(), "oo-pipeline: usage");
    assert!(!err.contains(MARKER));

    assert_input(b"", "invalid input");
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
    body["target"] = json!({"password": MARKER});
    let (code, out, err) = run(&["catalog-pg"], body.to_string().as_bytes());
    assert_eq!(code, 1, "{err}");
    assert!(out.is_empty());
    assert!(err.contains("invalid input"));
    assert!(!err.contains(MARKER));
    assert!(!out.contains(MARKER));

    body = minimal();
    body["source"]["tables"] = json!([]);
    assert_body(&body, "invalid input");

    let dir = env::temp_dir().join(format!("oo-a166-narrow-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let missing = dir.join("zz-absent-a166-path");
    body = minimal();
    body["source"]["password_file"] = json!(missing.display().to_string());
    let (code, out, err) = run(&["catalog-pg"], body.to_string().as_bytes());
    assert_eq!(code, 1, "{err}");
    assert!(out.is_empty());
    assert!(err.contains("source password file"));
    assert!(!err.contains("zz-absent-a166-path"));
    assert!(!err.contains(MARKER));
    let _ = fs::remove_dir_all(&dir);
    if let Ok(dir) = env::var("OO_A166_NARROW_EVIDENCE") {
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            Path::new(&dir).join("outcome.txt"),
            "noargs=2\nunknown_command=2\nextra_catalog=2\nextra_import=2\nempty=1\nmalformed=1\nnon_utf8=1\nduplicate_key=1\nbad_version=1\nstrict_target=1\nempty_tables=1\nmissing_password_file=1\nmarker_absent=1\npath_absent=1\n",
        )
        .unwrap();
    }
}

#[tokio::test]
#[ignore = "one private PostgreSQL 17.10; OO_A166_PG_HOST must be set by the deciding run"]
async fn catalog_stdout_matches_postgres_and_the_fixed_builder() {
    let mut harness = Harness::connect().await;
    let result = harness.exercise().await;
    harness.cleanup().await;
    result.expect("a166 catalog review");
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
    lines: Vec<String>,
}

impl Harness {
    async fn connect() -> Self {
        let host = env::var("OO_A166_PG_HOST").expect("OO_A166_PG_HOST");
        let port: u16 = env::var("OO_A166_PG_PORT").expect("OO_A166_PG_PORT").parse().unwrap();
        let admin_user = env::var("OO_A166_PG_ADMIN_USER").expect("OO_A166_PG_ADMIN_USER");
        let admin_password = read_secret("OO_A166_PG_ADMIN_PASSWORD_FILE");
        let reader_password = read_secret("OO_A166_PG_READER_PASSWORD_FILE");
        let writer_password = read_secret("OO_A166_PG_WRITER_PASSWORD_FILE");
        let ca_file = PathBuf::from(env::var("OO_A166_PG_CA_FILE").expect("OO_A166_PG_CA_FILE"));
        let ca = fs::read(&ca_file).expect("ca");
        let evidence = PathBuf::from(env::var("OO_A166_PG_EVIDENCE").expect("OO_A166_PG_EVIDENCE"));
        fs::create_dir_all(&evidence).unwrap();
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
            reader_file: PathBuf::from(env::var("OO_A166_PG_READER_PASSWORD_FILE").unwrap()),
            writer_file: PathBuf::from(env::var("OO_A166_PG_WRITER_PASSWORD_FILE").unwrap()),
            evidence,
            postgres,
            source,
            lines: Vec::new(),
        }
    }

    async fn exercise(&mut self) -> Result<(), String> {
        let result = self.run_steps().await;
        self.flush();
        result
    }

    async fn run_steps(&mut self) -> Result<(), String> {
        self.setup().await?;
        self.reader_catalog().await?;
        self.writer_refused().await?;
        self.writer_can_insert().await?;
        self.note("target_opened=0");
        self.note("target_installed=0");
        Ok(())
    }

    async fn setup(&mut self) -> Result<(), String> {
        reset(&mut self.postgres).await;
        role(&mut self.postgres, "oo_a166_reader", &self.reader_password).await;
        role(&mut self.postgres, "oo_a166_writer", &self.writer_password).await;
        raw(
            &mut self.postgres,
            "CREATE DATABASE oo_a166_src TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'",
        )
        .await;
        self.source = dial(
            &self.host,
            self.port,
            &self.admin_user,
            &self.admin_password,
            "oo_a166_src",
            &self.ca,
        )
        .await;
        raw(&mut self.source, SOURCE_SQL).await;
        let reader_insert = flag(
            &mut self.source,
            "SELECT has_table_privilege('oo_a166_reader', 'pier.mooring', 'INSERT')",
        )
        .await;
        let writer_insert = flag(
            &mut self.source,
            "SELECT has_table_privilege('oo_a166_writer', 'pier.mooring', 'INSERT')",
        )
        .await;
        if reader_insert || !writer_insert {
            return Err(format!("fixture privileges reader_insert={reader_insert} writer_insert={writer_insert}"));
        }
        let pubs = text(&mut self.source, "SELECT count(*)::text FROM pg_catalog.pg_publication").await;
        let oo_schema = text(
            &mut self.source,
            "SELECT count(*)::text FROM pg_catalog.pg_namespace WHERE nspname = '_oo'",
        )
        .await;
        if pubs != "0" || oo_schema != "0" {
            return Err(format!("source already has publication={pubs} _oo={oo_schema}"));
        }
        self.note("source_publication_count=0");
        self.note("source_target_schema=absent");
        Ok(())
    }

    async fn reader_catalog(&mut self) -> Result<(), String> {
        let before = self.database_list().await;
        let (code, out, err) = run(&["catalog-pg"], &self.request("oo_a166_reader", &self.reader_file));
        self.store("reader", code, &out, &err)?;
        if code != 0 {
            return Err(format!("reader code={code}"));
        }
        if out.ends_with('\n') || out.ends_with('\r') {
            return Err("catalog stdout has a trailing newline".to_string());
        }
        let after = self.database_list().await;
        if before != after {
            return Err("database list changed during catalog".to_string());
        }
        self.note("databases_unchanged=1");
        let doc: Value = serde_json::from_str(&out).map_err(|err| err.to_string())?;
        self.check_header(&doc)?;
        self.compare_oracle(&doc).await?;
        self.quiet("after_reader").await?;
        self.compose_draft(out.as_bytes()).await?;
        Ok(())
    }

    async fn writer_refused(&mut self) -> Result<(), String> {
        let before = self.database_list().await;
        let (code, out, err) = run(&["catalog-pg"], &self.request("oo_a166_writer", &self.writer_file));
        self.store("writer", code, &out, &err)?;
        if code != 1 || !out.is_empty() || err.trim() != "oo-pipeline: registration refused" {
            return Err(format!("writer code={code}"));
        }
        let after = self.database_list().await;
        if before != after {
            return Err("database list changed during refusal".to_string());
        }
        let oo_schema = text(
            &mut self.source,
            "SELECT count(*)::text FROM pg_catalog.pg_namespace WHERE nspname = '_oo'",
        )
        .await;
        if oo_schema != "0" {
            return Err("refusal created _oo".to_string());
        }
        self.note("writer_registration=refused");
        self.quiet("after_writer").await?;
        Ok(())
    }

    async fn writer_can_insert(&mut self) -> Result<(), String> {
        raw(&mut self.source, "INSERT INTO pier.berth (id, label) VALUES (1, 'kept')").await;
        let mut writer = dial(
            &self.host,
            self.port,
            "oo_a166_writer",
            &self.writer_password,
            "oo_a166_src",
            &self.ca,
        )
        .await;
        raw(&mut writer, "INSERT INTO pier.mooring (id, berth_id) VALUES (1, 1)").await;
        let mut reader = dial(
            &self.host,
            self.port,
            "oo_a166_reader",
            &self.reader_password,
            "oo_a166_src",
            &self.ca,
        )
        .await;
        let err = sqlx::query("INSERT INTO pier.berth (id, label) VALUES (9, 'nope')")
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
            "SELECT count(*)::text FROM pier.write_audit WHERE who = 'oo_a166_reader'",
        )
        .await;
        let writer_writes = text(
            &mut self.source,
            "SELECT count(*)::text FROM pier.write_audit WHERE who = 'oo_a166_writer'",
        )
        .await;
        if reader_writes != "0" || writer_writes != "1" {
            return Err(format!("audit reader={reader_writes} writer={writer_writes}"));
        }
        self.note("writer_insert=1");
        self.note("reader_insert=42501");
        self.note("source_writes=reader_0_writer_1");
        drop(writer);
        drop(reader);
        Ok(())
    }

    fn check_header(&mut self, doc: &Value) -> Result<(), String> {
        if doc["protocol"] != "oo.source.pgsql/v1"
            || doc["source_id"] != "a166src"
            || doc["database"] != "oo_a166_src"
        {
            return Err("catalog header".to_string());
        }
        let hash = doc["schema_hash"].as_str().unwrap_or("");
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
            return Err("schema_hash shape".to_string());
        }
        self.note(&format!("schema_hash={hash}"));
        self.note(&format!("captured_at_len={}", doc["captured_at"].as_str().unwrap_or("").len()));
        Ok(())
    }

    async fn compare_oracle(&mut self, doc: &Value) -> Result<(), String> {
        let tables = doc["tables"].as_array().ok_or("tables")?;
        if tables.len() != 2 {
            return Err(format!("table count {}", tables.len()));
        }
        let oracle = self.oracle().await?;
        let mut oracle_out = serde_json::Map::new();
        for (name, table) in &oracle {
            oracle_out.insert(name.clone(), table.to_json());
        }
        let oracle_bytes = serde_json::to_vec_pretty(&oracle_out).map_err(|err| err.to_string())?;
        self.store_bytes("oracle.json", &oracle_bytes)?;
        for table in tables {
            let name = table["name"].as_str().ok_or("table name")?;
            let expect = oracle.get(name).ok_or_else(|| format!("missing oracle {name}"))?;
            if table["schema"] != "pier" || table["relkind"] != "r" {
                return Err(format!("table identity {name}"));
            }
            expect.comment.assert_value(&table["comment"])?;
            let columns = table["columns"].as_array().ok_or("columns")?;
            if columns.len() != expect.columns.len() {
                return Err(format!("{name} column count {}", columns.len()));
            }
            for column in columns {
                let attnum = column["attnum"].as_i64().ok_or("attnum")?;
                let item = expect
                    .columns
                    .iter()
                    .find(|item| i64::from(item.attnum) == attnum)
                    .ok_or_else(|| format!("missing column {attnum}"))?;
                if column["name"] != item.name
                    || column["type_oid"] != item.type_oid
                    || column["type_name"] != item.type_name
                    || column["not_null"] != item.not_null
                {
                    return Err(format!("column mismatch {name} {attnum}"));
                }
                if column["current_privileges"]["select"] != item.select
                    || column["current_privileges"]["insert"] != item.insert
                    || column["current_privileges"]["update"] != item.update
                {
                    return Err(format!("privilege mismatch {name} {attnum}"));
                }
                item.comment.assert_value(&column["comment"])?;
            }
            let pk = table["primary_key"].as_array().ok_or("pk")?;
            if pk.len() != expect.primary_key.len() {
                return Err(format!("{name} pk width"));
            }
            for (got, item) in pk.iter().zip(&expect.primary_key) {
                if got["attnum"] != item.0 || got["name"] != item.1 {
                    return Err(format!("{name} pk"));
                }
            }
            let fks = table["foreign_keys"].as_array().ok_or("fks")?;
            if fks.len() != expect.foreign_keys.len() {
                return Err(format!("{name} fk count"));
            }
            for (got, item) in fks.iter().zip(&expect.foreign_keys) {
                if got["name"] != item.name {
                    return Err(format!("{name} fk name"));
                }
                let pairs = got["columns"].as_array().ok_or("fk columns")?;
                if pairs.len() != 1 {
                    return Err(format!("{name} fk width"));
                }
                let pair = &pairs[0];
                if pair["attnum"] != item.attnum
                    || pair["name"] != item.column
                    || pair["ref_schema"] != "pier"
                    || pair["ref_table"] != item.ref_table
                    || pair["ref_attnum"] != item.ref_attnum
                    || pair["ref_name"] != item.ref_name
                {
                    return Err(format!("{name} fk pair"));
                }
            }
        }
        self.note("columns_pk_fk_comment_hash=match");
        Ok(())
    }

    async fn oracle(&mut self) -> Result<BTreeMap<String, OracleTable>, String> {
        let mut out: BTreeMap<String, OracleTable> = BTreeMap::new();
        let columns = sqlx::query(
            "SELECT c.relname::text, a.attnum::int4, a.attname::text, a.atttypid::int8, t.typname::text, a.attnotnull, \
                    pg_catalog.col_description(a.attrelid, a.attnum), \
                    pg_catalog.has_column_privilege('oo_a166_reader', a.attrelid, a.attnum, 'SELECT'), \
                    pg_catalog.has_column_privilege('oo_a166_reader', a.attrelid, a.attnum, 'INSERT'), \
                    pg_catalog.has_column_privilege('oo_a166_reader', a.attrelid, a.attnum, 'UPDATE') \
             FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_catalog.pg_type t ON t.oid = a.atttypid \
             WHERE n.nspname = 'pier' AND c.relname IN ('berth', 'mooring') AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY c.relname, a.attnum",
        )
        .fetch_all(&mut self.source)
        .await
        .map_err(|err| err.to_string())?;
        for row in columns {
            let table: String = row.try_get(0).map_err(|err| err.to_string())?;
            let entry = out.entry(table).or_default();
            let comment: Option<String> = row.try_get(6).map_err(|err| err.to_string())?;
            entry.columns.push(OracleColumn {
                attnum: row.try_get(1).map_err(|err| err.to_string())?,
                name: row.try_get(2).map_err(|err| err.to_string())?,
                type_oid: row.try_get(3).map_err(|err| err.to_string())?,
                type_name: row.try_get(4).map_err(|err| err.to_string())?,
                not_null: row.try_get(5).map_err(|err| err.to_string())?,
                select: row.try_get(7).map_err(|err| err.to_string())?,
                insert: row.try_get(8).map_err(|err| err.to_string())?,
                update: row.try_get(9).map_err(|err| err.to_string())?,
                comment: HashOnly::from_server(comment),
            });
        }
        let tables = sqlx::query(
            "SELECT c.relname::text, pg_catalog.obj_description(c.oid, 'pg_class') \
             FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'pier' AND c.relname IN ('berth', 'mooring')",
        )
        .fetch_all(&mut self.source)
        .await
        .map_err(|err| err.to_string())?;
        for row in tables {
            let name: String = row.try_get(0).map_err(|err| err.to_string())?;
            let comment: Option<String> = row.try_get(1).map_err(|err| err.to_string())?;
            let entry = out.get_mut(&name).ok_or_else(|| format!("table {name}"))?;
            entry.comment = HashOnly::from_server(comment);
        }
        let keys = sqlx::query(
            "SELECT c.relname::text, a.attnum::int4, a.attname::text \
             FROM pg_catalog.pg_index i \
             JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS u(attnum, ord) ON true \
             JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = u.attnum \
             WHERE n.nspname = 'pier' AND c.relname IN ('berth', 'mooring') AND i.indisprimary \
             ORDER BY c.relname, u.ord",
        )
        .fetch_all(&mut self.source)
        .await
        .map_err(|err| err.to_string())?;
        for row in keys {
            let name: String = row.try_get(0).map_err(|err| err.to_string())?;
            let entry = out.get_mut(&name).ok_or_else(|| format!("pk {name}"))?;
            entry.primary_key.push((
                row.try_get::<i32, _>(1).map_err(|err| err.to_string())?,
                row.try_get(2).map_err(|err| err.to_string())?,
            ));
        }
        let fks = sqlx::query(
            "SELECT src.relname::text, con.conname::text, la.attnum::int4, la.attname::text, rt.relname::text, ra.attnum::int4, ra.attname::text \
             FROM pg_catalog.pg_constraint con \
             JOIN pg_catalog.pg_class src ON src.oid = con.conrelid \
             JOIN pg_catalog.pg_namespace sn ON sn.oid = src.relnamespace \
             JOIN pg_catalog.pg_class rt ON rt.oid = con.confrelid \
             JOIN LATERAL unnest(con.conkey, con.confkey) WITH ORDINALITY AS pair(local, remote, ord) ON true \
             JOIN pg_catalog.pg_attribute la ON la.attrelid = src.oid AND la.attnum = pair.local \
             JOIN pg_catalog.pg_attribute ra ON ra.attrelid = rt.oid AND ra.attnum = pair.remote \
             WHERE con.contype = 'f' AND sn.nspname = 'pier' \
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
        let berth = out.get("berth").ok_or("berth oracle")?;
        let mooring = out.get("mooring").ok_or("mooring oracle")?;
        if berth.comment.sha256.as_deref() != Some(sha256_hex(TABLE_COMMENT).as_str())
            || berth.columns.len() != 3
            || !berth.foreign_keys.is_empty()
            || mooring.foreign_keys.len() != 1
            || mooring.foreign_keys[0].name != "mooring_berth_fk"
            || mooring.comment.sha256.is_some()
        {
            return Err("oracle fixture shape".to_string());
        }
        let label = berth.columns.iter().find(|column| column.name == "label").ok_or("label")?;
        let moor = mooring.columns.iter().find(|column| column.name == "berth_id").ok_or("berth_id")?;
        let note = berth.columns.iter().find(|column| column.name == "note").ok_or("note")?;
        if label.comment.sha256.as_deref() != Some(sha256_hex(LABEL_COMMENT).as_str())
            || moor.comment.sha256.as_deref() != Some(sha256_hex(MOOR_COMMENT).as_str())
            || note.comment.sha256.is_some()
            || label.comment.byte_len != Some(u64::try_from(LABEL_COMMENT.len()).unwrap_or(0))
        {
            return Err("oracle comment hash diverged".to_string());
        }
        Ok(out)
    }

    async fn compose_draft(&mut self, catalog: &[u8]) -> Result<(), String> {
        let builder = PathBuf::from(env::var("OO_BUILDER_BIN").expect("OO_BUILDER_BIN"));
        if !builder.is_file() {
            return Err(format!("oo-builder is not built at {}", builder.display()));
        }
        let (code, out, err) = run_at(&builder, &["captured-pg-draft"], catalog);
        self.store_process("draft", code, &out, &err)?;
        if code != 0 || !err.is_empty() {
            return Err(format!("draft code={code}"));
        }
        let (again, again_out, again_err) = run_at(&builder, &["captured-pg-draft"], catalog);
        self.store_process("draft-again", again, &again_out, &again_err)?;
        if again != 0 || again_out != out || !again_err.is_empty() {
            return Err(format!("draft is not deterministic code={again}"));
        }
        let catalog_value: Value = serde_json::from_slice(catalog).map_err(|err| err.to_string())?;
        let review_in = serde_json::to_vec(&json!({"catalog": catalog_value})).map_err(|err| err.to_string())?;
        let (review_code, review_out, review_err) = run_at(&builder, &["review-pg-draft"], &review_in);
        self.store_process("review", review_code, &review_out, &review_err)?;
        if review_code != 0 || review_out != out || !review_err.is_empty() {
            return Err(format!("review code={review_code}"));
        }
        if out.ends_with('\n') || out.ends_with('\r') {
            return Err("draft stdout has a trailing newline".to_string());
        }
        let doc: Value = serde_json::from_str(&out).map_err(|err| err.to_string())?;
        if doc["protocol"] != "oo.builder.captured-pg-draft-command/v1" {
            return Err("draft protocol".to_string());
        }
        if doc["wrapper"]["row_import"] != "not_proven"
            || doc["wrapper"]["publication"] != "unavailable"
            || doc["wrapper"]["grants"] != "not_authorized"
        {
            return Err("wrapper seals".to_string());
        }
        if doc["draft"]["provenance"]["publication"] != "unavailable" {
            return Err("draft publication".to_string());
        }
        let datasets = doc["draft"]["generated"]["datasets"].as_array().ok_or("datasets")?;
        let mut columns = 0_usize;
        for dataset in datasets {
            for column in dataset["columns"].as_array().ok_or("dataset columns")? {
                columns += 1;
                if !column["pii"].is_null() {
                    return Err("pii was not null".to_string());
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
                && (item["state"] != "pending" || item["publishable"] != false || item["detail"] != "pii_unreviewed")
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
            return Err("relationship".to_string());
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
        self.note(&format!("draft_bytes={}", out.len()));
        self.note("draft_equal=captured_captured_review");
        self.note("pii=unknown");
        self.note("fk=tentative");
        self.note("row_import=not_proven");
        self.note("grants=not_authorized");
        self.note("publication=unavailable");
        Ok(())
    }

    fn request(&self, username: &str, password_file: &Path) -> Vec<u8> {
        json!({
            "protocol": "oo.pipeline.catalog-pg/v1",
            "source": {
                "source_id": "a166src",
                "host": self.host,
                "port": self.port,
                "database": "oo_a166_src",
                "username": username,
                "secret_ref": "secret://pg/a166/reader",
                "password_file": password_file.display().to_string(),
                "ca_file": self.ca_file.display().to_string(),
                "tables": [
                    {"schema": "pier", "table": "berth"},
                    {"schema": "pier", "table": "mooring"}
                ]
            }
        })
        .to_string()
        .into_bytes()
    }

    fn store(&self, stem: &str, code: i32, out: &str, err: &str) -> Result<(), String> {
        self.reject(stem, out)?;
        self.reject(stem, err)?;
        fs::write(self.evidence.join(format!("{stem}.stdout")), out).unwrap();
        fs::write(self.evidence.join(format!("{stem}.stderr")), err).unwrap();
        fs::write(self.evidence.join(format!("{stem}.exit")), code.to_string()).unwrap();
        Ok(())
    }

    fn store_process(&self, stem: &str, code: i32, out: &str, err: &str) -> Result<(), String> {
        self.store(stem, code, out, err)
    }

    fn store_bytes(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let text = String::from_utf8_lossy(bytes);
        self.reject(name, &text)?;
        fs::write(self.evidence.join(name), bytes).unwrap();
        Ok(())
    }

    fn reject(&self, label: &str, text: &str) -> Result<(), String> {
        for secret in [
            self.admin_password.as_str(),
            self.reader_password.as_str(),
            self.writer_password.as_str(),
            "PRIVATE KEY",
            TABLE_COMMENT,
            LABEL_COMMENT,
            MOOR_COMMENT,
            "a166-keel-comment",
        ] {
            if text.contains(secret) {
                return Err(format!("{label} contains a comment body or a secret"));
            }
        }
        Ok(())
    }

    async fn database_list(&mut self) -> String {
        text(
            &mut self.postgres,
            "SELECT coalesce(string_agg(datname, ',' ORDER BY datname), '') FROM pg_catalog.pg_database",
        )
        .await
    }

    async fn quiet(&mut self, label: &str) -> Result<(), String> {
        for _ in 0..40 {
            let n = text(
                &mut self.postgres,
                "SELECT count(*)::text FROM pg_catalog.pg_stat_activity WHERE application_name = 'oo-pg-source'",
            )
            .await;
            if n == "0" {
                self.note(&format!("connections_{label}=0"));
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(format!("source session remained {label}"))
    }

    fn note(&mut self, line: &str) {
        self.lines.push(line.to_string());
    }

    fn flush(&self) {
        let mut text = self.lines.join("\n");
        text.push('\n');
        fs::write(self.evidence.join("outcome.txt"), text).unwrap();
    }

    async fn cleanup(&mut self) {
        reset(&mut self.postgres).await;
    }
}

#[derive(Default)]
struct OracleTable {
    comment: HashOnly,
    columns: Vec<OracleColumn>,
    primary_key: Vec<(i32, String)>,
    foreign_keys: Vec<OracleFk>,
}

impl OracleTable {
    fn to_json(&self) -> Value {
        json!({
            "comment": self.comment.to_json(),
            "columns": self.columns.iter().map(OracleColumn::to_json).collect::<Vec<_>>(),
            "primary_key": self.primary_key.iter().map(|(attnum, name)| json!({"attnum": attnum, "name": name})).collect::<Vec<_>>(),
            "foreign_keys": self.foreign_keys.iter().map(OracleFk::to_json).collect::<Vec<_>>(),
        })
    }
}

struct OracleColumn {
    attnum: i32,
    name: String,
    type_oid: i64,
    type_name: String,
    not_null: bool,
    select: bool,
    insert: bool,
    update: bool,
    comment: HashOnly,
}

impl OracleColumn {
    fn to_json(&self) -> Value {
        json!({
            "attnum": self.attnum,
            "name": self.name,
            "type_oid": self.type_oid,
            "type_name": self.type_name,
            "not_null": self.not_null,
            "select": self.select,
            "insert": self.insert,
            "update": self.update,
            "comment": self.comment.to_json(),
        })
    }
}

struct OracleFk {
    name: String,
    attnum: i32,
    column: String,
    ref_table: String,
    ref_attnum: i32,
    ref_name: String,
}

impl OracleFk {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "attnum": self.attnum,
            "column": self.column,
            "ref_table": self.ref_table,
            "ref_attnum": self.ref_attnum,
            "ref_name": self.ref_name,
        })
    }
}

#[derive(Default)]
struct HashOnly {
    byte_len: Option<u64>,
    sha256: Option<String>,
}

impl HashOnly {
    fn from_server(text: Option<String>) -> Self {
        match text {
            None => Self::default(),
            Some(text) => Self {
                byte_len: Some(u64::try_from(text.len()).unwrap_or(u64::MAX)),
                sha256: Some(sha256_hex(&text)),
            },
        }
    }

    fn assert_value(&self, value: &Value) -> Result<(), String> {
        match &self.sha256 {
            None => {
                if value.is_null() {
                    Ok(())
                } else {
                    Err("unexpected comment object".to_string())
                }
            }
            Some(hash) => {
                let object = value.as_object().ok_or("comment object")?;
                if object.len() != 2 || !object.contains_key("byte_len") || !object.contains_key("sha256") {
                    return Err("comment fields".to_string());
                }
                if value["sha256"] != *hash || value["byte_len"] != self.byte_len.unwrap_or(u64::MAX) {
                    return Err("comment hash".to_string());
                }
                Ok(())
            }
        }
    }

    fn to_json(&self) -> Value {
        match (&self.byte_len, &self.sha256) {
            (Some(byte_len), Some(sha256)) => json!({"byte_len": byte_len, "sha256": sha256}),
            _ => Value::Null,
        }
    }
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

const SOURCE_SQL: &str = "\
CREATE SCHEMA pier;\n\
CREATE TABLE pier.berth (id int4 PRIMARY KEY, label text NOT NULL, note text);\n\
COMMENT ON TABLE pier.berth IS 'a166-keel-comment-berth-body';\n\
COMMENT ON COLUMN pier.berth.label IS 'a166-keel-comment-label-body';\n\
CREATE TABLE pier.mooring (\n\
  id int4 PRIMARY KEY,\n\
  berth_id int4 NOT NULL,\n\
  CONSTRAINT mooring_berth_fk FOREIGN KEY (berth_id) REFERENCES pier.berth (id)\n\
);\n\
COMMENT ON COLUMN pier.mooring.berth_id IS 'a166-keel-comment-moor-body';\n\
CREATE TABLE pier.write_audit (who text NOT NULL);\n\
CREATE FUNCTION pier.note_write() RETURNS trigger LANGUAGE plpgsql AS $audit$\n\
BEGIN\n\
  INSERT INTO pier.write_audit (who) VALUES (current_user);\n\
  RETURN NEW;\n\
END;\n\
$audit$;\n\
CREATE TRIGGER mooring_audit BEFORE INSERT OR UPDATE OR DELETE ON pier.mooring\n\
  FOR EACH ROW EXECUTE FUNCTION pier.note_write();\n\
GRANT USAGE ON SCHEMA pier TO oo_a166_reader, oo_a166_writer;\n\
GRANT SELECT ON pier.berth, pier.mooring TO oo_a166_reader;\n\
GRANT INSERT ON pier.mooring, pier.write_audit TO oo_a166_writer;\n\
";

fn minimal() -> Value {
    json!({
        "protocol": "oo.pipeline.catalog-pg/v1",
        "source": {
            "source_id": "a166src",
            "host": "db.example",
            "port": 5432,
            "database": "srcdb",
            "username": "reader",
            "secret_ref": "secret://pg/a166/reader",
            "password_file": "reader-password",
            "ca_file": "ca.crt",
            "tables": [{"schema": "pier", "table": "berth"}]
        }
    })
}

fn assert_body(body: &Value, needle: &str) {
    let (code, out, err) = run(&["catalog-pg"], body.to_string().as_bytes());
    assert_eq!(code, 1, "{err}");
    assert!(out.is_empty(), "{out}");
    assert!(err.contains(needle), "{err}");
    assert!(!err.contains(MARKER));
}

fn assert_input(bytes: &[u8], needle: &str) {
    let (code, out, err) = run(&["catalog-pg"], bytes);
    assert_eq!(code, 1, "{err}");
    assert!(out.is_empty());
    assert!(err.contains(needle), "{err}");
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

fn read_secret(var: &str) -> String {
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
        .ssl_root_cert_from_pem(ca.to_vec())
        .application_name("oo-a166-harness");
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
    raw(admin, "DROP DATABASE IF EXISTS oo_a166_src WITH (FORCE)").await;
    raw(admin, "DROP ROLE IF EXISTS oo_a166_reader").await;
    raw(admin, "DROP ROLE IF EXISTS oo_a166_writer").await;
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
