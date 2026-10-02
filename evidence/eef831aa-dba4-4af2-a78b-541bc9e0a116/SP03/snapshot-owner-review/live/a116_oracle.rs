//! Independent R1 oracle for SnapshotOwner. It calls the public API only.
//! Expected rows and catalog text were registered before this process opens an owner.
//! Author outcome labels are not inputs.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use oo_connectors::types::{
    ArrowScalar, CellPresence, DecodedCell, RejectCellPolicy, TypedBatch, UnsupportedColumnPolicy,
};
use oo_connectors::{
    config_identity, connector_capabilities, schema_hash, snapshot_input_digest, type_capabilities,
    type_policy_identity, AdmittedCatalog, AdmittedColumn, AdmittedTable, BackendKey, CutProbe,
    PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, PgTypePolicy,
    ProbeValue, SecretRef, SelectedTable, SnapshotOwner, SnapshotRequest, SourceError, SourceId,
    TableSelection, CLEANUP_OBSERVATION_BOUND_MS, SNAPSHOT_PROTOCOL, TYPE_CONVERSION_PROTOCOL,
};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Row};

const ROW_ITEM_7: &str = "int:7 text:cedar bool:true text:A116 null";
const ROW_LINE_21: &str = "int:21 int:7 int:4 text:sprig";
const ROW_LINE_22: &str = "int:22 int:7 int:5 null";
const FRESH_ITEM_7: &str = "int:7 text:willow bool:true text:A116 text:kept";
const FRESH_LINE_21: &str = "int:21 int:7 int:9 text:sprig";
const FRESH_LINE_22: &str = "int:22 int:7 int:5 null";
const FRESH_LINE_23: &str = "int:23 int:7 int:1 text:fresh";
const WRITER_SQL: [&str; 3] = [
    "UPDATE rev.item SET label = 'willow', \"odd\"\"note\" = 'kept' WHERE id = 7",
    "UPDATE rev.\"line item\" SET qty = 9 WHERE id = 21",
    "INSERT INTO rev.\"line item\" (id, item_id, qty, memo) VALUES (23, 7, 1, 'fresh')",
];
const ITEM_SQL: &str = "SELECT id::text, label, CASE WHEN flag THEN 'true' ELSE 'false' END, code, CASE WHEN \"odd\"\"note\" IS NULL THEN '<null>' ELSE \"odd\"\"note\" END FROM rev.item WHERE id = $1";
const LINE_SQL: &str = "SELECT id::text, item_id::text, qty::text, CASE WHEN memo IS NULL THEN '<null>' ELSE memo END FROM rev.\"line item\" WHERE id = $1";
const ATTR_SQL: &str = "SELECT count(*)::int8 FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'rev' AND c.relname = 'item' AND a.attnum > 0 AND NOT a.attisdropped";
const ACT_SQL: &str = "SELECT COALESCE(state, ''), (xact_start IS NOT NULL), COALESCE(application_name, ''), COALESCE(usename::text, ''), COALESCE(datname::text, '') FROM pg_stat_activity WHERE pid = $1 AND backend_start::text = $2";
const OO_SQL: &str = "SELECT count(*)::int8, COALESCE(sum(CASE WHEN state = 'idle in transaction' THEN 1 ELSE 0 END), 0)::int8 FROM pg_stat_activity WHERE application_name = 'oo-pg-source'";
const MATCH_SQL: &str = "SELECT count(*)::int8 FROM pg_stat_activity WHERE pid = $1 AND backend_start::text = $2 AND application_name = 'oo-pg-source' AND usename::text = $3 AND datname::text = $4";
const KILL_SQL: &str = "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE pid = $1 AND backend_start::text = $2 AND application_name = 'oo-pg-source' AND usename::text = $3 AND datname::text = $4";
const STATS_SQL: &str = "SELECT COALESCE(n_tup_ins, 0)::int8, COALESCE(n_tup_upd, 0)::int8, COALESCE(n_tup_del, 0)::int8 FROM pg_stat_user_tables WHERE schemaname = 'rev' AND relname = $1";
const CANARY_SQL: &str = "SELECT n::int8 FROM rev.canary WHERE id = 1";
const TWIN_COLS_SQL: &str = "SELECT a.attnum::int4, a.attname, a.atttypid::int8, a.atttypmod::int4, CASE WHEN a.attnotnull THEN 1 ELSE 0 END::int4 FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'rev' AND c.relname = 'twin' AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum";
const TWIN_N_SQL: &str = "SELECT count(*)::int8 FROM rev.twin WHERE id = 3";
const TWIN_PK_SQL: &str = "SELECT a.attname FROM pg_catalog.pg_constraint con JOIN pg_catalog.pg_class c ON c.oid = con.conrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = con.conkey[1] WHERE n.nspname = 'rev' AND c.relname = 'twin' AND con.contype = 'p'";
const ALLOWED: &[&str] = &["SELECT", "WITH", "START", "ROLLBACK", "SET", "SHOW"];
const FORBIDDEN: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "TRUNCATE", "MERGE", "COPY", "ALTER", "CREATE", "DROP", "GRANT",
    "REVOKE", "COMMENT", "COMMIT", "VACUUM", "CLUSTER", "REINDEX",
];

struct Col {
    schema: String,
    table: String,
    attnum: i32,
    name: String,
    type_oid: i64,
    typmod: i32,
    not_null: bool,
}

struct RowExpect {
    table: String,
    key: i64,
    tokens: String,
}

struct Cut {
    postmaster: String,
    database: String,
    principal: String,
    in_recovery: bool,
    version: String,
    columns: Vec<Col>,
    pks: Vec<(String, String, String)>,
    rows: Vec<RowExpect>,
    fresh: Vec<RowExpect>,
    absent: Vec<(String, i64)>,
    writer: Vec<String>,
    canary_before: i64,
    writer_canary_delta: i64,
    twin_count: i64,
    marker: String,
}

struct SpecTweaks {
    flip_config: bool,
    flip_policy: bool,
    shorten: bool,
}

struct OpenSpec {
    source: String,
    generation: u64,
    deadline: Instant,
    principal: String,
    database: String,
    endpoint_database: String,
    recovery: bool,
    postmaster: String,
    policy: PgTypePolicy,
    user: String,
    password: String,
    host: String,
    ca: Vec<u8>,
    secret_ref: String,
    tables: Vec<AdmittedTable>,
    tweaks: SpecTweaks,
    port: u16,
}

struct ProbeView<'a> {
    name: &'a str,
    table: &'a str,
    key: i64,
    generation: u64,
    digest: [u8; 32],
    source: &'a str,
}

struct Act {
    state: String,
    in_xact: bool,
    app: String,
    user: String,
    db: String,
}

struct Stat {
    ins: i64,
    upd: i64,
    del: i64,
}

struct Ctx {
    admin: PgConnection,
    cut: Cut,
    host: String,
    wrong_host: String,
    port: u16,
    reader_user: String,
    writer_user: String,
    guc_user: String,
    reader_password: String,
    writer_password: String,
    guc_password: String,
    ca: Vec<u8>,
    wrong_ca: Vec<u8>,
    secrets: Vec<String>,
    log_path: String,
    outcome: String,
    lines: Vec<String>,
    fails: Vec<String>,
    started: Instant,
    leak_noted: bool,
}

impl Drop for Ctx {
    fn drop(&mut self) {
        self.flush();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a116_snapshot_oracle() {
    let mut ctx = match Ctx::boot().await {
        Ok(ctx) => ctx,
        Err(reason) => panic!("oracle_boot_failed {reason}"),
    };
    ctx.phase("registered");
    if !ctx.cut_is_preregistered() {
        ctx.mark("cut_preregistered", false, "cut_file");
        ctx.close();
        return;
    }
    ctx.mark("cut_preregistered", true, "matched");
    ctx.local_checks();
    ctx.refusals().await;
    ctx.quiet().await;
    ctx.same_cut().await;
    ctx.abort_case().await;
    ctx.drop_case().await;
    ctx.deadline_probe().await;
    ctx.deadline_finish().await;
    ctx.terminate_case().await;
    ctx.admission().await;
    ctx.catalog_tamper().await;
    ctx.catalog_then_revoke().await;
    ctx.inflight_table().await;
    ctx.inflight_column().await;
    ctx.write_capable().await;
    ctx.add_column_case().await;
    ctx.inheritance_case().await;
    ctx.empty_key().await;
    ctx.text_key().await;
    ctx.fail_read().await;
    ctx.final_quiet().await;
    ctx.audit().await;
    ctx.close();
}

impl Ctx {
    async fn boot() -> Result<Self, &'static str> {
        let pointer_path = std::env::var("OO_A116_FIXTURE").map_err(|_| "missing_fixture")?;
        let cut_path = std::env::var("OO_A116_CUT").map_err(|_| "missing_cut")?;
        if cut_path != "/expected/cut.txt" {
            return Err("cut_path");
        }
        let pointer = std::fs::read_to_string(pointer_path).map_err(|_| "pointer_read")?;
        let map = pointer_map(&pointer);
        let host = req(&map, "host")?;
        let wrong_host = req(&map, "wrong_host")?;
        let database = req(&map, "database")?;
        let reader_user = req(&map, "reader_user")?;
        let writer_user = req(&map, "writer_user")?;
        let guc_user = req(&map, "guc_user")?;
        let admin_user = req(&map, "admin_user")?;
        let log_path = req(&map, "log_file")?;
        let port: u16 = req(&map, "port")?.parse().map_err(|_| "port")?;
        if host != "oo-a116-pg" || wrong_host != "oo-a116-wrong" || database != "revdb" || port != 5432 {
            return Err("pointer_identity");
        }
        if reader_user != "oo_rev_reader" || writer_user != "oo_rev_writer" || guc_user != "oo_rev_guc" || admin_user != "postgres" {
            return Err("pointer_roles");
        }
        if log_path != "/pglog/postgresql.log" || req(&map, "cut_file")? != "/expected/cut.txt" {
            return Err("pointer_paths");
        }
        let reader_password = read_secret(req(&map, "reader_password_file")?)?;
        let writer_password = read_secret(req(&map, "writer_password_file")?)?;
        let guc_password = read_secret(req(&map, "guc_password_file")?)?;
        let admin_password = read_secret(req(&map, "admin_password_file")?)?;
        let ca = read_ca(req(&map, "ca_file")?)?;
        let wrong_ca = read_ca(req(&map, "wrong_ca_file")?)?;
        let cut = parse_cut(&std::fs::read_to_string(cut_path).map_err(|_| "cut_read")?)?;
        let mut admin = connect_to(host, port, database, admin_user, &admin_password, "oo-a116-admin", &ca).await.map_err(|_| "admin_connect")?;
        sqlx::query("SET lock_timeout = '3s'").execute(&mut admin).await.map_err(|_| "admin_timeout")?;
        sqlx::query("SET statement_timeout = '30s'").execute(&mut admin).await.map_err(|_| "admin_timeout")?;
        let fallback = "/target/phases/oracle-outcome.txt".to_string();
        let outcome = std::env::var("OO_A116_OUTCOME").unwrap_or(fallback);
        if let Some(parent) = std::path::Path::new(&outcome).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut ctx = Self {
            admin,
            cut,
            host: host.to_string(),
            wrong_host: wrong_host.to_string(),
            port,
            reader_user: reader_user.to_string(),
            writer_user: writer_user.to_string(),
            guc_user: guc_user.to_string(),
            reader_password: reader_password.clone(),
            writer_password: writer_password.clone(),
            guc_password: guc_password.clone(),
            ca,
            wrong_ca,
            secrets: vec![reader_password, writer_password, guc_password, admin_password],
            log_path: log_path.to_string(),
            outcome,
            lines: Vec::new(),
            fails: Vec::new(),
            started: Instant::now(),
            leak_noted: false,
        };
        ctx.lines.push("oracle a116".to_string());
        ctx.lines.push("reviewed 2b6dd57a0dc3d36e098268638c0364af247d07fb".to_string());
        ctx.lines.push("author_outcome_reused no".to_string());
        ctx.lines.push("identity_drift source_inspected_not_executed".to_string());
        ctx.lines.push("nested_transaction source_inspected_not_executed".to_string());
        ctx.lines.push("boot pass".to_string());
        ctx.flush();
        Ok(ctx)
    }

    fn phase(&mut self, name: &str) {
        let ms = self.started.elapsed().as_millis();
        self.lines.push(format!("phase {name} ms={ms}"));
        self.flush();
    }

    fn mark(&mut self, name: &str, ok: bool, detail: &str) {
        if !ok {
            self.fails.push(name.to_string());
        }
        let status = if ok { "pass" } else { "fail" };
        self.lines.push(format!("{name} {status} {detail}"));
        self.flush();
    }

    fn flush(&mut self) {
        let mut body = self.lines.join("\n");
        body.push('\n');
        let mut leaked = false;
        for secret in &self.secrets {
            if secret.len() >= 8 && body.contains(secret) {
                body = body.replace(secret, "[redacted]");
                leaked = true;
            }
        }
        if body.contains("PRIVATE KEY") || body.contains("BEGIN CERTIFICATE") {
            body = body.replace("PRIVATE KEY", "[redacted]");
            body = body.replace("BEGIN CERTIFICATE", "[redacted]");
            leaked = true;
        }
        if leaked && !self.leak_noted {
            self.leak_noted = true;
            self.fails.push("password_in_outcome".to_string());
            self.lines.push("password_in_outcome fail".to_string());
            body = self.lines.join("\n");
            body.push('\n');
            for secret in &self.secrets {
                if secret.len() >= 8 {
                    body = body.replace(secret, "[redacted]");
                }
            }
        }
        let _ = std::fs::write(&self.outcome, body);
    }

    fn close(&mut self) {
        let count = self.fails.len();
        self.lines.push(format!("fail_count={count}"));
        if count == 0 {
            self.lines.push("oracle_result pass".to_string());
        } else {
            self.lines.push("oracle_result fail".to_string());
        }
        self.flush();
        if count > 0 {
            let names = self.fails.join(",");
            panic!("oracle_failed {names}");
        }
    }

    fn cut_is_preregistered(&self) -> bool {
        self.cut.database == "revdb"
            && self.cut.principal == "oo_rev_reader"
            && !self.cut.in_recovery
            && self.cut.version.contains("17.10")
            && !self.cut.postmaster.is_empty()
            && self.cut.marker == "A116_AUDIT_WINDOW_START"
            && self.cut.canary_before == 0
            && self.cut.writer_canary_delta == 3
            && self.cut.twin_count == 2
            && writer_registered(&self.cut.writer)
            && row_tokens(&self.cut.rows, "item", 7) == Some(ROW_ITEM_7)
            && row_tokens(&self.cut.rows, "line item", 21) == Some(ROW_LINE_21)
            && row_tokens(&self.cut.rows, "line item", 22) == Some(ROW_LINE_22)
            && row_tokens(&self.cut.fresh, "item", 7) == Some(FRESH_ITEM_7)
            && row_tokens(&self.cut.fresh, "line item", 21) == Some(FRESH_LINE_21)
            && row_tokens(&self.cut.fresh, "line item", 22) == Some(FRESH_LINE_22)
            && row_tokens(&self.cut.fresh, "line item", 23) == Some(FRESH_LINE_23)
            && self.cut.absent.contains(&("line item".to_string(), 99))
            && columns_match(&self.cut)
    }

    fn local_checks(&mut self) {
        self.phase("local");
        self.mark("snapshot_protocol", SNAPSHOT_PROTOCOL == "oo.source.pgsql.snapshot/v1", SNAPSHOT_PROTOCOL);
        self.mark("type_protocol", TYPE_CONVERSION_PROTOCOL == "oo.source.pgsql.types/v1", TYPE_CONVERSION_PROTOCOL);
        self.mark("cleanup_bound", CLEANUP_OBSERVATION_BOUND_MS == 5_000, "5000");
        let caps = connector_capabilities();
        let names: Vec<&str> = caps.iter().map(|item| item.name.as_str()).collect();
        let needed = ["scan", "bounded_read", "cdc", "type_conversion", "registry_api", "source_ddl", "source_write"];
        let caps_ok = caps.iter().all(|item| !item.supported) && needed.iter().all(|name| names.contains(name));
        self.mark("connector_capabilities", caps_ok, "unsupported");
        let bounded = type_capabilities().iter().any(|item| item.name == "bounded_reader" && !item.supported);
        self.mark("type_bounded_reader", bounded, "unsupported");
        let same = self.sample_config(&self.reader_password, &self.ca, &self.host, "rev_src");
        let other_password = self.sample_config("other-secret-value", &self.ca, &self.host, "rev_src");
        let other_ca = self.sample_config(&self.reader_password, &self.wrong_ca, &self.host, "rev_src");
        let other_host = self.sample_config(&self.reader_password, &self.ca, "oo-a116-other", "rev_src");
        let identity_ok = match (same, other_password, other_ca, other_host) {
            (Ok(left), Ok(password), Ok(ca), Ok(host)) => {
                let base = config_identity(&left);
                let hides = debug_hides(&left, &self.reader_password);
                self.mark("debug_redacts_config", hides, "redacted");
                base == config_identity(&password) && base == config_identity(&ca) && base != config_identity(&host)
            }
            _ => false,
        };
        self.mark("config_identity_stable", identity_ok, "password_ca_omitted");
        let preserve = type_policy_identity(PgTypePolicy::PRESERVE_ROW);
        let fail = type_policy_identity(PgTypePolicy {
            unsupported: UnsupportedColumnPolicy::FailRead,
            reject: RejectCellPolicy::PreserveRow,
        });
        self.mark("type_policy_identity_changes", preserve != fail, "policy");
        let hash = "ab".repeat(32);
        let first = snapshot_input_digest(&preserve, &preserve, &hash, 1);
        let second = snapshot_input_digest(&preserve, &preserve, &hash, 2);
        let digest_ok = matches!((first, second), (Ok(left), Ok(right)) if left != right);
        self.mark("digest_tracks_generation", digest_ok, "generation");
    }

    fn sample_config(&self, password: &str, ca: &[u8], host: &str, source: &str) -> Result<PgSourceConfig, SourceError> {
        let spec = self.reader_spec(source, 1);
        config_of(&spec, password, ca, host)
    }

    fn reader_spec(&self, source: &str, generation: u64) -> OpenSpec {
        self.spec(source, generation, self.admitted(&["item", "line item"]))
    }

    fn line_spec(&self, source: &str, generation: u64) -> OpenSpec {
        self.spec(source, generation, self.admitted(&["line item"]))
    }

    fn spec(&self, source: &str, generation: u64, tables: Vec<AdmittedTable>) -> OpenSpec {
        OpenSpec {
            source: source.to_string(),
            generation,
            deadline: Instant::now() + Duration::from_secs(30),
            principal: self.cut.principal.clone(),
            database: self.cut.database.clone(),
            endpoint_database: self.cut.database.clone(),
            recovery: self.cut.in_recovery,
            postmaster: self.cut.postmaster.clone(),
            policy: PgTypePolicy::PRESERVE_ROW,
            user: self.reader_user.clone(),
            password: self.reader_password.clone(),
            host: self.host.clone(),
            ca: self.ca.clone(),
            secret_ref: "secret://a116/reader".to_string(),
            tables,
            tweaks: SpecTweaks { flip_config: false, flip_policy: false, shorten: false },
            port: self.port,
        }
    }

    fn admitted(&self, names: &[&str]) -> Vec<AdmittedTable> {
        let mut out = Vec::new();
        for name in names {
            let mut columns: Vec<&Col> = self.cut.columns.iter().filter(|col| col.schema == "rev" && col.table == *name).collect();
            columns.sort_by_key(|col| col.attnum);
            let primary_key = self.cut.pks.iter().filter(|item| item.0 == "rev" && item.1 == *name).map(|item| item.2.clone()).collect();
            out.push(AdmittedTable {
                schema: "rev".to_string(),
                name: (*name).to_string(),
                columns: columns.iter().map(|col| AdmittedColumn {
                    attnum: col.attnum,
                    name: col.name.clone(),
                    type_oid: col.type_oid,
                    typmod: col.typmod,
                    not_null: col.not_null,
                }).collect(),
                primary_key,
            });
        }
        out
    }

    async fn refusals(&mut self) {
        self.phase("refusals");
        let before = self.oo_count().await;
        let mut spec = self.reader_spec("rev_src", 0);
        self.must_fail("generation_zero", &spec, "generation_invalid").await;
        spec.generation = 41;
        spec.deadline = Instant::now() - Duration::from_secs(1);
        self.must_fail("deadline_before_reserve", &spec, "deadline").await;
        spec.deadline = Instant::now() + Duration::from_secs(30);
        spec.tweaks.flip_config = true;
        self.must_fail("config_identity_mismatch", &spec, "config_identity").await;
        spec.tweaks.flip_config = false;
        spec.tweaks.flip_policy = true;
        self.must_fail("type_policy_mismatch", &spec, "type_policy_identity").await;
        spec.tweaks.flip_policy = false;
        spec.tweaks.shorten = true;
        self.must_fail("selection_mismatch", &spec, "selection").await;
        let after = self.oo_count().await;
        let stable = matches!((before, after), (Ok((left, _)), Ok((right, _))) if left == right);
        self.mark("prereserve_backend_stable", stable, "oo_pg_source");
    }

    async fn quiet(&mut self) {
        self.phase("quiet");
        self.inspect_reader().await;
        let before_stats = self.stats_set().await;
        let before_canary = self.canary().await;
        let spec = self.reader_spec("rev_src", 41);
        let Some(mut owner) = self.open_owner(&spec).await else { return };
        self.mark("quiet_debug", owner_hides(&owner, &self.reader_password), "redacted");
        self.check_binding("quiet", &owner, 41, "rev_src");
        self.expect_idle(&owner, "quiet_idle").await;
        let digest = owner.binding().input_digest;
        self.read_expect(&mut owner, view("quiet_item", "item", 7, 41, digest, "rev_src"), ROW_ITEM_7).await;
        self.read_expect(&mut owner, view("quiet_line_21", "line item", 21, 41, digest, "rev_src"), ROW_LINE_21).await;
        self.read_expect(&mut owner, view("quiet_line_22", "line item", 22, 41, digest, "rev_src"), ROW_LINE_22).await;
        self.read_absent(&mut owner, view("quiet_absent_99", "line item", 99, 41, digest, "rev_src")).await;
        self.read_absent(&mut owner, view("quiet_absent_23", "line item", 23, 41, digest, "rev_src")).await;
        self.finish_ok(owner, "quiet").await;
        self.must_fail("quiet_reuse", &spec, "generation_reused").await;
        self.mark_delta("quiet_delta", &before_stats, &before_canary).await;
    }

    async fn same_cut(&mut self) {
        self.phase("same_cut");
        let before_canary = self.canary().await;
        let spec = self.reader_spec("rev_src", 42);
        let Some(mut owner) = self.open_owner(&spec).await else { return };
        self.check_binding("same", &owner, 42, "rev_src");
        self.expect_idle(&owner, "same_idle").await;
        let digest = owner.binding().input_digest;
        let mut side = match self.connect_reader("oo-a116-rr-oracle").await {
            Ok(conn) => conn,
            Err(code) => {
                self.mark("same_side", false, &code);
                self.abort_ok(owner, "same_abort").await;
                return;
            }
        };
        if let Err(err) = sqlx::query("START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY").execute(&mut side).await {
            self.mark("same_side_begin", false, &driver_code(&err));
            self.abort_ok(owner, "same_abort").await;
            return;
        }
        let wrote = self.run_writer().await;
        self.compare(&mut owner, &mut side, view("same_item", "item", 7, 42, digest, "rev_src"), ROW_ITEM_7).await;
        self.compare(&mut owner, &mut side, view("same_line_21", "line item", 21, 42, digest, "rev_src"), ROW_LINE_21).await;
        self.compare(&mut owner, &mut side, view("same_line_22", "line item", 22, 42, digest, "rev_src"), ROW_LINE_22).await;
        self.read_absent(&mut owner, view("same_absent_99", "line item", 99, 42, digest, "rev_src")).await;
        self.read_absent(&mut owner, view("same_absent_23", "line item", 23, 42, digest, "rev_src")).await;
        let side_absent = side_tokens(&mut side, "line item", 23).await;
        self.mark("same_side_absent_23", matches!(side_absent, Ok(None)), "absent");
        self.finish_ok(owner, "same").await;
        let _ = sqlx::query("ROLLBACK").execute(&mut side).await;
        drop(side);
        self.must_fail("same_reuse", &spec, "generation_reused").await;
        if wrote {
            match (before_canary, self.canary().await) {
                (Ok(before), Ok(after)) => {
                    let delta = after - before;
                    self.mark("writer_canary_delta", delta == self.cut.writer_canary_delta, &format!("delta={delta}"));
                }
                _ => self.mark("writer_canary_delta", false, "canary"),
            }
        }
        self.fresh_generation().await;
    }

    async fn fresh_generation(&mut self) {
        let spec = self.reader_spec("rev_src", 70);
        let Some(mut owner) = self.open_owner(&spec).await else { return };
        let digest = owner.binding().input_digest;
        self.read_expect(&mut owner, view("fresh_item", "item", 7, 70, digest, "rev_src"), FRESH_ITEM_7).await;
        self.read_expect(&mut owner, view("fresh_line_21", "line item", 21, 70, digest, "rev_src"), FRESH_LINE_21).await;
        self.read_expect(&mut owner, view("fresh_line_22", "line item", 22, 70, digest, "rev_src"), FRESH_LINE_22).await;
        self.read_expect(&mut owner, view("fresh_line_23", "line item", 23, 70, digest, "rev_src"), FRESH_LINE_23).await;
        self.finish_ok(owner, "fresh").await;
        match self.connect_reader("oo-a116-rr-oracle").await {
            Ok(mut conn) => {
                let item = side_tokens(&mut conn, "item", 7).await;
                let line = side_tokens(&mut conn, "line item", 23).await;
                self.mark("autocommit_item", matches!(item, Ok(Some(ref tokens)) if tokens == FRESH_ITEM_7), "tokens");
                self.mark("autocommit_line_23", matches!(line, Ok(Some(ref tokens)) if tokens == FRESH_LINE_23), "tokens");
            }
            Err(code) => self.mark("autocommit_connect", false, &code),
        }
    }

    async fn abort_case(&mut self) {
        self.phase("abort");
        let spec = self.reader_spec("rev_src", 43);
        let Some(mut owner) = self.open_owner(&spec).await else { return };
        let digest = owner.binding().input_digest;
        self.read_expect(&mut owner, view("abort_item", "item", 7, 43, digest, "rev_src"), FRESH_ITEM_7).await;
        self.abort_ok(owner, "abort").await;
        self.must_fail("abort_reuse", &spec, "generation_reused").await;
    }

    async fn drop_case(&mut self) {
        self.phase("drop");
        let spec = self.reader_spec("rev_src", 44);
        let Some(owner) = self.open_owner(&spec).await else { return };
        let key = owner.binding().backend.clone();
        drop(owner);
        self.lines.push("drop_returned_snapshot_finished no".to_string());
        tokio::task::yield_now().await;
        self.release(&key, "drop_release").await;
        self.must_fail("drop_reuse", &spec, "generation_reused").await;
    }

    async fn deadline_probe(&mut self) {
        self.phase("deadline_probe");
        let mut spec = self.reader_spec("rev_src", 45);
        spec.deadline = Instant::now() + Duration::from_secs(20);
        let Some(mut owner) = self.open_owner(&spec).await else { return };
        let key = owner.binding().backend.clone();
        wait_past(spec.deadline).await;
        match probe_at(&mut owner, "item", 7).await {
            Err(err) => self.mark("deadline_probe", err.fact() == Some("deadline"), &show(&err)),
            Ok(_) => self.mark("deadline_probe", false, "batch"),
        }
        match owner.finish().await {
            Ok(_) => self.mark("deadline_probe_finish", false, "SnapshotFinished"),
            Err(err) => self.mark("deadline_probe_finish", err.fact() == Some("deadline"), &show(&err)),
        }
        self.release(&key, "deadline_probe_release").await;
    }

    async fn deadline_finish(&mut self) {
        self.phase("deadline_finish");
        let mut spec = self.reader_spec("rev_src", 46);
        spec.deadline = Instant::now() + Duration::from_secs(20);
        let Some(owner) = self.open_owner(&spec).await else { return };
        let key = owner.binding().backend.clone();
        wait_past(spec.deadline).await;
        match owner.finish().await {
            Ok(_) => self.mark("deadline_finish", false, "SnapshotFinished"),
            Err(err) => self.mark("deadline_finish", err.fact() == Some("deadline"), &show(&err)),
        }
        self.release(&key, "deadline_finish_release").await;
    }

    async fn terminate_case(&mut self) {
        self.phase("terminate");
        let spec = self.reader_spec("rev_src", 47);
        let Some(mut owner) = self.open_owner(&spec).await else { return };
        let key = owner.binding().backend.clone();
        let Some(pid) = i32::try_from(key.pid).ok() else {
            self.mark("terminate_pid", false, "pid");
            self.abort_ok(owner, "terminate_abort").await;
            return;
        };
        let count = self.match_count(pid, &key.backend_start).await;
        if count != Ok(1) {
            self.mark("terminate_match", false, "count");
            self.abort_ok(owner, "terminate_abort").await;
            return;
        }
        self.mark("terminate_match", true, "1");
        let killed = self.terminate(pid, &key.backend_start).await;
        self.mark("terminate_sent", killed == Ok(true), "sent");
        match probe_at(&mut owner, "item", 7).await {
            Err(err) => self.mark("terminate_probe", true, &show(&err)),
            Ok(_) => self.mark("terminate_probe", false, "batch"),
        }
        match owner.finish().await {
            Ok(_) => self.mark("terminate_finish", false, "SnapshotFinished"),
            Err(err) => self.mark("terminate_finish", true, &show(&err)),
        }
        self.release(&key, "terminate_release").await;
        self.must_fail("terminate_reuse", &spec, "generation_reused").await;
        let later = self.reader_spec("rev_src", 71);
        let Some(mut owner) = self.open_owner(&later).await else { return };
        let digest = owner.binding().input_digest;
        self.read_expect(&mut owner, view("terminate_later", "item", 7, 71, digest, "rev_src"), FRESH_ITEM_7).await;
        self.finish_ok(owner, "terminate_later").await;
    }

    async fn admission(&mut self) {
        self.phase("admission");
        let mut principal = self.reader_spec("rev_src", 49);
        principal.principal = "oo_rev_other".to_string();
        self.fail_release("wrong_principal", &principal, "principal").await;
        let mut database = self.reader_spec("rev_src", 50);
        database.database = "otherdb".to_string();
        self.fail_release("wrong_database", &database, "database").await;
        let mut postmaster = self.reader_spec("rev_src", 51);
        postmaster.postmaster = "not-this-postmaster".to_string();
        self.fail_release("wrong_postmaster", &postmaster, "read_node").await;
        let mut recovery = self.reader_spec("rev_src", 52);
        recovery.recovery = true;
        self.fail_release("wrong_recovery", &recovery, "read_node").await;
        self.inspect_guc().await;
        let mut guc = self.reader_spec("rev_guc", 1);
        guc.user = self.guc_user.clone();
        guc.password = self.guc_password.clone();
        guc.principal = self.guc_user.clone();
        guc.secret_ref = "secret://a116/guc".to_string();
        self.fail_release("guc_startup", &guc, "startup_options").await;
        let mut wrong_ca = self.reader_spec("rev_src", 53);
        wrong_ca.ca = self.wrong_ca.clone();
        self.fail_release("wrong_ca", &wrong_ca, "tls_handshake").await;
        let mut wrong_host = self.reader_spec("rev_src", 54);
        wrong_host.host = self.wrong_host.clone();
        self.fail_release("wrong_host", &wrong_host, "tls_handshake").await;
    }

    async fn catalog_tamper(&mut self) {
        self.phase("catalog_tamper");
        let mut spec = self.reader_spec("rev_src", 55);
        if let Some(column) = spec.tables.first_mut().and_then(|table| table.columns.first_mut()) {
            column.type_oid += 1;
        }
        self.fail_release("catalog_tamper", &spec, "catalog_identity").await;
    }

    async fn catalog_then_revoke(&mut self) {
        self.phase("catalog_then_revoke");
        let spec = self.reader_spec("rev_src", 56);
        let config = match build_config(&spec) {
            Ok(config) => config,
            Err(err) => {
                self.mark("prior_catalog_config", false, &show(&err));
                return;
            }
        };
        match PgConnector::new(config).connect().await {
            Ok(mut session) => match session.catalog().await {
                Ok(_) => self.mark("prior_catalog", true, "collected"),
                Err(err) => self.mark("prior_catalog", false, &show(&err)),
            },
            Err(err) => self.mark("prior_catalog", false, &show(&err)),
        }
        self.settle("prior_catalog_release").await;
        if !self.admin_exec("revoke_before_open", "REVOKE SELECT ON TABLE rev.item FROM oo_rev_reader").await {
            return;
        }
        self.fail_release("revoked_open", &spec, "select_not_granted").await;
        self.admin_exec("grant_after_revoked_open", "GRANT SELECT ON TABLE rev.item TO oo_rev_reader").await;
    }

    async fn inflight_table(&mut self) {
        self.phase("inflight_table");
        let spec = self.reader_spec("rev_src", 57);
        let Some(mut owner) = self.open_owner(&spec).await else { return };
        if !self.admin_exec("revoke_inflight_table", "REVOKE SELECT ON TABLE rev.item FROM oo_rev_reader").await {
            self.abort_ok(owner, "inflight_table_abort").await;
            return;
        }
        match probe_at(&mut owner, "item", 7).await {
            Err(err) => self.mark("inflight_table_probe", privilege_fact(&err), &show(&err)),
            Ok(_) => self.mark("inflight_table_probe", false, "batch"),
        }
        self.finish_not(owner, "inflight_table_finish").await;
        self.admin_exec("grant_inflight_table", "GRANT SELECT ON TABLE rev.item TO oo_rev_reader").await;
    }

    async fn inflight_column(&mut self) {
        self.phase("inflight_column");
        let spec = self.reader_spec("rev_src", 58);
        let Some(mut owner) = self.open_owner(&spec).await else { return };
        if !self.admin_exec("revoke_inflight_column", "REVOKE SELECT (memo) ON TABLE rev.\"line item\" FROM oo_rev_reader").await {
            self.abort_ok(owner, "inflight_column_abort").await;
            return;
        }
        match probe_at(&mut owner, "line item", 22).await {
            Err(err) => self.mark("inflight_column_probe", privilege_fact(&err), &show(&err)),
            Ok(_) => self.mark("inflight_column_probe", false, "batch"),
        }
        self.abort_ok(owner, "inflight_column_abort").await;
        self.admin_exec("grant_inflight_column", "GRANT SELECT (memo) ON TABLE rev.\"line item\" TO oo_rev_reader").await;
    }

    async fn write_capable(&mut self) {
        self.phase("write_capable");
        if !self.admin_exec("grant_insert", "GRANT INSERT ON TABLE rev.item TO oo_rev_reader").await {
            return;
        }
        let spec = self.reader_spec("rev_src", 59);
        self.fail_release("write_capable", &spec, "write_capable").await;
        self.admin_exec("revoke_insert", "REVOKE INSERT ON TABLE rev.item FROM oo_rev_reader").await;
    }

    async fn add_column_case(&mut self) {
        self.phase("add_column");
        let mut spec = self.reader_spec("rev_src", 60);
        spec.deadline = Instant::now() + Duration::from_secs(30);
        let Some(owner) = self.open_owner(&spec).await else { return };
        let mut side = match self.connect_reader("oo-a116-rr-oracle").await {
            Ok(conn) => conn,
            Err(code) => {
                self.mark("add_column_side", false, &code);
                self.abort_ok(owner, "add_column_abort").await;
                return;
            }
        };
        if let Err(err) = sqlx::query("START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY").execute(&mut side).await {
            self.mark("add_column_begin", false, &driver_code(&err));
            self.abort_ok(owner, "add_column_abort").await;
            return;
        }
        let altered = self.admin_exec("add_column", "ALTER TABLE rev.item ADD COLUMN a116_extra text").await;
        let count = attr_count(&mut side).await;
        let _ = sqlx::query("ROLLBACK").execute(&mut side).await;
        self.decide_add_column(owner, altered, count).await;
        if altered {
            let next = self.reader_spec("rev_src", 61);
            self.fail_release("add_column_next", &next, "catalog_identity").await;
        }
    }

    async fn decide_add_column(&mut self, mut owner: SnapshotOwner, altered: bool, count: Result<i64, String>) {
        let digest = owner.binding().input_digest;
        match (altered, count) {
            (true, Ok(5)) => match probe_at(&mut owner, "item", 7).await {
                Ok(batch) => {
                    self.check_batch("add_column", &batch, "item", 60, digest, "rev_src");
                    match tokens_of(&batch) {
                        Ok(tokens) => self.mark("add_column_tokens", tokens == FRESH_ITEM_7, "old_shape"),
                        Err(reason) => self.mark("add_column_tokens", false, reason),
                    }
                    self.mark("add_column_agreement", batch.arrow.len() == 5, "side_count=5");
                    self.finish_ok(owner, "add_column").await;
                }
                Err(err) => {
                    self.mark("add_column_agreement", false, &show(&err));
                    self.finish_not(owner, "add_column_finish").await;
                }
            },
            (true, Ok(6)) => {
                match probe_at(&mut owner, "item", 7).await {
                    Err(err) => self.mark("add_column_agreement", err.fact() == Some("schema_drift"), &show(&err)),
                    Ok(_) => self.mark("add_column_agreement", false, "batch"),
                }
                self.finish_not(owner, "add_column_finish").await;
            }
            (_, Err(code)) => {
                self.mark("add_column_count", false, &code);
                self.abort_ok(owner, "add_column_abort").await;
            }
            _ => {
                self.mark("add_column_agreement", false, "alter_or_count");
                self.abort_ok(owner, "add_column_abort").await;
            }
        }
    }

    async fn inheritance_case(&mut self) {
        self.phase("inheritance");
        let Some(table) = self.twin_table().await else { return };
        let mut spec = self.reader_spec("rev_twin", 1);
        spec.tables = vec![table];
        spec.secret_ref = "secret://a116/twin".to_string();
        let Some(mut owner) = self.open_owner(&spec).await else { return };
        match probe_at(&mut owner, "twin", 3).await {
            Err(err) => self.mark("twin_probe", err.fact() == Some("probe_key"), &show(&err)),
            Ok(_) => self.mark("twin_probe", false, "batch"),
        }
        self.finish_not(owner, "twin_finish").await;
    }

    async fn empty_key(&mut self) {
        self.phase("empty_key");
        let spec = self.line_spec("rev_src", 62);
        let Some(mut owner) = self.open_owner(&spec).await else { return };
        let result = owner.read_probe(CutProbe {
            schema: "rev".to_string(),
            table: "line item".to_string(),
            key: Vec::new(),
        }).await;
        match result {
            Err(err) => self.mark("empty_key", err.fact() == Some("probe_key"), &show(&err)),
            Ok(_) => self.mark("empty_key", false, "batch"),
        }
        self.abort_ok(owner, "empty_key_abort").await;
    }

    async fn text_key(&mut self) {
        self.phase("text_key");
        let spec = self.line_spec("rev_src", 63);
        let Some(mut owner) = self.open_owner(&spec).await else { return };
        let result = owner.read_probe(CutProbe {
            schema: "rev".to_string(),
            table: "line item".to_string(),
            key: vec![ProbeValue::Text("x".to_string())],
        }).await;
        match result {
            Err(err) => self.mark("text_key", err.fact() == Some("probe_key"), &show(&err)),
            Ok(_) => self.mark("text_key", false, "batch"),
        }
        self.finish_not(owner, "text_key_finish").await;
    }

    async fn fail_read(&mut self) {
        self.phase("fail_read");
        let mut spec = self.line_spec("rev_line", 1);
        spec.policy = PgTypePolicy {
            unsupported: UnsupportedColumnPolicy::FailRead,
            reject: RejectCellPolicy::PreserveRow,
        };
        spec.secret_ref = "secret://a116/line".to_string();
        let Some(mut owner) = self.open_owner(&spec).await else { return };
        let digest = owner.binding().input_digest;
        let policy_ok = owner.binding().type_policy_identity == type_policy_identity(spec.policy);
        self.mark("fail_read_policy", policy_ok, "identity");
        self.read_expect(&mut owner, view("fail_read_line", "line item", 22, 1, digest, "rev_line"), FRESH_LINE_22).await;
        self.finish_ok(owner, "fail_read").await;
    }

    async fn final_quiet(&mut self) {
        self.phase("final_quiet");
        let before_stats = self.stats_set().await;
        let before_canary = self.canary().await;
        let spec = self.line_spec("rev_line", 2);
        let Some(mut owner) = self.open_owner(&spec).await else { return };
        let digest = owner.binding().input_digest;
        self.read_expect(&mut owner, view("final_line", "line item", 22, 2, digest, "rev_line"), FRESH_LINE_22).await;
        self.finish_ok(owner, "final").await;
        self.mark_delta("final_delta", &before_stats, &before_canary).await;
    }

    async fn audit(&mut self) {
        self.phase("audit");
        if let Err(err) = sqlx::query("SELECT 'A116_AUDIT_WINDOW_END'").execute(&mut self.admin).await {
            self.mark("audit_end", false, &driver_code(&err));
        }
        let started = Instant::now();
        let mut text = std::fs::read_to_string(&self.log_path).unwrap_or_default();
        while !text.contains("A116_AUDIT_WINDOW_END") && started.elapsed() <= Duration::from_secs(8) {
            tokio::time::sleep(Duration::from_millis(200)).await;
            text = std::fs::read_to_string(&self.log_path).unwrap_or_default();
        }
        let bytes = text.len();
        self.mark("audit_nonempty", bytes > 0, &format!("bytes={bytes}"));
        let mut password = text.contains("PRIVATE KEY");
        for secret in &self.secrets {
            if secret.len() >= 8 && text.contains(secret) {
                password = true;
            }
        }
        self.mark("password_in_log", !password, if password { "yes" } else { "no" });
        let report = classify_log(&text);
        self.mark("audit_window", report.window, "markers");
        self.mark("audit_marker_before_oo", report.marker_before, "order");
        self.lines.push(hist_line("audit_oo", &report.oo));
        self.lines.push(hist_line("audit_writer", &report.writer));
        self.lines.push(hist_line("audit_admin", &report.admin));
        self.lines.push(hist_line("audit_oracle", &report.oracle));
        self.lines.push(hist_line("audit_guc", &report.guc));
        let select = count_of(&report.oo, "SELECT");
        let start = count_of(&report.oo, "START");
        let rollback = count_of(&report.oo, "ROLLBACK");
        self.mark("audit_oo_activity", select > 0 && start > 0 && rollback > 0, &format!("select={select} start={start} rollback={rollback}"));
        let forbidden = report.oo_forbidden;
        self.mark("audit_oo_forbidden", forbidden == 0, &format!("count={forbidden}"));
        self.mark("audit_oo_unknown", report.oo_unknown.is_empty(), &report.oo_unknown);
        let guc_forbidden = report.guc_forbidden;
        self.mark("audit_guc_forbidden", guc_forbidden == 0, &format!("count={guc_forbidden}"));
        self.mark("audit_other", report.other == 0, &report.other_sample);
        self.mark("audit_limit", report.marks.limit, "LIMIT");
        self.mark("audit_line_item", report.marks.line, "quoted");
        self.mark("audit_odd_note", report.marks.odd, "quoted");
        let updates = count_of(&report.writer, "UPDATE");
        let inserts = count_of(&report.writer, "INSERT");
        self.mark("audit_writer", updates >= 2 && inserts >= 1, &format!("update={updates} insert={inserts}"));
        self.flush();
    }

    async fn inspect_reader(&mut self) {
        let spec = self.reader_spec("rev_src", 41);
        let Ok(config) = build_config(&spec) else {
            self.mark("inspect_config", false, "build");
            return;
        };
        match PgConnector::new(config).connect().await {
            Ok(mut session) => match session.inspect_access().await {
                Ok(report) => {
                    self.mark("inspect_registration", report.registration_allowed, "registration");
                    let tls_ok = report.tls.mode == "verify-full" && report.tls.hostname_verification;
                    self.mark("inspect_tls", tls_ok, &report.tls.mode);
                    self.mark("inspect_write", report.write.state == "read_only", &report.write.state);
                    let version_ok = report.version.server_version.contains("17.10") && report.version.admission == "tested";
                    self.mark("inspect_version", version_ok, &report.version.admission);
                    let caps_off = report.capabilities.iter().all(|item| !item.supported);
                    self.mark("inspect_capabilities", caps_off, "unsupported");
                }
                Err(err) => self.mark("inspect_access", false, &show(&err)),
            },
            Err(err) => self.mark("inspect_connect", false, &show(&err)),
        }
        self.settle("inspect_release").await;
    }

    async fn inspect_guc(&mut self) {
        let mut spec = self.reader_spec("rev_guc", 1);
        spec.user = self.guc_user.clone();
        spec.password = self.guc_password.clone();
        spec.principal = self.guc_user.clone();
        spec.secret_ref = "secret://a116/guc".to_string();
        let Ok(config) = build_config(&spec) else {
            self.mark("guc_inspect_config", false, "build");
            return;
        };
        match PgConnector::new(config).connect().await {
            Ok(mut session) => match session.inspect_access().await {
                Ok(report) => {
                    let startup = report.block_reasons.iter().any(|reason| reason.fact == "startup_options" || reason.code == "startup_option");
                    self.mark("guc_inspect", !report.registration_allowed && startup, "startup_options");
                }
                Err(err) => self.mark("guc_inspect", false, &show(&err)),
            },
            Err(err) => self.mark("guc_inspect", false, &show(&err)),
        }
        self.settle("guc_inspect_release").await;
    }

    fn check_binding(&mut self, name: &str, owner: &SnapshotOwner, generation: u64, source: &str) {
        let binding = owner.binding();
        self.mark(&format!("{name}_protocol"), binding.protocol == SNAPSHOT_PROTOCOL, binding.protocol);
        self.mark(&format!("{name}_generation"), binding.generation == generation, "generation");
        self.mark(&format!("{name}_source"), binding.source_id == source, "source");
        self.mark(&format!("{name}_database"), binding.database == self.cut.database, "database");
        self.mark(&format!("{name}_principal"), binding.principal == self.cut.principal, "principal");
        self.mark(&format!("{name}_recovery"), binding.in_recovery == self.cut.in_recovery, "recovery");
        self.mark(&format!("{name}_postmaster"), binding.postmaster_start == self.cut.postmaster, "postmaster");
        let backend_ok = binding.backend.pid > 0 && !binding.backend.backend_start.is_empty();
        self.mark(&format!("{name}_backend"), backend_ok, "backend");
        let recomputed = schema_hash(&binding.catalog.tables);
        let hash_ok = recomputed == binding.schema_hash && recomputed == binding.catalog.schema_hash;
        self.mark(&format!("{name}_schema_hash"), hash_ok, "schema_hash");
        let digest = snapshot_input_digest(&binding.config_identity, &binding.type_policy_identity, &binding.schema_hash, binding.generation);
        let digest_ok = matches!(digest, Ok(value) if value == binding.input_digest);
        self.mark(&format!("{name}_digest"), digest_ok, "digest");
        self.mark(&format!("{name}_type_conversion"), binding.catalog.type_conversion == "unsupported", &binding.catalog.type_conversion);
        let converted = binding.catalog.tables.iter().flat_map(|table| table.columns.iter()).all(|column| !column.type_support.converted);
        self.mark(&format!("{name}_converted"), converted, "converted");
        for table in &binding.access {
            let ok = table.select == Some(true) && table.insert == Some(false) && table.update == Some(false) && table.delete == Some(false) && table.truncate == Some(false);
            let label = table.name.replace(' ', "_");
            self.mark(&format!("{name}_access_{label}"), ok, "privileges");
        }
    }

    fn check_batch(&mut self, name: &str, batch: &TypedBatch, table: &str, generation: u64, digest: [u8; 32], source: &str) {
        self.mark(&format!("{name}_batch_protocol"), batch.protocol == TYPE_CONVERSION_PROTOCOL, batch.protocol);
        self.mark(&format!("{name}_batch_generation"), batch.run_generation == generation, "generation");
        self.mark(&format!("{name}_batch_source"), batch.source_id == source, "source");
        self.mark(&format!("{name}_batch_table"), batch.schema == "rev" && batch.table == table, "table");
        self.mark(&format!("{name}_batch_digest"), batch.input_digest == digest, "digest");
        self.mark(&format!("{name}_batch_rejects"), batch.rejects.is_empty(), "rejects");
        let names = batch.arrow.iter().map(|column| column.name.as_str()).collect::<Vec<_>>().join(",");
        self.mark(&format!("{name}_columns"), names == self.expect_names(table), &names);
    }

    fn expect_names(&self, table: &str) -> String {
        let mut columns: Vec<&Col> = self.cut.columns.iter().filter(|col| col.table == table).collect();
        columns.sort_by_key(|col| col.attnum);
        columns.iter().map(|col| col.name.as_str()).collect::<Vec<_>>().join(",")
    }

    async fn read_expect(&mut self, owner: &mut SnapshotOwner, view: ProbeView<'_>, want: &str) {
        let name = view.name;
        match probe_at(owner, view.table, view.key).await {
            Ok(batch) => {
                self.check_batch(name, &batch, view.table, view.generation, view.digest, view.source);
                match tokens_of(&batch) {
                    Ok(tokens) => self.mark(&format!("{name}_tokens"), tokens == want, "tokens"),
                    Err(reason) => self.mark(&format!("{name}_tokens"), false, reason),
                }
            }
            Err(err) => self.mark(name, false, &show(&err)),
        }
    }

    async fn read_absent(&mut self, owner: &mut SnapshotOwner, view: ProbeView<'_>) {
        let name = view.name;
        match probe_at(owner, view.table, view.key).await {
            Ok(batch) => {
                self.check_batch(name, &batch, view.table, view.generation, view.digest, view.source);
                let empty = batch.rows.is_empty() && batch.arrow.iter().all(|column| column.values.is_empty());
                self.mark(&format!("{name}_empty"), empty, "empty");
            }
            Err(err) => self.mark(name, false, &show(&err)),
        }
    }

    async fn compare(&mut self, owner: &mut SnapshotOwner, side: &mut PgConnection, view: ProbeView<'_>, want: &str) {
        let name = view.name;
        let owner_tokens = match probe_at(owner, view.table, view.key).await {
            Ok(batch) => {
                self.check_batch(name, &batch, view.table, view.generation, view.digest, view.source);
                tokens_of(&batch)
            }
            Err(err) => {
                self.mark(name, false, &show(&err));
                return;
            }
        };
        let side_got = side_tokens(side, view.table, i32::try_from(view.key).unwrap_or(i32::MAX)).await;
        match (owner_tokens, side_got) {
            (Ok(owner_text), Ok(Some(side_text))) => {
                let ok = owner_text == want && side_text == want;
                self.mark(&format!("{name}_tokens"), ok, "cut_and_side");
            }
            _ => self.mark(&format!("{name}_tokens"), false, "compare"),
        }
    }

    async fn open_owner(&mut self, spec: &OpenSpec) -> Option<SnapshotOwner> {
        let request = match build_request(spec) {
            Ok(request) => request,
            Err(err) => {
                let source = &spec.source;
                self.mark(&format!("build_{source}"), false, &show(&err));
                return None;
            }
        };
        match SnapshotOwner::open(request).await {
            Ok(owner) => Some(owner),
            Err(err) => {
                let source = &spec.source;
                let generation = spec.generation;
                self.mark(&format!("open_{source}_{generation}"), false, &show(&err));
                None
            }
        }
    }

    async fn must_fail(&mut self, name: &str, spec: &OpenSpec, fact: &str) {
        let request = match build_request(spec) {
            Ok(request) => request,
            Err(err) => {
                self.mark(name, false, &show(&err));
                return;
            }
        };
        match SnapshotOwner::open(request).await {
            Err(err) => self.mark(name, err.fact() == Some(fact), &show(&err)),
            Ok(owner) => {
                let key = owner.binding().backend.clone();
                drop(owner);
                self.mark(name, false, "opened");
                self.release(&key, &format!("{name}_unexpected_release")).await;
            }
        }
    }

    async fn fail_release(&mut self, name: &str, spec: &OpenSpec, fact: &str) {
        self.must_fail(name, spec, fact).await;
        self.settle(&format!("{name}_release")).await;
    }

    async fn finish_ok(&mut self, owner: SnapshotOwner, name: &str) {
        let key = owner.binding().backend.clone();
        let generation = owner.binding().generation;
        let source = owner.binding().source_id.clone();
        match owner.finish().await {
            Ok(done) => {
                let ok = done.generation == generation && done.source_id == source && done.backend == key;
                self.mark(&format!("{name}_finished"), ok, "SnapshotFinished");
            }
            Err(err) => self.mark(&format!("{name}_finished"), false, &show(&err)),
        }
        self.release(&key, &format!("{name}_release")).await;
    }

    async fn finish_not(&mut self, owner: SnapshotOwner, name: &str) {
        let key = owner.binding().backend.clone();
        match owner.finish().await {
            Ok(_) => self.mark(name, false, "SnapshotFinished"),
            Err(err) => self.mark(name, true, &show(&err)),
        }
        self.release(&key, &format!("{name}_release")).await;
    }

    async fn abort_ok(&mut self, owner: SnapshotOwner, name: &str) {
        let key = owner.binding().backend.clone();
        let generation = owner.binding().generation;
        match owner.abort().await {
            Ok(done) => {
                let ok = done.generation == generation && done.backend == key;
                self.mark(name, ok, "SnapshotAborted");
            }
            Err(err) => self.mark(name, false, &show(&err)),
        }
        self.release(&key, &format!("{name}_release")).await;
    }

    async fn run_writer(&mut self) -> bool {
        if !writer_registered(&self.cut.writer) {
            self.mark("writer_sql", false, "mismatch");
            return false;
        }
        let mut conn = match connect_to(&self.host, self.port, &self.cut.database, &self.writer_user, &self.writer_password, "oo-fixture-writer", &self.ca).await {
            Ok(conn) => conn,
            Err(code) => {
                self.mark("writer_connect", false, &code);
                return false;
            }
        };
        for sql in WRITER_SQL {
            if let Err(err) = sqlx::query(sql).execute(&mut conn).await {
                self.mark("writer_exec", false, &driver_code(&err));
                return false;
            }
        }
        self.mark("writer_exec", true, "3");
        true
    }

    async fn connect_reader(&self, app: &str) -> Result<PgConnection, String> {
        connect_to(&self.host, self.port, &self.cut.database, &self.reader_user, &self.reader_password, app, &self.ca).await
    }

    async fn expect_idle(&mut self, owner: &SnapshotOwner, name: &str) {
        let key = &owner.binding().backend;
        let Some(pid) = i32::try_from(key.pid).ok() else {
            self.mark(name, false, "pid");
            return;
        };
        let started = Instant::now();
        loop {
            match self.activity(pid, &key.backend_start).await {
                Ok(Some(act)) => {
                    let ok = act.state == "idle in transaction" && act.in_xact && act.app == "oo-pg-source" && act.user == self.cut.principal && act.db == self.cut.database;
                    if ok {
                        self.mark(name, true, &format!("pid={pid}"));
                        return;
                    }
                    if started.elapsed() >= Duration::from_secs(2) {
                        let state = &act.state;
                        self.mark(name, false, &format!("state={state}"));
                        return;
                    }
                }
                Ok(None) => {
                    self.mark(name, false, "missing");
                    return;
                }
                Err(code) => {
                    self.mark(name, false, &code);
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn release(&mut self, key: &BackendKey, name: &str) -> bool {
        let Some(pid) = i32::try_from(key.pid).ok() else {
            self.mark(name, false, "pid");
            return false;
        };
        let started = Instant::now();
        let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
        loop {
            match self.activity(pid, &key.backend_start).await {
                Ok(None) => {
                    let elapsed = started.elapsed();
                    let ok = elapsed <= bound;
                    let ms = elapsed.as_millis();
                    self.mark(name, ok, &format!("gone_ms={ms}"));
                    return ok;
                }
                Ok(Some(act)) => {
                    if started.elapsed() >= bound {
                        let state = &act.state;
                        self.mark(name, false, &format!("remaining state={state}"));
                        return false;
                    }
                }
                Err(code) => {
                    self.mark(name, false, &code);
                    return false;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn settle(&mut self, name: &str) -> bool {
        let started = Instant::now();
        let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
        loop {
            match self.oo_count().await {
                Ok((0, _)) => {
                    let elapsed = started.elapsed();
                    let ok = elapsed <= bound;
                    let ms = elapsed.as_millis();
                    self.mark(name, ok, &format!("gone_ms={ms}"));
                    return ok;
                }
                Ok((count, idle)) => {
                    if started.elapsed() >= bound {
                        self.mark(name, false, &format!("remaining={count} idle={idle}"));
                        return false;
                    }
                }
                Err(code) => {
                    self.mark(name, false, &code);
                    return false;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn activity(&mut self, pid: i32, start: &str) -> Result<Option<Act>, String> {
        let row = sqlx::query(ACT_SQL).bind(pid).bind(start).fetch_optional(&mut self.admin).await.map_err(|err| driver_code(&err))?;
        let Some(row) = row else { return Ok(None) };
        Ok(Some(Act {
            state: row.try_get(0).map_err(|_| "decode".to_string())?,
            in_xact: row.try_get(1).map_err(|_| "decode".to_string())?,
            app: row.try_get(2).map_err(|_| "decode".to_string())?,
            user: row.try_get(3).map_err(|_| "decode".to_string())?,
            db: row.try_get(4).map_err(|_| "decode".to_string())?,
        }))
    }

    async fn oo_count(&mut self) -> Result<(i64, i64), String> {
        let row = sqlx::query(OO_SQL).fetch_one(&mut self.admin).await.map_err(|err| driver_code(&err))?;
        let total: i64 = row.try_get(0).map_err(|_| "decode".to_string())?;
        let idle: i64 = row.try_get(1).map_err(|_| "decode".to_string())?;
        Ok((total, idle))
    }

    async fn match_count(&mut self, pid: i32, start: &str) -> Result<i64, String> {
        let user = self.cut.principal.clone();
        let db = self.cut.database.clone();
        let row = sqlx::query(MATCH_SQL).bind(pid).bind(start).bind(user).bind(db).fetch_one(&mut self.admin).await.map_err(|err| driver_code(&err))?;
        row.try_get(0).map_err(|_| "decode".to_string())
    }

    async fn terminate(&mut self, pid: i32, start: &str) -> Result<bool, String> {
        let user = self.cut.principal.clone();
        let db = self.cut.database.clone();
        let row = sqlx::query(KILL_SQL).bind(pid).bind(start).bind(user).bind(db).fetch_one(&mut self.admin).await.map_err(|err| driver_code(&err))?;
        row.try_get(0).map_err(|_| "decode".to_string())
    }

    async fn canary(&mut self) -> Result<i64, String> {
        let row = sqlx::query(CANARY_SQL).fetch_one(&mut self.admin).await.map_err(|err| driver_code(&err))?;
        row.try_get(0).map_err(|_| "decode".to_string())
    }

    async fn stats_of(&mut self, rel: &str) -> Result<Stat, String> {
        let row = sqlx::query(STATS_SQL).bind(rel).fetch_optional(&mut self.admin).await.map_err(|err| driver_code(&err))?;
        let Some(row) = row else {
            return Ok(Stat { ins: 0, upd: 0, del: 0 });
        };
        Ok(Stat {
            ins: row.try_get(0).map_err(|_| "decode".to_string())?,
            upd: row.try_get(1).map_err(|_| "decode".to_string())?,
            del: row.try_get(2).map_err(|_| "decode".to_string())?,
        })
    }

    async fn stats_set(&mut self) -> Result<(Stat, Stat, Stat), String> {
        Ok((self.stats_of("item").await?, self.stats_of("line item").await?, self.stats_of("canary").await?))
    }

    async fn mark_delta(&mut self, name: &str, before: &Result<(Stat, Stat, Stat), String>, before_canary: &Result<i64, String>) {
        let after = self.stats_set().await;
        let after_canary = self.canary().await;
        match (before, before_canary, after, after_canary) {
            (Ok(left), Ok(canary_before), Ok(right), Ok(canary_after)) => {
                let ok = same_stat(&left.0, &right.0) && same_stat(&left.1, &right.1) && same_stat(&left.2, &right.2) && canary_before == &canary_after;
                self.mark(name, ok, "zero");
            }
            _ => self.mark(name, false, "stats"),
        }
    }

    async fn admin_exec(&mut self, name: &str, sql: &'static str) -> bool {
        match sqlx::query(sql).execute(&mut self.admin).await {
            Ok(_) => {
                self.mark(name, true, "ok");
                true
            }
            Err(err) => {
                self.mark(name, false, &driver_code(&err));
                false
            }
        }
    }

    async fn twin_table(&mut self) -> Option<AdmittedTable> {
        let count = sqlx::query(TWIN_N_SQL).fetch_one(&mut self.admin).await;
        let pk = sqlx::query(TWIN_PK_SQL).fetch_one(&mut self.admin).await;
        let cols = sqlx::query(TWIN_COLS_SQL).fetch_all(&mut self.admin).await;
        let Ok(count_row) = count else {
            self.mark("twin_shape", false, "count");
            return None;
        };
        let Ok(count_value) = count_row.try_get::<i64, _>(0) else {
            self.mark("twin_shape", false, "count_decode");
            return None;
        };
        let Ok(pk_row) = pk else {
            self.mark("twin_shape", false, "pk");
            return None;
        };
        let Ok(pk_name) = pk_row.try_get::<String, _>(0) else {
            self.mark("twin_shape", false, "pk_decode");
            return None;
        };
        let Ok(rows) = cols else {
            self.mark("twin_shape", false, "columns");
            return None;
        };
        if count_value != 2 || pk_name != "id" || rows.len() != 2 {
            self.mark("twin_shape", false, "shape");
            return None;
        }
        let mut columns = Vec::new();
        for row in &rows {
            let attnum: i32 = row.try_get(0).unwrap_or(0);
            let name: String = row.try_get(1).unwrap_or_default();
            let type_oid: i64 = row.try_get(2).unwrap_or(0);
            let typmod: i32 = row.try_get(3).unwrap_or(0);
            let not_null: i32 = row.try_get(4).unwrap_or(0);
            columns.push(AdmittedColumn { attnum, name, type_oid, typmod, not_null: not_null == 1 });
        }
        let shape_ok = columns.len() == 2
            && columns[0].name == "id"
            && columns[0].type_oid == 23
            && columns[0].typmod == -1
            && columns[0].not_null
            && columns[1].name == "label"
            && columns[1].type_oid == 25
            && columns[1].typmod == -1
            && columns[1].not_null;
        if !shape_ok {
            self.mark("twin_shape", false, "columns");
            return None;
        }
        self.mark("twin_shape", true, "id_label");
        Some(AdmittedTable {
            schema: "rev".to_string(),
            name: "twin".to_string(),
            columns,
            primary_key: vec!["id".to_string()],
        })
    }
}

fn view<'a>(name: &'a str, table: &'a str, key: i64, generation: u64, digest: [u8; 32], source: &'a str) -> ProbeView<'a> {
    ProbeView { name, table, key, generation, digest, source }
}

fn writer_registered(writer: &[String]) -> bool {
    writer.len() == WRITER_SQL.len() && writer.iter().zip(WRITER_SQL).all(|(got, want)| got == want)
}

fn same_stat(left: &Stat, right: &Stat) -> bool {
    left.ins == right.ins && left.upd == right.upd && left.del == right.del
}

fn privilege_fact(err: &SourceError) -> bool {
    match err.fact() {
        Some("select_not_granted") => true,
        Some("insufficient_privilege") => err.sqlstate() == Some("42501"),
        _ => false,
    }
}

fn owner_hides(owner: &SnapshotOwner, password: &str) -> bool {
    let text = format!("{owner:?}");
    !password.is_empty() && !text.contains(password) && !text.contains("BEGIN CERTIFICATE")
}

fn debug_hides(config: &PgSourceConfig, password: &str) -> bool {
    let text = format!("{config:?}");
    !password.is_empty() && !text.contains(password) && !text.contains("BEGIN CERTIFICATE") && !text.contains("PRIVATE KEY") && text.contains("[redacted]")
}

fn show(err: &SourceError) -> String {
    let fact = err.fact().unwrap_or("-");
    let state = err.sqlstate().unwrap_or("-");
    let kind = err.kind();
    format!("kind={kind:?} fact={fact} sqlstate={state}")
}

fn driver_code(err: &sqlx::Error) -> String {
    match err.as_database_error().and_then(|db| db.code()) {
        Some(code) => {
            let code = code.as_ref();
            format!("sqlstate={code}")
        }
        None => "sqlstate=-".to_string(),
    }
}

async fn wait_past(deadline: Instant) {
    let rest = deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(250);
    tokio::time::sleep(rest).await;
}

async fn probe_at(owner: &mut SnapshotOwner, table: &str, key: i64) -> Result<TypedBatch, SourceError> {
    owner.read_probe(CutProbe {
        schema: "rev".to_string(),
        table: table.to_string(),
        key: vec![ProbeValue::Int8(key)],
    }).await
}

async fn side_tokens(conn: &mut PgConnection, table: &str, key: i32) -> Result<Option<String>, String> {
    let sql = if table == "item" { ITEM_SQL } else { LINE_SQL };
    let row = sqlx::query(sql).bind(key).fetch_optional(&mut *conn).await.map_err(|err| driver_code(&err))?;
    let Some(row) = row else { return Ok(None) };
    if table == "item" {
        let id: String = row.try_get(0).map_err(|_| "decode".to_string())?;
        let label: String = row.try_get(1).map_err(|_| "decode".to_string())?;
        let flag: String = row.try_get(2).map_err(|_| "decode".to_string())?;
        let code: String = row.try_get(3).map_err(|_| "decode".to_string())?;
        let note: String = row.try_get(4).map_err(|_| "decode".to_string())?;
        let note_token = if note == "<null>" { "null".to_string() } else { format!("text:{note}") };
        return Ok(Some(format!("int:{id} text:{label} bool:{flag} text:{code} {note_token}")));
    }
    let id: String = row.try_get(0).map_err(|_| "decode".to_string())?;
    let item_id: String = row.try_get(1).map_err(|_| "decode".to_string())?;
    let qty: String = row.try_get(2).map_err(|_| "decode".to_string())?;
    let memo: String = row.try_get(3).map_err(|_| "decode".to_string())?;
    let memo_token = if memo == "<null>" { "null".to_string() } else { format!("text:{memo}") };
    Ok(Some(format!("int:{id} int:{item_id} int:{qty} {memo_token}")))
}

async fn attr_count(conn: &mut PgConnection) -> Result<i64, String> {
    let row = sqlx::query(ATTR_SQL).fetch_one(&mut *conn).await.map_err(|err| driver_code(&err))?;
    row.try_get(0).map_err(|_| "decode".to_string())
}

async fn connect_to(host: &str, port: u16, database: &str, user: &str, password: &str, app: &str, ca: &[u8]) -> Result<PgConnection, String> {
    let options = PgConnectOptions::from_explicit()
        .host(host)
        .port(port)
        .username(user)
        .password(password)
        .database(database)
        .application_name(app)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    let options = options.disable_statement_logging();
    options.connect().await.map_err(|err| driver_code(&err))
}

fn build_request(spec: &OpenSpec) -> Result<SnapshotRequest, SourceError> {
    let config = build_config(spec)?;
    let mut identity = config_identity(&config);
    let mut policy_identity = type_policy_identity(spec.policy);
    if spec.tweaks.flip_config {
        identity[0] ^= 0xff;
    }
    if spec.tweaks.flip_policy {
        policy_identity[0] ^= 0xff;
    }
    let mut tables = spec.tables.clone();
    if spec.tweaks.shorten {
        tables.pop();
    }
    Ok(SnapshotRequest {
        config,
        admitted: AdmittedCatalog { tables },
        config_identity: identity,
        type_policy_identity: policy_identity,
        policy: spec.policy,
        generation: spec.generation,
        deadline: spec.deadline,
        expected_principal: spec.principal.clone(),
        expected_database: spec.database.clone(),
        expected_in_recovery: spec.recovery,
        expected_postmaster_start: spec.postmaster.clone(),
    })
}

fn build_config(spec: &OpenSpec) -> Result<PgSourceConfig, SourceError> {
    config_of(spec, &spec.password, &spec.ca, &spec.host)
}

fn config_of(spec: &OpenSpec, password: &str, ca: &[u8], host: &str) -> Result<PgSourceConfig, SourceError> {
    let mut selected = Vec::new();
    for table in &spec.tables {
        selected.push(SelectedTable::new(table.schema.as_str(), table.name.as_str())?);
    }
    let selection = TableSelection::new(selected)?;
    let endpoint = PgEndpoint::new(host, spec.port, &spec.endpoint_database)?;
    let credentials = PgCredentials::new(spec.user.as_str(), password)?;
    let secret = SecretRef::new(&spec.secret_ref)?;
    let tls = PgTlsConfig::from_ca_pem(ca)?;
    let budget = PgBudget::new(Duration::from_secs(5), Duration::from_secs(20), Duration::from_secs(20), 1_000, 1024 * 1024)?;
    let source = SourceId::new(&spec.source)?;
    PgSourceConfig::try_new(source, endpoint, credentials, secret, tls, budget, selection)
}

fn tokens_of(batch: &TypedBatch) -> Result<String, &'static str> {
    if batch.rows.len() != 1 {
        return Err("row_count");
    }
    let cells = &batch.rows[0].cells;
    if cells.len() != batch.arrow.len() {
        return Err("width");
    }
    let mut parts = Vec::new();
    for (cell, column) in cells.iter().zip(batch.arrow.iter()) {
        if column.values.len() != 1 {
            return Err("arrow_len");
        }
        let token = arrow_token(&column.values[0])?;
        if !presence_ok(cell, &token) {
            return Err("presence");
        }
        parts.push(token);
    }
    Ok(parts.join(" "))
}

fn arrow_token(value: &ArrowScalar) -> Result<String, &'static str> {
    match value {
        ArrowScalar::Bool(true) => Ok("bool:true".to_string()),
        ArrowScalar::Bool(false) => Ok("bool:false".to_string()),
        ArrowScalar::Int16(value) => Ok(format!("int:{value}")),
        ArrowScalar::Int32(value) => Ok(format!("int:{value}")),
        ArrowScalar::Int64(value) => Ok(format!("int:{value}")),
        ArrowScalar::Utf8(value) => Ok(format!("text:{value}")),
        ArrowScalar::Null => Ok("null".to_string()),
        _ => Err("arrow"),
    }
}

fn presence_ok(cell: &DecodedCell, token: &str) -> bool {
    if token == "null" {
        return cell.presence == CellPresence::Null && matches!(cell.arrow, Some(ArrowScalar::Null));
    }
    cell.presence == CellPresence::Value && cell.arrow.is_some()
}

fn row_tokens<'a>(rows: &'a [RowExpect], table: &str, key: i64) -> Option<&'a str> {
    rows.iter().find(|row| row.table == table && row.key == key).map(|row| row.tokens.as_str())
}

fn columns_match(cut: &Cut) -> bool {
    let item = [
        (1, "id", 23_i64, -1, true),
        (2, "label", 25, -1, true),
        (3, "flag", 16, -1, true),
        (4, "code", 1043, 12, true),
        (5, "odd\"note", 25, -1, false),
    ];
    let line = [
        (1, "id", 23_i64, -1, true),
        (2, "item_id", 23, -1, true),
        (3, "qty", 21, -1, true),
        (4, "memo", 25, -1, false),
    ];
    shape_is(cut, "item", &item) && shape_is(cut, "line item", &line) && pk_is(cut, "item", "id") && pk_is(cut, "line item", "id")
}

fn shape_is(cut: &Cut, table: &str, want: &[(i32, &str, i64, i32, bool)]) -> bool {
    let mut got: Vec<&Col> = cut.columns.iter().filter(|col| col.schema == "rev" && col.table == table).collect();
    got.sort_by_key(|col| col.attnum);
    if got.len() != want.len() {
        return false;
    }
    got.iter().zip(want.iter()).all(|(col, expected)| {
        col.attnum == expected.0 && col.name == expected.1 && col.type_oid == expected.2 && col.typmod == expected.3 && col.not_null == expected.4
    })
}

fn pk_is(cut: &Cut, table: &str, key: &str) -> bool {
    let keys: Vec<&str> = cut.pks.iter().filter(|item| item.0 == "rev" && item.1 == table).map(|item| item.2.as_str()).collect();
    keys == [key]
}

fn pointer_map(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(' ') else { continue };
        map.insert(key.to_string(), value.to_string());
    }
    map
}

fn req<'a>(map: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, &'static str> {
    map.get(key).map(String::as_str).ok_or("pointer")
}

fn read_secret(path: &str) -> Result<String, &'static str> {
    if !path.starts_with("/secrets/") || path.contains("..") {
        return Err("secret_path");
    }
    let text = std::fs::read_to_string(path).map_err(|_| "secret_read")?;
    let text = text.trim().to_string();
    if text.is_empty() { Err("secret_empty") } else { Ok(text) }
}

fn read_ca(path: &str) -> Result<Vec<u8>, &'static str> {
    if !path.starts_with("/secrets/") || path.contains("..") {
        return Err("ca_path");
    }
    let bytes = std::fs::read(path).map_err(|_| "ca_read")?;
    if bytes.is_empty() { Err("ca_empty") } else { Ok(bytes) }
}

fn parse_cut(text: &str) -> Result<Cut, &'static str> {
    let mut cut = Cut {
        postmaster: String::new(),
        database: String::new(),
        principal: String::new(),
        in_recovery: false,
        version: String::new(),
        columns: Vec::new(),
        pks: Vec::new(),
        rows: Vec::new(),
        fresh: Vec::new(),
        absent: Vec::new(),
        writer: Vec::new(),
        canary_before: -1,
        writer_canary_delta: -1,
        twin_count: -1,
        marker: String::new(),
    };
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        match parts.first().copied() {
            Some("META") if parts.len() >= 3 => match parts[1] {
                "postmaster" => cut.postmaster = parts[2..].join("\t"),
                "database" => cut.database = parts[2].to_string(),
                "principal" => cut.principal = parts[2].to_string(),
                "in_recovery" => cut.in_recovery = parts[2] == "true",
                "version" => cut.version = parts[2..].join("\t"),
                _ => {}
            },
            Some("COL") if parts.len() == 8 => cut.columns.push(Col {
                schema: parts[1].to_string(),
                table: parts[2].to_string(),
                attnum: parts[3].parse().map_err(|_| "attnum")?,
                name: parts[4].to_string(),
                type_oid: parts[5].parse().map_err(|_| "type_oid")?,
                typmod: parts[6].parse().map_err(|_| "typmod")?,
                not_null: parts[7] == "true",
            }),
            Some("PK") if parts.len() >= 4 => cut.pks.push((parts[1].to_string(), parts[2].to_string(), parts[3].to_string())),
            Some("ROW") => cut.rows.push(parse_row(&parts)?),
            Some("FRESH") => cut.fresh.push(parse_row(&parts)?),
            Some("ABSENT") if parts.len() == 3 => cut.absent.push((parts[1].to_string(), parts[2].parse().map_err(|_| "absent")?)),
            Some("WRITER") if parts.len() >= 3 => cut.writer.push(parts[2..].join("\t")),
            Some("EXPECT") if parts.len() == 3 => match parts[1] {
                "canary_before" => cut.canary_before = parts[2].parse().map_err(|_| "canary")?,
                "writer_canary_delta" => cut.writer_canary_delta = parts[2].parse().map_err(|_| "delta")?,
                "twin_count" => cut.twin_count = parts[2].parse().map_err(|_| "twin")?,
                _ => {}
            },
            Some("MARKER") if parts.len() == 2 => cut.marker = parts[1].to_string(),
            _ => return Err("cut_line"),
        }
    }
    if cut.postmaster.is_empty() || cut.columns.is_empty() || cut.writer.len() != 3 {
        return Err("cut_incomplete");
    }
    Ok(cut)
}

fn parse_row(parts: &[&str]) -> Result<RowExpect, &'static str> {
    if parts.len() < 4 {
        return Err("row");
    }
    Ok(RowExpect {
        table: parts[1].to_string(),
        key: parts[2].parse().map_err(|_| "row_key")?,
        tokens: parts[3..].join(" "),
    })
}

struct AuditMarks {
    limit: bool,
    line: bool,
    odd: bool,
}

struct Classified {
    oo: BTreeMap<String, usize>,
    writer: BTreeMap<String, usize>,
    admin: BTreeMap<String, usize>,
    oracle: BTreeMap<String, usize>,
    guc: BTreeMap<String, usize>,
    oo_forbidden: usize,
    oo_unknown: String,
    guc_forbidden: usize,
    other: usize,
    other_sample: String,
    marks: AuditMarks,
    window: bool,
    marker_before: bool,
}

fn classify_log(log: &str) -> Classified {
    let records = log_records(log);
    let start = records.iter().position(|rec| rec.sql.contains("A116_AUDIT_WINDOW_START"));
    let end = records.iter().position(|rec| rec.sql.contains("A116_AUDIT_WINDOW_END"));
    let mut out = Classified {
        oo: BTreeMap::new(),
        writer: BTreeMap::new(),
        admin: BTreeMap::new(),
        oracle: BTreeMap::new(),
        guc: BTreeMap::new(),
        oo_forbidden: 0,
        oo_unknown: String::new(),
        guc_forbidden: 0,
        other: 0,
        other_sample: String::new(),
        marks: AuditMarks { limit: false, line: false, odd: false },
        window: matches!((start, end), (Some(left), Some(right)) if left < right),
        marker_before: false,
    };
    let (Some(start), Some(end)) = (start, end) else {
        return out;
    };
    let mut saw_oo = false;
    let mut oo_after = true;
    for (index, rec) in records.iter().enumerate() {
        if class_of(&rec.app, &rec.user) == "oo" {
            saw_oo = true;
            oo_after &= index >= start;
        }
        if (start..=end).contains(&index) {
            tally(&mut out, rec);
        }
    }
    out.marker_before = saw_oo && oo_after;
    out
}

fn tally(out: &mut Classified, rec: &LogRec) {
    let verb = verb_of(&rec.sql);
    if verb.is_empty() {
        return;
    }
    match class_of(&rec.app, &rec.user) {
        "oo" => tally_oo(out, rec, &verb),
        "writer" => bump(&mut out.writer, &verb),
        "admin" => bump(&mut out.admin, &verb),
        "oracle" => bump(&mut out.oracle, &verb),
        "guc" => tally_guc(out, &verb),
        _ => note_other(out, rec),
    }
}

fn tally_oo(out: &mut Classified, rec: &LogRec, verb: &str) {
    bump(&mut out.oo, verb);
    if FORBIDDEN.contains(&verb) {
        out.oo_forbidden += 1;
    } else if !ALLOWED.contains(&verb) && out.oo_unknown.is_empty() {
        out.oo_unknown = verb.to_string();
    }
    if rec.sql.contains("LIMIT $") {
        out.marks.limit = true;
    }
    if rec.sql.contains("\"line item\"") {
        out.marks.line = true;
    }
    if rec.sql.contains("odd\"\"note") {
        out.marks.odd = true;
    }
}

fn tally_guc(out: &mut Classified, verb: &str) {
    bump(&mut out.guc, verb);
    if FORBIDDEN.contains(&verb) {
        out.guc_forbidden += 1;
    }
}

fn note_other(out: &mut Classified, rec: &LogRec) {
    out.other += 1;
    if out.other_sample.is_empty() {
        let app = &rec.app;
        let user = &rec.user;
        out.other_sample = format!("{app} {user}");
    }
}

fn bump(map: &mut BTreeMap<String, usize>, verb: &str) {
    *map.entry(verb.to_string()).or_insert(0) += 1;
}

fn class_of(app: &str, user: &str) -> &'static str {
    if user == "oo_rev_guc" {
        "guc"
    } else if user == "oo_rev_writer" || app == "oo-fixture-writer" {
        "writer"
    } else if app == "oo-a116-rr-oracle" {
        "oracle"
    } else if user == "postgres" || app == "oo-a116-admin" {
        "admin"
    } else if app == "oo-pg-source" && user == "oo_rev_reader" {
        "oo"
    } else {
        "other"
    }
}

struct LogRec {
    app: String,
    user: String,
    sql: String,
}

fn log_records(log: &str) -> Vec<LogRec> {
    let mut out = Vec::new();
    let mut header = String::new();
    let mut body = String::new();
    let mut open = false;
    for line in log.split_inclusive('\n') {
        if line_is_record(line) {
            if open {
                push_rec(&mut out, &header, &body);
            }
            header = line.to_string();
            body.clear();
            open = true;
        } else if open {
            body.push_str(line);
        }
    }
    if open {
        push_rec(&mut out, &header, &body);
    }
    out
}

fn line_is_record(line: &str) -> bool {
    line.starts_with(|ch: char| ch.is_ascii_digit()) && line.contains(" app=") && line.contains(" user=") && line.contains(" db=")
}

fn push_rec(out: &mut Vec<LogRec>, header: &str, body: &str) {
    let Some(sql) = sql_text(header, body) else { return };
    out.push(LogRec {
        app: header_field(header, "app=").to_string(),
        user: header_field(header, "user=").to_string(),
        sql,
    });
}

fn header_field<'a>(header: &'a str, key: &str) -> &'a str {
    let Some(pos) = header.find(key) else { return "" };
    let rest = &header[pos + key.len()..];
    let end = rest.find(' ').unwrap_or(rest.len());
    &rest[..end]
}

fn sql_text(header: &str, body: &str) -> Option<String> {
    let all = format!("{header}{body}");
    let marker = if let Some(pos) = all.find("statement: ") {
        pos + "statement: ".len()
    } else if let Some(pos) = all.find("execute ") {
        let rest = &all[pos..];
        let colon = rest.find(": ")?;
        pos + colon + 2
    } else {
        return None;
    };
    let text = all[marker..].split("\nDETAIL:").next().unwrap_or("").trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn verb_of(sql: &str) -> String {
    let sql = sql.split("\nDETAIL:").next().unwrap_or(sql);
    let token = sql.split_whitespace().next().unwrap_or("");
    let mut verb = String::new();
    for ch in token.chars() {
        if ch.is_ascii_alphabetic() {
            verb.push(ch.to_ascii_uppercase());
        } else {
            break;
        }
    }
    verb
}

fn hist_line(prefix: &str, map: &BTreeMap<String, usize>) -> String {
    let mut line = prefix.to_string();
    for (verb, count) in map {
        line.push(' ');
        line.push_str(verb);
        line.push('=');
        line.push_str(&count.to_string());
    }
    line
}

fn count_of(map: &BTreeMap<String, usize>, verb: &str) -> usize {
    map.get(verb).copied().unwrap_or(0)
}
