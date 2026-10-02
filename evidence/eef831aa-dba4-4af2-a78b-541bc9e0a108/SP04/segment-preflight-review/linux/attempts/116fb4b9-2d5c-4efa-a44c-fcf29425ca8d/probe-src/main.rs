#![forbid(unsafe_code)]
//! Independent PostgreSQL 17.10 oracle for commit 6cd955.
//! It executes the archive SQL. It is not the author's test and not a Loader.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const COMMIT: &str = "6cd955af38ce426bb1102a6e078e8580febed776";
const SHA_0000: &str = "37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208";
const SHA_0001: &str = "97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9";
const SHA_0002: &str = "59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512";
const SHA_0003: &str = "d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3";
const SHA_V1: &str = "429faa9d32e4b42960e805e0883a2f67c8783d936e9094f0747e9c5e21a23521";
const SHA_V2: &str = "3a6ca1f4db777f98e8b76f0bd6cc6b5039f2e82d72c478ef4c8fe37713254bac";
const LIE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const OWNER: &str = "11111111-1111-4111-8111-111111111111";
const ABSENT: &str = "target-fence-receipts/1 segment relation is absent";
const HISTORY: &str = "refusing target segment correction: preexisting apply receipts violate the closed segment rule; this migration does not delete, rewrite, or mutate receipts";
const MISMATCH: &str = "migration checksum mismatch";
const ADOPT: &str = "target segment preflight refuses to adopt commit_apply_receipt";
const FILE_V3: &str = "0003_target_segment_preflight_expand.sql";
const FILE_V2: &str = "0002_target_segment_null_expand.sql";

struct Gate {
    root: PathBuf,
    fails: u32,
}

struct Answer {
    rows: Vec<Vec<String>>,
    code: Option<String>,
    message: Option<String>,
    status: u8,
}

impl Answer {
    fn cells(&self) -> String {
        self.rows
            .iter()
            .map(|row| row.join("|"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

struct ApplyOut {
    route: &'static str,
    code: Option<String>,
    message: Option<String>,
    executed: &'static str,
}

struct Conn {
    stream: TcpStream,
}

struct Sql {
    boot: String,
    expand: String,
    segment: String,
    preflight: String,
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("harness: {err}");
            std::process::exit(2);
        }
    }
}

fn run() -> Result<i32, String> {
    let root = PathBuf::from("/evidence");
    fs::create_dir_all(root.join("cases")).map_err(|e| e.to_string())?;
    let mut gate = Gate {
        root: root.clone(),
        fails: 0,
    };
    let rustc = Command::new("rustc")
        .arg("--version")
        .output()
        .map_err(|e| e.to_string())?;
    let rustc_text = String::from_utf8(rustc.stdout).map_err(|e| e.to_string())?;
    fs::write(root.join("rustc-version.txt"), &rustc_text).map_err(|e| e.to_string())?;
    gate.check(
        "rustc-1.94",
        rustc.status.success() && rustc_text.starts_with("rustc 1.94.0"),
        rustc_text.trim(),
    );
    let host = fs::read_to_string("/fixture/host.txt").map_err(|e| e.to_string())?;
    let host = host.trim();
    if host.is_empty() || host.contains('/') || host.contains(' ') {
        return Err("fixture host is empty or unsafe".into());
    }
    let archive = PathBuf::from("/src");
    let sql = load_sql(&archive, &mut gate)?;
    scan_sources(&archive, &mut gate);
    let mut admin = Conn::connect(host, "postgres", "postgres")?;
    let version = admin.query("SELECT version();");
    gate.case("server-version", &version);
    let server = version.cells();
    fs::write(root.join("server-version.txt"), &server).map_err(|e| e.to_string())?;
    gate.check(
        "postgres-17.10",
        version.code.is_none() && server.contains("PostgreSQL 17.10"),
        server.trim(),
    );
    gate.note(&format!("commit={COMMIT}"));
    let exercised = exercise(host, &mut gate, &sql);
    match release_cluster(host) {
        Ok(()) => gate.note("cluster_release=dropped_a108_databases_and_roles"),
        Err(err) => {
            gate.fails += 1;
            gate.note(&format!("cluster_release_error={err}"));
        }
    }
    if let Err(err) = exercised {
        gate.fails += 1;
        gate.note(&format!("exercise_error={err}"));
    }
    gate.note(&format!("fails={}", gate.fails));
    let code = if gate.fails == 0 { 0 } else { 1 };
    fs::write(root.join("probe.exit"), format!("{code}\n")).map_err(|e| e.to_string())?;
    println!("probe_exit={code} fails={}", gate.fails);
    Ok(code)
}

fn load_sql(archive: &Path, gate: &mut Gate) -> Result<Sql, String> {
    let boot_path = archive.join("crates/oo-pipeline/migrations/0000_bootstrap_roles.sql");
    let expand_path = archive.join("crates/oo-pipeline/migrations/0001_target_fence_expand.sql");
    let segment_path = archive.join("crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql");
    let preflight_path = archive.join("crates/oo-pipeline/migrations/0003_target_segment_preflight_expand.sql");
    let v1_path = archive.join("contracts/pipeline/target-fence-receipts/target-fence-receipts-v1.json");
    let v2_path = archive.join("contracts/pipeline/target-fence-receipts/target-fence-receipts-v2.json");
    let boot = read_string(&boot_path)?;
    let expand = read_string(&expand_path)?;
    let segment = read_string(&segment_path)?;
    let preflight = read_string(&preflight_path)?;
    let v1 = read_string(&v1_path)?;
    let v2 = read_string(&v2_path)?;
    gate.check("hash-0000", sha256_hex(boot.as_bytes()) == SHA_0000, SHA_0000);
    gate.check("hash-0001", sha256_hex(expand.as_bytes()) == SHA_0001, SHA_0001);
    gate.check("hash-0002", sha256_hex(segment.as_bytes()) == SHA_0002, SHA_0002);
    gate.check("hash-0003", sha256_hex(preflight.as_bytes()) == SHA_0003, SHA_0003);
    gate.check("hash-v1-json", sha256_hex(v1.as_bytes()) == SHA_V1, SHA_V1);
    gate.check("hash-v2-json", sha256_hex(v2.as_bytes()) == SHA_V2, SHA_V2);
    if sha256_hex(preflight.as_bytes()) != SHA_0003 || sha256_hex(segment.as_bytes()) != SHA_0002 {
        return Err("archive SQL is not the bound 0002/0003 bytes".into());
    }
    let fn_start = "CREATE OR REPLACE FUNCTION _oo.commit_apply_receipt(";
    let fn_end = "COMMENT ON FUNCTION _oo.commit_apply_receipt(";
    let fn_seg = cut(&segment, fn_start, fn_end)?;
    let fn_pre = cut(&preflight, fn_start, fn_end)?;
    let hist_seg = cut(&segment, "DO $segment_history$", "$segment_history$;")?;
    let hist_pre = cut(&preflight, "DO $segment_history$", "$segment_history$;")?;
    let check_seg = cut(
        &segment,
        "ADD CONSTRAINT apply_receipts_segment_check CHECK (",
        "COMMENT ON CONSTRAINT apply_receipts_segment_check",
    )?;
    let check_pre = cut(
        &preflight,
        "ADD CONSTRAINT apply_receipts_segment_check CHECK (",
        "COMMENT ON CONSTRAINT apply_receipts_segment_check",
    )?;
    gate.check("slice-function", fn_seg == fn_pre, "commit_apply_receipt");
    gate.check("slice-history", hist_seg == hist_pre, "segment_history");
    gate.check("slice-check", check_seg == check_pre, "segment_check");
    gate.check(
        "slice-function-not-true",
        fn_pre.contains(") IS NOT TRUE THEN"),
        "function form",
    );
    gate.check(
        "slice-check-is-true",
        check_pre.contains(") IS TRUE"),
        "check form",
    );
    let segment_at = preflight.find("closed segment metadata").unwrap_or(usize::MAX);
    let fence_at = preflight.find("PERFORM _oo._load_held_generation").unwrap_or(0);
    gate.check(
        "segment-before-fence",
        segment_at < fence_at,
        "closed segment metadata precedes _load_held_generation",
    );
    gate.check(
        "no-regclass-cast",
        !preflight.contains("::regclass"),
        "0003 has no ::regclass token",
    );
    gate.check(
        "old-regclass-cast-remains",
        segment.contains("::pg_catalog.regclass"),
        "0002 still has the eager cast",
    );
    gate.check(
        "absent-message-count",
        preflight.matches(ABSENT).count() == 3,
        "three static OO002 messages",
    );
    gate.check(
        "no-receipt-delete",
        !preflight.contains("DELETE FROM _oo.apply_receipts"),
        "no delete",
    );
    gate.check(
        "no-receipt-update",
        !preflight.contains("UPDATE _oo.apply_receipts"),
        "no update",
    );
    gate.check(
        "no-owner-grant",
        !preflight.contains("GRANT oo_target_owner TO oo_pipeline_runtime"),
        "no runtime owner grant",
    );
    gate.check(
        "no-oo001-inside-file",
        !preflight.contains("OO001"),
        "checksum mismatch stays in the applicator",
    );
    gate.check(
        "ready-body-has-no-file-read",
        !cut(&preflight, "AS $preflight_ready$", "$preflight_ready$;")?.contains("pg_read_file"),
        "segment_preflight_ready body",
    );
    let (legal, illegal) = grid_counts();
    gate.check("static-grid-180", legal + illegal == 180, "180 cells");
    gate.check("static-grid-17", legal == 17, "17 legal");
    gate.check("static-grid-163", illegal == 163, "163 illegal");
    gate.note("static_grid_is_not_a_new_pg_180_run");
    Ok(Sql {
        boot,
        expand,
        segment,
        preflight,
    })
}

fn scan_sources(archive: &Path, gate: &mut Gate) {
    let roots = [
        archive.join("crates/oo-pipeline/src"),
        archive.join("crates/oo-pipeline/src/bin"),
    ];
    let mut seen = 0u32;
    let mut hits = 0u32;
    for src in roots {
        let Ok(entries) = fs::read_dir(&src) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            seen += 1;
            if let Ok(text) = fs::read_to_string(&path) {
                if text.contains("apply_reference") || text.contains("segment_preflight_correction") {
                    hits += 1;
                }
            }
        }
    }
    gate.check("production-src-present", seen >= 2, "lib.rs and bin");
    gate.check(
        "production-src-does-not-call-test-route",
        hits == 0,
        "apply_reference is test-only",
    );
}

fn grid_counts() -> (u32, u32) {
    let indexes = [None, Some(0), Some(-1), Some(1), Some(8191), Some(8192)];
    let counts = [None, Some(0), Some(1), Some(-1), Some(8192), Some(8193)];
    let bitmaps = [None, Some(0usize), Some(1), Some(8192), Some(8193)];
    let mut legal = 0u32;
    let mut illegal = 0u32;
    for index in indexes {
        for count in counts {
            for bitmap in bitmaps {
                if segment_legal(index, count, bitmap) {
                    legal += 1;
                } else {
                    illegal += 1;
                }
            }
        }
    }
    (legal, illegal)
}

fn segment_legal(index: Option<i32>, count: Option<i32>, bitmap_len: Option<usize>) -> bool {
    match (index, count, bitmap_len) {
        (None, None, Some(0)) => true,
        (Some(index), Some(count), Some(len))
            if count >= 1 && index >= 0 && index < count && (1..=8192).contains(&len) =>
        {
            true
        }
        _ => false,
    }
}

fn exercise(host: &str, gate: &mut Gate, sql: &Sql) -> Result<(), String> {
    let mut admin = Conn::connect(host, "postgres", "postgres")?;
    must(gate, &mut admin, "create-empty", "CREATE DATABASE oo_a108_empty;")?;
    drop(admin);
    phase_empty(host, gate, sql)?;
    let mut admin = Conn::connect(host, "postgres", "postgres")?;
    must(gate, &mut admin, "create-schema", "CREATE DATABASE oo_a108_schema;")?;
    drop(admin);
    phase_schema(host, gate, sql)?;
    let mut admin = Conn::connect(host, "postgres", "postgres")?;
    must(
        gate,
        &mut admin,
        "create-logins",
        "CREATE ROLE oo_a108_bootstrap LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION; CREATE ROLE oo_a108_runtime LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION; GRANT oo_target_owner TO oo_a108_bootstrap WITH INHERIT FALSE, SET TRUE; GRANT oo_pipeline_runtime TO oo_a108_runtime WITH INHERIT TRUE, SET FALSE; ALTER ROLE oo_a108_bootstrap SET search_path = pg_catalog; ALTER ROLE oo_a108_runtime SET search_path = pg_catalog;",
    )?;
    drop(admin);
    for db in [
        "oo_a108_rel",
        "oo_a108_con",
        "oo_a108_fresh",
        "oo_a108_law",
        "oo_a108_v2",
        "oo_a108_v2bad",
        "oo_a108_bad",
        "oo_a108_redo",
        "oo_a108_mis",
        "oo_a108_roll",
        "oo_a108_lock",
        "oo_a108_lie",
        "oo_a108_own",
    ] {
        let mut admin = Conn::connect(host, "postgres", "postgres")?;
        must(gate, &mut admin, &format!("create-{db}"), &format!("CREATE DATABASE {db};"))?;
        drop(admin);
        install_v1(host, gate, sql, db)?;
        let mut admin = Conn::connect(host, "postgres", "postgres")?;
        must(
            gate,
            &mut admin,
            &format!("grant-{db}"),
            &format!("GRANT CONNECT ON DATABASE {db} TO oo_a108_bootstrap; GRANT CONNECT ON DATABASE {db} TO oo_a108_runtime;"),
        )?;
    }
    phase_relation(host, gate, sql)?;
    phase_constraint(host, gate, sql)?;
    phase_fresh(host, gate, sql)?;
    phase_lawful(host, gate, sql)?;
    phase_v2(host, gate, sql)?;
    phase_v2_bad(host, gate, sql)?;
    phase_illegal(host, gate, sql)?;
    phase_redo(host, gate, sql)?;
    phase_mismatch(host, gate, sql)?;
    phase_rollback(host, gate, sql)?;
    phase_lock(host, gate, sql)?;
    phase_lie(host, gate, sql)?;
    phase_owner(host, gate, sql)?;
    Ok(())
}

fn release_cluster(host: &str) -> Result<(), String> {
    let mut admin = Conn::connect(host, "postgres", "postgres")?;
    let databases = [
        "oo_a108_empty",
        "oo_a108_schema",
        "oo_a108_rel",
        "oo_a108_con",
        "oo_a108_fresh",
        "oo_a108_law",
        "oo_a108_v2",
        "oo_a108_v2bad",
        "oo_a108_bad",
        "oo_a108_redo",
        "oo_a108_mis",
        "oo_a108_roll",
        "oo_a108_lock",
        "oo_a108_lie",
        "oo_a108_own",
    ];
    for db in databases {
        let terminated = admin.query(&format!(
            "SELECT pg_catalog.pg_terminate_backend(pid) FROM pg_catalog.pg_stat_activity WHERE datname = '{db}' AND pid <> pg_catalog.pg_backend_pid();"
        ));
        if terminated.code.is_some() {
            return Err(format!("terminate {db} {}", terminated.message.unwrap_or_default()));
        }
        let mut dropped = false;
        for _ in 0..20 {
            let answer = admin.query(&format!("DROP DATABASE IF EXISTS {db};"));
            if answer.code.is_none() {
                dropped = true;
                break;
            }
            let message = answer.message.unwrap_or_default();
            if !message.contains("being accessed by other users") {
                return Err(format!("drop database {db} {}", message));
            }
            thread::sleep(Duration::from_millis(200));
        }
        if !dropped {
            return Err(format!("drop database {db} still in use"));
        }
    }
    for role in [
        "oo_a108_bootstrap",
        "oo_a108_runtime",
        "oo_target_owner",
        "oo_pipeline_runtime",
    ] {
        let answer = admin.query(&format!("DROP ROLE IF EXISTS {role};"));
        if answer.code.is_some() {
            return Err(format!("drop role {role} {}", answer.message.unwrap_or_default()));
        }
    }
    Ok(())
}

fn phase_empty(host: &str, gate: &mut Gate, sql: &Sql) -> Result<(), String> {
    let out = apply(host, "oo_a108_empty", sql, "15s", false)?;
    record_apply(gate, "empty", &out);
    expect_route(gate, "empty", &out, "AbsentPreflight", Some("OO002"), Some(ABSENT), "0003");
    let mut conn = Conn::connect(host, "postgres", "oo_a108_empty")?;
    let absent = one(gate, &mut conn, "empty-schema", "SELECT (pg_catalog.to_regnamespace('_oo') IS NULL)::text;")?;
    gate.check("empty-no-schema", absent == "true", &absent);
    Ok(())
}

fn phase_schema(host: &str, gate: &mut Gate, sql: &Sql) -> Result<(), String> {
    let mut conn = Conn::connect(host, "postgres", "oo_a108_schema")?;
    must(gate, &mut conn, "schema-bootstrap", &format!("BEGIN; {} COMMIT;", sql.boot))?;
    let dead = conn.query(&format!(
        "BEGIN; SET ROLE oo_target_owner;\n{}\n{}\nCOMMIT;",
        sql.segment, sql.preflight
    ));
    gate.case("dead-path", &dead);
    if dead.status == b'E' {
        conn.rollback()?;
    }
    let dead_message = dead.message.clone().unwrap_or_default();
    gate.check("dead-path-42P01", dead.code.as_deref() == Some("42P01"), &dead_message);
    gate.check(
        "dead-path-not-gold-oo002",
        dead_message != ABSENT && dead_message.contains("relation \"_oo.apply_receipts\" does not exist"),
        &dead_message,
    );
    gate.note("dead_path=RED reference_does_not_use_0002_then_0003");
    drop(conn);
    let out = apply(host, "oo_a108_schema", sql, "15s", false)?;
    record_apply(gate, "schema", &out);
    expect_route(gate, "schema", &out, "AbsentPreflight", Some("OO002"), Some(ABSENT), "0003");
    let mut conn = Conn::connect(host, "postgres", "oo_a108_schema")?;
    let absent = one(gate, &mut conn, "schema-absent", "SELECT (pg_catalog.to_regnamespace('_oo') IS NULL)::text;")?;
    gate.check("schema-no-leak", absent == "true", &absent);
    Ok(())
}

fn phase_relation(host: &str, gate: &mut Gate, sql: &Sql) -> Result<(), String> {
    let mut conn = Conn::connect(host, "postgres", "oo_a108_rel")?;
    must(gate, &mut conn, "drop-relation", "DROP TABLE _oo.apply_receipts CASCADE;")?;
    drop(conn);
    let out = apply(host, "oo_a108_rel", sql, "15s", false)?;
    record_apply(gate, "missing-relation", &out);
    expect_route(gate, "missing-relation", &out, "AbsentPreflight", Some("OO002"), Some(ABSENT), "0003");
    let mut conn = Conn::connect(host, "postgres", "oo_a108_rel")?;
    let count = one(gate, &mut conn, "missing-relation-marker", "SELECT count(*)::text FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/3';")?;
    gate.check("missing-relation-no-marker", count == "0", &count);
    let ready = one(gate, &mut conn, "missing-relation-v1", &format!("SELECT _oo.schema_ready('{SHA_0001}')::text;"))?;
    gate.check("missing-relation-v1-ready", ready == "true", &ready);
    Ok(())
}

fn phase_constraint(host: &str, gate: &mut Gate, sql: &Sql) -> Result<(), String> {
    let mut conn = Conn::connect(host, "postgres", "oo_a108_con")?;
    must(
        gate,
        &mut conn,
        "drop-constraint",
        "ALTER TABLE _oo.apply_receipts DROP CONSTRAINT apply_receipts_segment_check;",
    )?;
    drop(conn);
    let out = apply(host, "oo_a108_con", sql, "15s", false)?;
    record_apply(gate, "absent-constraint", &out);
    expect_route(gate, "absent-constraint", &out, "AbsentPreflight", Some("OO002"), Some(ABSENT), "0003");
    let mut conn = Conn::connect(host, "postgres", "oo_a108_con")?;
    let count = one(gate, &mut conn, "absent-constraint-marker", "SELECT count(*)::text FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/3';")?;
    gate.check("absent-constraint-no-marker", count == "0", &count);
    let still = one(
        gate,
        &mut conn,
        "absent-constraint-still",
        "SELECT count(*)::text FROM pg_catalog.pg_constraint WHERE conname = 'apply_receipts_segment_check' AND conrelid = pg_catalog.to_regclass('_oo.apply_receipts');",
    )?;
    gate.check("absent-constraint-not-repaired-by-failed-file", still == "0", &still);
    Ok(())
}

fn phase_fresh(host: &str, gate: &mut Gate, sql: &Sql) -> Result<(), String> {
    let db = "oo_a108_fresh";
    let out = apply(host, db, sql, "15s", false)?;
    record_apply(gate, "fresh", &out);
    expect_route(gate, "fresh", &out, "SupersedeFromV1", None, None, "0003");
    let mut conn = Conn::connect(host, "postgres", db)?;
    let count = one(gate, &mut conn, "fresh-marker", "SELECT count(*)::text FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/3';")?;
    gate.check("fresh-one-marker", count == "1", &count);
    let flags = one(
        gate,
        &mut conn,
        "fresh-flags",
        &format!("SELECT _oo.schema_ready('{SHA_0001}')::text || '|' || _oo.schema_ready('{SHA_0003}')::text || '|' || _oo.segment_preflight_ready('{SHA_0003}')::text || '|' || _oo.segment_preflight_ready('{LIE}')::text || '|' || (SELECT schema_version_present::text FROM _oo.segment_preflight_readiness) || '|' || (pg_catalog.to_regprocedure('_oo.segment_contract_ready(text)') IS NULL)::text;"),
    )?;
    gate.check("fresh-flags", flags == "true|false|true|false|true|true", &flags);
    gate.check(
        "fresh-conjunction",
        flags.split('|').nth(2) == Some("true") && sha256_hex(sql.preflight.as_bytes()) == SHA_0003,
        "external file hash and segment_preflight_ready",
    );
    gate.check(
        "fresh-v1-does-not-authorize-v3",
        flags.starts_with("true|false|"),
        &flags,
    );
    let versions = one(gate, &mut conn, "fresh-versions", marker_sql())?;
    gate.check(
        "fresh-v3-checksum",
        versions.contains(&format!("target-fence-receipts/3|{SHA_0003}|")),
        &versions,
    );
    gate.check("fresh-no-v2-marker", !versions.contains("target-fence-receipts/2"), &versions);
    let def = one(gate, &mut conn, "fresh-check", check_sql())?;
    gate.check("fresh-check-is-true", def.contains("IS TRUE"), &def);
    let order = one(
        gate,
        &mut conn,
        "fresh-prosrc-order",
        "SELECT (strpos(prosrc, 'closed segment metadata') < strpos(prosrc, '_load_held_generation'))::text FROM pg_catalog.pg_proc JOIN pg_catalog.pg_namespace ON pg_namespace.oid = pronamespace WHERE nspname = '_oo' AND proname = 'commit_apply_receipt';",
    )?;
    gate.check("installed-segment-before-fence", order == "true", &order);
    let body = one(
        gate,
        &mut conn,
        "fresh-ready-body",
        "SELECT prosrc FROM pg_catalog.pg_proc JOIN pg_catalog.pg_namespace ON pg_namespace.oid = pronamespace WHERE nspname = '_oo' AND proname = 'segment_preflight_ready';",
    )?;
    gate.check(
        "installed-ready-does-not-read-file",
        !body.contains("pg_read_file") && !body.contains("pg_read_binary_file") && !body.contains("lo_import"),
        "prosrc",
    );
    drop(conn);
    phase_shapes(host, gate, db)?;
    phase_direct(host, gate, db)?;
    phase_stale(host, gate, db)?;
    phase_replay(host, gate, db)?;
    phase_search(host, gate, db)?;
    phase_acl(host, gate, db)?;
    phase_share_lock(host, gate, db)?;
    Ok(())
}

fn phase_shapes(host: &str, gate: &mut Gate, db: &str) -> Result<(), String> {
    let rows = [
        ("null-index", "NULL", "1", "'\\x01'::bytea", false),
        ("null-count", "0", "NULL", "'\\x01'::bytea", false),
        ("null-both-byte", "NULL", "NULL", "'\\x01'::bytea", false),
        ("absent-valid", "NULL", "NULL", "''::bytea", true),
        ("present-valid", "0", "1", "'\\x01'::bytea", true),
        ("bound-8192", "0", "1", "decode(repeat('aa', 8192), 'hex')", true),
        ("bound-8193", "0", "1", "decode(repeat('aa', 8193), 'hex')", false),
    ];
    for (name, index, count, bitmap, allow) in rows {
        let mut conn = Conn::connect(host, "oo_a108_runtime", db)?;
        let shard = format!("sh_{}", name.replace('-', "_"));
        let sql = format!(
            "{}; {}",
            adopt(&shard),
            receipt(&shard, 1, name, 'e', index, count, bitmap)
        );
        let answer = conn.query(&sql);
        gate.case(&format!("shape-{name}"), &answer);
        if allow {
            gate.check(
                &format!("shape-{name}-inserted"),
                answer.code.is_none() && answer.cells().contains("inserted"),
                &answer.cells(),
            );
        } else {
            let message = answer.message.clone().unwrap_or_default();
            gate.check(
                &format!("shape-{name}-23514"),
                answer.code.as_deref() == Some("23514") && message.contains("closed segment metadata") && !message.contains("OO013"),
                &message,
            );
        }
    }
    Ok(())
}

fn phase_direct(host: &str, gate: &mut Gate, db: &str) -> Result<(), String> {
    let mut conn = Conn::connect(host, "postgres", db)?;
    let answer = conn.query(
        "BEGIN; SET ROLE oo_target_owner; SELECT pg_catalog.set_config('_oo.writer', 'apply-receipt', true); INSERT INTO _oo.apply_receipts (source, table_generation, txn_or_batch_id, config_version, config_digest, type_policy_version, type_digest, hook_digest, schema_digest, payload_digest, protocol_version, position_codec, position_lineage, source_epoch, applied_pos, partial_txn, cross_table_consistent, txn_complete, segment_index, segment_count, segment_bitmap, fence_shard, fence_generation) VALUES ('warehouse', 'orders_g1', 'direct-unknown', 'config/v1', repeat('a', 64), 'type-policy/v1', repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'batch/v1', 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, NULL, 1, '\\x01'::bytea, 'owner_direct', 1); COMMIT;",
    );
    gate.case("direct-unknown", &answer);
    if answer.status == b'E' {
        conn.rollback()?;
    }
    let message = answer.message.clone().unwrap_or_default();
    gate.check(
        "direct-unknown-23514",
        answer.code.as_deref() == Some("23514") && message.contains("apply_receipts_segment_check"),
        &message,
    );
    let left = one(gate, &mut conn, "direct-unknown-absent", "SELECT count(*)::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'direct-unknown';")?;
    gate.check("direct-unknown-rolled-back", left == "0", &left);
    Ok(())
}

fn phase_stale(host: &str, gate: &mut Gate, db: &str) -> Result<(), String> {
    let mut conn = Conn::connect(host, "oo_a108_runtime", db)?;
    let sql = format!(
        "{adopt}\nSELECT _oo.fail_target_generation('shard_stale', '{OWNER}'::uuid, 1);\n{adopt}\nDO $stale$ BEGIN BEGIN PERFORM _oo.commit_apply_receipt('shard_stale', '{OWNER}'::uuid, 1, 'warehouse', 'orders_g1', 'stale-bad', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, NULL, 1, '\\x01'::bytea); RAISE EXCEPTION 'illegal stale segment was stored'; EXCEPTION WHEN SQLSTATE '23514' THEN NULL; WHEN SQLSTATE 'OO013' THEN RAISE EXCEPTION 'OO013 hid the illegal segment'; END; BEGIN PERFORM _oo.commit_apply_receipt('shard_stale', '{OWNER}'::uuid, 1, 'warehouse', 'orders_g1', 'stale-legal', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('f', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, NULL, NULL, ''::bytea); RAISE EXCEPTION 'stale legal receipt was stored'; EXCEPTION WHEN SQLSTATE 'OO013' THEN NULL; END; END $stale$;",
        adopt = adopt("shard_stale")
    );
    let answer = conn.query(&sql);
    gate.case("stale", &answer);
    gate.check("stale-block", answer.code.is_none(), &answer.message.clone().unwrap_or_default());
    drop(conn);
    let mut conn = Conn::connect(host, "postgres", db)?;
    let counts = one(
        gate,
        &mut conn,
        "stale-counts",
        "SELECT count(*)::text || '|' || (SELECT generation::text FROM _oo.target_generations WHERE shard_id = 'shard_stale') FROM _oo.apply_receipts WHERE txn_or_batch_id IN ('stale-bad', 'stale-legal');",
    )?;
    gate.check("stale-23514-before-oo013", counts == "0|2", &counts);
    Ok(())
}

fn phase_replay(host: &str, gate: &mut Gate, db: &str) -> Result<(), String> {
    let mut admin = Conn::connect(host, "postgres", db)?;
    must(
        gate,
        &mut admin,
        "fixture-schema",
        "SET ROLE oo_target_owner; CREATE SCHEMA fixture_synth AUTHORIZATION oo_target_owner; REVOKE ALL ON SCHEMA fixture_synth FROM PUBLIC; GRANT USAGE ON SCHEMA fixture_synth TO oo_pipeline_runtime; CREATE TABLE fixture_synth.applied_row (batch_id text PRIMARY KEY, payload_digest text NOT NULL, applied_pos numeric(38, 0) NOT NULL); REVOKE ALL ON fixture_synth.applied_row FROM PUBLIC; GRANT SELECT, INSERT ON fixture_synth.applied_row TO oo_pipeline_runtime; RESET ROLE;",
    )?;
    drop(admin);
    let mut runtime = Conn::connect(host, "oo_a108_runtime", db)?;
    let setup = format!(
        "{}; {}",
        adopt("shard_replay"),
        receipt("shard_replay", 1, "batch-replay", 'e', "NULL", "NULL", "''::bytea")
    );
    let inserted = runtime.query(&setup);
    gate.case("replay-insert", &inserted);
    gate.check("replay-inserted", inserted.cells().contains("inserted"), &inserted.cells());
    let mut admin = Conn::connect(host, "postgres", db)?;
    let stamp = one(
        gate,
        &mut admin,
        "replay-stamp",
        "SELECT payload_digest || '~' || fence_generation::text || '~' || target_commit_at::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'batch-replay';",
    )?;
    drop(admin);
    // The receipt function holds a session advisory lock. Replay and conflict stay on this backend.
    let again = runtime.query(&receipt("shard_replay", 1, "batch-replay", 'e', "NULL", "NULL", "''::bytea"));
    gate.case("replay-again", &again);
    gate.check("replay-same", again.cells().contains("replay"), &again.cells());
    let conflict_sql = format!(
        "BEGIN; INSERT INTO fixture_synth.applied_row VALUES ('conflict-side', repeat('f', 64), 1); {} COMMIT;",
        receipt("shard_replay", 1, "batch-replay", 'f', "NULL", "NULL", "''::bytea")
    );
    let conflict = runtime.query(&conflict_sql);
    gate.case("conflict", &conflict);
    if conflict.status == b'E' {
        runtime.rollback()?;
    }
    gate.check("conflict-oo010", conflict.code.as_deref() == Some("OO010"), &conflict.message.clone().unwrap_or_default());
    drop(runtime);
    let mut admin = Conn::connect(host, "postgres", db)?;
    let stamp2 = one(
        gate,
        &mut admin,
        "replay-stamp-2",
        "SELECT payload_digest || '~' || fence_generation::text || '~' || target_commit_at::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'batch-replay';",
    )?;
    gate.check("replay-stamp-gen-payload", stamp == stamp2 && !stamp.is_empty(), &format!("{stamp} => {stamp2}"));
    let side = one(gate, &mut admin, "conflict-side", "SELECT count(*)::text FROM fixture_synth.applied_row WHERE batch_id = 'conflict-side';")?;
    gate.check("conflict-side-rollback", side == "0", &side);
    Ok(())
}

fn phase_search(host: &str, gate: &mut Gate, db: &str) -> Result<(), String> {
    let mut admin = Conn::connect(host, "postgres", db)?;
    must(
        gate,
        &mut admin,
        "evil-schema",
        "CREATE SCHEMA a108_evil; CREATE FUNCTION a108_evil.clock_timestamp() RETURNS timestamptz LANGUAGE sql AS $$ SELECT timestamptz '2000-01-01 00:00:00+00' $$; CREATE FUNCTION a108_evil.octet_length(bytea) RETURNS integer LANGUAGE sql AS $$ SELECT 0 $$; GRANT USAGE ON SCHEMA a108_evil TO oo_pipeline_runtime;",
    )?;
    drop(admin);
    let mut runtime = Conn::connect(host, "oo_a108_runtime", db)?;
    let sql = format!(
        "SET search_path = a108_evil, pg_catalog; {}; {}",
        adopt("shard_path"),
        receipt("shard_path", 1, "path-one", 'e', "0", "1", "'\\x01'::bytea")
    );
    let answer = runtime.query(&sql);
    gate.case("search-path", &answer);
    gate.check(
        "search-path-inserted",
        answer.code.is_none() && answer.cells().contains("inserted"),
        &answer.message.clone().unwrap_or_else(|| answer.cells()),
    );
    drop(runtime);
    let mut admin = Conn::connect(host, "postgres", db)?;
    let year = one(
        gate,
        &mut admin,
        "search-path-year",
        "SELECT extract(year FROM target_commit_at)::integer::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'path-one';",
    )?;
    gate.check("search-path-year-2026", year == "2026", &year);
    let ready = one(
        gate,
        &mut admin,
        "preflight-proconfig",
        "SELECT prosecdef::text || '|' || proconfig::text FROM pg_catalog.pg_proc JOIN pg_catalog.pg_namespace ON pg_namespace.oid = pronamespace WHERE nspname = '_oo' AND proname = 'segment_preflight_ready';",
    )?;
    gate.check("preflight-invoker-path", ready == "false|{\"search_path=pg_catalog, _oo\"}", &ready);
    Ok(())
}

fn phase_acl(host: &str, gate: &mut Gate, db: &str) -> Result<(), String> {
    let mut runtime = Conn::connect(host, "oo_a108_runtime", db)?;
    let denied = runtime.query("INSERT INTO _oo.apply_receipts (source) VALUES ('warehouse');");
    gate.case("runtime-insert", &denied);
    gate.check("runtime-insert-42501", denied.code.as_deref() == Some("42501"), &denied.message.clone().unwrap_or_default());
    drop(runtime);
    let mut admin = Conn::connect(host, "postgres", db)?;
    let acl = one(
        gate,
        &mut admin,
        "acl",
        "SELECT pg_catalog.pg_has_role('oo_pipeline_runtime', 'oo_target_owner', 'member')::text || '|' || pg_catalog.has_function_privilege('oo_pipeline_runtime', receipt.oid, 'execute')::text || '|' || pg_catalog.has_function_privilege('oo_pipeline_runtime', ready.oid, 'execute')::text || '|' || pg_catalog.has_table_privilege('oo_pipeline_runtime', ready_view.oid, 'select')::text || '|' || pg_catalog.has_table_privilege('oo_pipeline_runtime', receipt_table.oid, 'insert')::text || '|' || pg_catalog.pg_get_userbyid(receipt.proowner) || '|' || receipt.prosecdef::text || '|' || receipt.proconfig::text FROM pg_catalog.pg_proc AS receipt JOIN pg_catalog.pg_namespace AS nsp ON nsp.oid = receipt.pronamespace JOIN pg_catalog.pg_proc AS ready ON ready.pronamespace = nsp.oid AND ready.proname = 'segment_preflight_ready' JOIN pg_catalog.pg_class AS ready_view ON ready_view.relnamespace = nsp.oid AND ready_view.relname = 'segment_preflight_readiness' JOIN pg_catalog.pg_class AS receipt_table ON receipt_table.relnamespace = nsp.oid AND receipt_table.relname = 'apply_receipts' WHERE nsp.nspname = '_oo' AND receipt.proname = 'commit_apply_receipt';",
    )?;
    gate.check(
        "acl-no-owner-membership",
        acl == "false|true|true|true|false|oo_target_owner|true|{\"search_path=pg_catalog, _oo\"}",
        &acl,
    );
    let view = one(
        gate,
        &mut admin,
        "readiness-view",
        "SELECT coalesce(reloptions::text, '') FROM pg_catalog.pg_class JOIN pg_catalog.pg_namespace ON pg_namespace.oid = relnamespace WHERE nspname = '_oo' AND relname = 'segment_preflight_readiness';",
    )?;
    gate.check("readiness-security-invoker", view.contains("security_invoker=true"), &view);
    Ok(())
}

fn phase_share_lock(host: &str, gate: &mut Gate, db: &str) -> Result<(), String> {
    let mut holder = Conn::connect(host, "postgres", db)?;
    let held = holder.query(
        "BEGIN; SET ROLE oo_target_owner; DO $touch$ BEGIN PERFORM 1 FROM _oo.apply_receipts WHERE false; EXCEPTION WHEN undefined_table THEN RAISE EXCEPTION 'touch saw undefined_table'; END $touch$; LOCK TABLE _oo.apply_receipts IN SHARE ROW EXCLUSIVE MODE; SELECT 'held';",
    );
    gate.case("lock-held", &held);
    gate.check("share-lock-held", held.code.is_none() && held.status == b'T' && held.cells().contains("held"), &held.cells());
    let started = Instant::now();
    let mut waiter = Conn::connect(host, "postgres", db)?;
    let waited = waiter.query(
        "BEGIN; SET LOCAL lock_timeout = '2s'; SET ROLE oo_target_owner; LOCK TABLE _oo.apply_receipts IN SHARE ROW EXCLUSIVE MODE; COMMIT;",
    );
    let elapsed = started.elapsed();
    gate.case("lock-waiter", &waited);
    if waited.status == b'E' {
        waiter.rollback()?;
    }
    holder.rollback()?;
    gate.check("share-lock-55P03", waited.code.as_deref() == Some("55P03"), &waited.message.clone().unwrap_or_default());
    gate.check("share-lock-bounded", elapsed < Duration::from_secs(8), &format!("{}", elapsed.as_millis()));
    gate.note(&format!("share_lock_elapsed_ms={}", elapsed.as_millis()));
    Ok(())
}

fn phase_lawful(host: &str, gate: &mut Gate, sql: &Sql) -> Result<(), String> {
    let db = "oo_a108_law";
    seed_lawful(host, gate, db, "shard_law")?;
    let mut admin = Conn::connect(host, "postgres", db)?;
    let before = one(gate, &mut admin, "law-before", snapshot_sql())?;
    let marks = one(gate, &mut admin, "law-marks-before", marker_sql())?;
    drop(admin);
    let out = apply(host, db, sql, "15s", false)?;
    record_apply(gate, "law", &out);
    expect_route(gate, "law", &out, "SupersedeFromV1", None, None, "0003");
    let mut admin = Conn::connect(host, "postgres", db)?;
    let after = one(gate, &mut admin, "law-after", snapshot_sql())?;
    let marks_after = one(gate, &mut admin, "law-marks-after", marker_sql())?;
    gate.check("law-receipts-immutable", before == after && !before.is_empty(), "snapshot");
    gate.check("law-v1-stamp-kept", v1_line(&marks) == v1_line(&marks_after) && v1_line(&marks).contains(SHA_0001), &marks_after);
    gate.check("law-no-v2-marker", !marks_after.contains("target-fence-receipts/2"), &marks_after);
    let ready = one(gate, &mut admin, "law-ready", &format!("SELECT _oo.segment_preflight_ready('{SHA_0003}')::text;"))?;
    gate.check("law-ready-file-hash", ready == "true", &ready);
    Ok(())
}

fn phase_v2(host: &str, gate: &mut Gate, sql: &Sql) -> Result<(), String> {
    let db = "oo_a108_v2";
    seed_lawful(host, gate, db, "shard_v2")?;
    let mut admin = Conn::connect(host, "postgres", db)?;
    let before = one(gate, &mut admin, "v2-receipts-before", snapshot_sql())?;
    drop(admin);
    install_historical_v2(host, gate, sql, db)?;
    let mut admin = Conn::connect(host, "postgres", db)?;
    let after_v2 = one(gate, &mut admin, "v2-receipts-after-0002", snapshot_sql())?;
    gate.check("v2-historical-receipts", before == after_v2, "0002 did not rewrite receipts");
    let marks = one(gate, &mut admin, "v2-marks-before", marker_sql())?;
    let fn_before = one(
        gate,
        &mut admin,
        "v2-fn-before",
        "SELECT pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure('_oo.segment_contract_ready(text)'));",
    )?;
    drop(admin);
    let out = apply(host, db, sql, "15s", false)?;
    record_apply(gate, "v2-upgrade", &out);
    expect_route(gate, "v2-upgrade", &out, "UpgradeFromV2", None, None, "0003");
    let mut admin = Conn::connect(host, "postgres", db)?;
    let after = one(gate, &mut admin, "v2-receipts-after", snapshot_sql())?;
    let marks_after = one(gate, &mut admin, "v2-marks-after", marker_sql())?;
    let fn_after = one(
        gate,
        &mut admin,
        "v2-fn-after",
        "SELECT pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure('_oo.segment_contract_ready(text)'));",
    )?;
    gate.check("v2-receipts-immutable", before == after, "receipts");
    gate.check("v2-function-kept", fn_before == fn_after && !fn_before.is_empty(), "segment_contract_ready");
    gate.check("v2-old-lines-kept", lines_kept(&marks, &marks_after), &marks_after);
    gate.check(
        "v2-new-marker",
        marks_after.contains(&format!("target-fence-receipts/3|{SHA_0003}|")),
        &marks_after,
    );
    let ready = one(
        gate,
        &mut admin,
        "v2-ready",
        &format!("SELECT _oo.segment_contract_ready('{SHA_0002}')::text || '|' || _oo.segment_preflight_ready('{SHA_0003}')::text || '|' || _oo.schema_ready('{SHA_0001}')::text || '|' || _oo.segment_preflight_ready('{SHA_0002}')::text;"),
    )?;
    gate.check("v2-ready-conjunction", ready == "true|true|true|false", &ready);
    gate.check(
        "v2-marker-does-not-authorize-v3",
        ready.ends_with("|false"),
        "segment_preflight_ready of the v2 file hash is false",
    );
    let owners = one(
        gate,
        &mut admin,
        "v2-owners",
        "SELECT coalesce(string_agg(proname || '=' || pg_catalog.pg_get_userbyid(proowner), ',' ORDER BY proname), '') FROM pg_catalog.pg_proc JOIN pg_catalog.pg_namespace ON pg_namespace.oid = pronamespace WHERE nspname = '_oo' AND proname IN ('commit_apply_receipt', 'segment_contract_ready', 'segment_preflight_ready');",
    )?;
    gate.check(
        "v2-owners",
        owners == "commit_apply_receipt=oo_target_owner,segment_contract_ready=oo_target_owner,segment_preflight_ready=oo_target_owner",
        &owners,
    );
    Ok(())
}

fn phase_v2_bad(host: &str, gate: &mut Gate, sql: &Sql) -> Result<(), String> {
    let db = "oo_a108_v2bad";
    let mut conn = Conn::connect(host, "postgres", db)?;
    must(
        gate,
        &mut conn,
        "book-lie-v2",
        &format!("INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES ('target-fence-receipts/2', '{LIE}', 'expand', '{FILE_V2}');"),
    )?;
    let before = one(gate, &mut conn, "v2bad-check-before", check_sql())?;
    drop(conn);
    let out = apply(host, db, sql, "15s", false)?;
    record_apply(gate, "v2-mismatch", &out);
    expect_route(gate, "v2-mismatch", &out, "MismatchV2", Some("OO001"), Some(MISMATCH), "");
    let mut conn = Conn::connect(host, "postgres", db)?;
    let checksum = one(gate, &mut conn, "v2bad-checksum", "SELECT checksum FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/2';")?;
    let after = one(gate, &mut conn, "v2bad-check-after", check_sql())?;
    gate.check("v2-lie-unchanged", checksum == LIE, &checksum);
    gate.check("v2-lie-file-not-executed", before == after && !after.contains("IS TRUE"), &after);
    Ok(())
}

fn phase_illegal(host: &str, gate: &mut Gate, sql: &Sql) -> Result<(), String> {
    let db = "oo_a108_bad";
    let mut conn = Conn::connect(host, "postgres", db)?;
    must(
        gate,
        &mut conn,
        "seed-illegal",
        "BEGIN; SET ROLE oo_target_owner; SELECT pg_catalog.set_config('_oo.writer', 'apply-receipt', true); INSERT INTO _oo.apply_receipts (source, table_generation, txn_or_batch_id, config_version, config_digest, type_policy_version, type_digest, hook_digest, schema_digest, payload_digest, protocol_version, position_codec, position_lineage, source_epoch, applied_pos, partial_txn, cross_table_consistent, txn_complete, segment_index, segment_count, segment_bitmap, fence_shard, fence_generation) VALUES ('warehouse', 'orders_g1', 'illegal-null', 'config/v1', repeat('a', 64), 'type-policy/v1', repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'batch/v1', 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, NULL, 1, '\\x01'::bytea, 'bad_shard', 1); COMMIT;",
    )?;
    let before = one(gate, &mut conn, "bad-before", snapshot_sql())?;
    let before_def = one(gate, &mut conn, "bad-check-before", check_sql())?;
    drop(conn);
    let out = apply(host, db, sql, "15s", false)?;
    record_apply(gate, "bad", &out);
    expect_route(gate, "bad", &out, "SupersedeFromV1", Some("OO021"), Some(HISTORY), "0003");
    let mut conn = Conn::connect(host, "postgres", db)?;
    let after = one(gate, &mut conn, "bad-after", snapshot_sql())?;
    let after_def = one(gate, &mut conn, "bad-check-after", check_sql())?;
    let count = one(gate, &mut conn, "bad-marker", "SELECT count(*)::text FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/3';")?;
    gate.check("illegal-receipts-unchanged", before == after && before.contains("illegal-null"), &format!("{}", before.len()));
    gate.check("illegal-check-unchanged", before_def == after_def && !after_def.contains("IS TRUE"), &after_def);
    gate.check("illegal-no-marker", count == "0", &count);
    Ok(())
}

fn phase_redo(host: &str, gate: &mut Gate, sql: &Sql) -> Result<(), String> {
    let db = "oo_a108_redo";
    let first = apply(host, db, sql, "15s", false)?;
    record_apply(gate, "redo-first", &first);
    expect_route(gate, "redo-first", &first, "SupersedeFromV1", None, None, "0003");
    let mut conn = Conn::connect(host, "postgres", db)?;
    let stamp = one(gate, &mut conn, "redo-stamp", marker_sql())?;
    let oid = one(gate, &mut conn, "redo-oid", "SELECT constraint_row.oid::text FROM pg_catalog.pg_constraint AS constraint_row WHERE constraint_row.conname = 'apply_receipts_segment_check' AND constraint_row.conrelid = pg_catalog.to_regclass('_oo.apply_receipts');")?;
    drop(conn);
    let second = apply(host, db, sql, "15s", false)?;
    record_apply(gate, "redo-second", &second);
    expect_route(gate, "redo-second", &second, "Skip", None, None, "");
    let mut conn = Conn::connect(host, "postgres", db)?;
    let stamp2 = one(gate, &mut conn, "redo-stamp-2", marker_sql())?;
    let oid2 = one(gate, &mut conn, "redo-oid-2", "SELECT constraint_row.oid::text FROM pg_catalog.pg_constraint AS constraint_row WHERE constraint_row.conname = 'apply_receipts_segment_check' AND constraint_row.conrelid = pg_catalog.to_regclass('_oo.apply_receipts');")?;
    gate.check("redo-stamp-stable", stamp == stamp2 && stamp.contains(SHA_0003), &stamp2);
    gate.check("redo-constraint-oid-stable", oid == oid2 && !oid.is_empty(), &format!("{oid} {oid2}"));
    Ok(())
}

fn phase_mismatch(host: &str, gate: &mut Gate, sql: &Sql) -> Result<(), String> {
    let db = "oo_a108_mis";
    let mut conn = Conn::connect(host, "postgres", db)?;
    must(
        gate,
        &mut conn,
        "book-lie-v3",
        &format!("INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES ('target-fence-receipts/3', '{LIE}', 'expand', '{FILE_V3}');"),
    )?;
    let before = one(gate, &mut conn, "mis-check-before", check_sql())?;
    drop(conn);
    let out = apply(host, db, sql, "15s", false)?;
    record_apply(gate, "mismatch", &out);
    expect_route(gate, "mismatch", &out, "MismatchV3", Some("OO001"), Some(MISMATCH), "");
    let mut conn = Conn::connect(host, "postgres", db)?;
    let checksum = one(gate, &mut conn, "mismatch-checksum", "SELECT checksum FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/3';")?;
    let after = one(gate, &mut conn, "mis-check-after", check_sql())?;
    gate.check("mismatch-row-unchanged", checksum == LIE, &checksum);
    gate.check("mismatch-file-not-executed", before == after && !after.contains("IS TRUE"), &after);
    Ok(())
}

fn phase_rollback(host: &str, gate: &mut Gate, sql: &Sql) -> Result<(), String> {
    let db = "oo_a108_roll";
    seed_lawful(host, gate, db, "shard_roll")?;
    let mut conn = Conn::connect(host, "postgres", db)?;
    let before = one(gate, &mut conn, "roll-before", snapshot_sql())?;
    let before_def = one(gate, &mut conn, "roll-check-before", check_sql())?;
    drop(conn);
    let out = apply(host, db, sql, "15s", true)?;
    record_apply(gate, "rollback", &out);
    expect_route(gate, "rollback", &out, "SupersedeFromV1", Some("OO003"), Some("forced migration failure"), "0003");
    let mut conn = Conn::connect(host, "postgres", db)?;
    let after = one(gate, &mut conn, "roll-after", snapshot_sql())?;
    let after_def = one(gate, &mut conn, "roll-check-after", check_sql())?;
    let count = one(gate, &mut conn, "roll-marker", "SELECT count(*)::text FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/3';")?;
    gate.check("rollback-receipts", before == after, "snapshot");
    gate.check("rollback-check", before_def == after_def && !after_def.contains("IS TRUE"), &after_def);
    gate.check("rollback-no-marker", count == "0", &count);
    Ok(())
}

fn phase_lock(host: &str, gate: &mut Gate, sql: &Sql) -> Result<(), String> {
    let db = "oo_a108_lock";
    let mut holder = Conn::connect(host, "postgres", db)?;
    let held = holder.query("BEGIN; SELECT pg_catalog.pg_advisory_xact_lock(6404202605); SELECT 'locked';");
    gate.case("advisory-held", &held);
    gate.check("advisory-held", held.code.is_none() && held.status == b'T', &held.cells());
    let started = Instant::now();
    let out = apply(host, db, sql, "2s", false)?;
    let elapsed = started.elapsed();
    record_apply(gate, "advisory-waiter", &out);
    holder.rollback()?;
    gate.check("advisory-55P03", out.code.as_deref() == Some("55P03") && out.executed.is_empty(), &out.message.clone().unwrap_or_default());
    gate.check("advisory-bounded", elapsed < Duration::from_secs(8), &format!("{}", elapsed.as_millis()));
    gate.note(&format!("advisory_lock_elapsed_ms={}", elapsed.as_millis()));
    let mut conn = Conn::connect(host, "postgres", db)?;
    let count = one(gate, &mut conn, "lock-marker", "SELECT count(*)::text FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/3';")?;
    gate.check("advisory-no-marker", count == "0", &count);
    Ok(())
}

fn phase_lie(host: &str, gate: &mut Gate, sql: &Sql) -> Result<(), String> {
    let db = "oo_a108_lie";
    book_lie(host, gate, sql, db)?;
    let mut conn = Conn::connect(host, "postgres", db)?;
    let flags = one(
        gate,
        &mut conn,
        "lie-flags",
        &format!("SELECT _oo.schema_ready('{SHA_0001}')::text || '|' || _oo.segment_preflight_ready('{LIE}')::text || '|' || _oo.segment_preflight_ready('{SHA_0003}')::text || '|' || (SELECT checksum FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/1');"),
    )?;
    gate.check("stored-lie-flags", flags == format!("true|true|false|{SHA_0001}"), &flags);
    let file_hash = sha256_hex(sql.preflight.as_bytes());
    let ready_for_file = flags.split('|').nth(2) == Some("true");
    let ready_for_lie = flags.split('|').nth(1) == Some("true");
    gate.check(
        "stored-lie-conjunction-false",
        file_hash == SHA_0003 && !ready_for_file && ready_for_lie && LIE != file_hash,
        "a stored lie authorizes only the lie",
    );
    Ok(())
}

fn phase_owner(host: &str, gate: &mut Gate, sql: &Sql) -> Result<(), String> {
    let db = "oo_a108_own";
    let mut conn = Conn::connect(host, "postgres", db)?;
    must(
        gate,
        &mut conn,
        "alter-owner",
        "DO $own$ DECLARE fn oid; BEGIN SELECT proc.oid INTO fn FROM pg_catalog.pg_proc AS proc JOIN pg_catalog.pg_namespace AS nsp ON nsp.oid = proc.pronamespace WHERE nsp.nspname = '_oo' AND proc.proname = 'commit_apply_receipt'; EXECUTE format('ALTER FUNCTION %s OWNER TO postgres', fn::regprocedure); END $own$;",
    )?;
    drop(conn);
    let out = apply(host, db, sql, "15s", false)?;
    record_apply(gate, "owner", &out);
    expect_route(gate, "owner", &out, "SupersedeFromV1", Some("42501"), Some(ADOPT), "0003");
    let mut conn = Conn::connect(host, "postgres", db)?;
    let count = one(gate, &mut conn, "owner-marker", "SELECT count(*)::text FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/3';")?;
    let owner = one(
        gate,
        &mut conn,
        "owner-still",
        "SELECT pg_catalog.pg_get_userbyid(proowner) FROM pg_catalog.pg_proc JOIN pg_catalog.pg_namespace ON pg_namespace.oid = pronamespace WHERE nspname = '_oo' AND proname = 'commit_apply_receipt';",
    )?;
    let def = one(gate, &mut conn, "owner-check", check_sql())?;
    gate.check("foreign-owner-no-marker", count == "0", &count);
    gate.check("foreign-owner-not-replaced", owner == "postgres", &owner);
    gate.check("foreign-owner-check-unchanged", !def.contains("IS TRUE"), &def);
    Ok(())
}

fn install_v1(host: &str, gate: &mut Gate, sql: &Sql, db: &str) -> Result<(), String> {
    let mut conn = Conn::connect(host, "postgres", db)?;
    let body = format!(
        "BEGIN; SET LOCAL lock_timeout = '15s'; SELECT pg_catalog.pg_advisory_xact_lock(6404202605);\n{}\nSET ROLE oo_target_owner;\n{}\nSELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\n{}RESET ROLE; COMMIT;",
        bootstrap_sql(&sql.boot, true),
        sql.expand,
        book(&[
            ("target-bootstrap/1", SHA_0000, "0000_bootstrap_roles.sql"),
            ("target-fence-receipts/1", SHA_0001, "0001_target_fence_expand.sql"),
        ])
    );
    must(gate, &mut conn, &format!("install-v1-{db}"), &body)
}

fn install_historical_v2(host: &str, gate: &mut Gate, sql: &Sql, db: &str) -> Result<(), String> {
    let mut conn = Conn::connect(host, "postgres", db)?;
    let body = format!(
        "BEGIN; SET LOCAL lock_timeout = '15s'; SELECT pg_catalog.pg_advisory_xact_lock(6404202605); SET ROLE oo_target_owner; SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\n{}\n{}RESET ROLE; COMMIT;",
        sql.segment,
        book(&[("target-fence-receipts/2", SHA_0002, FILE_V2)])
    );
    gate.check("historical-v2-contains-cast", body.contains("::pg_catalog.regclass"), FILE_V2);
    must(gate, &mut conn, &format!("historical-v2-{db}"), &body)
}

fn seed_lawful(host: &str, gate: &mut Gate, db: &str, shard: &str) -> Result<(), String> {
    let mut conn = Conn::connect(host, "oo_a108_runtime", db)?;
    let sql = format!(
        "{}\n{}\n{}",
        adopt(shard),
        receipt(shard, 1, &format!("{shard}-empty"), 'e', "NULL", "NULL", "''::bytea"),
        receipt(shard, 1, &format!("{shard}-one"), 'e', "0", "1", "'\\x01'::bytea")
    );
    let answer = conn.query(&sql);
    gate.case(&format!("seed-{shard}"), &answer);
    if answer.code.is_some() || !answer.cells().contains("inserted") {
        return Err(format!("seed {shard} {}", answer.message.clone().unwrap_or_else(|| answer.cells())));
    }
    Ok(())
}

fn book_lie(host: &str, gate: &mut Gate, sql: &Sql, db: &str) -> Result<(), String> {
    let file_hash = sha256_hex(sql.preflight.as_bytes());
    if file_hash != SHA_0003 || LIE == file_hash {
        return Err("lie digest collided with the file".into());
    }
    let mut conn = Conn::connect(host, "postgres", db)?;
    let begun = conn.query("BEGIN; SET LOCAL lock_timeout = '15s'; SELECT pg_catalog.pg_advisory_xact_lock(6404202605);");
    if begun.code.is_some() {
        conn.rollback()?;
        return Err(format!("lie open {}", begun.message.unwrap_or_default()));
    }
    let observed = observe(&mut conn)?;
    if !observed.0 || observed.3.is_some() || observed.4.is_some() {
        conn.rollback()?;
        return Err("lie database was not a clean v1".into());
    }
    conn.query("SET ROLE oo_target_owner;");
    if observed.1 {
        let locked = conn.query("SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;");
        if locked.code.is_some() {
            conn.rollback()?;
            return Err("lie migration lock".into());
        }
    }
    let ran = conn.query(&sql.preflight);
    gate.case("stored-lie-file", &ran);
    if ran.code.is_some() {
        conn.rollback()?;
        return Err(format!("lie file {}", ran.message.unwrap_or_default()));
    }
    let booked = conn.query(&format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES ('target-fence-receipts/3', '{LIE}', 'expand', '{FILE_V3}'); RESET ROLE; COMMIT;"
    ));
    gate.case("stored-lie", &booked);
    if booked.code.is_some() {
        if booked.status == b'E' {
            conn.rollback()?;
        }
        return Err(format!("lie book {}", booked.message.unwrap_or_default()));
    }
    Ok(())
}

fn apply(host: &str, db: &str, sql: &Sql, timeout: &str, fail_before_marker: bool) -> Result<ApplyOut, String> {
    let mut conn = Conn::connect(host, "postgres", db)?;
    let file_hash = sha256_hex(sql.preflight.as_bytes());
    if file_hash != SHA_0003 {
        return Err("refusing to apply a file whose hash is not the bound 0003 digest".into());
    }
    let begun = conn.query(&format!(
        "BEGIN; SET LOCAL lock_timeout = '{timeout}'; SELECT pg_catalog.pg_advisory_xact_lock(6404202605);"
    ));
    if begun.code.is_some() {
        let code = begun.code.clone();
        let message = begun.message.clone();
        if begun.status == b'E' {
            conn.rollback()?;
        }
        return Ok(ApplyOut {
            route: "lock",
            code,
            message,
            executed: "",
        });
    }
    let (relation, migration_lock, role, v2, v3) = observe(&mut conn)?;
    let (route, executed) = if !relation {
        ("AbsentPreflight", "0003")
    } else if let Some(stored) = &v3 {
        if stored == &file_hash {
            ("Skip", "")
        } else {
            ("MismatchV3", "")
        }
    } else if let Some(stored) = &v2 {
        if stored == SHA_0002 {
            ("UpgradeFromV2", "0003")
        } else {
            ("MismatchV2", "")
        }
    } else {
        ("SupersedeFromV1", "0003")
    };
    if route == "Skip" {
        let done = conn.query("COMMIT;");
        if done.code.is_some() {
            return Err(format!("skip commit {}", done.message.unwrap_or_default()));
        }
        return Ok(ApplyOut {
            route,
            code: None,
            message: None,
            executed,
        });
    }
    if route == "MismatchV3" || route == "MismatchV2" {
        let failed = conn.query(&format!(
            "DO $checksum$ BEGIN RAISE EXCEPTION '{MISMATCH}' USING ERRCODE = 'OO001'; END $checksum$;"
        ));
        let code = failed.code.clone();
        let message = failed.message.clone();
        if failed.status == b'E' {
            conn.rollback()?;
        }
        return Ok(ApplyOut {
            route,
            code,
            message,
            executed,
        });
    }
    if role {
        let set = conn.query("SET ROLE oo_target_owner;");
        if set.code.is_some() {
            conn.rollback()?;
            return Err(format!("set role {}", set.message.unwrap_or_default()));
        }
    }
    if migration_lock {
        let locked = conn.query("SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;");
        if locked.code.is_some() {
            conn.rollback()?;
            return Err(format!("migration lock {}", locked.message.unwrap_or_default()));
        }
    }
    if sql.preflight.contains("::regclass") {
        conn.rollback()?;
        return Err("reference file acquired a regclass cast".into());
    }
    let ran = conn.query(&sql.preflight);
    if ran.code.is_some() {
        let code = ran.code.clone();
        let message = ran.message.clone();
        if ran.status == b'E' || ran.status == b'T' {
            conn.rollback()?;
        }
        return Ok(ApplyOut {
            route,
            code,
            message,
            executed,
        });
    }
    if fail_before_marker {
        let failed = conn.query(
            "DO $fail$ BEGIN RAISE EXCEPTION 'forced migration failure' USING ERRCODE = 'OO003'; END $fail$;",
        );
        let code = failed.code.clone();
        let message = failed.message.clone();
        if failed.status == b'E' || failed.status == b'T' {
            conn.rollback()?;
        }
        return Ok(ApplyOut {
            route,
            code,
            message,
            executed,
        });
    }
    let booked = conn.query(&format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES ('target-fence-receipts/3', '{file_hash}', 'expand', '{FILE_V3}'); RESET ROLE; COMMIT;"
    ));
    if booked.code.is_some() && (booked.status == b'E' || booked.status == b'T') {
        let code = booked.code.clone();
        let message = booked.message.clone();
        conn.rollback()?;
        return Ok(ApplyOut {
            route,
            code,
            message,
            executed,
        });
    }
    Ok(ApplyOut {
        route,
        code: booked.code,
        message: booked.message,
        executed,
    })
}

fn observe(conn: &mut Conn) -> Result<(bool, bool, bool, Option<String>, Option<String>), String> {
    let answer = conn.query(
        "SELECT (pg_catalog.to_regclass('_oo.apply_receipts') IS NOT NULL)::text, (EXISTS (SELECT 1 FROM pg_catalog.pg_constraint AS constraint_row WHERE constraint_row.conname = 'apply_receipts_segment_check' AND constraint_row.conrelid = pg_catalog.to_regclass('_oo.apply_receipts')))::text, (pg_catalog.to_regclass('_oo.schema_migrations') IS NOT NULL)::text, (EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_target_owner'))::text, (pg_catalog.to_regclass('_oo.migration_lock') IS NOT NULL)::text;",
    );
    if answer.code.is_some() || answer.rows.is_empty() {
        return Err(format!("observe {}", answer.message.unwrap_or_default()));
    }
    let row = &answer.rows[0];
    let relation = row[0] == "true" && row[1] == "true";
    let migrations = row[2] == "true";
    let role = row[3] == "true";
    let migration_lock = row[4] == "true";
    let (v2, v3) = if migrations {
        let marks = conn.query(
            "SELECT version || '=' || checksum FROM _oo.schema_migrations WHERE version IN ('target-fence-receipts/2', 'target-fence-receipts/3') AND phase = 'expand' ORDER BY version;",
        );
        if marks.code.is_some() {
            return Err(format!("markers {}", marks.message.unwrap_or_default()));
        }
        let mut v2 = None;
        let mut v3 = None;
        for mark in &marks.rows {
            let Some((version, checksum)) = mark[0].split_once('=') else {
                return Err("marker row".into());
            };
            if version == "target-fence-receipts/2" {
                v2 = Some(checksum.to_string());
            } else if version == "target-fence-receipts/3" {
                v3 = Some(checksum.to_string());
            }
        }
        (v2, v3)
    } else {
        (None, None)
    };
    Ok((relation, migration_lock, role, v2, v3))
}

fn expect_route(
    gate: &mut Gate,
    name: &str,
    out: &ApplyOut,
    route: &str,
    code: Option<&str>,
    message: Option<&str>,
    executed: &str,
) {
    gate.check(&format!("{name}-route"), out.route == route, out.route);
    gate.check(&format!("{name}-sqlstate"), out.code.as_deref() == code, &out.code.clone().unwrap_or_default());
    if let Some(message) = message {
        gate.check(
            &format!("{name}-message"),
            out.message.as_deref() == Some(message),
            &out.message.clone().unwrap_or_default(),
        );
    }
    gate.check(&format!("{name}-files"), out.executed == executed, out.executed);
}

fn record_apply(gate: &mut Gate, name: &str, out: &ApplyOut) {
    let answer = Answer {
        rows: vec![vec![format!("{}|{}", out.route, out.executed)]],
        code: out.code.clone(),
        message: out.message.clone(),
        status: b'I',
    };
    gate.case(name, &answer);
}

fn must(gate: &mut Gate, conn: &mut Conn, name: &str, sql: &str) -> Result<(), String> {
    let answer = conn.query(sql);
    gate.case(name, &answer);
    if answer.status == b'E' {
        conn.rollback()?;
    }
    if let Some(code) = answer.code {
        return Err(format!("{name} {code}: {}", answer.message.unwrap_or_default()));
    }
    Ok(())
}

fn one(gate: &mut Gate, conn: &mut Conn, name: &str, sql: &str) -> Result<String, String> {
    let answer = conn.query(sql);
    gate.case(name, &answer);
    if let Some(code) = answer.code {
        gate.check(name, false, &format!("{code}: {}", answer.message.unwrap_or_default()));
        return Ok(String::new());
    }
    Ok(answer.cells())
}

fn marker_sql() -> &'static str {
    "SELECT coalesce(string_agg(version || '|' || checksum || '|' || applied_at::text, E'\\n' ORDER BY version), '') FROM _oo.schema_migrations WHERE version IN ('target-fence-receipts/1', 'target-fence-receipts/2', 'target-fence-receipts/3');"
}

fn snapshot_sql() -> &'static str {
    "SELECT coalesce(string_agg(row(receipt.*)::text, E'\\n' ORDER BY source, table_generation, txn_or_batch_id), '') FROM _oo.apply_receipts AS receipt;"
}

fn check_sql() -> &'static str {
    "SELECT pg_catalog.pg_get_constraintdef(constraint_row.oid) FROM pg_catalog.pg_constraint AS constraint_row WHERE constraint_row.conname = 'apply_receipts_segment_check' AND constraint_row.conrelid = pg_catalog.to_regclass('_oo.apply_receipts');"
}

fn v1_line(marks: &str) -> String {
    marks
        .lines()
        .find(|line| line.starts_with("target-fence-receipts/1|"))
        .unwrap_or("")
        .to_string()
}

fn lines_kept(before: &str, after: &str) -> bool {
    !before.is_empty()
        && before.lines().all(|line| after.lines().any(|kept| kept == line))
}

fn adopt(shard: &str) -> String {
    format!("SELECT _oo.adopt_target_generation('{shard}', '{OWNER}'::uuid);")
}

fn receipt(shard: &str, generation: i64, batch: &str, payload: char, index: &str, count: &str, bitmap: &str) -> String {
    format!(
        "SELECT _oo.commit_apply_receipt('{shard}', '{OWNER}'::uuid, {generation}, 'warehouse', 'orders_g1', '{batch}', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('{payload}', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, {index}, {count}, {bitmap});"
    )
}

fn book(rows: &[(&str, &str, &str)]) -> String {
    let values = rows
        .iter()
        .map(|(version, checksum, filename)| format!("  ('{version}', '{checksum}', 'expand', '{filename}')"))
        .collect::<Vec<_>>()
        .join(",\n");
    format!("INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values};\n")
}

fn bootstrap_sql(src: &str, roles_exist: bool) -> String {
    if !roles_exist {
        return src.to_string();
    }
    let mut out = String::new();
    let mut skipping = false;
    for line in src.lines() {
        if line.starts_with("CREATE ROLE ") {
            skipping = true;
        }
        if !skipping {
            out.push_str(line);
            out.push('\n');
        }
        if skipping && line.trim_end().ends_with(';') {
            skipping = false;
        }
    }
    out
}

fn cut<'a>(text: &'a str, start: &str, end: &str) -> Result<&'a str, String> {
    let at = text.find(start).ok_or_else(|| format!("missing {start}"))?;
    let rel = text[at + start.len()..]
        .find(end)
        .ok_or_else(|| format!("missing {end}"))?;
    Ok(&text[at..at + start.len() + rel])
}

impl Gate {
    fn note(&mut self, line: &str) {
        self.write_summary(&format!("NOTE {line}"));
        println!("NOTE {line}");
    }

    fn check(&mut self, name: &str, pass: bool, detail: &str) {
        if !pass {
            self.fails += 1;
        }
        let state = if pass { "PASS" } else { "FAIL" };
        let flat = detail.replace('\n', " ");
        self.write_summary(&format!("{state} {name} {flat}"));
        println!("{state} {name}");
    }

    fn case(&self, name: &str, answer: &Answer) {
        let dir = self.root.join("cases");
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(dir.join(format!("{name}.stdout")), answer.cells() + "\n");
        let err = match (&answer.code, &answer.message) {
            (Some(code), Some(message)) => format!("{code}: {message}\n"),
            (Some(code), None) => format!("{code}\n"),
            _ => String::new(),
        };
        let _ = fs::write(dir.join(format!("{name}.stderr")), err);
    }

    fn write_summary(&self, line: &str) {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(self.root.join("summary.log")) {
            let _ = writeln!(file, "{line}");
        }
    }
}

impl Conn {
    fn connect(host: &str, user: &str, db: &str) -> Result<Self, String> {
        let stream = TcpStream::connect((host, 5432)).map_err(|e| format!("connect {host} {db}: {e}"))?;
        stream.set_nodelay(true).map_err(|e| e.to_string())?;
        let timeout = Some(Duration::from_secs(60));
        stream.set_read_timeout(timeout).map_err(|e| e.to_string())?;
        stream.set_write_timeout(timeout).map_err(|e| e.to_string())?;
        let mut conn = Self { stream };
        conn.startup(user, db)?;
        Ok(conn)
    }

    fn startup(&mut self, user: &str, db: &str) -> Result<(), String> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&196_608u32.to_be_bytes());
        for (key, value) in [("user", user), ("database", db), ("client_encoding", "UTF8")] {
            payload.extend_from_slice(key.as_bytes());
            payload.push(0);
            payload.extend_from_slice(value.as_bytes());
            payload.push(0);
        }
        payload.push(0);
        let len = u32::try_from(payload.len() + 4).map_err(|_| "startup len")?;
        self.stream.write_all(&len.to_be_bytes()).map_err(|e| e.to_string())?;
        self.stream.write_all(&payload).map_err(|e| e.to_string())?;
        let mut authed = false;
        loop {
            let (tag, body) = self.read_message()?;
            match tag {
                b'R' => {
                    if body.len() < 4 {
                        return Err("auth body".into());
                    }
                    let kind = u32::from_be_bytes(body[..4].try_into().map_err(|_| "auth kind")?);
                    if kind != 0 {
                        return Err(format!("fixture auth is trust, got auth kind {kind}"));
                    }
                    authed = true;
                }
                b'Z' => {
                    if !authed {
                        return Err("ready before authentication".into());
                    }
                    return Ok(());
                }
                b'E' => {
                    let (code, message) = error_fields(&body);
                    return Err(format!("startup {code}: {message}"));
                }
                b'S' | b'K' | b'N' => {}
                other => return Err(format!("startup tag {}", other as char)),
            }
        }
    }

    fn query(&mut self, sql: &str) -> Answer {
        let mut payload = Vec::with_capacity(sql.len() + 1);
        payload.extend_from_slice(sql.as_bytes());
        payload.push(0);
        let Ok(len) = u32::try_from(payload.len() + 4) else {
            return Answer {
                rows: Vec::new(),
                code: Some("XX000".into()),
                message: Some("query too large".into()),
                status: b'I',
            };
        };
        if self.stream.write_all(b"Q").is_err()
            || self.stream.write_all(&len.to_be_bytes()).is_err()
            || self.stream.write_all(&payload).is_err()
        {
            return Answer {
                rows: Vec::new(),
                code: Some("08006".into()),
                message: Some("write failed".into()),
                status: b'I',
            };
        }
        let mut rows = Vec::new();
        let mut code = None;
        let mut message = None;
        loop {
            let Ok((tag, body)) = self.read_message() else {
                return Answer {
                    rows,
                    code: Some("08006".into()),
                    message: Some("read failed".into()),
                    status: b'I',
                };
            };
            match tag {
                b'Z' => {
                    return Answer {
                        rows,
                        code,
                        message,
                        status: body.first().copied().unwrap_or(b'?'),
                    };
                }
                b'E' => {
                    let (found, text) = error_fields(&body);
                    code = Some(found);
                    message = Some(text);
                }
                b'D' => rows.push(data_row(&body)),
                b'T' | b'C' | b'N' | b'S' | b'A' | b'I' | b'n' => {}
                other => {
                    return Answer {
                        rows,
                        code: Some("08P01".into()),
                        message: Some(format!("response tag {}", other as char)),
                        status: b'I',
                    };
                }
            }
        }
    }

    fn rollback(&mut self) -> Result<(), String> {
        let answer = self.query("ROLLBACK");
        if answer.code.is_some() {
            return Err(format!(
                "rollback {}: {}",
                answer.code.unwrap_or_default(),
                answer.message.unwrap_or_default()
            ));
        }
        Ok(())
    }

    fn read_message(&mut self) -> Result<(u8, Vec<u8>), String> {
        let mut tag = [0u8; 1];
        self.stream.read_exact(&mut tag).map_err(|e| e.to_string())?;
        let mut len_bytes = [0u8; 4];
        self.stream.read_exact(&mut len_bytes).map_err(|e| e.to_string())?;
        let len = usize::try_from(u32::from_be_bytes(len_bytes)).map_err(|_| "length")?;
        if !(4..=32 * 1024 * 1024).contains(&len) {
            return Err(format!("message length {len}"));
        }
        let mut body = vec![0u8; len - 4];
        self.stream.read_exact(&mut body).map_err(|e| e.to_string())?;
        Ok((tag[0], body))
    }
}

fn error_fields(body: &[u8]) -> (String, String) {
    let mut code = String::new();
    let mut message = String::new();
    let mut cursor = 0;
    while cursor < body.len() {
        let kind = body[cursor];
        cursor += 1;
        if kind == 0 {
            break;
        }
        let Some(end) = body[cursor..].iter().position(|byte| *byte == 0) else {
            break;
        };
        let text = String::from_utf8_lossy(&body[cursor..cursor + end]).into_owned();
        cursor += end + 1;
        match kind {
            b'C' => code = text,
            b'M' => message = text,
            _ => {}
        }
    }
    (code, message)
}

fn data_row(body: &[u8]) -> Vec<String> {
    if body.len() < 2 {
        return Vec::new();
    }
    let count = u16::from_be_bytes([body[0], body[1]]);
    let mut cursor = 2;
    let mut cells = Vec::new();
    for _ in 0..count {
        if cursor + 4 > body.len() {
            break;
        }
        let len = i32::from_be_bytes(body[cursor..cursor + 4].try_into().unwrap_or([0, 0, 0, 0]));
        cursor += 4;
        if len < 0 {
            cells.push(String::new());
            continue;
        }
        let take = usize::try_from(len).unwrap_or(0);
        if cursor + take > body.len() {
            break;
        }
        cells.push(String::from_utf8_lossy(&body[cursor..cursor + take]).into_owned());
        cursor += take;
    }
    cells
}

fn read_string(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn sha256_hex(message: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
        0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
        0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
        0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
        0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
        0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a, 0x510e_527f, 0x9b05_688c, 0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let bits = u64::try_from(message.len()).expect("message length") * 8;
    let mut data = message.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bits.to_be_bytes());
    for chunk in data.chunks(64) {
        let mut words = [0u32; 64];
        for (index, block) in chunk.chunks(4).enumerate() {
            words[index] = u32::from_be_bytes(block.try_into().expect("sha word"));
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7) ^ words[index - 15].rotate_right(18) ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17) ^ words[index - 2].rotate_right(19) ^ (words[index - 2] >> 10);
            words[index] = words[index - 16].wrapping_add(s0).wrapping_add(words[index - 7]).wrapping_add(s1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let temp1 = h.wrapping_add(s1).wrapping_add(choose).wrapping_add(K[index]).wrapping_add(words[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
    let mut hex = String::with_capacity(64);
    for word in state {
        hex.push_str(&format!("{word:08x}"));
    }
    hex
}
