//! Live R3 typed key plans on PostgreSQL 17.10.
//! `OO_PG_KEYS_FIXTURE` is required. A missing pointer is a failure.

use std::cell::Cell;
use std::fs;
use std::time::{Duration, Instant};

use arrow_array::{Array, Int32Array, RecordBatch, StringArray};
use oo_connectors::{
    config_identity, type_policy_identity, AdmittedCatalog, AdmittedColumn, AdmittedTable,
    DeclaredKeyKind, KeyRange, PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig,
    PlannedChunk, ProbeValue, SecretRef, SelectedTable, SnapshotOwner, SnapshotRequest, SourceId,
    StreamBudget, StreamItem, TableKeyPlan, TableSelection, CLEANUP_OBSERVATION_BOUND_MS,
    KEY_PLAN_PROTOCOL,
};
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};
use zeroize::Zeroizing;

const SOURCE: &str = "keys_src";
const SCHEMA: &str = "r3k";
const TABLES: &[&str] = &["sparse", "words", "combo", "uniq", "nokey"];

#[derive(Deserialize)]
struct Pointer {
    host: String,
    port: u16,
    database: String,
    reader_user: String,
    writer_user: String,
    super_user: String,
    password_dir: String,
    ca_file: String,
    log_file: String,
}

struct World {
    pointer: Pointer,
    reader_pass: Zeroizing<String>,
    writer_pass: Zeroizing<String>,
    super_pass: Zeroizing<String>,
    ca: Vec<u8>,
    admin: PgConnection,
    generation: Cell<u64>,
    postmaster: String,
    in_recovery: bool,
    admitted: AdmittedCatalog,
    collation: String,
    outcomes: Vec<String>,
    outcome_path: Option<String>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typed_key_plan_live() {
    let mut world = World::start().await;
    world.seed().await;
    let before = world.sparse_ids().await;
    assert_eq!(before, vec![-40, -1, 0, 3, 10, 100]);
    let words_pg = world.ordered_text("words", "word", None).await;
    let words_c = world.ordered_text("words", "word", Some("C")).await;
    assert_ne!(words_pg, words_c, "column collation matched C");
    world.note(format!(
        "collation={} pg_order={} c_order={}",
        world.collation,
        words_pg.join(","),
        words_c.join(",")
    ));

    let mut owner = world.open(TABLES).await;
    let plan = owner.plan_keys(2).await.expect("plan");
    assert_eq!(plan.protocol, KEY_PLAN_PROTOCOL);
    assert_eq!(plan.generation, owner.binding().generation);
    assert_eq!(plan.input_digest, owner.binding().input_digest);
    assert_eq!(plan.target_rows, 2);
    assert_eq!(plan.tables.len(), TABLES.len());

    let sparse = &plan.tables[0];
    assert_eq!(sparse.table, "sparse");
    assert_eq!(sparse.row_count, Some(6));
    assert_eq!(sparse.stride, Some(2));
    assert_primary(sparse, &["id"]);
    assert_eq!(
        sparse.chunks[0].lower,
        Some(vec![ProbeValue::Int8(-40)])
    );
    assert_eq!(sparse.chunks[0].upper, Some(vec![ProbeValue::Int8(0)]));
    assert_eq!(sparse.chunks[1].lower, Some(vec![ProbeValue::Int8(0)]));
    assert_eq!(sparse.chunks[1].upper, Some(vec![ProbeValue::Int8(10)]));
    assert_eq!(sparse.chunks[2].lower, Some(vec![ProbeValue::Int8(10)]));
    assert_eq!(sparse.chunks[2].upper, None);
    assert!(sparse.chunks.iter().all(|chunk| chunk.keyed && !chunk.resumable));
    let sparse_rows = world.drain_chunks(&mut owner, &sparse.chunks[..1]).await;
    assert_eq!(sparse_rows.ids, vec![-40, -1]);
    world.note("sparse_chunk0_matches_cut".to_string());

    world.write_late().await;
    let rest = world.drain_chunks(&mut owner, &sparse.chunks[1..]).await;
    assert_eq!(rest.ids, vec![0, 3, 10, 100]);
    assert!(sparse_rows.ids.iter().chain(rest.ids.iter()).eq(before.iter()));
    assert!(!sparse_rows.batch_resumable && !sparse_rows.resumable_key);
    assert!(!rest.batch_resumable && !rest.resumable_key);

    let words = &plan.tables[1];
    assert_eq!(words.table, "words");
    assert_primary(words, &["word"]);
    let word_plan = words
        .declared
        .as_ref()
        .expect("word key")
        .columns
        .iter()
        .find(|column| column.name == "word")
        .expect("word column");
    assert_eq!(word_plan.collation_name.as_deref(), Some(world.collation.as_str()));
    let got_words = world.drain_chunks(&mut owner, &words.chunks).await;
    assert_eq!(got_words.text, words_pg);

    let combo = &plan.tables[2];
    assert_primary(combo, &["name", "n"]);
    let got_combo = world.drain_chunks(&mut owner, &combo.chunks).await;
    assert_eq!(got_combo.pairs, world.combo_pairs().await);

    let uniq = &plan.tables[3];
    assert_eq!(uniq.table, "uniq");
    let declared = uniq.declared.as_ref().expect("unique key");
    assert_eq!(declared.kind, DeclaredKeyKind::Unique);
    assert_eq!(
        declared.columns.iter().map(|column| column.name.as_str()).collect::<Vec<_>>(),
        vec!["code"]
    );
    let got_codes = world.drain_chunks(&mut owner, &uniq.chunks).await;
    assert_eq!(got_codes.text, world.ordered_text("uniq", "code", None).await);

    let nokey = &plan.tables[4];
    assert!(nokey.declared.is_none());
    assert_eq!(nokey.chunks.len(), 1);
    assert!(!nokey.chunks[0].keyed);
    assert!(!nokey.chunks[0].resumable);
    assert!(nokey.chunks[0].lower.is_none() && nokey.chunks[0].upper.is_none());

    let finished = owner.finish().await.expect("finish old cut");
    assert_eq!(finished.generation, plan.generation);
    world.wait_clear().await;
    world.note("old_cut_finished_without_late_rows".to_string());

    let after = world.sparse_ids().await;
    assert_eq!(after, vec![-100, -40, -1, 0, 3, 7, 10, 100, 1000]);
    let mut fresh = world.open(TABLES).await;
    let again = fresh.plan_keys(2).await.expect("fresh plan");
    let fresh_sparse = &again.tables[0];
    assert_eq!(fresh_sparse.row_count, Some(9));
    let ranged = fresh_sparse.chunks[0].clone();
    fresh
        .read_keys(
            SCHEMA,
            "sparse",
            KeyRange {
                lower: None,
                upper: None,
            },
            world.budget(),
        )
        .await
        .expect("full ordered read");
    let full = world.drain(&mut fresh).await;
    assert_eq!(full.ids, after);
    fresh.close_stream().await.expect("close full read");
    fresh
        .read_keys(
            SCHEMA,
            "sparse",
            KeyRange {
                lower: ranged.lower.clone(),
                upper: ranged.upper.clone(),
            },
            world.budget(),
        )
        .await
        .expect("half-open read");
    let first = world.drain(&mut fresh).await;
    assert_eq!(first.ids, vec![-100, -40]);
    let finished = fresh.finish().await.expect("finish fresh");
    assert_ne!(finished.generation, plan.generation);
    world.wait_clear().await;

    let mut plain = world.open(&["nokey"]).await;
    plain.open_stream(world.budget()).await.expect("nokey stream");
    let nokey_rows = world.drain(&mut plain).await;
    let mut payloads = nokey_rows.ids;
    payloads.sort();
    assert_eq!(payloads, vec![1, 2, 3]);
    let eof_resumable = nokey_rows.resumable_key;
    assert!(!eof_resumable);
    assert!(!nokey_rows.batch_resumable);
    let finished = plain.finish().await.expect("finish nokey");
    world.wait_clear().await;
    world.note(format!("nokey_generation={}", finished.generation));
    world.audit();
    world.note("keys_live_complete".to_string());
}

#[derive(Default)]
struct Drained {
    ids: Vec<i32>,
    text: Vec<String>,
    pairs: Vec<(String, i32)>,
    resumable_key: bool,
    batch_resumable: bool,
}

impl World {
    async fn start() -> Self {
        let path = std::env::var("OO_PG_KEYS_FIXTURE").expect("OO_PG_KEYS_FIXTURE is required");
        let pointer: Pointer = serde_json::from_str(&fs::read_to_string(&path).expect("pointer"))
            .expect("pointer json");
        let reader_pass = read_pass(&pointer.password_dir, "reader.pass");
        let writer_pass = read_pass(&pointer.password_dir, "writer.pass");
        let super_pass = read_pass(&pointer.password_dir, "super.pass");
        let ca = fs::read(&pointer.ca_file).expect("ca");
        let mut admin = dial(&pointer, &pointer.super_user, &super_pass, &ca, "oo-fixture-admin").await;
        let (postmaster, in_recovery) = node_of(&mut admin).await;
        let outcome_path = std::env::var("OO_PG_KEYS_OUTCOME").ok();
        Self {
            pointer,
            reader_pass,
            writer_pass,
            super_pass,
            ca,
            admin,
            generation: Cell::new(0),
            postmaster,
            in_recovery,
            admitted: AdmittedCatalog { tables: Vec::new() },
            collation: String::new(),
            outcomes: Vec::new(),
            outcome_path,
        }
    }

    fn note(&mut self, line: String) {
        eprintln!("outcome {line}");
        self.outcomes.push(line);
    }

    fn next_gen(&self) -> u64 {
        let next = self.generation.get() + 1;
        self.generation.set(next);
        next
    }

    fn budget(&self) -> StreamBudget {
        StreamBudget::try_new(4, 1024 * 1024, 64 * 1024, 64 * 1024).expect("stream budget")
    }

    async fn seed(&mut self) {
        let collation = pick_collation(&mut self.admin).await;
        self.collation = collation.clone();
        let ddl = format!(
            "DROP SCHEMA IF EXISTS r3k CASCADE; \
             CREATE SCHEMA r3k; \
             CREATE TABLE r3k.sparse (id int PRIMARY KEY, note text NOT NULL); \
             INSERT INTO r3k.sparse (id, note) VALUES \
               (-40, 'a'), (-1, 'b'), (0, 'c'), (3, 'd'), (10, 'e'), (100, 'f'); \
             CREATE TABLE r3k.words (word text COLLATE {collation} PRIMARY KEY, note text NOT NULL); \
             INSERT INTO r3k.words (word, note) VALUES \
               ('A', 'a'), ('B', 'b'), ('a', 'c'), ('b', 'd'), ('ä', 'e'), ('z', 'f'); \
             CREATE TABLE r3k.combo (name text NOT NULL, n int NOT NULL, note text NOT NULL, PRIMARY KEY (name, n)); \
             INSERT INTO r3k.combo (name, n, note) VALUES \
               ('a', -2, 'a'), ('a', -1, 'b'), ('a', 4, 'c'), ('b', -5, 'd'), ('b', 0, 'e'), ('m', 3, 'f'); \
             CREATE TABLE r3k.uniq (id uuid PRIMARY KEY, code text NOT NULL, payload int NOT NULL, CONSTRAINT uniq_code UNIQUE (code)); \
             CREATE UNIQUE INDEX uniq_payload_pos ON r3k.uniq (payload) WHERE payload > 0; \
             INSERT INTO r3k.uniq (id, code, payload) VALUES \
               ('00000000-0000-0000-0000-000000000001', 'm', 1), \
               ('00000000-0000-0000-0000-000000000002', 'a', 2), \
               ('00000000-0000-0000-0000-000000000003', 'z', 3), \
               ('00000000-0000-0000-0000-000000000004', 'b', 4); \
             CREATE TABLE r3k.nokey (payload int NOT NULL); \
             INSERT INTO r3k.nokey (payload) VALUES (2), (1), (3); \
             GRANT USAGE ON SCHEMA r3k TO {reader}, {writer}; \
             GRANT SELECT ON ALL TABLES IN SCHEMA r3k TO {reader}; \
             GRANT INSERT ON r3k.sparse TO {writer}",
            collation = quote_ident(&collation),
            reader = quote_ident(&self.pointer.reader_user),
            writer = quote_ident(&self.pointer.writer_user),
        );
        sqlx::raw_sql(sqlx::AssertSqlSafe(ddl))
            .execute(&mut self.admin)
            .await
            .expect("seed");
        self.admitted = load_admitted(&mut self.admin).await;
        self.note(format!("seeded collation={collation}"));
    }

    async fn write_late(&mut self) {
        let mut writer = dial(
            &self.pointer,
            &self.pointer.writer_user,
            &self.writer_pass,
            &self.ca,
            "oo-fixture-writer",
        )
        .await;
        sqlx::query(
            "INSERT INTO r3k.sparse (id, note) VALUES (-100, 'late'), (7, 'late'), (1000, 'late')",
        )
        .execute(&mut writer)
        .await
        .expect("late insert");
        writer.close().await.expect("writer close");
        self.note("writer_committed_late_keys".to_string());
    }

    async fn sparse_ids(&mut self) -> Vec<i32> {
        let rows = sqlx::query("SELECT id FROM r3k.sparse ORDER BY id")
            .fetch_all(&mut self.admin)
            .await
            .expect("sparse oracle");
        rows.iter().map(|row| row.try_get(0).expect("id")).collect()
    }

    async fn ordered_text(&mut self, table: &str, column: &str, collate: Option<&str>) -> Vec<String> {
        let sql = match collate {
            Some(name) => format!(
                "SELECT {column} FROM r3k.{table} ORDER BY {column} COLLATE {collate}",
                column = quote_ident(column),
                table = quote_ident(table),
                collate = quote_ident(name),
            ),
            None => format!(
                "SELECT {column} FROM r3k.{table} ORDER BY {column}",
                column = quote_ident(column),
                table = quote_ident(table),
            ),
        };
        let rows = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .fetch_all(&mut self.admin)
            .await
            .expect("text oracle");
        rows.iter().map(|row| row.try_get(0).expect("text")).collect()
    }

    async fn combo_pairs(&mut self) -> Vec<(String, i32)> {
        let rows = sqlx::query("SELECT name, n FROM r3k.combo ORDER BY name, n")
            .fetch_all(&mut self.admin)
            .await
            .expect("combo oracle");
        rows.iter()
            .map(|row| (row.try_get(0).expect("name"), row.try_get(1).expect("n")))
            .collect()
    }

    async fn open(&mut self, names: &[&str]) -> SnapshotOwner {
        let generation = self.next_gen();
        SnapshotOwner::open(self.request(generation, names))
            .await
            .expect("open")
    }

    fn request(&self, generation: u64, names: &[&str]) -> SnapshotRequest {
        let config = self.config(names);
        let policy = oo_connectors::PgTypePolicy::PRESERVE_ROW;
        let admitted = AdmittedCatalog {
            tables: names
                .iter()
                .map(|name| {
                    self.admitted
                        .tables
                        .iter()
                        .find(|table| table.name == *name)
                        .expect("admitted table")
                        .clone()
                })
                .collect(),
        };
        SnapshotRequest {
            config_identity: config_identity(&config),
            type_policy_identity: type_policy_identity(policy),
            config,
            admitted,
            policy,
            generation,
            deadline: Instant::now() + Duration::from_secs(90),
            expected_principal: self.pointer.reader_user.clone(),
            expected_database: self.pointer.database.clone(),
            expected_in_recovery: self.in_recovery,
            expected_postmaster_start: self.postmaster.clone(),
        }
    }

    fn config(&self, names: &[&str]) -> PgSourceConfig {
        PgSourceConfig::try_new(
            SourceId::new(SOURCE).expect("source"),
            PgEndpoint::new(&self.pointer.host, self.pointer.port, &self.pointer.database).expect("endpoint"),
            PgCredentials::new(&self.pointer.reader_user, self.reader_pass.as_str()).expect("credentials"),
            SecretRef::new("secret://pg/keys/reader").expect("secret"),
            PgTlsConfig::from_ca_pem(&self.ca).expect("tls"),
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(60),
                Duration::from_secs(5),
                128,
                8 * 1024 * 1024,
            )
            .expect("budget"),
            TableSelection::new(
                names
                    .iter()
                    .map(|name| SelectedTable::new(SCHEMA, *name).expect("table"))
                    .collect(),
            )
            .expect("selection"),
        )
        .expect("config")
    }

    async fn drain_chunks(&mut self, owner: &mut SnapshotOwner, chunks: &[PlannedChunk]) -> Drained {
        let mut out = Drained::default();
        for chunk in chunks {
            owner.open_chunk(chunk, self.budget()).await.expect("open chunk");
            let drained = self.drain(owner).await;
            out.ids.extend(drained.ids);
            out.text.extend(drained.text);
            out.pairs.extend(drained.pairs);
            out.resumable_key |= drained.resumable_key;
            out.batch_resumable |= drained.batch_resumable;
            owner.close_stream().await.expect("close chunk");
        }
        out
    }

    async fn drain(&mut self, owner: &mut SnapshotOwner) -> Drained {
        let mut out = Drained::default();
        loop {
            match owner.next_batch().await.expect("batch") {
                StreamItem::Batch(batch) => {
                    out.batch_resumable |= batch.resumable;
                    push_batch(&mut out, &batch.record_batch);
                }
                StreamItem::TableEof(eof) => {
                    out.resumable_key = eof.resumable_key;
                    break;
                }
                StreamItem::Exhausted(_) => panic!("exhausted before table eof"),
            }
        }
        out
    }

    async fn wait_clear(&mut self) {
        let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
        let began = Instant::now();
        loop {
            let left = oo_backends(&mut self.admin, &self.pointer.reader_user).await;
            if left.is_empty() {
                return;
            }
            if began.elapsed() > bound {
                panic!("reader backends still present after {}ms: {left:?}", bound.as_millis());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn audit(&mut self) {
        let raw = fs::read_to_string(&self.pointer.log_file).expect("postgres log");
        for secret in [self.reader_pass.as_str(), self.writer_pass.as_str(), self.super_pass.as_str()] {
            if !secret.is_empty() && raw.contains(secret) {
                panic!("postgres log contains a closed string");
            }
        }
        let mut oo_statements = 0_usize;
        let mut writer_writes = 0_usize;
        let mut saw_start = false;
        let mut saw_nokey = false;
        for line in raw.lines() {
            if line.contains("app=oo-fixture-writer")
                && let Some(sql) = statement_sql(line)
                && write_word(&strip_sql_literals(sql)).is_some()
            {
                writer_writes += 1;
            }
            if !line.contains("app=oo-pg-source") || !line.contains(&format!("user={}", self.pointer.reader_user)) {
                continue;
            }
            let Some(sql) = statement_sql(line) else {
                continue;
            };
            oo_statements += 1;
            let verb = sql.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
            if matches!(verb.as_str(), "START" | "BEGIN") {
                saw_start = true;
            }
            if write_word(&strip_sql_literals(sql)).is_some() || has_token(sql, "OFFSET") {
                panic!("reader sql has a forbidden token");
            }
            let bare = sql.replace('"', "");
            if bare.contains("FROM r3k.nokey") {
                saw_nokey = true;
                assert!(!sql.to_ascii_uppercase().contains("ORDER BY"), "{sql}");
                assert!(!sql.to_ascii_uppercase().contains("WHERE"), "{sql}");
            }
        }
        assert!(oo_statements > 0, "reader sql log was empty");
        assert!(saw_start, "reader transaction start was not logged");
        assert!(writer_writes > 0, "fixture writer was not logged");
        assert!(saw_nokey, "nokey scan was not logged");
        self.note(format!(
            "sql_audit oo_statements={oo_statements} writer_writes={writer_writes} dml_ddl_commit_offset=0"
        ));
    }
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(path) = &self.outcome_path {
            let _ = fs::write(path, self.outcomes.join("\n") + "\n");
        }
    }
}

fn assert_primary(plan: &TableKeyPlan, names: &[&str]) {
    let declared = plan.declared.as_ref().expect("declared key");
    assert_eq!(declared.kind, DeclaredKeyKind::Primary);
    assert_eq!(
        declared.columns.iter().map(|column| column.name.as_str()).collect::<Vec<_>>(),
        names
    );
}

fn push_batch(out: &mut Drained, batch: &RecordBatch) {
    if let Some(ids) = batch
        .column_by_name("id")
        .or_else(|| batch.column_by_name("payload"))
        .or_else(|| batch.column_by_name("n"))
        && ids.as_any().downcast_ref::<Int32Array>().is_some()
        && batch.column_by_name("name").is_none()
        && batch.column_by_name("code").is_none()
        && batch.column_by_name("word").is_none()
    {
        out.ids.extend(int32_values(ids));
    }
    if let Some(word) = batch.column_by_name("word") {
        out.text.extend(utf8_values(word));
    }
    if let Some(code) = batch.column_by_name("code") {
        out.text.extend(utf8_values(code));
    }
    if let (Some(name), Some(n)) = (batch.column_by_name("name"), batch.column_by_name("n")) {
        let names = utf8_values(name);
        let nums = int32_values(n);
        out.pairs.extend(names.into_iter().zip(nums));
    }
}

fn int32_values(array: &arrow_array::array::ArrayRef) -> Vec<i32> {
    let values = array.as_any().downcast_ref::<Int32Array>().expect("int32");
    (0..values.len())
        .map(|index| {
            assert!(!values.is_null(index));
            values.value(index)
        })
        .collect()
}

fn utf8_values(array: &arrow_array::array::ArrayRef) -> Vec<String> {
    let values = array.as_any().downcast_ref::<StringArray>().expect("utf8");
    (0..values.len())
        .map(|index| {
            assert!(!values.is_null(index));
            values.value(index).to_string()
        })
        .collect()
}

async fn pick_collation(admin: &mut PgConnection) -> String {
    let rows = sqlx::query(
        "SELECT c.collname::text \
         FROM pg_catalog.pg_collation c \
         JOIN pg_catalog.pg_database d ON d.datname = current_database() \
         WHERE c.collname <> 'C' AND c.collname <> 'POSIX' \
           AND (c.collencoding = d.encoding OR c.collencoding = -1) \
         ORDER BY c.collname",
    )
    .fetch_all(&mut *admin)
    .await
    .expect("collations");
    let mut tried = Vec::new();
    for row in &rows {
        let name: String = row.try_get(0).expect("collname");
        if !safe_collation(&name) {
            continue;
        }
        let pg = sample_order(admin, &name).await;
        let c_order = sample_order(admin, "C").await;
        tried.push(name.clone());
        if pg != c_order {
            return name;
        }
    }
    panic!("no non-C collation changed the sample order; tried={tried:?}");
}

async fn sample_order(admin: &mut PgConnection, collation: &str) -> Vec<String> {
    let sql = format!(
        "SELECT v.word FROM (VALUES ('A'), ('B'), ('a'), ('b'), ('ä'), ('z')) AS v(word) \
         ORDER BY v.word COLLATE {}",
        quote_ident(collation)
    );
    let rows = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *admin)
        .await
        .expect("sample order");
    rows.iter().map(|row| row.try_get(0).expect("word")).collect()
}

fn safe_collation(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

async fn load_admitted(admin: &mut PgConnection) -> AdmittedCatalog {
    let mut tables = Vec::new();
    for name in TABLES {
        let rows = sqlx::query(
            "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull \
             FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'r3k' AND c.relname = $1 AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
        )
        .bind(*name)
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
             WHERE n.nspname = 'r3k' AND c.relname = $1 AND i.indisprimary \
             ORDER BY u.ord",
        )
        .bind(*name)
        .fetch_all(&mut *admin)
        .await
        .expect("admitted keys");
        tables.push(AdmittedTable {
            schema: SCHEMA.to_string(),
            name: (*name).to_string(),
            columns,
            primary_key: keys.iter().map(|row| row.try_get(0).unwrap()).collect(),
        });
    }
    AdmittedCatalog { tables }
}

async fn node_of(admin: &mut PgConnection) -> (String, bool) {
    let row = sqlx::query(
        "SELECT pg_catalog.pg_postmaster_start_time()::text, pg_catalog.pg_is_in_recovery()",
    )
    .fetch_one(&mut *admin)
    .await
    .expect("node");
    (row.try_get(0).expect("postmaster"), row.try_get(1).expect("recovery"))
}

async fn oo_backends(admin: &mut PgConnection, user: &str) -> Vec<(i64, String)> {
    let rows = sqlx::query(
        "SELECT pid::int8, backend_start::text FROM pg_catalog.pg_stat_activity \
         WHERE application_name = 'oo-pg-source' AND usename::text = $1",
    )
    .bind(user)
    .fetch_all(&mut *admin)
    .await
    .expect("backends");
    rows.iter()
        .map(|row| (row.try_get(0).expect("pid"), row.try_get(1).expect("start")))
        .collect()
}

async fn dial(pointer: &Pointer, user: &str, password: &str, ca: &[u8], app: &str) -> PgConnection {
    let options = PgConnectOptions::from_explicit()
        .host(&pointer.host)
        .port(pointer.port)
        .username(user)
        .password(password)
        .database(&pointer.database)
        .application_name(app)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    options.disable_statement_logging().connect().await.expect("dial")
}

fn read_pass(dir: &str, name: &str) -> Zeroizing<String> {
    let text = fs::read_to_string(format!("{dir}/{name}")).expect("password file");
    Zeroizing::new(text.trim().to_string())
}

fn strip_sql_literals(sql: &str) -> String {
    let mut out = String::new();
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\'' {
            while let Some(inner) = chars.next() {
                if inner == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                        continue;
                    }
                    break;
                }
            }
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

fn write_word(sql: &str) -> Option<&'static str> {
    const WORDS: &[&str] = &[
        "INSERT", "UPDATE", "DELETE", "TRUNCATE", "MERGE", "COPY", "CREATE", "ALTER", "DROP",
        "GRANT", "REVOKE", "COMMIT",
    ];
    for token in sql.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_') {
        let upper = token.to_ascii_uppercase();
        if let Some(word) = WORDS.iter().copied().find(|word| *word == upper) {
            return Some(word);
        }
    }
    None
}

fn has_token(sql: &str, want: &str) -> bool {
    sql.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .any(|token| token.eq_ignore_ascii_case(want))
}

fn statement_sql(line: &str) -> Option<&str> {
    if let Some(rest) = line.split_once("statement:") {
        return Some(rest.1.trim());
    }
    if let Some(rest) = line.split_once("execute ") {
        return rest.1.split_once(':').map(|(_, sql)| sql.trim());
    }
    None
}
