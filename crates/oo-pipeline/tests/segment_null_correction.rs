//! Forward segment correction for target-fence-receipts/2.
//!
//! The package smoke checks the published bytes. The ignored test drives one
//! PostgreSQL 17 fixture. v1 readiness does not authorize this contract.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const V1_JSON: &str = include_str!(
    "../../../contracts/pipeline/target-fence-receipts/target-fence-receipts-v1.json"
);
const V2_JSON: &str = include_str!(
    "../../../contracts/pipeline/target-fence-receipts/target-fence-receipts-v2.json"
);
const BOOTSTRAP: &str = include_str!("../migrations/0000_bootstrap_roles.sql");
const EXPAND: &str = include_str!("../migrations/0001_target_fence_expand.sql");
const SEGMENT: &str = include_str!("../migrations/0002_target_segment_null_expand.sql");

const OWNER: &str = "11111111-1111-4111-8111-111111111111";
const V1_BOOT: &str = "37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208";
const V1_EXPAND: &str = "97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9";
const V2_FILE: &str = "59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512";
const LIE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const PG_IMAGE: &str =
    "docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb";
const RUN_ID: &str = "d4e8a092-7c31-4b6e-9f10-a0925e600001";

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn sha256(path: &Path) -> String {
    let output = if Path::new("/usr/bin/sha256sum").exists() {
        Command::new("/usr/bin/sha256sum").arg(path).output()
    } else {
        Command::new("/usr/bin/shasum")
            .args(["-a", "256"])
            .arg(path)
            .output()
    }
    .unwrap_or_else(|error| panic!("hash {}: {error}", path.display()));
    assert!(
        output.status.success(),
        "hash {} failed: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .expect("hash")
        .to_string()
}

fn json_string<'a>(text: &'a str, key: &str) -> &'a str {
    let needle = format!("\"{key}\": \"");
    let start = text
        .find(&needle)
        .unwrap_or_else(|| panic!("missing {key}"))
        + needle.len();
    let rest = &text[start..];
    &rest[..rest.find('"').expect("string end")]
}

fn sha_after_filename<'a>(contract: &'a str, filename: &str) -> &'a str {
    let marker = format!("\"filename\": \"{filename}\"");
    let slice = contract
        .get(contract.find(&marker).unwrap_or_else(|| panic!("missing {filename}"))..)
        .expect("slice");
    let key = "\"sha256\": \"";
    let start = slice.find(key).expect("sha256") + key.len();
    let rest = &slice[start..];
    &rest[..rest.find('"').expect("sha end")]
}

fn driver_requiring_v2_authorized(v1_ready: bool, v2_ready_for_file_hash: bool) -> bool {
    let _v1_ready_is_not_sufficient = v1_ready;
    v2_ready_for_file_hash
}

#[test]
fn published_v1_bytes_stay_and_v2_closes_unknown_segments() {
    let root = crate_root();
    let boot = root.join("migrations/0000_bootstrap_roles.sql");
    let expand = root.join("migrations/0001_target_fence_expand.sql");
    let segment = root.join("migrations/0002_target_segment_null_expand.sql");
    assert_eq!(sha256(&boot), V1_BOOT);
    assert_eq!(sha256(&expand), V1_EXPAND);
    assert_eq!(sha256(&segment), V2_FILE);
    assert_eq!(sha256(&boot), sha_after_filename(V1_JSON, "0000_bootstrap_roles.sql"));
    assert_eq!(sha256(&expand), sha_after_filename(V1_JSON, "0001_target_fence_expand.sql"));
    assert_eq!(sha256(&boot), sha_after_filename(V2_JSON, "0000_bootstrap_roles.sql"));
    assert_eq!(sha256(&expand), sha_after_filename(V2_JSON, "0001_target_fence_expand.sql"));
    assert_eq!(
        sha256(&segment),
        sha_after_filename(V2_JSON, "0002_target_segment_null_expand.sql")
    );
    assert_eq!(json_string(V1_JSON, "contract"), "oo.pipeline.target-fence-receipts/v1");
    assert_eq!(json_string(V1_JSON, "schema_version"), "target-fence-receipts/1");
    assert_eq!(json_string(V2_JSON, "contract"), "oo.pipeline.target-fence-receipts/v2");
    assert_eq!(json_string(V2_JSON, "schema_version"), "target-fence-receipts/2");
    assert!(V2_JSON.contains("\"v1_schema_ready_authorizes_this_contract\": false"));
    assert!(V2_JSON.contains("\"segment_contract_ready_reads_migration_file\": false"));
    assert!(V2_JSON.contains("\"illegal_segment_history\": \"OO021\""));
    assert!(!EXPAND.contains("IS NOT TRUE"));
    assert!(EXPAND.contains("p_segment_count >= 1"));
    assert!(SEGMENT.contains("IS NOT TRUE"));
    assert!(SEGMENT.contains(") IS TRUE"));
    assert!(SEGMENT.contains("p_segment_count IS NOT NULL"));
    assert!(SEGMENT.contains("segment_count IS NOT NULL"));
    assert!(SEGMENT.contains("ERRCODE = 'OO021'"));
    assert!(SEGMENT.contains(
        "this migration does not delete, rewrite, or mutate receipts"
    ));
    assert!(!SEGMENT.contains("DELETE FROM _oo.apply_receipts"));
    assert!(!SEGMENT.contains("UPDATE _oo.apply_receipts"));
    assert!(!SEGMENT.contains("GRANT oo_target_owner TO oo_pipeline_runtime"));
    let segment_at = SEGMENT
        .find("closed segment metadata")
        .expect("segment rejection");
    let fence_at = SEGMENT
        .find("PERFORM _oo._load_held_generation")
        .expect("fence check");
    assert!(segment_at < fence_at, "segment rejection follows the fence check");
    assert_eq!(BOOTSTRAP, fs::read_to_string(&boot).unwrap());
    assert_eq!(EXPAND, fs::read_to_string(&expand).unwrap());
    assert_eq!(SEGMENT, fs::read_to_string(&segment).unwrap());
    assert!(!driver_requiring_v2_authorized(true, false));
    assert!(driver_requiring_v2_authorized(true, true));
    assert!(!driver_requiring_v2_authorized(false, false));
}

fn secret() -> String {
    let mut bytes = [0u8; 24];
    fs::File::open("/dev/urandom")
        .expect("urandom")
        .read_exact(&mut bytes)
        .expect("urandom read");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

struct Pg {
    name: String,
    net: String,
    vol: String,
    evidence: PathBuf,
    admin_password: String,
    runtime_password: String,
    bootstrap_password: String,
    cleanup: bool,
}

impl Pg {
    fn scrub(&self, text: &str) -> String {
        text.replace(&self.admin_password, "***")
            .replace(&self.runtime_password, "***")
            .replace(&self.bootstrap_password, "***")
    }

    fn note(&self, line: &str) {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.evidence.join("summary.log"))
            .expect("summary");
        writeln!(file, "{}", self.scrub(line)).expect("summary write");
    }

    fn write_case(&self, name: &str, output: &Output) {
        let dir = self.evidence.join("cases");
        fs::create_dir_all(&dir).expect("cases");
        fs::write(dir.join(format!("{name}.stdout")), self.scrub(&String::from_utf8_lossy(&output.stdout))).unwrap();
        fs::write(dir.join(format!("{name}.stderr")), self.scrub(&String::from_utf8_lossy(&output.stderr))).unwrap();
        fs::write(
            dir.join(format!("{name}.exit")),
            output.status.code().unwrap_or(127).to_string() + "\n",
        )
        .unwrap();
    }

    fn docker(&self, args: &[&str]) -> Output {
        Command::new("docker")
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("docker {}: {error}", args.join(" ")))
    }

    fn install_pgpass(&self) {
        let body = format!(
            "127.0.0.1:5432:*:oo_fixture_admin:{admin}\n127.0.0.1:5432:*:oo_a092_runtime:{runtime}\n127.0.0.1:5432:*:oo_a092_bootstrap:{bootstrap}\n",
            admin = self.admin_password,
            runtime = self.runtime_password,
            bootstrap = self.bootstrap_password
        );
        let mut child = Command::new("docker")
            .args(["exec", "-i", &self.name, "bash", "-c", "umask 077; cat > /tmp/pgpass"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("pgpass");
        child.stdin.take().expect("stdin").write_all(body.as_bytes()).expect("pgpass write");
        let output = child.wait_with_output().expect("pgpass wait");
        assert!(output.status.success(), "pgpass {}", self.scrub(&String::from_utf8_lossy(&output.stderr)));
    }

    fn psql(&self, user: &str, db: &str, sql: &str) -> Output {
        let mut child = Command::new("docker")
            .args([
                "exec",
                "-i",
                "-e",
                "PGPASSFILE=/tmp/pgpass",
                &self.name,
                "psql",
                "-U",
                user,
                "-d",
                db,
                "-h",
                "127.0.0.1",
                "-X",
                "-q",
                "-t",
                "-A",
                "-v",
                "ON_ERROR_STOP=1",
                "-v",
                "VERBOSITY=verbose",
                "-P",
                "pager=off",
                "-f",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("psql");
        child.stdin.take().expect("stdin").write_all(sql.as_bytes()).expect("sql");
        child.wait_with_output().expect("psql wait")
    }

    fn psql_named(&self, name: &str, user: &str, db: &str, sql: &str) -> Output {
        let output = self.psql(user, db, sql);
        self.write_case(name, &output);
        output
    }

    fn ok(&self, name: &str, user: &str, db: &str, sql: &str) -> String {
        let output = self.psql_named(name, user, db, sql);
        assert!(
            output.status.success(),
            "{name} failed\n{}",
            self.scrub(&String::from_utf8_lossy(&output.stderr))
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn expect_state(&self, name: &str, user: &str, db: &str, sql: &str, state: &str) {
        let output = self.psql_named(name, user, db, sql);
        assert!(
            !output.status.success(),
            "{name} succeeded and should have raised {state}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        let found = sqlstate(&stderr);
        assert_eq!(found.as_deref(), Some(state), "{name} stderr {}", self.scrub(&stderr));
        self.note(&format!("{name}={state}"));
    }
}

impl Drop for Pg {
    fn drop(&mut self) {
        if self.cleanup {
            return;
        }
        self.remove();
    }
}

fn sqlstate(stderr: &str) -> Option<String> {
    for line in stderr.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("SQLSTATE:") {
            return Some(rest.trim().to_string());
        }
        let Some(pos) = line.find("ERROR:") else {
            continue;
        };
        let rest = line[pos + "ERROR:".len()..].trim();
        let code: String = rest.chars().take(5).collect();
        if code.len() == 5
            && code.chars().all(|ch| ch.is_ascii_alphanumeric())
            && rest.as_bytes().get(5) == Some(&b':')
        {
            return Some(code);
        }
    }
    None
}

fn receipt(shard: &str, generation: i64, batch: &str, payload: char, index: &str, count: &str, bitmap: &str) -> String {
    format!(
        "SELECT _oo.commit_apply_receipt('{shard}', '{OWNER}'::uuid, {generation}, 'warehouse', 'orders_g1', '{batch}', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('{payload}', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, {index}, {count}, {bitmap});\n"
    )
}

fn adopt(shard: &str) -> String {
    format!("SELECT _oo.adopt_target_generation('{shard}', '{OWNER}'::uuid);\n")
}

fn snapshot_sql() -> &'static str {
    "SELECT coalesce(string_agg(row(receipt.*)::text, E'\\n' ORDER BY source, table_generation, txn_or_batch_id), '') FROM _oo.apply_receipts AS receipt;\n"
}

fn bootstrap_sql(roles_exist: bool) -> String {
    if !roles_exist {
        return BOOTSTRAP.to_string();
    }
    let mut out = String::new();
    let mut skipping = false;
    for line in BOOTSTRAP.lines() {
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

fn book_sql(rows: &[(&str, &str, &str)]) -> String {
    let values = rows
        .iter()
        .map(|(version, checksum, filename)| {
            format!("  ('{version}', '{checksum}', 'expand', '{filename}')")
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!("INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values};\n")
}

#[test]
#[ignore = "one owned PostgreSQL 17 fixture; run outside the offline package smoke"]
fn live_segment_null_correction() {
    let mut pg = provision();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| exercise(&pg)));
    pg.remove();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

fn exercise(pg: &Pg) {
    let version = pg.ok("version", "oo_fixture_admin", "postgres", "SELECT version();\n");
    pg.note(&format!("server={version}"));
    assert!(version.contains("PostgreSQL 17.10"), "{version}");
    for db in ["oo_seg_red", "oo_seg_fix", "oo_seg_fresh", "oo_seg_roll", "oo_seg_lie"] {
        pg.ok(&format!("create-{db}"), "oo_fixture_admin", "postgres", &format!("CREATE DATABASE {db};\n"));
    }
    apply(pg, "oo_seg_red", false, false);
    create_logins(pg);
    phase_red(pg);
    apply(pg, "oo_seg_fix", true, false);
    phase_fix(pg);
    apply(pg, "oo_seg_fresh", true, true);
    phase_fresh_checks(pg);
    apply(pg, "oo_seg_roll", true, false);
    phase_rollback(pg);
    apply(pg, "oo_seg_lie", true, false);
    phase_lie(pg);
    pg.note("live=pass");
}

fn apply(pg: &Pg, db: &str, roles_exist: bool, with_v2: bool) {
    let mut sql = String::from(
        "BEGIN;\nSET LOCAL lock_timeout = '5s';\nSELECT pg_catalog.pg_advisory_xact_lock(6404202605);\n",
    );
    sql.push_str(&bootstrap_sql(roles_exist));
    sql.push_str("SET ROLE oo_target_owner;\n");
    sql.push_str(EXPAND);
    sql.push('\n');
    sql.push_str("SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\n");
    if with_v2 {
        sql.push_str(SEGMENT);
        sql.push('\n');
        sql.push_str(&format!(
            "SELECT 'before=' || _oo.segment_contract_ready('{V2_FILE}')::text;\n"
        ));
    }
    let mut rows = vec![
        ("target-bootstrap/1", V1_BOOT, "0000_bootstrap_roles.sql"),
        ("target-fence-receipts/1", V1_EXPAND, "0001_target_fence_expand.sql"),
    ];
    if with_v2 {
        rows.push((
            "target-fence-receipts/2",
            V2_FILE,
            "0002_target_segment_null_expand.sql",
        ));
    }
    sql.push_str(&book_sql(&rows));
    if with_v2 {
        sql.push_str(&format!(
            "SELECT 'after=' || _oo.segment_contract_ready('{V2_FILE}')::text;\nSELECT 'v1=' || _oo.schema_ready('{V1_EXPAND}')::text;\n"
        ));
    }
    sql.push_str("RESET ROLE;\nCOMMIT;\n");
    let output = pg.psql_named(&format!("apply-{db}"), "oo_fixture_admin", db, &sql);
    assert!(
        output.status.success(),
        "apply {db}\n{}",
        pg.scrub(&String::from_utf8_lossy(&output.stderr))
    );
    if with_v2 {
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("before=false"), "{db} ready before marker: {text}");
        assert!(text.contains("after=true"), "{db} ready after marker: {text}");
        assert!(text.contains("v1=true"), "{db} v1 ready: {text}");
        pg.note(&format!("{db} fresh_ready before=false after=true v1=true"));
    }
}

fn create_logins(pg: &Pg) {
    let sql = format!(
        "CREATE ROLE oo_a092_bootstrap LOGIN PASSWORD '{bootstrap}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;\nCREATE ROLE oo_a092_runtime LOGIN PASSWORD '{runtime}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION;\nGRANT oo_target_owner TO oo_a092_bootstrap WITH INHERIT FALSE, SET TRUE;\nGRANT oo_pipeline_runtime TO oo_a092_runtime WITH INHERIT TRUE, SET FALSE;\nALTER ROLE oo_a092_bootstrap SET search_path = pg_catalog;\nALTER ROLE oo_a092_runtime SET search_path = pg_catalog;\n",
        bootstrap = pg.bootstrap_password,
        runtime = pg.runtime_password
    );
    let output = pg.psql("oo_fixture_admin", "oo_seg_red", &sql);
    pg.write_case("create-logins", &output);
    assert!(output.status.success(), "{}", pg.scrub(&String::from_utf8_lossy(&output.stderr)));
    pg.install_pgpass();
    for db in ["oo_seg_red", "oo_seg_fix", "oo_seg_fresh", "oo_seg_roll", "oo_seg_lie"] {
        pg.ok(
            &format!("grant-{db}"),
            "oo_fixture_admin",
            db,
            "DO $grant$ BEGIN EXECUTE format('GRANT CONNECT ON DATABASE %I TO oo_a092_bootstrap', current_database()); END $grant$;\n",
        );
    }
}

fn phase_red(pg: &Pg) {
    let db = "oo_seg_red";
    let cases = [
        ("seg-null-index", "seg_null_index", "NULL", "1", "'\\x01'::bytea"),
        ("seg-null-count", "seg_null_count", "0", "NULL", "'\\x01'::bytea"),
        ("seg-null-both", "seg_null_both", "NULL", "NULL", "'\\x01'::bytea"),
    ];
    for (batch, shard, index, count, bitmap) in cases {
        let inserted = pg.ok(
            &format!("red-adopt-{batch}"),
            "oo_a092_runtime",
            db,
            &format!("{}{}", adopt(shard), receipt(shard, 1, batch, 'e', index, count, bitmap)),
        );
        assert_eq!(inserted.lines().next_back().unwrap(), "inserted", "{batch} {inserted}");
        pg.note(&format!("red {batch}=inserted"));
    }
    let before = pg.ok("red-snapshot-before", "oo_fixture_admin", db, snapshot_sql());
    pg.expect_state(
        "red-forward",
        "oo_fixture_admin",
        db,
        &format!(
            "BEGIN;\nSET LOCAL lock_timeout = '5s';\nSELECT pg_catalog.pg_advisory_xact_lock(6404202605);\nSET ROLE oo_target_owner;\nSELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\n{SEGMENT}\n{}\nRESET ROLE;\nCOMMIT;\n",
            book_sql(&[("target-fence-receipts/2", V2_FILE, "0002_target_segment_null_expand.sql")])
        ),
        "OO021",
    );
    let after = pg.ok("red-snapshot-after", "oo_fixture_admin", db, snapshot_sql());
    assert_eq!(before, after, "bad history changed");
    let ready = pg.ok(
        "red-ready",
        "oo_fixture_admin",
        db,
        "SELECT EXISTS (SELECT 1 FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/2')::text;\n",
    );
    assert_eq!(ready, "false");
    let v1 = pg.ok(
        "red-v1-ready",
        "oo_a092_runtime",
        db,
        &format!("SELECT _oo.schema_ready('{V1_EXPAND}')::text;\n"),
    );
    assert_eq!(v1, "true");
    assert!(!driver_requiring_v2_authorized(true, false));
    let again = pg.ok(
        "red-still-open",
        "oo_a092_runtime",
        db,
        &format!(
            "{}{}",
            adopt("seg_null_again"),
            receipt("seg_null_again", 1, "seg-null-again", 'e', "NULL", "1", "'\\x01'::bytea")
        ),
    );
    assert_eq!(again.lines().next_back().unwrap(), "inserted");
    let adopt_hide = format!(
        "SELECT _oo.adopt_target_generation('seg_hide', '{OWNER}'::uuid);\nSELECT _oo.fail_target_generation('seg_hide', '{OWNER}'::uuid, 1);\nSELECT _oo.adopt_target_generation('seg_hide', '{OWNER}'::uuid);"
    );
    let hidden = pg.ok(
        "red-stale-hidden",
        "oo_a092_runtime",
        db,
        &format!(
            "{adopt_hide}\nDO $hide$\nBEGIN\n  BEGIN\n    PERFORM _oo.commit_apply_receipt('seg_hide', '{OWNER}'::uuid, 1, 'warehouse', 'orders_g1', 'seg-hide', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, NULL, 1, '\\x01'::bytea);\n    RAISE EXCEPTION 'v1 stored the stale illegal segment';\n  EXCEPTION\n    WHEN SQLSTATE 'OO013' THEN\n      NULL;\n    WHEN SQLSTATE '23514' THEN\n      RAISE EXCEPTION 'v1 rejected the segment instead of hiding it as OO013';\n  END;\nEND\n$hide$;\nSELECT 'hidden';\n"
        ),
    );
    assert!(hidden.contains("hidden"), "{hidden}");
    pg.note("red history_unchanged=true v2_ready=false stale_illegal=OO013");
}

fn phase_fix(pg: &Pg) {
    let db = "oo_seg_fix";
    pg.ok(
        "fix-fixture",
        "oo_a092_bootstrap",
        db,
        "SET ROLE oo_target_owner;\nCREATE SCHEMA fixture_synth AUTHORIZATION oo_target_owner;\nREVOKE ALL ON SCHEMA fixture_synth FROM PUBLIC;\nGRANT USAGE ON SCHEMA fixture_synth TO oo_pipeline_runtime;\nCREATE TABLE fixture_synth.applied_row (batch_id text PRIMARY KEY, payload_digest text NOT NULL, applied_pos numeric(38, 0) NOT NULL);\nREVOKE ALL ON fixture_synth.applied_row FROM PUBLIC;\nGRANT SELECT, INSERT ON fixture_synth.applied_row TO oo_pipeline_runtime;\nRESET ROLE;\n",
    );
    pg.ok(
        "fix-seed-adopt",
        "oo_a092_runtime",
        db,
        &format!(
            "{}{}{}",
            adopt("shard_lawful"),
            receipt("shard_lawful", 1, "batch-empty", 'e', "NULL", "NULL", "''::bytea"),
            receipt("shard_lawful", 1, "batch-one", 'e', "0", "1", "'\\x01'::bytea")
        ),
    );
    let before = pg.ok("fix-snapshot-before", "oo_fixture_admin", db, snapshot_sql());
    let generations = pg.ok(
        "fix-generations-before",
        "oo_fixture_admin",
        db,
        "SELECT coalesce(string_agg(row(gen.*)::text, E'\\n' ORDER BY shard_id), '') FROM _oo.target_generations AS gen;\n",
    );
    let applied = forward_v2(pg, db);
    let after = pg.ok("fix-snapshot-after", "oo_fixture_admin", db, snapshot_sql());
    let generations_after = pg.ok(
        "fix-generations-after",
        "oo_fixture_admin",
        db,
        "SELECT coalesce(string_agg(row(gen.*)::text, E'\\n' ORDER BY shard_id), '') FROM _oo.target_generations AS gen;\n",
    );
    assert_eq!(before, after, "lawful receipts changed");
    assert_eq!(generations, generations_after, "generations changed");
    pg.note("fix snapshot_byte_equal=true");
    let repeated = pg.ok(
        "fix-repeat",
        "oo_fixture_admin",
        db,
        &format!("BEGIN;\nSET LOCAL lock_timeout = '3s';\nSELECT pg_catalog.pg_advisory_xact_lock(6404202605);\nSET ROLE oo_target_owner;\nSELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\nDO $check$\nDECLARE recorded text;\nBEGIN\n  SELECT checksum INTO recorded FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/2';\n  IF recorded IS DISTINCT FROM '{V2_FILE}' THEN\n    RAISE EXCEPTION 'migration checksum mismatch' USING ERRCODE = 'OO001';\n  END IF;\nEND\n$check$;\nRESET ROLE;\nCOMMIT;\nSELECT applied_at::text FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/2';\n"),
    );
    assert_eq!(applied, repeated.lines().next_back().unwrap());
    pg.expect_state(
        "fix-mismatch",
        "oo_fixture_admin",
        db,
        &format!("BEGIN;\nSET LOCAL lock_timeout = '3s';\nSELECT pg_catalog.pg_advisory_xact_lock(6404202605);\nSET ROLE oo_target_owner;\nSELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\nDO $check$\nDECLARE recorded text;\nBEGIN\n  SELECT checksum INTO recorded FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/2';\n  IF recorded IS DISTINCT FROM '{LIE}' THEN\n    RAISE EXCEPTION 'migration checksum mismatch' USING ERRCODE = 'OO001';\n  END IF;\nEND\n$check$;\nRESET ROLE;\nCOMMIT;\n"),
        "OO001",
    );
    let still = pg.ok(
        "fix-mismatch-row",
        "oo_fixture_admin",
        db,
        "SELECT checksum FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/2';\n",
    );
    assert_eq!(still, V2_FILE);
    matrix(pg, db);
    preserved(pg, db);
    side_and_replay(pg, db);
    locks_and_kill(pg, db);
    catalog(pg, db);
}

fn forward_v2(pg: &Pg, db: &str) -> String {
    let output = pg.psql_named(
        "fix-forward",
        "oo_fixture_admin",
        db,
        &format!(
            "BEGIN;\nSET LOCAL lock_timeout = '5s';\nSELECT pg_catalog.pg_advisory_xact_lock(6404202605);\nSET ROLE oo_target_owner;\nSELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\n{SEGMENT}\nSELECT 'before=' || _oo.segment_contract_ready('{V2_FILE}')::text;\n{}\nSELECT 'after=' || _oo.segment_contract_ready('{V2_FILE}')::text;\nRESET ROLE;\nCOMMIT;\n",
            book_sql(&[("target-fence-receipts/2", V2_FILE, "0002_target_segment_null_expand.sql")])
        ),
    );
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(output.status.success(), "{}", pg.scrub(&String::from_utf8_lossy(&output.stderr)));
    assert!(text.contains("before=false"), "{text}");
    assert!(text.contains("after=true"), "{text}");
    pg.ok(
        "fix-applied-at",
        "oo_fixture_admin",
        db,
        "SELECT applied_at::text FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/2';\n",
    )
}

fn matrix(pg: &Pg, db: &str) {
    let rows = [
        ("cross-absent", "NULL", "NULL", "''::bytea", true),
        ("cross-both-byte", "NULL", "NULL", "'\\x01'::bytea", false),
        ("cross-null-index-empty", "NULL", "1", "''::bytea", false),
        ("cross-null-index-byte", "NULL", "1", "'\\x01'::bytea", false),
        ("cross-null-count-empty", "0", "NULL", "''::bytea", false),
        ("cross-null-count-byte", "0", "NULL", "'\\x01'::bytea", false),
        ("cross-empty-present", "0", "1", "''::bytea", false),
        ("cross-one", "0", "1", "'\\x01'::bytea", true),
        ("null-bitmap", "0", "1", "NULL", false),
        ("bound-8192", "0", "1", "decode(repeat('aa', 8192), 'hex')", true),
        ("bound-8193", "0", "1", "decode(repeat('aa', 8193), 'hex')", false),
        ("bound-last", "8191", "8192", "'\\x01'::bytea", true),
        ("bound-index-eq", "8192", "8192", "'\\x01'::bytea", false),
        ("neg-index", "-1", "1", "'\\x01'::bytea", false),
        ("neg-count", "0", "-1", "'\\x01'::bytea", false),
        ("count-zero", "0", "0", "'\\x01'::bytea", false),
        ("index-eq-count", "1", "1", "'\\x01'::bytea", false),
    ];
    for (name, index, count, bitmap, allow) in rows {
        let shard = format!("m_{name}").replace('-', "_");
        let sql = format!(
            "{}{}",
            adopt(&shard),
            receipt(&shard, 1, name, 'e', index, count, bitmap)
        );
        if allow {
            let got = pg.ok(&format!("fix-{name}"), "oo_a092_runtime", db, &sql);
            assert_eq!(got.lines().next_back().unwrap(), "inserted", "{name}");
            pg.note(&format!("fix {name}=inserted"));
        } else {
            pg.expect_state(&format!("fix-{name}"), "oo_a092_runtime", db, &sql, "23514");
            let stderr = fs::read_to_string(pg.evidence.join("cases").join(format!("fix-{name}.stderr"))).unwrap();
            if bitmap == "NULL" {
                assert!(stderr.contains("closed completion metadata"), "{name} {stderr}");
            } else {
                assert!(stderr.contains("closed segment metadata"), "{name} {stderr}");
                assert!(!stderr.contains("OO013"), "{name} hid behind OO013");
            }
        }
    }
    pg.ok(
        "fix-stale-calls",
        "oo_a092_runtime",
        db,
        &format!(
            "{}\nSELECT _oo.fail_target_generation('shard_stale', '{OWNER}'::uuid, 1);\n{}\nDO $stale$\nBEGIN\n  BEGIN\n    PERFORM _oo.commit_apply_receipt('shard_stale', '{OWNER}'::uuid, 1, 'warehouse', 'orders_g1', 'stale-bad', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, NULL, 1, '\\x01'::bytea);\n    RAISE EXCEPTION 'illegal stale segment was stored';\n  EXCEPTION\n    WHEN SQLSTATE '23514' THEN NULL;\n    WHEN SQLSTATE 'OO013' THEN RAISE EXCEPTION 'OO013 hid the illegal segment';\n  END;\n  BEGIN\n    PERFORM _oo.commit_apply_receipt('shard_stale', '{OWNER}'::uuid, 1, 'warehouse', 'orders_g1', 'stale-legal', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('f', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, NULL, NULL, ''::bytea);\n    RAISE EXCEPTION 'stale legal receipt was stored';\n  EXCEPTION\n    WHEN SQLSTATE 'OO013' THEN NULL;\n  END;\nEND\n$stale$;\n",
            adopt("shard_stale"),
            adopt("shard_stale")
        ),
    );
    let counts = pg.ok(
        "fix-stale-counts",
        "oo_fixture_admin",
        db,
        "SELECT count(*)::text FROM _oo.apply_receipts WHERE txn_or_batch_id IN ('stale-bad', 'stale-legal');\nSELECT generation::text FROM _oo.target_generations WHERE shard_id = 'shard_stale';\n",
    );
    assert!(counts.contains('0'), "{counts}");
    assert!(counts.lines().any(|line| line == "2"), "{counts}");
    pg.note("fix stale_illegal=23514 stale_legal=OO013");
}

fn preserved(pg: &Pg, db: &str) {
    pg.expect_state(
        "fix-runtime-insert",
        "oo_a092_runtime",
        db,
        "INSERT INTO _oo.apply_receipts (source) VALUES ('warehouse');\n",
        "42501",
    );
    pg.ok("fix-codec-adopt", "oo_a092_runtime", db, &adopt("shard_codec"));
    pg.expect_state(
        "fix-codec",
        "oo_a092_runtime",
        db,
        &receipt("shard_codec", 1, "batch-codec", 'e', "NULL", "NULL", "''::bytea").replace("'pg-commit-lsn/v1'", "'nope'"),
        "OO015",
    );
    pg.ok(
        "fix-direct-legal",
        "oo_a092_bootstrap",
        db,
        "BEGIN;\nSET ROLE oo_target_owner;\nSELECT pg_catalog.set_config('_oo.writer', 'apply-receipt', true);\nINSERT INTO _oo.apply_receipts (source, table_generation, txn_or_batch_id, config_version, config_digest, type_policy_version, type_digest, hook_digest, schema_digest, payload_digest, protocol_version, position_codec, position_lineage, source_epoch, applied_pos, partial_txn, cross_table_consistent, txn_complete, segment_index, segment_count, segment_bitmap, fence_shard, fence_generation) VALUES ('warehouse', 'orders_g1', 'direct-legal', 'config/v1', repeat('a', 64), 'type-policy/v1', repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'batch/v1', 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, NULL, NULL, ''::bytea, 'owner_direct', 1);\nROLLBACK;\n",
    );
    pg.expect_state(
        "fix-direct-illegal",
        "oo_a092_bootstrap",
        db,
        "BEGIN;\nSET ROLE oo_target_owner;\nSELECT pg_catalog.set_config('_oo.writer', 'apply-receipt', true);\nINSERT INTO _oo.apply_receipts (source, table_generation, txn_or_batch_id, config_version, config_digest, type_policy_version, type_digest, hook_digest, schema_digest, payload_digest, protocol_version, position_codec, position_lineage, source_epoch, applied_pos, partial_txn, cross_table_consistent, txn_complete, segment_index, segment_count, segment_bitmap, fence_shard, fence_generation) VALUES ('warehouse', 'orders_g1', 'direct-illegal', 'config/v1', repeat('a', 64), 'type-policy/v1', repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'batch/v1', 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, NULL, 1, '\\x01'::bytea, 'owner_direct', 1);\nCOMMIT;\n",
        "23514",
    );
    let direct_err = fs::read_to_string(pg.evidence.join("cases").join("fix-direct-illegal.stderr")).unwrap();
    assert!(
        direct_err.contains("apply_receipts_segment_check"),
        "direct insert did not hit the segment check: {direct_err}"
    );
    let absent = pg.ok(
        "fix-direct-absent",
        "oo_fixture_admin",
        db,
        "SELECT count(*)::text FROM _oo.apply_receipts WHERE txn_or_batch_id IN ('direct-legal', 'direct-illegal');\n",
    );
    assert_eq!(absent, "0");
}

fn side_and_replay(pg: &Pg, db: &str) {
    pg.expect_state(
        "side-reject",
        "oo_a092_runtime",
        db,
        &format!(
            "BEGIN;\nINSERT INTO fixture_synth.applied_row VALUES ('side-bad', repeat('e', 64), 0);\n{}{}COMMIT;\n",
            adopt("shard_side"),
            receipt("shard_side", 1, "side-bad", 'e', "NULL", "1", "'\\x01'::bytea")
        ),
        "23514",
    );
    let gone = pg.ok(
        "side-gone",
        "oo_fixture_admin",
        db,
        "SELECT count(*)::text FROM fixture_synth.applied_row WHERE batch_id = 'side-bad';\nSELECT count(*)::text FROM _oo.target_generations WHERE shard_id = 'shard_side';\n",
    );
    assert_eq!(gone, "0\n0", "{gone}");
    let replay_sql = format!(
        "{}{}SELECT pg_catalog.pg_sleep(2);\n{}",
        adopt("shard_replay"),
        receipt("shard_replay", 1, "batch-replay", 'e', "NULL", "NULL", "''::bytea"),
        receipt("shard_replay", 1, "batch-replay", 'e', "NULL", "NULL", "''::bytea")
    );
    let stdout_path = pg.evidence.join("replay-holder.stdout");
    let stderr_path = pg.evidence.join("replay-holder.stderr");
    let mut holder = Command::new("docker")
        .args([
            "exec", "-i", "-e", "PGPASSFILE=/tmp/pgpass", "-e", "PGAPPNAME=a092-replay", &pg.name, "psql",
            "-U", "oo_a092_runtime", "-d", db, "-h", "127.0.0.1", "-X", "-q", "-t", "-A",
            "-v", "ON_ERROR_STOP=1", "-f", "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::from(fs::File::create(&stdout_path).unwrap()))
        .stderr(Stdio::from(fs::File::create(&stderr_path).unwrap()))
        .spawn()
        .expect("replay holder");
    holder.stdin.take().unwrap().write_all(replay_sql.as_bytes()).unwrap();
    thread::sleep(Duration::from_secs(1));
    let line = pg.ok(
        "replay-line",
        "oo_fixture_admin",
        db,
        "SELECT payload_digest || '~' || fence_generation::text || '~' || target_commit_at::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'batch-replay';\n",
    );
    assert!(!line.is_empty(), "replay seed was not visible");
    thread::sleep(Duration::from_secs(4));
    let status = holder.wait().expect("replay holder wait");
    let holder_out = fs::read_to_string(&stdout_path).unwrap();
    assert!(
        status.success(),
        "replay holder {}\n{holder_out}",
        fs::read_to_string(&stderr_path).unwrap_or_default()
    );
    assert_eq!(holder_out.lines().next_back().unwrap(), "replay", "{holder_out}");
    let again = pg.ok(
        "replay-line-2",
        "oo_fixture_admin",
        db,
        "SELECT payload_digest || '~' || fence_generation::text || '~' || target_commit_at::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'batch-replay';\n",
    );
    assert_eq!(line, again);
    let takeover = pg.ok(
        "replay-takeover",
        "oo_a092_runtime",
        db,
        &format!(
            "{}{}",
            adopt("shard_replay"),
            receipt("shard_replay", 2, "batch-replay", 'e', "NULL", "NULL", "''::bytea")
        ),
    );
    assert_eq!(takeover.lines().next_back().unwrap(), "replay", "{takeover}");
    let kept = pg.ok(
        "replay-kept",
        "oo_fixture_admin",
        db,
        "SELECT payload_digest || '~' || fence_generation::text || '~' || target_commit_at::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'batch-replay';\n",
    );
    assert_eq!(kept, line);
    pg.expect_state(
        "conflict",
        "oo_a092_runtime",
        db,
        &format!(
            "{}BEGIN;\nINSERT INTO fixture_synth.applied_row VALUES ('conflict-side', repeat('f', 64), 1);\n{}COMMIT;\n",
            adopt("shard_replay"),
            receipt("shard_replay", 3, "batch-replay", 'f', "NULL", "NULL", "''::bytea")
        ),
        "OO010",
    );
    let conflict_side = pg.ok(
        "conflict-side",
        "oo_fixture_admin",
        db,
        "SELECT count(*)::text FROM fixture_synth.applied_row WHERE batch_id = 'conflict-side';\n",
    );
    assert_eq!(conflict_side, "0");
    let still = pg.ok(
        "conflict-row",
        "oo_fixture_admin",
        db,
        "SELECT payload_digest || '~' || fence_generation::text || '~' || target_commit_at::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'batch-replay';\n",
    );
    assert_eq!(still, line);
    pg.note("fix replay_immutable=true side_rollback=true conflict=OO010");
}

fn locks_and_kill(pg: &Pg, db: &str) {
    let holder_sql = format!("{}SELECT pg_catalog.pg_sleep(12);\n", adopt("shard_held"));
    let mut holder = Command::new("docker")
        .args([
            "exec", "-i", "-e", "PGPASSFILE=/tmp/pgpass", "-e", "PGAPPNAME=a092-holder", &pg.name, "psql",
            "-U", "oo_a092_runtime", "-d", db, "-h", "127.0.0.1", "-X", "-q", "-v", "ON_ERROR_STOP=1", "-f", "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::from(fs::File::create(pg.evidence.join("holder.stdout")).unwrap()))
        .stderr(Stdio::from(fs::File::create(pg.evidence.join("holder.stderr")).unwrap()))
        .spawn()
        .expect("holder");
    holder.stdin.take().unwrap().write_all(holder_sql.as_bytes()).unwrap();
    thread::sleep(Duration::from_secs(2));
    pg.ok(
        "locks",
        "oo_a092_runtime",
        db,
        &format!(
            "DO $locks$\nBEGIN\n  BEGIN\n    PERFORM _oo.adopt_target_generation('shard_held', '{OWNER}'::uuid);\n    RAISE EXCEPTION 'same shard lock was acquired';\n  EXCEPTION WHEN SQLSTATE '55P03' THEN NULL;\n  END;\n  IF _oo.adopt_target_generation('shard_open', '{OWNER}'::uuid) <> 1 THEN\n    RAISE EXCEPTION 'other shard was not generation 1';\n  END IF;\nEND\n$locks$;\nSELECT 'locks-ok';\n"
        ),
    );
    let _ = holder.kill();
    let _ = holder.wait();
    let mig_sql = "BEGIN;\nSET LOCAL lock_timeout = '5s';\nSELECT pg_catalog.pg_advisory_xact_lock(6404202605);\nSELECT pg_catalog.pg_sleep(10);\nCOMMIT;\n";
    let mut mig = Command::new("docker")
        .args([
            "exec", "-i", "-e", "PGPASSFILE=/tmp/pgpass", "-e", "PGAPPNAME=a092-mig", &pg.name, "psql",
            "-U", "oo_fixture_admin", "-d", db, "-h", "127.0.0.1", "-X", "-q", "-v", "ON_ERROR_STOP=1", "-f", "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::from(fs::File::create(pg.evidence.join("mig.stderr")).unwrap()))
        .spawn()
        .expect("mig");
    mig.stdin.take().unwrap().write_all(mig_sql.as_bytes()).unwrap();
    thread::sleep(Duration::from_secs(2));
    let started = Instant::now();
    pg.expect_state(
        "mig-wait",
        "oo_fixture_admin",
        db,
        "BEGIN;\nSET LOCAL lock_timeout = '2s';\nSELECT pg_catalog.pg_advisory_xact_lock(6404202605);\nCOMMIT;\n",
        "55P03",
    );
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(1500) && elapsed < Duration::from_secs(8), "migration wait {elapsed:?}");
    pg.note(&format!("fix migration_wait_ms={}", elapsed.as_millis()));
    let _ = mig.kill();
    let _ = mig.wait();

    spawn_sleep(pg, db, "a092-kill-before", &format!(
        "SET application_name = 'a092-kill-before';\n{}BEGIN;\n{}SELECT pg_catalog.pg_sleep(30);\nCOMMIT;\n",
        adopt("shard_kill_before"),
        receipt("shard_kill_before", 1, "batch-kill-before", 'e', "NULL", "NULL", "''::bytea")
    ));
    terminate_when_visible(pg, db, "a092-kill-before");
    let before_count = pg.ok("kill-before-count", "oo_fixture_admin", db, "SELECT count(*)::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'batch-kill-before';\n");
    assert_eq!(before_count, "0");
    let retried = pg.ok(
        "kill-before-retry",
        "oo_a092_runtime",
        db,
        &format!("{}{}", adopt("shard_kill_before"), receipt("shard_kill_before", 2, "batch-kill-before", 'e', "NULL", "NULL", "''::bytea")),
    );
    assert_eq!(retried.lines().next_back().unwrap(), "inserted", "{retried}");

    spawn_sleep(pg, db, "a092-kill-after", &format!(
        "SET application_name = 'a092-kill-after';\n{}BEGIN;\n{}COMMIT;\nSELECT pg_catalog.pg_sleep(30);\n",
        adopt("shard_kill_after"),
        receipt("shard_kill_after", 1, "batch-kill-after", 'e', "0", "1", "'\\x01'::bytea")
    ));
    let mut seen = false;
    for wait in [2u64, 4] {
        thread::sleep(Duration::from_secs(wait));
        let count = pg.ok("kill-after-poll", "oo_fixture_admin", db, "SELECT count(*)::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'batch-kill-after';\n");
        if count == "1" {
            seen = true;
            break;
        }
    }
    assert!(seen, "committed receipt was not visible");
    let line = pg.ok("kill-after-line", "oo_fixture_admin", db, "SELECT payload_digest || '~' || fence_generation::text || '~' || target_commit_at::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'batch-kill-after';\n");
    terminate_when_visible(pg, db, "a092-kill-after");
    let after = pg.ok("kill-after-count", "oo_fixture_admin", db, "SELECT count(*)::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'batch-kill-after';\n");
    assert_eq!(after, "1");
    let replay = pg.ok(
        "kill-after-replay",
        "oo_a092_runtime",
        db,
        &format!("{}{}", adopt("shard_kill_after"), receipt("shard_kill_after", 2, "batch-kill-after", 'e', "0", "1", "'\\x01'::bytea")),
    );
    assert_eq!(replay.lines().next_back().unwrap(), "replay", "{replay}");
    let kept = pg.ok("kill-after-kept", "oo_fixture_admin", db, "SELECT payload_digest || '~' || fence_generation::text || '~' || target_commit_at::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'batch-kill-after';\n");
    assert_eq!(kept, line);
    pg.note("fix locks=55P03/other_shard kill_before=0 kill_after=1");
}

fn spawn_sleep(pg: &Pg, db: &str, name: &str, sql: &str) {
    let mut child = Command::new("docker")
        .args([
            "exec", "-i", "-e", "PGPASSFILE=/tmp/pgpass", "-e", &format!("PGAPPNAME={name}"), &pg.name, "psql",
            "-U", "oo_a092_runtime", "-d", db, "-h", "127.0.0.1", "-X", "-q", "-v", "ON_ERROR_STOP=1", "-f", "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::from(fs::File::create(pg.evidence.join(format!("{name}.stdout"))).unwrap()))
        .stderr(Stdio::from(fs::File::create(pg.evidence.join(format!("{name}.stderr"))).unwrap()))
        .spawn()
        .expect(name);
    child.stdin.take().unwrap().write_all(sql.as_bytes()).unwrap();
    // Detach the child. terminate_when_visible stops the backend; the process exits.
    thread::spawn(move || {
        let _ = child.wait();
    });
    thread::sleep(Duration::from_secs(2));
}

fn terminate_when_visible(pg: &Pg, db: &str, app: &str) {
    let mut pid = String::new();
    for wait in [0u64, 3] {
        if wait > 0 {
            thread::sleep(Duration::from_secs(wait));
        }
        pid = pg.ok(
            &format!("pid-{app}"),
            "oo_fixture_admin",
            db,
            &format!("SELECT coalesce(pid::text, '') FROM pg_stat_activity WHERE application_name = '{app}' AND pid <> pg_backend_pid() LIMIT 1;\n"),
        );
        if !pid.is_empty() {
            break;
        }
    }
    assert!(!pid.is_empty(), "backend {app} did not appear");
    let terminated = pg.ok(
        &format!("term-{app}"),
        "oo_fixture_admin",
        db,
        &format!("SELECT pg_terminate_backend({pid})::text;\n"),
    );
    assert_eq!(terminated, "true", "{app}");
}

fn catalog(pg: &Pg, db: &str) {
    pg.ok(
        "catalog",
        "oo_fixture_admin",
        db,
        "DO $cat$\nDECLARE def text; cfg text;\nBEGIN\n  SELECT pg_get_constraintdef(oid) INTO def FROM pg_constraint WHERE conname = 'apply_receipts_segment_check' AND conrelid = '_oo.apply_receipts'::regclass;\n  IF def NOT LIKE '%IS TRUE%' OR def NOT LIKE '%IS NOT NULL%' THEN\n    RAISE EXCEPTION 'segment check is not the closed predicate: %', def;\n  END IF;\n  SELECT proconfig::text INTO cfg FROM pg_proc JOIN pg_namespace ON pg_namespace.oid = pronamespace WHERE nspname = '_oo' AND proname = 'commit_apply_receipt';\n  IF cfg NOT LIKE '%search_path=pg_catalog, _oo%' THEN\n    RAISE EXCEPTION 'commit_apply_receipt proconfig %', cfg;\n  END IF;\n  SELECT proconfig::text INTO cfg FROM pg_proc JOIN pg_namespace ON pg_namespace.oid = pronamespace WHERE nspname = '_oo' AND proname = 'segment_contract_ready';\n  IF cfg NOT LIKE '%search_path=pg_catalog, _oo%' THEN\n    RAISE EXCEPTION 'segment_contract_ready proconfig %', cfg;\n  END IF;\n  IF pg_has_role('oo_pipeline_runtime', 'oo_target_owner', 'MEMBER') THEN\n    RAISE EXCEPTION 'runtime is a member of the owner';\n  END IF;\n  IF NOT has_function_privilege('oo_pipeline_runtime', '_oo.segment_contract_ready(text)', 'EXECUTE') THEN\n    RAISE EXCEPTION 'runtime lacks segment_contract_ready';\n  END IF;\n  IF has_function_privilege('public', '_oo.segment_contract_ready(text)', 'EXECUTE') THEN\n    RAISE EXCEPTION 'public can execute segment_contract_ready';\n  END IF;\n  IF has_table_privilege('oo_pipeline_runtime', '_oo.apply_receipts', 'INSERT') THEN\n    RAISE EXCEPTION 'runtime can insert receipts';\n  END IF;\n  IF has_table_privilege('oo_pipeline_runtime', '_oo.segment_service_readiness', 'SELECT') IS NOT TRUE THEN\n    RAISE EXCEPTION 'runtime cannot read segment readiness';\n  END IF;\nEND\n$cat$;\nSELECT 'catalog-ok';\n",
    );
    pg.note("fix catalog=constraints/proconfig/acl");
}

fn phase_fresh_checks(pg: &Pg) {
    let db = "oo_seg_fresh";
    pg.expect_state(
        "fresh-both",
        "oo_a092_runtime",
        db,
        &format!("{}{}", adopt("fresh_both"), receipt("fresh_both", 1, "fresh-both", 'e', "NULL", "NULL", "'\\x01'::bytea")),
        "23514",
    );
    let absent = pg.ok("fresh-absent", "oo_fixture_admin", db, "SELECT count(*)::text FROM _oo.apply_receipts;\n");
    assert_eq!(absent, "0");
    pg.note("fresh illegal=23514");
}

fn phase_rollback(pg: &Pg) {
    let db = "oo_seg_roll";
    pg.expect_state(
        "roll",
        "oo_fixture_admin",
        db,
        &format!(
            "BEGIN;\nSET LOCAL lock_timeout = '5s';\nSELECT pg_catalog.pg_advisory_xact_lock(6404202605);\nSET ROLE oo_target_owner;\nSELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\n{SEGMENT}\nDO $fail$ BEGIN RAISE EXCEPTION 'forced migration failure' USING ERRCODE = 'OO003'; END $fail$;\nRESET ROLE;\nCOMMIT;\n"
        ),
        "OO003",
    );
    let marker = pg.ok("roll-marker", "oo_fixture_admin", db, "SELECT EXISTS (SELECT 1 FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/2')::text;\n");
    assert_eq!(marker, "false");
    let probe = "SELECT pg_get_functiondef('_oo.commit_apply_receipt(text,uuid,bigint,text,text,text,text,text,text,text,text,text,text,integer,numeric,boolean,boolean,boolean,integer,integer,bytea)'::regprocedure);\n";
    let body = pg.ok("roll-body", "oo_fixture_admin", db, probe);
    assert!(!body.contains("IS NOT TRUE"), "rollback left the new function");
    let ready = pg.ok("roll-v1", "oo_fixture_admin", db, &format!("SELECT _oo.schema_ready('{V1_EXPAND}')::text;\n"));
    assert_eq!(ready, "true");
    assert!(!driver_requiring_v2_authorized(true, false));
    pg.note("rollback=OO003 v2_ready=false v1_function_restored=true");
}

fn phase_lie(pg: &Pg) {
    let db = "oo_seg_lie";
    let output = pg.psql_named(
        "lie-apply",
        "oo_fixture_admin",
        db,
        &format!(
            "BEGIN;\nSET LOCAL lock_timeout = '5s';\nSELECT pg_catalog.pg_advisory_xact_lock(6404202605);\nSET ROLE oo_target_owner;\nSELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\n{SEGMENT}\n{}\nRESET ROLE;\nCOMMIT;\n",
            book_sql(&[("target-fence-receipts/2", LIE, "0002_target_segment_null_expand.sql")])
        ),
    );
    assert!(output.status.success(), "{}", pg.scrub(&String::from_utf8_lossy(&output.stderr)));
    let flags = pg.ok(
        "lie-flags",
        "oo_a092_runtime",
        db,
        &format!(
            "SELECT _oo.schema_ready('{V1_EXPAND}')::text || '~' || _oo.segment_contract_ready('{LIE}')::text || '~' || _oo.segment_contract_ready('{V2_FILE}')::text || '~' || (SELECT schema_version_present::text FROM _oo.segment_service_readiness);\n"
        ),
    );
    assert_eq!(flags, "true~true~false~true", "{flags}");
    let v1_ready = true;
    let v2_ready_for_file = false;
    assert!(!driver_requiring_v2_authorized(v1_ready, v2_ready_for_file));
    pg.note("lie stored_checksum_is_not_file_hash v1_ready_does_not_authorize=true");
}

fn provision() -> Pg {
    let root = crate_root();
    let evidence = std::env::var("OO_SEGMENT_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            root.join("../../evidence/eef831aa-dba4-4af2-a78b-541bc9e0a092/SP04/segment-null-correction/pg")
        });
    fs::create_dir_all(&evidence).expect("evidence");
    let name = format!("oo-a092-pg1710-{RUN_ID}");
    let net = format!("oo-a092-pg-{RUN_ID}-net");
    let vol = format!("oo-a092-pg-{RUN_ID}-data");
    let secrets = PathBuf::from("/tmp/oo-a092-eef831aa-dba4-4af2-a78b-541bc9e0a092/pg-secrets");
    fs::create_dir_all(&secrets).expect("secrets");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&secrets, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let admin_password = secret();
    let runtime_password = secret();
    let bootstrap_password = secret();
    fs::write(secrets.join("admin_password"), &admin_password).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(secrets.join("admin_password"), fs::Permissions::from_mode(0o600)).unwrap();
    }
    let existing = Command::new("docker")
        .args(["ps", "-a", "--filter", "label=oo.owner=a092", "--filter", "label=oo.kind=postgres", "--format", "{{.Names}}"])
        .output()
        .expect("docker ps");
    let names = String::from_utf8_lossy(&existing.stdout);
    assert!(names.trim().is_empty(), "a092 postgres already exists: {names}");
    let image = Command::new("docker").args(["image", "inspect", PG_IMAGE]).output().expect("image");
    assert!(image.status.success(), "pinned postgres image is not present locally; not pulling");
    let _ = Command::new("docker").args(["network", "create", "--label", "oo.project=open-ontology", "--label", "oo.owner=a092", "--label", "oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a092", "--label", &format!("oo.run_id={RUN_ID}"), "--label", "oo.kind=postgres", &net]).status();
    let _ = Command::new("docker").args(["volume", "create", "--label", "oo.project=open-ontology", "--label", "oo.owner=a092", "--label", "oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a092", "--label", &format!("oo.run_id={RUN_ID}"), "--label", "oo.kind=postgres", &vol]).status();
    let run = Command::new("docker")
        .args([
            "run", "-d", "--name", &name, "--platform", "linux/arm64", "--pull", "never", "--init",
            "--security-opt", "no-new-privileges:true", "--cap-drop", "ALL",
            "--cap-add", "CHOWN", "--cap-add", "DAC_OVERRIDE", "--cap-add", "FOWNER", "--cap-add", "SETGID", "--cap-add", "SETUID",
            "--memory", "1g", "--cpus", "1", "--pids-limit", "256", "--network", &net,
            "--tmpfs", "/tmp:rw,nosuid,nodev,size=256m",
            "--mount", &format!("type=volume,source={vol},target=/var/lib/postgresql/data"),
            "--mount", &format!("type=bind,source={},target=/run/secrets,readonly", secrets.display()),
            "-e", "POSTGRES_USER=oo_fixture_admin", "-e", "POSTGRES_DB=postgres",
            "-e", "POSTGRES_PASSWORD_FILE=/run/secrets/admin_password",
            "-e", "POSTGRES_INITDB_ARGS=--data-checksums", "-e", "POSTGRES_HOST_AUTH_METHOD=scram-sha-256",
            "--label", "oo.project=open-ontology", "--label", "oo.owner=a092",
            "--label", "oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a092",
            "--label", "oo.task=segment-null-correction", "--label", &format!("oo.run_id={RUN_ID}"),
            "--label", "oo.scope=sp04-segment-null", "--label", "oo.kind=postgres", "--label", "oo.platform=linux/arm64",
            PG_IMAGE, "-c", "listen_addresses=*", "-c", "max_connections=40", "-c", "shared_preload_libraries=",
        ])
        .output()
        .expect("docker run");
    if !run.status.success() {
        let _ = Command::new("docker").args(["rm", "-f", &name]).status();
        let _ = Command::new("docker").args(["network", "rm", &net]).status();
        let _ = Command::new("docker").args(["volume", "rm", &vol]).status();
        panic!("postgres start {}", String::from_utf8_lossy(&run.stderr));
    }
    fs::write(evidence.join("container-name.txt"), &name).unwrap();
    fs::write(evidence.join("image-ref.txt"), PG_IMAGE).unwrap();
    let pg = Pg {
        name,
        net,
        vol,
        evidence,
        admin_password,
        runtime_password,
        bootstrap_password,
        cleanup: false,
    };
    pg.install_pgpass();
    let mut ready = false;
    for wait in [2u64, 4, 8, 8] {
        thread::sleep(Duration::from_secs(wait));
        let probe = Command::new("docker")
            .args(["exec", &pg.name, "pg_isready", "-U", "oo_fixture_admin"])
            .output()
            .expect("pg_isready");
        if probe.status.success() {
            ready = true;
            break;
        }
    }
    assert!(ready, "postgres did not become ready");
    let host = pg.docker(&["inspect", &pg.name, "--format", "privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} security={{json .HostConfig.SecurityOpt}}"]);
    fs::write(pg.evidence.join("hostconfig.txt"), &host.stdout).unwrap();
    let host_text = String::from_utf8_lossy(&host.stdout);
    assert!(host_text.contains("privileged=false"), "{host_text}");
    assert!(host_text.contains("ports={}"), "{host_text}");
    pg
}

impl Pg {
    fn remove(&mut self) {
        if self.cleanup {
            return;
        }
        self.cleanup = true;
        let rm = self.docker(&["rm", "-f", &self.name]);
        let net = self.docker(&["network", "rm", &self.net]);
        let vol = self.docker(&["volume", "rm", &self.vol]);
        let text = format!(
            "rm_exit={}\nnetwork_exit={}\nvolume_exit={}\n",
            rm.status.code().unwrap_or(127),
            net.status.code().unwrap_or(127),
            vol.status.code().unwrap_or(127)
        );
        let _ = fs::write(self.evidence.join("cleanup.txt"), text);
    }
}
