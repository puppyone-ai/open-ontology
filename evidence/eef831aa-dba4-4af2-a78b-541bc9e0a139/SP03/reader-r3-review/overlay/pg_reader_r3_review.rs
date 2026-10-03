//! Independent public-API probe for fixed reader R3.
//! One cut: declared keys, SQL half-open coverage, and owner Drop after a
//! dropped `next_batch` left the data backend active.
//! `OO_PG_REVIEW_FIXTURE` is required.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::pin::pin;
use std::time::{Duration, Instant};

use arrow_array::{Array, Int32Array, RecordBatch, StringArray};
use oo_connectors::{
    config_identity, type_policy_identity, AdmittedCatalog, AdmittedColumn, AdmittedTable, CutProbe,
    DeclaredKeyKind, KeyRange, PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig,
    PlannedChunk, ProbeValue, SecretRef, SelectedTable, SnapshotOwner, SnapshotRequest, SourceError,
    SourceId, StreamBudget, StreamItem, TableKeyPlan, TableSelection, CLEANUP_OBSERVATION_BOUND_MS,
    KEY_PLAN_PROTOCOL, MAX_PLAN_CHUNKS, SNAPSHOT_DEADLINE_DEFAULT,
};
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};
use zeroize::Zeroizing;

const SOURCE: &str = "review_src";
const SCHEMA: &str = "r3v";
const TABLES: &[&str] = &["sparse", "words", "combo", "fb", "soft", "bare", "onlyu"];

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
    log_mark: u64,
}

struct Pulled<T> {
    values: Vec<T>,
    batch_resumable: bool,
    resumable_key: bool,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn review_public_cut_and_owner_drop() {
    assert_eq!(SNAPSHOT_DEADLINE_DEFAULT, Duration::from_secs(300));
    assert_eq!(MAX_PLAN_CHUNKS, 64);

    let mut world = World::start().await;
    world.seed().await;
    world.phase_keys().await;
    world.phase_uuid_probe().await;
    world.phase_owner_drop().await;
    world.audit();
    world.note("review_probe_complete".to_string());
}

impl World {
    async fn start() -> Self {
        let path = std::env::var("OO_PG_REVIEW_FIXTURE").expect("OO_PG_REVIEW_FIXTURE is required");
        let pointer: Pointer = serde_json::from_str(&fs::read_to_string(&path).expect("pointer"))
            .expect("pointer json");
        let reader_pass = read_pass(&pointer.password_dir, "reader.pass");
        let writer_pass = read_pass(&pointer.password_dir, "writer.pass");
        let super_pass = read_pass(&pointer.password_dir, "super.pass");
        let ca = fs::read(&pointer.ca_file).expect("ca");
        let mut admin = dial(&pointer, &pointer.super_user, &super_pass, &ca, "oo-fixture-admin").await;
        let (postmaster, in_recovery) = node_of(&mut admin).await;
        let log_mark = fs::metadata(&pointer.log_file).map(|meta| meta.len()).unwrap_or(0);
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
            outcome_path: std::env::var("OO_PG_REVIEW_OUTCOME").ok(),
            log_mark,
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
        StreamBudget::try_new(8, 1024 * 1024, 64 * 1024, 64 * 1024).expect("stream budget")
    }

    async fn seed(&mut self) {
        let collation = self.collation_that_differs().await;
        self.collation = collation.clone();
        let reader = quote_ident(&self.pointer.reader_user);
        let writer = quote_ident(&self.pointer.writer_user);
        let coll = quote_ident(&collation);
        let ddl = format!(
            "DROP SCHEMA IF EXISTS r3v CASCADE; \
             CREATE SCHEMA r3v; \
             CREATE TABLE r3v.sparse (id int PRIMARY KEY, note text NOT NULL); \
             INSERT INTO r3v.sparse (id, note) VALUES (-8, 'a'), (-3, 'b'), (1, 'c'), (4, 'd'); \
             CREATE TABLE r3v.words (word text COLLATE {coll} PRIMARY KEY, note text NOT NULL); \
             INSERT INTO r3v.words (word, note) VALUES \
               ('A', 'a'), ('B', 'b'), ('a', 'c'), ('b', 'd'), ('ä', 'e'), ('z', 'f'); \
             CREATE TABLE r3v.combo (name text NOT NULL, n int NOT NULL, note text NOT NULL, PRIMARY KEY (name, n)); \
             INSERT INTO r3v.combo (name, n, note) VALUES \
               ('a', -2, 'a'), ('a', -1, 'b'), ('a', 4, 'c'), ('b', -5, 'd'), ('b', 0, 'e'), ('m', 3, 'f'); \
             CREATE TABLE r3v.fb (id uuid PRIMARY KEY, code text NOT NULL, payload int NOT NULL, CONSTRAINT m_code UNIQUE (code)); \
             CREATE UNIQUE INDEX a_partial ON r3v.fb (payload) WHERE payload > 0; \
             INSERT INTO r3v.fb (id, code, payload) VALUES \
               ('00000000-0000-0000-0000-000000000001', 'm', 1), \
               ('00000000-0000-0000-0000-000000000002', 'a', 2), \
               ('00000000-0000-0000-0000-000000000003', 'z', 3), \
               ('00000000-0000-0000-0000-000000000004', 'b', 4); \
             CREATE TABLE r3v.soft (name text UNIQUE, payload int NOT NULL); \
             INSERT INTO r3v.soft (name, payload) VALUES (NULL, 1), ('b', 2), ('a', 3); \
             CREATE TABLE r3v.bare (payload int NOT NULL); \
             INSERT INTO r3v.bare (payload) VALUES (3), (1), (2); \
             CREATE TABLE r3v.onlyu (id uuid PRIMARY KEY, payload int NOT NULL); \
             INSERT INTO r3v.onlyu (id, payload) VALUES \
               ('00000000-0000-0000-0000-00000000000a', 1), \
               ('00000000-0000-0000-0000-00000000000b', 2); \
             REVOKE ALL ON SCHEMA r3v FROM PUBLIC; \
             REVOKE ALL ON ALL TABLES IN SCHEMA r3v FROM PUBLIC; \
             GRANT USAGE ON SCHEMA r3v TO {reader}, {writer}; \
             GRANT SELECT ON ALL TABLES IN SCHEMA r3v TO {reader}; \
             GRANT INSERT ON r3v.sparse TO {writer}"
        );
        sqlx::raw_sql(sqlx::AssertSqlSafe(ddl))
            .execute(&mut self.admin)
            .await
            .expect("seed");
        self.admitted = load_admitted(&mut self.admin).await;
        let partial = sqlx::query(
            "SELECT ic.relname::text, (i.indpred IS NOT NULL) \
             FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_catalog.pg_index i ON i.indrelid = c.oid \
             JOIN pg_catalog.pg_class ic ON ic.oid = i.indexrelid \
             WHERE n.nspname = 'r3v' AND c.relname = 'fb' \
             ORDER BY ic.relname",
        )
        .fetch_all(&mut self.admin)
        .await
        .expect("fb indexes");
        let described: Vec<String> = partial
            .iter()
            .map(|row| {
                format!(
                    "{}:pred={}",
                    row.try_get::<String, _>(0).expect("index"),
                    row.try_get::<bool, _>(1).expect("pred")
                )
            })
            .collect();
        assert!(
            described.iter().any(|line| line == "a_partial:pred=true"),
            "{described:?}"
        );
        self.note(format!(
            "seeded collation={} fb_indexes={}",
            collation,
            described.join(",")
        ));
    }

    async fn phase_keys(&mut self) {
        let before = self.sparse_range(None, None).await;
        assert_eq!(before, vec![-8, -3, 1, 4]);
        let words_sql = self.text_order(None, None).await;
        let words_c = self.text_order_c().await;
        let mut words_rust = words_sql.clone();
        words_rust.sort();
        assert_ne!(words_sql, words_c, "column collation matched C");
        assert_ne!(words_sql, words_rust, "column collation matched Rust sort");
        self.note(format!(
            "collation={} pg_order={} c_order={} rust_order={}",
            self.collation,
            words_sql.join(","),
            words_c.join(","),
            words_rust.join(",")
        ));
        let combo_sql = self.combo_order(None, None).await;
        assert!(combo_sql.iter().any(|(_, n)| *n < 0));
        let code_sql = self.code_order(None, None).await;

        let mut owner = self.open(TABLES, 90, 60, 30).await;
        let rejected = owner.plan_keys(0).await.expect_err("zero target");
        assert_eq!(rejected.fact(), Some("target_rows"));
        let rejected = owner.plan_keys(50_001).await.expect_err("high target");
        assert_eq!(rejected.fact(), Some("target_rows"));
        let plan = owner.plan_keys(2).await.expect("plan");
        assert_eq!(plan.protocol, KEY_PLAN_PROTOCOL);
        assert_eq!(plan.generation, owner.binding().generation);
        assert_eq!(plan.input_digest, owner.binding().input_digest);
        assert_eq!(plan.target_rows, 2);
        assert_eq!(plan.tables.len(), TABLES.len());
        let old_generation = plan.generation;
        let old_digest = plan.input_digest;

        let sparse = table(&plan.tables, "sparse");
        assert_eq!(sparse.row_count, Some(4));
        assert_eq!(sparse.stride, Some(2));
        assert_primary(sparse, &["id"]);
        assert_chain(&sparse.chunks);
        assert!(sparse.chunks.len() < usize::try_from(sparse.row_count.unwrap_or(0)).unwrap_or(0));
        let sparse_chunks = sparse.chunks.clone();
        let words = table(&plan.tables, "words");
        assert_eq!(words.row_count, Some(6));
        assert_eq!(words.stride, Some(2));
        assert_primary(words, &["word"]);
        assert_eq!(
            words.declared.as_ref().expect("words").columns[0].collation_name.as_deref(),
            Some(self.collation.as_str())
        );
        assert_chain(&words.chunks);
        let word_chunks = words.chunks.clone();
        let combo = table(&plan.tables, "combo");
        assert_primary(combo, &["name", "n"]);
        assert_eq!(combo.row_count, Some(6));
        assert_eq!(combo.stride, Some(2));
        assert_chain(&combo.chunks);
        let combo_chunks = combo.chunks.clone();
        let fb = table(&plan.tables, "fb");
        let declared = fb.declared.as_ref().expect("fb key");
        assert_eq!(declared.kind, DeclaredKeyKind::Unique);
        assert_eq!(declared.index_name, "m_code");
        assert_eq!(
            declared.columns.iter().map(|column| column.name.as_str()).collect::<Vec<_>>(),
            vec!["code"]
        );
        assert!(declared.columns.iter().all(|column| column.name != "id"));
        assert_eq!(fb.row_count, Some(4));
        assert_chain(&fb.chunks);
        let fb_chunks = fb.chunks.clone();
        assert_unkeyed(table(&plan.tables, "soft"));
        assert_unkeyed(table(&plan.tables, "bare"));
        let soft_chunk = table(&plan.tables, "soft").chunks[0].clone();
        let bare_chunk = table(&plan.tables, "bare").chunks[0].clone();
        for planned in &plan.tables {
            assert!(planned.chunks.len() <= usize::try_from(MAX_PLAN_CHUNKS).unwrap());
            assert!(planned.chunks.iter().all(|chunk| {
                !chunk.resumable
                    && chunk.generation == old_generation
                    && chunk.input_digest == old_digest
            }));
        }
        self.note(format!(
            "plan_chunks sparse={} words={} combo={} fb={} stored_starts_lt_rows=1",
            sparse_chunks.len(),
            word_chunks.len(),
            combo_chunks.len(),
            fb_chunks.len()
        ));

        let sparse_expect = self.chunk_ints(&sparse_chunks).await;
        let got0 = self.read_i32(&mut owner, &sparse_chunks[0], "id").await;
        assert_eq!(got0.values, sparse_expect[0]);
        assert!(!got0.batch_resumable && !got0.resumable_key);
        self.write_late().await;
        let mut got_sparse = got0.values;
        for (chunk, expect) in sparse_chunks.iter().skip(1).zip(sparse_expect.iter().skip(1)) {
            let got = self.read_i32(&mut owner, chunk, "id").await;
            assert_eq!(got.values, *expect);
            assert!(!got.batch_resumable && !got.resumable_key);
            got_sparse.extend(got.values);
        }
        assert_eq!(got_sparse, before);
        assert!(!got_sparse.contains(&0) && !got_sparse.contains(&9));
        let after_gap = self.sparse_range(one_i64(&sparse_chunks[0].lower), one_i64(&sparse_chunks[0].upper)).await;
        let after_tail = self.sparse_range(one_i64(&sparse_chunks[1].lower), None).await;
        assert!(after_gap.contains(&0), "writer row 0 was not visible to a new query");
        assert!(after_tail.contains(&9), "writer row 9 was not visible to a new query");
        self.note(format!(
            "sparse_old_cut={:?} admin_after_gap={:?} admin_after_tail={:?}",
            got_sparse, after_gap, after_tail
        ));

        let mut got_words = Vec::new();
        for chunk in &word_chunks {
            let expect = self.text_order(one_text(&chunk.lower), one_text(&chunk.upper)).await;
            let got = self.read_text(&mut owner, chunk, "word").await;
            assert_eq!(got.values, expect);
            assert!(!got.batch_resumable && !got.resumable_key);
            got_words.extend(got.values);
        }
        assert_eq!(got_words, words_sql);

        let mut got_combo = Vec::new();
        for chunk in &combo_chunks {
            let expect = self.combo_order(one_pair(&chunk.lower), one_pair(&chunk.upper)).await;
            let got = self.read_pairs(&mut owner, chunk).await;
            assert_eq!(got.values, expect);
            assert!(!got.batch_resumable && !got.resumable_key);
            got_combo.extend(got.values);
        }
        assert_eq!(got_combo, combo_sql);

        let mut got_codes = Vec::new();
        for chunk in &fb_chunks {
            let expect = self.code_order(one_text(&chunk.lower), one_text(&chunk.upper)).await;
            let got = self.read_text(&mut owner, chunk, "code").await;
            assert_eq!(got.values, expect);
            assert!(!got.batch_resumable && !got.resumable_key);
            got_codes.extend(got.values);
        }
        assert_eq!(got_codes, code_sql);

        let missing = owner
            .read_keys(
                SCHEMA,
                "soft",
                KeyRange { lower: None, upper: None },
                self.budget(),
            )
            .await
            .expect_err("nullable unique is not a declared key");
        assert_eq!(missing.fact(), Some("key_required"));
        let soft = self.read_i32(&mut owner, &soft_chunk, "payload").await;
        assert!(!soft.batch_resumable && !soft.resumable_key);
        let mut soft_values = soft.values;
        soft_values.sort();
        assert_eq!(soft_values, vec![1, 2, 3]);
        let bare = self.read_i32(&mut owner, &bare_chunk, "payload").await;
        assert!(!bare.batch_resumable && !bare.resumable_key);
        let mut bare_values = bare.values;
        bare_values.sort();
        assert_eq!(bare_values, vec![1, 2, 3]);
        let onlyu_chunk = table(&plan.tables, "onlyu").chunks[0].clone();
        assert_unkeyed(table(&plan.tables, "onlyu"));
        let onlyu = self.read_i32(&mut owner, &onlyu_chunk, "payload").await;
        assert!(!onlyu.batch_resumable && !onlyu.resumable_key);
        let mut onlyu_values = onlyu.values;
        onlyu_values.sort();
        assert_eq!(onlyu_values, vec![1, 2]);
        self.note("uuid_pk_only_unkeyed_resumable_false".to_string());

        let finished = owner.finish().await.expect("finish old cut");
        assert_eq!(finished.generation, old_generation);
        self.wait_clear().await;
        self.note(format!("old_cut_finished generation={}", finished.generation));

        let after = self.sparse_range(None, None).await;
        assert_eq!(after, vec![-8, -3, 0, 1, 4, 9]);
        let mut fresh = self.open(&["sparse"], 90, 60, 30).await;
        fresh
            .read_keys(SCHEMA, "sparse", KeyRange { lower: None, upper: None }, self.budget())
            .await
            .expect("fresh full key read");
        let fresh_rows = self.drain_i32(&mut fresh, "id").await;
        assert_eq!(fresh_rows.values, after);
        assert!(!fresh_rows.batch_resumable && !fresh_rows.resumable_key);
        fresh.close_stream().await.expect("close fresh");
        let fresh_done = fresh.finish().await.expect("finish fresh");
        assert_ne!(fresh_done.generation, old_generation);
        assert_ne!(fresh_done.generation, 0);
        self.wait_clear().await;
        self.note(format!(
            "fresh_generation={} fresh={:?} old_partial_excluded_0_and_9=1",
            fresh_done.generation, fresh_rows.values
        ));
    }

    async fn phase_uuid_probe(&mut self) {
        let mut owner = self.open(&["fb"], 90, 60, 30).await;
        let err = owner
            .read_probe(CutProbe {
                schema: SCHEMA.to_string(),
                table: "fb".to_string(),
                key: vec![ProbeValue::Int8(1)],
            })
            .await
            .expect_err("uuid primary key probe");
        assert_eq!(err.fact(), Some("probe_key"));
        let aborted = owner.abort().await.expect("abort poisoned uuid probe");
        self.wait_clear().await;
        self.note(format!(
            "uuid_probe_fact=probe_key abort_generation={} snapshot_finished=0",
            aborted.generation
        ));
    }

    async fn phase_owner_drop(&mut self) {
        self.lock_sparse().await;
        let mut owner = self.open(&["sparse"], 90, 60, 60).await;
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        owner.open_stream(self.budget()).await.expect("open blocked stream");
        {
            let mut pull = pin!(owner.next_batch());
            let saw = self.drive_until_active(&mut pull, pid, &start).await;
            assert!(saw, "blocked pull never became active");
        }
        let still = self.state_of(pid, &start).await;
        assert_eq!(still.as_deref(), Some("active"), "dropped next_batch released the backend");
        let before = self.reader_backends().await;
        assert!(before.len() >= 2, "expected data and guard backends, got {before:?}");
        self.note(format!(
            "next_batch_drop_state=active data_pid={pid} reader_backends={}",
            before.len()
        ));
        let began = Instant::now();
        drop(owner);
        self.wait_absent(&before, began).await;
        self.unlock_sparse().await;
        self.wait_clear().await;
        self.note("owner_drop_finish_called=0 snapshot_finished=0".to_string());
    }

    async fn collation_that_differs(&mut self) -> String {
        let rows = sqlx::query(
            "SELECT collname::text FROM pg_catalog.pg_collation \
             WHERE collprovider = 'i' ORDER BY collname",
        )
        .fetch_all(&mut self.admin)
        .await
        .expect("collations");
        let names: Vec<String> = rows.iter().map(|row| row.try_get(0).expect("collname")).collect();
        assert!(!names.is_empty(), "postgres image has no icu collation");
        let samples = ["A", "B", "a", "b", "ä", "z"];
        let c_order = self.sample_order("C", &samples).await;
        let mut rust = samples.to_vec();
        rust.sort_unstable();
        for name in &names {
            if !safe_ident(name) {
                continue;
            }
            let pg = self.sample_order(name, &samples).await;
            if pg != c_order && pg != rust {
                return name.clone();
            }
        }
        panic!("no icu collation differed from both C and Rust sort");
    }

    async fn sample_order(&mut self, collation: &str, samples: &[&str]) -> Vec<String> {
        let sql = format!(
            "SELECT w FROM (VALUES ('A'),('B'),('a'),('b'),('ä'),('z')) AS v(w) ORDER BY w COLLATE {}",
            quote_ident(collation)
        );
        let rows = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .fetch_all(&mut self.admin)
            .await
            .expect("sample order");
        let got: Vec<String> = rows.iter().map(|row| row.try_get(0).expect("w")).collect();
        assert_eq!(got.len(), samples.len());
        got
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
        sqlx::query("INSERT INTO r3v.sparse (id, note) VALUES (0, 'gap'), (9, 'late')")
            .execute(&mut writer)
            .await
            .expect("late insert");
        writer.close().await.expect("writer close");
        self.note("writer_committed_gap_and_tail".to_string());
    }

    async fn sparse_range(&mut self, lower: Option<i64>, upper: Option<i64>) -> Vec<i32> {
        let lower = lower.map(|value| i32::try_from(value).expect("lower"));
        let upper = upper.map(|value| i32::try_from(value).expect("upper"));
        let rows = match (lower, upper) {
            (Some(lo), Some(hi)) => sqlx::query(
                "SELECT id FROM r3v.sparse WHERE id >= $1 AND id < $2 ORDER BY id",
            )
            .bind(lo)
            .bind(hi)
            .fetch_all(&mut self.admin)
            .await
            .expect("sparse range"),
            (Some(lo), None) => sqlx::query("SELECT id FROM r3v.sparse WHERE id >= $1 ORDER BY id")
                .bind(lo)
                .fetch_all(&mut self.admin)
                .await
                .expect("sparse tail"),
            (None, None) => sqlx::query("SELECT id FROM r3v.sparse ORDER BY id")
                .fetch_all(&mut self.admin)
                .await
                .expect("sparse all"),
            (None, Some(_)) => panic!("upper without lower"),
        };
        rows.iter().map(|row| row.try_get(0).expect("id")).collect()
    }

    async fn chunk_ints(&mut self, chunks: &[PlannedChunk]) -> Vec<Vec<i32>> {
        let mut out = Vec::new();
        for chunk in chunks {
            out.push(self.sparse_range(one_i64(&chunk.lower), one_i64(&chunk.upper)).await);
        }
        out
    }

    async fn text_order(&mut self, lower: Option<String>, upper: Option<String>) -> Vec<String> {
        let rows = match (lower, upper) {
            (Some(lo), Some(hi)) => sqlx::query(
                "SELECT word FROM r3v.words WHERE word >= $1 AND word < $2 ORDER BY word",
            )
            .bind(lo)
            .bind(hi)
            .fetch_all(&mut self.admin)
            .await
            .expect("word range"),
            (Some(lo), None) => sqlx::query("SELECT word FROM r3v.words WHERE word >= $1 ORDER BY word")
                .bind(lo)
                .fetch_all(&mut self.admin)
                .await
                .expect("word tail"),
            (None, None) => sqlx::query("SELECT word FROM r3v.words ORDER BY word")
                .fetch_all(&mut self.admin)
                .await
                .expect("word all"),
            (None, Some(_)) => panic!("upper without lower"),
        };
        rows.iter().map(|row| row.try_get(0).expect("word")).collect()
    }

    async fn text_order_c(&mut self) -> Vec<String> {
        let rows = sqlx::query("SELECT word FROM r3v.words ORDER BY word COLLATE \"C\"")
            .fetch_all(&mut self.admin)
            .await
            .expect("word C");
        rows.iter().map(|row| row.try_get(0).expect("word")).collect()
    }

    async fn combo_order(
        &mut self,
        lower: Option<(String, i32)>,
        upper: Option<(String, i32)>,
    ) -> Vec<(String, i32)> {
        let rows = match (lower, upper) {
            (Some((ln, li)), Some((un, ui))) => sqlx::query(
                "SELECT name, n FROM r3v.combo WHERE (name, n) >= ($1, $2) AND (name, n) < ($3, $4) ORDER BY name, n",
            )
            .bind(ln)
            .bind(li)
            .bind(un)
            .bind(ui)
            .fetch_all(&mut self.admin)
            .await
            .expect("combo range"),
            (Some((ln, li)), None) => {
                sqlx::query("SELECT name, n FROM r3v.combo WHERE (name, n) >= ($1, $2) ORDER BY name, n")
                    .bind(ln)
                    .bind(li)
                    .fetch_all(&mut self.admin)
                    .await
                    .expect("combo tail")
            }
            (None, None) => sqlx::query("SELECT name, n FROM r3v.combo ORDER BY name, n")
                .fetch_all(&mut self.admin)
                .await
                .expect("combo all"),
            (None, Some(_)) => panic!("upper without lower"),
        };
        rows.iter()
            .map(|row| (row.try_get(0).expect("name"), row.try_get(1).expect("n")))
            .collect()
    }

    async fn code_order(&mut self, lower: Option<String>, upper: Option<String>) -> Vec<String> {
        let rows = match (lower, upper) {
            (Some(lo), Some(hi)) => {
                sqlx::query("SELECT code FROM r3v.fb WHERE code >= $1 AND code < $2 ORDER BY code")
                    .bind(lo)
                    .bind(hi)
                    .fetch_all(&mut self.admin)
                    .await
                    .expect("code range")
            }
            (Some(lo), None) => sqlx::query("SELECT code FROM r3v.fb WHERE code >= $1 ORDER BY code")
                .bind(lo)
                .fetch_all(&mut self.admin)
                .await
                .expect("code tail"),
            (None, None) => sqlx::query("SELECT code FROM r3v.fb ORDER BY code")
                .fetch_all(&mut self.admin)
                .await
                .expect("code all"),
            (None, Some(_)) => panic!("upper without lower"),
        };
        rows.iter().map(|row| row.try_get(0).expect("code")).collect()
    }

    async fn open(&mut self, names: &[&str], deadline_secs: u64, statement_secs: u64, lock_secs: u64) -> SnapshotOwner {
        let generation = self.next_gen();
        SnapshotOwner::open(self.request(generation, names, deadline_secs, statement_secs, lock_secs))
            .await
            .expect("open")
    }

    fn request(
        &self,
        generation: u64,
        names: &[&str],
        deadline_secs: u64,
        statement_secs: u64,
        lock_secs: u64,
    ) -> SnapshotRequest {
        let config = self.config(names, statement_secs, lock_secs);
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
            deadline: Instant::now() + Duration::from_secs(deadline_secs),
            expected_principal: self.pointer.reader_user.clone(),
            expected_database: self.pointer.database.clone(),
            expected_in_recovery: self.in_recovery,
            expected_postmaster_start: self.postmaster.clone(),
        }
    }

    fn config(&self, names: &[&str], statement_secs: u64, lock_secs: u64) -> PgSourceConfig {
        PgSourceConfig::try_new(
            SourceId::new(SOURCE).expect("source"),
            PgEndpoint::new(&self.pointer.host, self.pointer.port, &self.pointer.database).expect("endpoint"),
            PgCredentials::new(&self.pointer.reader_user, self.reader_pass.as_str()).expect("credentials"),
            SecretRef::new("secret://pg/review/reader").expect("secret"),
            PgTlsConfig::from_ca_pem(&self.ca).expect("tls"),
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(statement_secs),
                Duration::from_secs(lock_secs),
                256,
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

    async fn read_i32(&mut self, owner: &mut SnapshotOwner, chunk: &PlannedChunk, column: &str) -> Pulled<i32> {
        owner.open_chunk(chunk, self.budget()).await.expect("open chunk");
        let pulled = self.drain_i32(owner, column).await;
        owner.close_stream().await.expect("close chunk");
        pulled
    }

    async fn read_text(&mut self, owner: &mut SnapshotOwner, chunk: &PlannedChunk, column: &str) -> Pulled<String> {
        owner.open_chunk(chunk, self.budget()).await.expect("open chunk");
        let pulled = self.drain_text(owner, column).await;
        owner.close_stream().await.expect("close chunk");
        pulled
    }

    async fn read_pairs(&mut self, owner: &mut SnapshotOwner, chunk: &PlannedChunk) -> Pulled<(String, i32)> {
        owner.open_chunk(chunk, self.budget()).await.expect("open chunk");
        let pulled = self.drain_pairs(owner).await;
        owner.close_stream().await.expect("close chunk");
        pulled
    }

    async fn drain_i32(&mut self, owner: &mut SnapshotOwner, column: &str) -> Pulled<i32> {
        let mut pulled = Pulled { values: Vec::new(), batch_resumable: false, resumable_key: false };
        loop {
            match owner.next_batch().await.expect("batch") {
                StreamItem::Batch(batch) => {
                    pulled.batch_resumable |= batch.resumable;
                    pulled.values.extend(i32_column(&batch.record_batch, column));
                }
                StreamItem::TableEof(eof) => {
                    pulled.resumable_key = eof.resumable_key;
                    break;
                }
                StreamItem::Exhausted(_) => panic!("exhausted before table eof"),
            }
        }
        pulled
    }

    async fn drain_text(&mut self, owner: &mut SnapshotOwner, column: &str) -> Pulled<String> {
        let mut pulled = Pulled { values: Vec::new(), batch_resumable: false, resumable_key: false };
        loop {
            match owner.next_batch().await.expect("batch") {
                StreamItem::Batch(batch) => {
                    pulled.batch_resumable |= batch.resumable;
                    pulled.values.extend(text_column(&batch.record_batch, column));
                }
                StreamItem::TableEof(eof) => {
                    pulled.resumable_key = eof.resumable_key;
                    break;
                }
                StreamItem::Exhausted(_) => panic!("exhausted before table eof"),
            }
        }
        pulled
    }

    async fn drain_pairs(&mut self, owner: &mut SnapshotOwner) -> Pulled<(String, i32)> {
        let mut pulled = Pulled { values: Vec::new(), batch_resumable: false, resumable_key: false };
        loop {
            match owner.next_batch().await.expect("batch") {
                StreamItem::Batch(batch) => {
                    pulled.batch_resumable |= batch.resumable;
                    let names = text_column(&batch.record_batch, "name");
                    let nums = i32_column(&batch.record_batch, "n");
                    assert_eq!(names.len(), nums.len());
                    pulled.values.extend(names.into_iter().zip(nums));
                }
                StreamItem::TableEof(eof) => {
                    pulled.resumable_key = eof.resumable_key;
                    break;
                }
                StreamItem::Exhausted(_) => panic!("exhausted before table eof"),
            }
        }
        pulled
    }

    async fn lock_sparse(&mut self) {
        let _ = sqlx::query("ROLLBACK").execute(&mut self.admin).await;
        sqlx::query("BEGIN").execute(&mut self.admin).await.expect("lock begin");
        sqlx::query("LOCK TABLE r3v.sparse IN ACCESS EXCLUSIVE MODE")
            .execute(&mut self.admin)
            .await
            .expect("lock");
    }

    async fn unlock_sparse(&mut self) {
        sqlx::query("ROLLBACK").execute(&mut self.admin).await.expect("unlock");
    }

    async fn state_of(&mut self, pid: i64, start: &str) -> Option<String> {
        let pid = i32::try_from(pid).expect("pid");
        let row = sqlx::query(
            "SELECT state FROM pg_catalog.pg_stat_activity WHERE pid = $1::int4 AND backend_start::text = $2",
        )
        .bind(pid)
        .bind(start)
        .fetch_optional(&mut self.admin)
        .await
        .expect("state");
        row.and_then(|row| row.try_get(0).ok())
    }

    async fn reader_backends(&mut self) -> Vec<(i64, String, String)> {
        oo_backends(&mut self.admin, &self.pointer.reader_user).await
    }

    async fn backend_detail(&mut self) -> Vec<String> {
        backend_detail(&mut self.admin, &self.pointer.reader_user).await
    }

    async fn drive_until_active(
        &mut self,
        pull: &mut std::pin::Pin<&mut impl std::future::Future<Output = Result<StreamItem, SourceError>>>,
        pid: i64,
        start: &str,
    ) -> bool {
        let limit = Instant::now() + Duration::from_secs(15);
        while Instant::now() < limit {
            tokio::select! {
                biased;
                _ = pull.as_mut() => panic!("pull ended before the backend was active"),
                _ = tokio::time::sleep(Duration::from_millis(30)) => {}
            }
            if self.state_of(pid, start).await.as_deref() == Some("active") {
                return true;
            }
        }
        false
    }

    async fn wait_absent(&mut self, before: &[(i64, String, String)], began: Instant) {
        let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
        loop {
            let left = self.reader_backends().await;
            let still: Vec<_> = before
                .iter()
                .filter(|row| left.iter().any(|live| live.0 == row.0 && live.1 == row.1))
                .collect();
            let busy: Vec<_> = left
                .iter()
                .filter(|row| row.2 == "active" || row.2 == "idle in transaction")
                .collect();
            if still.is_empty() && busy.is_empty() {
                self.note(format!("owner_drop_absent_ms={}", began.elapsed().as_millis()));
                return;
            }
            if began.elapsed() > bound {
                let detail = self.backend_detail().await;
                panic!(
                    "reader backends remained after {}ms still={still:?} busy={busy:?} left={left:?} detail={detail:?}",
                    bound.as_millis()
                );
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn wait_clear(&mut self) {
        let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
        let began = Instant::now();
        loop {
            let left = self.reader_backends().await;
            if left.is_empty() {
                return;
            }
            if began.elapsed() > bound {
                panic!("reader backends still present after {}ms: {left:?}", bound.as_millis());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn audit(&mut self) {
        let raw_all = fs::read(&self.pointer.log_file).expect("postgres log");
        let raw_all_text = String::from_utf8_lossy(&raw_all);
        for secret in [self.reader_pass.as_str(), self.writer_pass.as_str(), self.super_pass.as_str()] {
            if !secret.is_empty() && raw_all_text.contains(secret) {
                panic!("postgres log contains a closed string");
            }
        }
        let mark = usize::try_from(self.log_mark).unwrap_or(0).min(raw_all.len());
        let raw = String::from_utf8_lossy(&raw_all[mark..]).to_string();
        let allowed = ["SELECT", "WITH", "START", "ROLLBACK", "SAVEPOINT", "RELEASE"];
        let mut oo_statements = 0_usize;
        let mut cancel_selects = 0_usize;
        let mut writer_writes = 0_usize;
        let mut verbs = BTreeMap::<String, usize>::new();
        let mut saw_bare = false;
        let mut saw_soft = false;
        let mut saw_page = false;
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
            *verbs.entry(verb.clone()).or_default() += 1;
            assert!(allowed.contains(&verb.as_str()), "reader verb {verb}: {sql}");
            if write_word(&strip_sql_literals(sql)).is_some() || has_token(sql, "OFFSET") {
                panic!("reader sql has a forbidden token: {sql}");
            }
            let upper = sql.to_ascii_uppercase();
            assert!(!upper.contains("COLLATE"), "{sql}");
            assert!(!sql.contains("pg_terminate_backend"), "{sql}");
            if sql.contains("pg_cancel_backend") {
                assert_eq!(verb, "SELECT", "cancel statement was not a select: {sql}");
                cancel_selects += 1;
            }
            if upper.contains("COUNT(*) OVER") {
                assert!(upper.contains("LIMIT"), "{sql}");
                saw_page = true;
            }
            let bare = sql.replace('"', "");
            if bare.contains("FROM r3v.bare") && bare.contains("SELECT") {
                saw_bare = true;
                assert!(!upper.contains("ORDER BY"), "{sql}");
                assert!(!upper.contains("WHERE"), "{sql}");
            }
            if bare.contains("FROM r3v.soft") && upper.contains("FROM") && !upper.contains("PG_CLASS") {
                if upper.starts_with("SELECT") && bare.contains("payload") {
                    saw_soft = true;
                    assert!(!upper.contains("ORDER BY"), "{sql}");
                    assert!(!upper.contains("WHERE"), "{sql}");
                }
            }
        }
        assert!(oo_statements > 0, "reader sql log was empty");
        assert!(cancel_selects > 0, "reader did not send select pg_cancel_backend");
        assert!(writer_writes > 0, "fixture writer was not logged");
        assert!(saw_page, "planning page sql was not logged");
        assert!(saw_bare, "unkeyed bare scan was not logged");
        assert!(saw_soft, "unkeyed soft scan was not logged");
        let verb_text = verbs
            .iter()
            .map(|(verb, count)| format!("{verb}={count}"))
            .collect::<Vec<_>>()
            .join(",");
        self.note(format!(
            "sql_audit oo_statements={oo_statements} cancel_selects={cancel_selects} writer_writes={writer_writes} dml_ddl_commit_offset=0 verbs={verb_text}"
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

fn table<'a>(tables: &'a [TableKeyPlan], name: &str) -> &'a TableKeyPlan {
    tables.iter().find(|table| table.table == name).expect(name)
}

fn assert_primary(plan: &TableKeyPlan, names: &[&str]) {
    let declared = plan.declared.as_ref().expect("declared key");
    assert_eq!(declared.kind, DeclaredKeyKind::Primary);
    assert_eq!(
        declared.columns.iter().map(|column| column.name.as_str()).collect::<Vec<_>>(),
        names
    );
}

fn assert_chain(chunks: &[PlannedChunk]) {
    assert!(chunks.len() >= 2);
    assert!(chunks.iter().all(|chunk| chunk.keyed && !chunk.resumable));
    assert!(chunks[0].lower.is_some());
    assert!(chunks.last().expect("chunk").upper.is_none());
    for window in chunks.windows(2) {
        assert_eq!(window[0].upper, window[1].lower);
    }
}

fn assert_unkeyed(plan: &TableKeyPlan) {
    assert!(plan.declared.is_none());
    assert_eq!(plan.row_count, None);
    assert_eq!(plan.stride, None);
    assert_eq!(plan.chunks.len(), 1);
    let chunk = &plan.chunks[0];
    assert!(!chunk.keyed && !chunk.resumable);
    assert!(chunk.lower.is_none() && chunk.upper.is_none());
}

fn one_i64(bound: &Option<Vec<ProbeValue>>) -> Option<i64> {
    let Some(values) = bound else {
        return None;
    };
    assert_eq!(values.len(), 1);
    match &values[0] {
        ProbeValue::Int8(value) => Some(*value),
        other => panic!("expected int bound, got {other:?}"),
    }
}

fn one_text(bound: &Option<Vec<ProbeValue>>) -> Option<String> {
    let Some(values) = bound else {
        return None;
    };
    assert_eq!(values.len(), 1);
    match &values[0] {
        ProbeValue::Text(value) => Some(value.clone()),
        other => panic!("expected text bound, got {other:?}"),
    }
}

fn one_pair(bound: &Option<Vec<ProbeValue>>) -> Option<(String, i32)> {
    let Some(values) = bound else {
        return None;
    };
    assert_eq!(values.len(), 2);
    match (&values[0], &values[1]) {
        (ProbeValue::Text(name), ProbeValue::Int8(n)) => Some((name.clone(), i32::try_from(*n).expect("n"))),
        other => panic!("expected composite bound, got {other:?}"),
    }
}

fn i32_column(batch: &RecordBatch, name: &str) -> Vec<i32> {
    let column = batch.column_by_name(name).unwrap_or_else(|| panic!("missing {name}"));
    let values = column.as_any().downcast_ref::<Int32Array>().unwrap_or_else(|| panic!("{name} int32"));
    (0..values.len())
        .map(|index| {
            assert!(!values.is_null(index));
            values.value(index)
        })
        .collect()
}

fn text_column(batch: &RecordBatch, name: &str) -> Vec<String> {
    let column = batch.column_by_name(name).unwrap_or_else(|| panic!("missing {name}"));
    let values = column.as_any().downcast_ref::<StringArray>().unwrap_or_else(|| panic!("{name} utf8"));
    (0..values.len())
        .map(|index| {
            assert!(!values.is_null(index));
            values.value(index).to_string()
        })
        .collect()
}

fn safe_ident(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn quote_ident(name: &str) -> String {
    assert!(safe_ident(name), "{name}");
    format!("\"{name}\"")
}

async fn load_admitted(admin: &mut PgConnection) -> AdmittedCatalog {
    let rows = sqlx::query(
        "SELECT c.relname::text, a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
         WHERE n.nspname = 'r3v' AND c.relkind = 'r' \
         ORDER BY c.relname, a.attnum",
    )
    .fetch_all(&mut *admin)
    .await
    .expect("admitted columns");
    let mut columns: BTreeMap<String, Vec<AdmittedColumn>> = BTreeMap::new();
    for row in &rows {
        let table: String = row.try_get(0).unwrap();
        columns.entry(table).or_default().push(AdmittedColumn {
            attnum: row.try_get(1).unwrap(),
            name: row.try_get(2).unwrap(),
            type_oid: row.try_get(3).unwrap(),
            typmod: row.try_get(4).unwrap(),
            not_null: row.try_get(5).unwrap(),
        });
    }
    let keys = sqlx::query(
        "SELECT c.relname::text, a.attname::text \
         FROM pg_catalog.pg_index i \
         JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS u(attnum, ord) ON true \
         JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = u.attnum \
         WHERE n.nspname = 'r3v' AND i.indisprimary \
         ORDER BY c.relname, u.ord",
    )
    .fetch_all(&mut *admin)
    .await
    .expect("admitted keys");
    let mut primary: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in &keys {
        primary.entry(row.try_get(0).unwrap()).or_default().push(row.try_get(1).unwrap());
    }
    let mut tables = Vec::new();
    for name in TABLES {
        tables.push(AdmittedTable {
            schema: SCHEMA.to_string(),
            name: (*name).to_string(),
            columns: columns.remove(*name).unwrap_or_else(|| panic!("missing {name}")),
            primary_key: primary.remove(*name).unwrap_or_default(),
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

async fn backend_detail(admin: &mut PgConnection, user: &str) -> Vec<String> {
    let rows = sqlx::query(
        "SELECT pid::int8, COALESCE(state, ''), COALESCE(wait_event_type, ''), \
         COALESCE(wait_event, ''), xact_start IS NOT NULL, left(query, 48) \
         FROM pg_catalog.pg_stat_activity \
         WHERE application_name = 'oo-pg-source' AND usename::text = $1 \
         ORDER BY pid",
    )
    .bind(user)
    .fetch_all(&mut *admin)
    .await
    .expect("backend detail");
    rows.iter()
        .map(|row| {
            format!(
                "pid={} state={} wait={}/{} xact={} query={}",
                row.try_get::<i64, _>(0).unwrap_or(0),
                row.try_get::<String, _>(1).unwrap_or_default(),
                row.try_get::<String, _>(2).unwrap_or_default(),
                row.try_get::<String, _>(3).unwrap_or_default(),
                row.try_get::<bool, _>(4).unwrap_or(false),
                row.try_get::<String, _>(5).unwrap_or_default()
            )
        })
        .collect()
}

async fn oo_backends(admin: &mut PgConnection, user: &str) -> Vec<(i64, String, String)> {
    let rows = sqlx::query(
        "SELECT pid::int8, backend_start::text, COALESCE(state, '') \
         FROM pg_catalog.pg_stat_activity \
         WHERE application_name = 'oo-pg-source' AND usename::text = $1",
    )
    .bind(user)
    .fetch_all(&mut *admin)
    .await
    .expect("backends");
    rows.iter()
        .map(|row| {
            (
                row.try_get(0).expect("pid"),
                row.try_get(1).expect("start"),
                row.try_get(2).expect("state"),
            )
        })
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
