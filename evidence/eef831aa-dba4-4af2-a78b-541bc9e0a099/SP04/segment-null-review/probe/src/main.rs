#![forbid(unsafe_code)]
//! Reviewer-owned PostgreSQL 17.10 probe for the committed segment correction.
//! It executes the archive's SQL bytes. It is not the author's test harness.

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const COMMIT: &str = "9a71efaf9cb63a2d63061a241f6b8ee8b00426df";
const SHA_0000: &str = "37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208";
const SHA_0001: &str = "97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9";
const SHA_0002: &str = "59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512";
const LIE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const OWNER_A: &str = "11111111-1111-4111-8111-111111111111";
const OWNER_B: &str = "22222222-2222-4222-8222-222222222222";
const IMAGE: &str =
    "docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb";
const OO021: &str = "refusing target segment correction: preexisting apply receipts violate the closed segment rule; this migration does not delete, rewrite, or mutate receipts";
const SESSION: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a099";

#[derive(Clone, Copy)]
enum Bmp {
    Null,
    Empty,
    One,
    N8192,
    N8193,
}

struct Outcome {
    name: String,
    kind: &'static str,
    pass: bool,
    detail: String,
}

struct App {
    evidence: PathBuf,
    container: String,
    network: String,
    volume: String,
    run_id: String,
    secrets: PathBuf,
    admin_pw: String,
    runtime_pw: String,
    bootstrap_pw: String,
    nobody_pw: String,
    boot: String,
    expand: String,
    segment: String,
    roles_exist: bool,
    logins_exist: bool,
    outcomes: Vec<Outcome>,
}

fn main() {
    let mut args = env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| "run".to_string());
    let root = PathBuf::from("/tmp/oo-a099-eef831aa-dba4-4af2-a78b-541bc9e0a099");
    let evidence = PathBuf::from(
        "/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a099/SP04/segment-null-review/pg",
    );
    if cmd == "cleanup" {
        let code = cleanup(&evidence, &root);
        std::process::exit(code);
    }
    if cmd != "run" {
        eprintln!("usage: probe run|cleanup");
        std::process::exit(2);
    }
    match run(root, evidence) {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("harness: {err}");
            std::process::exit(2);
        }
    }
}

fn run(root: PathBuf, evidence: PathBuf) -> Result<i32, String> {
    fs::create_dir_all(evidence.join("cases")).map_err(|e| e.to_string())?;
    fs::create_dir_all(evidence.join("sql")).map_err(|e| e.to_string())?;
    let snapshot = root.join("snapshot");
    if !snapshot.join("crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql").is_file() {
        return Err("fixed archive snapshot is not on disk".into());
    }
    let boot = read_string(&snapshot.join("crates/oo-pipeline/migrations/0000_bootstrap_roles.sql"))?;
    let expand = read_string(&snapshot.join("crates/oo-pipeline/migrations/0001_target_fence_expand.sql"))?;
    let segment = read_string(&snapshot.join("crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql"))?;
    expect_hash(&snapshot.join("crates/oo-pipeline/migrations/0000_bootstrap_roles.sql"), SHA_0000)?;
    expect_hash(&snapshot.join("crates/oo-pipeline/migrations/0001_target_fence_expand.sql"), SHA_0001)?;
    expect_hash(&snapshot.join("crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql"), SHA_0002)?;
    let v1_json = read_string(&snapshot.join("contracts/pipeline/target-fence-receipts/target-fence-receipts-v1.json"))?;
    let v1_orig = Command::new("git")
        .args(["show", "83704982a8d9459c51ed7c2020c51d21c91fb8cd:contracts/pipeline/target-fence-receipts/target-fence-receipts-v1.json"])
        .current_dir("/Users/redux/puppy-one/open-ontology")
        .output()
        .map_err(|e| e.to_string())?;
    if !v1_orig.status.success() || v1_orig.stdout != v1_json.as_bytes() {
        return Err("v1 contract bytes differ from 83704982".into());
    }
    capacity_postgres()?;
    let run_id = uuid_v4();
    let container = format!("oo-a099-pg1710-{run_id}");
    let network = format!("oo-a099-pg-{run_id}-net");
    let volume = format!("oo-a099-pg-{run_id}-data");
    let secrets = root.join("secrets");
    fs::create_dir_all(&secrets).map_err(|e| e.to_string())?;
    let admin_pw = random_hex(24);
    let runtime_pw = random_hex(24);
    let bootstrap_pw = random_hex(24);
    let nobody_pw = random_hex(24);
    write_secret(&secrets.join("admin_password"), &admin_pw)?;
    let mut app = App {
        evidence,
        container,
        network,
        volume,
        run_id,
        secrets,
        admin_pw,
        runtime_pw,
        bootstrap_pw,
        nobody_pw,
        boot,
        expand,
        segment,
        roles_exist: false,
        logins_exist: false,
        outcomes: Vec::new(),
    };
    app.write_resources();
    if let Err(err) = app.provision() {
        app.note("provision", "harness", false, &err);
        let _ = app.finish();
        return Ok(2);
    }
    app.exercise();
    app.finish()
}

impl App {
    fn write_resources(&self) {
        let text = format!(
            "run_id={}\ncontainer={}\nnetwork={}\nvolume={}\nimage={}\ncommit={}\n",
            self.run_id, self.container, self.network, self.volume, IMAGE, COMMIT
        );
        let _ = fs::write(self.evidence.join("resources.txt"), text);
    }

    fn provision(&mut self) -> Result<(), String> {
        let before = docker_out(&["ps", "-a", "--filter", "label=oo.kind=postgres", "--format", "{{.Names}} {{.Status}} {{.Label \"oo.owner\"}}"])?;
        fs::write(self.evidence.join("postgres-before.txt"), before).map_err(|e| e.to_string())?;
        docker_ok(&[
            "network", "create",
            "--label", "oo.project=open-ontology",
            "--label", "oo.owner=a099",
            "--label", &format!("oo.session={SESSION}"),
            "--label", &format!("oo.run_id={}", self.run_id),
            "--label", "oo.kind=postgres",
            &self.network,
        ])?;
        docker_ok(&["volume", "create", "--label", "oo.project=open-ontology", "--label", "oo.owner=a099", "--label", &format!("oo.session={SESSION}"), "--label", &format!("oo.run_id={}", self.run_id), "--label", "oo.kind=postgres", &self.volume])?;
        let id = docker_out(&[
            "run", "-d",
            "--name", &self.container,
            "--platform", "linux/arm64",
            "--pull", "never",
            "--init",
            "--security-opt", "no-new-privileges:true",
            "--cap-drop", "ALL",
            "--cap-add", "CHOWN",
            "--cap-add", "DAC_OVERRIDE",
            "--cap-add", "FOWNER",
            "--cap-add", "SETGID",
            "--cap-add", "SETUID",
            "--memory", "1g",
            "--cpus", "1",
            "--pids-limit", "256",
            "--network", &self.network,
            "--tmpfs", "/tmp:rw,nosuid,nodev,size=256m",
            "--mount", &format!("type=volume,source={},target=/var/lib/postgresql/data", self.volume),
            "--mount", &format!("type=bind,source={},target=/run/secrets,readonly", self.secrets.display()),
            "-e", "POSTGRES_USER=oo_a099_admin",
            "-e", "POSTGRES_DB=postgres",
            "-e", "POSTGRES_PASSWORD_FILE=/run/secrets/admin_password",
            "-e", "POSTGRES_INITDB_ARGS=--data-checksums",
            "-e", "POSTGRES_HOST_AUTH_METHOD=scram-sha-256",
            "--label", "oo.project=open-ontology",
            "--label", "oo.owner=a099",
            "--label", &format!("oo.session={SESSION}"),
            "--label", "oo.task=segment-null-review",
            "--label", &format!("oo.run_id={}", self.run_id),
            "--label", "oo.kind=postgres",
            "--label", "oo.platform=linux/arm64",
            IMAGE,
            "-c", "listen_addresses=*",
            "-c", "max_connections=50",
        ])?;
        fs::write(self.evidence.join("container.id"), &id).map_err(|e| e.to_string())?;
        let host = docker_out(&["inspect", &self.container, "--format", "privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}} security={{json .HostConfig.SecurityOpt}}"])?;
        fs::write(self.evidence.join("hostconfig.txt"), host).map_err(|e| e.to_string())?;
        let env = docker_out(&["inspect", &self.container, "--format", "{{range .Config.Env}}{{println .}}{{end}}"])?;
        let keys = env.lines().filter_map(|line| line.split('=').next()).collect::<Vec<_>>().join("\n");
        fs::write(self.evidence.join("env-keys.txt"), keys + "\n").map_err(|e| e.to_string())?;
        let image_id = docker_out(&["inspect", &self.container, "--format", "{{.Image}}"])?;
        fs::write(self.evidence.join("image-id.txt"), image_id).map_err(|e| e.to_string())?;
        self.wait_ready()?;
        self.install_pgpass()?;
        let version = self.psql("version", "oo_a099_admin", "postgres", "SELECT version();\nSHOW data_checksums;\n", true);
        let text = version.stdout.clone() + &version.stderr;
        self.note("version", "require", version.exit == 0 && text.contains("PostgreSQL 17.10"), &trim_detail(&text));
        self.note("checksums", "require", text.lines().any(|line| line.trim().eq_ignore_ascii_case("on")), &trim_detail(&text));
        if version.exit != 0 || !text.contains("PostgreSQL 17.10") {
            return Err(format!("postgres did not report 17.10: {}", trim_detail(&text)));
        }
        Ok(())
    }

    fn wait_ready(&self) -> Result<(), String> {
        for attempt in 1..=40 {
            let out = Command::new("docker").args(["exec", &self.container, "pg_isready", "-U", "oo_a099_admin", "-d", "postgres"]).output().map_err(|e| e.to_string())?;
            if out.status.success() {
                fs::write(self.evidence.join("ready.txt"), format!("attempt={attempt}\n")).map_err(|e| e.to_string())?;
                return Ok(());
            }
            thread::sleep(Duration::from_secs(2));
        }
        Err("postgres was not ready".into())
    }

    fn install_pgpass(&self) -> Result<(), String> {
        let body = format!(
            "*:*:*:oo_a099_admin:{}\n*:*:*:oo_a099_runtime:{}\n*:*:*:oo_a099_bootstrap:{}\n*:*:*:oo_a099_nobody:{}\n",
            self.admin_pw, self.runtime_pw, self.bootstrap_pw, self.nobody_pw
        );
        let mut child = Command::new("docker")
            .args(["exec", "-i", "-u", "0", &self.container, "tee", "/tmp/a099.pgpass"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        child.stdin.as_mut().unwrap().write_all(body.as_bytes()).map_err(|e| e.to_string())?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!("pgpass tee failed: {}", String::from_utf8_lossy(&out.stderr)));
        }
        docker_ok(&["exec", "-u", "0", &self.container, "chown", "postgres:postgres", "/tmp/a099.pgpass"])?;
        docker_ok(&["exec", "-u", "0", &self.container, "chmod", "600", "/tmp/a099.pgpass"])?;
        Ok(())
    }

    fn exercise(&mut self) {
        self.create_db("db_red");
        if !self.install("db_red", Install::V1) {
            self.note("install-v1", "harness", false, "v1 install failed; later product cases were not run");
            return;
        }
        self.create_logins();
        self.grant_connect("db_red");
        self.phase_red();
        self.phase_single("db_idx", "null-index", None, Some(1), Bmp::One);
        self.phase_single("db_count", "null-count", Some(0), None, Bmp::One);
        self.phase_single("db_both", "null-both", None, None, Bmp::One);
        self.phase_hist();
        self.phase_bypass();
        self.phase_lawful();
        self.phase_fresh();
        self.phase_repeat();
        self.phase_rollback();
        self.phase_lie();
        self.phase_missing();
        self.phase_shape();
        self.phase_regress();
    }

    fn create_db(&mut self, name: &str) {
        let sql = format!("CREATE DATABASE {name};\n");
        let run = self.psql(&format!("create-{name}"), "oo_a099_admin", "postgres", &sql, true);
        self.note(&format!("create-{name}"), "harness", run.exit == 0, &trim_detail(&run.stderr));
    }

    fn grant_connect(&mut self, db: &str) {
        let sql = format!("GRANT CONNECT ON DATABASE {db} TO oo_a099_bootstrap;\n");
        let run = self.psql(&format!("grant-{db}"), "oo_a099_admin", "postgres", &sql, true);
        self.note(&format!("grant-{db}"), "harness", run.exit == 0, &trim_detail(&run.stderr));
    }

    fn create_logins(&mut self) {
        let sql = format!(
            "CREATE ROLE oo_a099_bootstrap LOGIN PASSWORD '{b}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;\nCREATE ROLE oo_a099_runtime LOGIN PASSWORD '{r}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION;\nCREATE ROLE oo_a099_nobody LOGIN PASSWORD '{n}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;\nGRANT oo_target_owner TO oo_a099_bootstrap WITH INHERIT FALSE, SET TRUE;\nGRANT oo_pipeline_runtime TO oo_a099_runtime WITH INHERIT TRUE, SET FALSE;\nALTER ROLE oo_a099_bootstrap SET search_path = pg_catalog;\nALTER ROLE oo_a099_runtime SET search_path = pg_catalog;\nALTER ROLE oo_a099_nobody SET search_path = pg_catalog;\n",
            b = self.bootstrap_pw, r = self.runtime_pw, n = self.nobody_pw
        );
        let redacted = sql.replace(&self.bootstrap_pw, "<redacted>").replace(&self.runtime_pw, "<redacted>").replace(&self.nobody_pw, "<redacted>");
        let _ = fs::write(self.evidence.join("sql/create-logins.sql"), redacted);
        let run = self.psql_raw("create-logins", "oo_a099_admin", "postgres", &sql);
        self.note("create-logins", "harness", run.exit == 0, &self.scrub(&trim_detail(&run.stderr)));
        if run.exit == 0 {
            self.logins_exist = true;
        }
    }

    fn install(&mut self, db: &str, how: Install) -> bool {
        let upgrade = matches!(how, Install::V2Upgrade | Install::V2ThenFail);
        let mut sql = String::from("BEGIN;\nSET LOCAL lock_timeout = '5s';\nSELECT pg_catalog.pg_advisory_xact_lock(6404202605);\n");
        if upgrade {
            sql.push_str("SET ROLE oo_target_owner;\nSELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\n");
        } else {
            sql.push_str(&bootstrap_text(&self.boot, self.roles_exist));
            sql.push_str("SET ROLE oo_target_owner;\n");
            if !matches!(how, Install::BootstrapOnly) {
                sql.push_str(&self.expand);
                sql.push('\n');
                sql.push_str("SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\n");
            }
        }
        if matches!(how, Install::V2 | Install::V2Upgrade | Install::V2Lie | Install::V2ThenFail) {
            sql.push_str(&self.segment);
            sql.push('\n');
            sql.push_str(&format!("SELECT 'READY_BEFORE|' || _oo.segment_contract_ready('{SHA_0002}')::text;\n"));
        }
        if matches!(how, Install::V2ThenFail) {
            sql.push_str("DO $fail$ BEGIN RAISE EXCEPTION 'forced migration failure' USING ERRCODE = 'OO003'; END $fail$;\n");
        }
        let mut rows = Vec::new();
        if !upgrade && !matches!(how, Install::BootstrapOnly) {
            rows.push(("target-bootstrap/1", SHA_0000, "0000_bootstrap_roles.sql"));
            rows.push(("target-fence-receipts/1", SHA_0001, "0001_target_fence_expand.sql"));
        }
        if matches!(how, Install::V2 | Install::V2Upgrade) {
            rows.push(("target-fence-receipts/2", SHA_0002, "0002_target_segment_null_expand.sql"));
        }
        if matches!(how, Install::V2Lie) {
            rows.push(("target-fence-receipts/2", LIE, "0002_target_segment_null_expand.sql"));
        }
        if !rows.is_empty() {
            sql.push_str(&book_sql(&rows));
        }
        if matches!(how, Install::V2 | Install::V2Upgrade) {
            sql.push_str(&format!("SELECT 'READY_AFTER|' || _oo.segment_contract_ready('{SHA_0002}')::text;\nSELECT 'V1_READY|' || _oo.schema_ready('{SHA_0001}')::text;\nSELECT 'V1_OF_V2|' || _oo.schema_ready('{SHA_0002}')::text;\n"));
        }
        if matches!(how, Install::V2Lie) {
            sql.push_str(&format!("SELECT 'LIE|' || _oo.segment_contract_ready('{LIE}')::text || '|' || _oo.segment_contract_ready('{SHA_0002}')::text || '|' || _oo.schema_ready('{SHA_0001}')::text || '|' || _oo.schema_ready('{SHA_0002}')::text;\n"));
        }
        sql.push_str("RESET ROLE;\n");
        if self.logins_exist {
            sql.push_str(&format!("GRANT CONNECT ON DATABASE {db} TO oo_a099_bootstrap;\n"));
        }
        sql.push_str("COMMIT;\n");
        let expect_fail = matches!(how, Install::V2ThenFail);
        let run = self.psql(&format!("install-{db}"), "oo_a099_admin", db, &sql, true);
        let ok = if expect_fail {
            run.state.as_deref() == Some("OO003")
        } else {
            run.exit == 0
        };
        self.note(&format!("install-{db}"), if expect_fail { "require" } else { "harness" }, ok, &format!("exit={} state={} {}", run.exit, run.state.unwrap_or_default(), trim_detail(&run.stderr)));
        if ok && !matches!(how, Install::BootstrapOnly) {
            self.roles_exist = true;
        }
        if ok && matches!(how, Install::BootstrapOnly) {
            self.roles_exist = true;
        }
        if ok && matches!(how, Install::V2 | Install::V2Upgrade) {
            let text = run.stdout.clone();
            self.note(&format!("ready-before-{db}"), "require", text.contains("READY_BEFORE|false"), &trim_detail(&text));
            self.note(&format!("ready-after-{db}"), "require", text.contains("READY_AFTER|true"), &trim_detail(&text));
            self.note(&format!("v1-ready-{db}"), "require", text.contains("V1_READY|true") && text.contains("V1_OF_V2|false"), &trim_detail(&text));
        }
        if ok && matches!(how, Install::V2Lie) {
            let text = run.stdout.clone();
            self.note("lie-flags", "require", text.contains(&format!("LIE|true|false|true|false")), &trim_detail(&text));
        }
        ok
    }

    fn phase_red(&mut self) {
        let db = "db_red";
        for (name, index, count, bmp) in [
            ("red-null-index", None, Some(1), Bmp::One),
            ("red-null-count", Some(0), None, Bmp::One),
            ("red-null-both", None, None, Bmp::One),
        ] {
            let run = self.call_receipt(db, "oo_a099_runtime", &format!("a099{name}").replace('-', ""), name, 1, index, count, bmp, 'e');
            let stored = run.stdout.contains("RESULT|inserted") && run.exit == 0;
            self.note(name, "require", stored, &format!("v1 stored the historical NULL mixture exit={} {}", run.exit, trim_detail(&run.stdout)));
        }
        let hide = self.script(db, "oo_a099_runtime", "red-stale-hide", &format!("{}\n{}\n", adopt("a099hide", OWNER_A), receipt("a099hide", 99, "hide-batch", 'e', None, Some(1), Bmp::One)));
        self.note("red-stale-hide", "require", hide.state.as_deref() == Some("OO013"), &format!("v1 still reports stale fence for the illegal segment state={}", hide.state.unwrap_or_default()));
        let before = self.snapshot(db, "red-snap-before");
        let forward = self.forward_expect_oo021(db, "red-forward");
        let after = self.snapshot(db, "red-snap-after");
        self.note("red-history-equal", "require", forward && before == after && !before.is_empty(), "snapshot bytes including target_commit_at stayed equal and OO021 aborted");
        let red_marker = self.marker_count(db);
        let red_fn = self.function_has(db, "IS NOT TRUE");
        let red_ck = self.constraint_has(db, "IS TRUE");
        self.note("red-no-v2-marker", "require", red_marker == Some(0), "no target-fence-receipts/2 row");
        self.note("red-function-unchanged", "require", !red_fn, "v1 function body has no IS NOT TRUE");
        self.note("red-check-unchanged", "require", !red_ck, "v1 check has no IS TRUE");
    }

    fn phase_single(&mut self, db: &str, label: &str, index: Option<i32>, count: Option<i32>, bmp: Bmp) {
        self.create_db(db);
        if !self.install(db, Install::V1) {
            return;
        }
        let shard = format!("a099{label}").replace('-', "");
        let run = self.call_receipt(db, "oo_a099_runtime", &shard, label, 1, index, count, bmp, 'e');
        self.note(&format!("{label}-stored"), "require", run.exit == 0 && run.stdout.contains("RESULT|inserted"), &trim_detail(&run.stdout));
        let before = self.snapshot(db, &format!("{label}-before"));
        let forward = self.forward_expect_oo021(db, &format!("{label}-forward"));
        let after = self.snapshot(db, &format!("{label}-after"));
        self.note(&format!("{label}-isolated"), "require", forward && before == after, "one illegal shape cannot be skipped by an earlier failure");
    }

    fn phase_hist(&mut self) {
        let db = "db_hist";
        self.create_db(db);
        if !self.install(db, Install::V1) {
            return;
        }
        let shapes = [
            ("h-null-index", None, Some(1), Bmp::One),
            ("h-null-count", Some(0), None, Bmp::One),
            ("h-null-both", None, None, Bmp::One),
            ("h-null-count-2", Some(4), None, Bmp::One),
            ("h-both-8192", None, None, Bmp::N8192),
        ];
        for (name, index, count, bmp) in shapes {
            let shard = format!("a099{name}").replace('-', "");
            let run = self.call_receipt(db, "oo_a099_runtime", &shard, name, 1, index, count, bmp, 'e');
            self.note(&format!("hist-plant-{name}"), "require", run.exit == 0 && run.stdout.contains("RESULT|inserted"), &trim_detail(&(run.stdout.clone() + &run.stderr)));
        }
        let before = self.snapshot(db, "hist-before");
        let count_before = self.receipt_count(db);
        let forward = self.forward_expect_oo021(db, "hist-forward");
        let after = self.snapshot(db, "hist-after");
        let message_same = self.case_stderr("hist-forward").contains(OO021);
        let hist_marker = self.marker_count(db);
        let hist_fn = self.function_has(db, "IS NOT TRUE");
        self.note("hist-all-equal", "require", forward && before == after && count_before == Some(5) && message_same, "every planted illegal row stayed; OO021 message is the fixed text");
        self.note("hist-no-delete", "require", hist_marker == Some(0) && !hist_fn, "migration rolled back");
    }

    fn phase_bypass(&mut self) {
        let db = "db_bypass";
        self.create_db(db);
        if !self.install(db, Install::V1) {
            return;
        }
        let mut sql = String::from("BEGIN;\nSET ROLE oo_target_owner;\nSELECT pg_catalog.set_config('_oo.writer', 'apply-receipt', true);\nALTER TABLE _oo.apply_receipts DROP CONSTRAINT apply_receipts_segment_check;\n");
        let false_shapes = [
            ("by-eq", Some(1), Some(1), Bmp::One),
            ("by-zero", Some(0), Some(0), Bmp::One),
            ("by-neg-i", Some(-1), Some(1), Bmp::One),
            ("by-neg-c", Some(0), Some(-1), Bmp::One),
            ("by-8193", Some(0), Some(1), Bmp::N8193),
            ("by-empty", Some(0), Some(1), Bmp::Empty),
            ("by-edge-eq", Some(8192), Some(8192), Bmp::One),
            ("by-null-i-zero", None, Some(0), Bmp::One),
            ("by-both-8193", None, None, Bmp::N8193),
            ("by-null-index", None, Some(1), Bmp::One),
        ];
        for (name, index, count, bmp) in false_shapes {
            sql.push_str(&direct_insert(name, index, count, bmp));
        }
        sql.push_str("ALTER TABLE _oo.apply_receipts ADD CONSTRAINT apply_receipts_segment_check CHECK (( (segment_count IS NULL AND segment_index IS NULL AND octet_length(segment_bitmap) = 0) OR (segment_count >= 1 AND segment_index >= 0 AND segment_index < segment_count AND octet_length(segment_bitmap) BETWEEN 1 AND 8192) )) NOT VALID;\nCOMMIT;\n");
        let planted = self.psql("bypass-plant", "oo_a099_admin", db, &sql, true);
        self.note("bypass-plant", "harness", planted.exit == 0, &trim_detail(&planted.stderr));
        if planted.exit != 0 {
            return;
        }
        let before = self.snapshot(db, "bypass-before");
        let forward = self.forward_expect_oo021(db, "bypass-forward");
        let after = self.snapshot(db, "bypass-after");
        let still = self.psql("bypass-validated", "oo_a099_admin", db, "SELECT convalidated::text || '|' || pg_get_constraintdef(oid) FROM pg_constraint WHERE conname = 'apply_receipts_segment_check';\n", true);
        let unchanged = before == after && still.stdout.contains("false|") && !still.stdout.contains("IS TRUE");
        let bypass_marker = self.marker_count(db);
        self.note("bypass-all-fields", "require", forward && unchanged && bypass_marker == Some(0), "false and unknown planted rows all survived OO021; check stayed the v1 NOT VALID form");
    }

    fn phase_lawful(&mut self) {
        let db = "db_lawful";
        self.create_db(db);
        if !self.install(db, Install::V1) {
            return;
        }
        let rows = [
            ("law-absent", None, None, Bmp::Empty, false),
            ("law-one", Some(0), Some(1), Bmp::One, false),
            ("law-8192", Some(8191), Some(8192), Bmp::N8192, false),
            ("law-partial", None, None, Bmp::Empty, true),
        ];
        for (name, index, count, bmp, partial) in rows {
            let shard = format!("a099{name}").replace('-', "");
            let run = if partial {
                self.call_partial(db, &shard, name)
            } else {
                self.call_receipt(db, "oo_a099_runtime", &shard, name, 1, index, count, bmp, 'e')
            };
            self.note(&format!("law-plant-{name}"), "require", run.exit == 0 && run.stdout.contains("RESULT|inserted"), &trim_detail(&(run.stdout + &run.stderr)));
        }
        let before = self.snapshot(db, "law-before");
        if !self.install(db, Install::V2Upgrade) {
            self.note("law-upgrade", "require", false, "v2 upgrade failed");
            return;
        }
        let after = self.snapshot(db, "law-after");
        self.note("law-byte-equal", "require", before == after && before.matches('\n').count() >= 3, "generation, payload, and target_commit_at stayed byte-equal");
        // The planting connection has exited, so this acquisition increments the shard.
        // Replay must pass that new generation. The stored fence_generation stays 1.
        let replay = self.psql("law-replay", "oo_a099_runtime", db, &receipt_at_adopted("a099lawone", "law-one", 'e', Some(0), Some(1), Bmp::One), true);
        let after_replay = self.snapshot(db, "law-after-replay");
        self.note("law-replay-same-bytes", "require", replay.stdout.contains("RESULT|replay") && after_replay == after, &trim_detail(&replay.stdout));
    }

    fn phase_fresh(&mut self) {
        let db = "db_fresh";
        self.create_db(db);
        if !self.install(db, Install::V2) {
            self.note("fresh-install", "require", false, "fresh v2 install failed");
            return;
        }
        self.three_value(db);
        self.fresh_named(db);
        self.fresh_grid(db);
        self.fresh_stale(db);
        self.fresh_catalog(db);
        self.fresh_direct(db);
        self.fresh_poison(db);
        self.fresh_acl(db);
        self.fresh_dml_gap(db);
    }

    fn three_value(&mut self, db: &str) {
        let expr = "( (segment_count IS NULL AND segment_index IS NULL AND octet_length(segment_bitmap) = 0) OR (segment_count >= 1 AND segment_index >= 0 AND segment_index < segment_count AND octet_length(segment_bitmap) BETWEEN 1 AND 8192) )";
        let samples = [
            ("mix-index", "NULL::integer, 1, '\\x01'::bytea"),
            ("mix-count", "0, NULL::integer, '\\x01'::bytea"),
            ("mix-both", "NULL::integer, NULL::integer, '\\x01'::bytea"),
            ("null-bitmap", "NULL::integer, NULL::integer, NULL::bytea"),
        ];
        let mut sql = format!("CREATE TEMP TABLE old_seg (segment_index integer, segment_count integer, segment_bitmap bytea);\nALTER TABLE old_seg ADD CONSTRAINT old_seg_check CHECK ({expr});\nCREATE TEMP TABLE new_seg (segment_index integer, segment_count integer, segment_bitmap bytea);\nALTER TABLE new_seg ADD CONSTRAINT new_seg_check CHECK (({expr}) IS TRUE);\nCREATE TEMP TABLE tv_result (line text);\n");
        for (name, values) in samples {
            sql.push_str(&format!("INSERT INTO old_seg VALUES ({values});\nSELECT 'OLD|{name}|' || count(*)::text FROM old_seg;\nDELETE FROM old_seg;\n"));
        }
        sql.push_str("DO $tv$\nBEGIN\n");
        for (name, values) in samples {
            sql.push_str(&format!("  BEGIN\n    INSERT INTO new_seg VALUES ({values});\n    INSERT INTO tv_result VALUES ('accepted|{name}');\n  EXCEPTION WHEN SQLSTATE '23514' THEN\n    INSERT INTO tv_result VALUES ('rejected|{name}');\n  END;\n"));
        }
        sql.push_str("END\n$tv$;\nSELECT line FROM tv_result ORDER BY line;\n");
        let run = self.psql("three-value", "oo_a099_admin", db, &sql, true);
        let old_ok = run.stdout.matches("OLD|").count() == 4 && !run.stdout.contains("|0");
        self.note("three-value-old-accepts-unknown", "require", old_ok && run.stdout.contains("OLD|null-bitmap|1"), &trim_detail(&run.stdout));
        let rejected = ["mix-index", "mix-count", "mix-both", "null-bitmap"].iter().all(|name| run.stdout.contains(&format!("rejected|{name}")));
        self.note("three-value-is-true-rejects", "require", rejected && !run.stdout.contains("accepted|"), &trim_detail(&run.stdout));
        let flags = self.psql("three-flags", "oo_a099_admin", db, "SELECT (( (NULL IS NULL AND NULL IS NULL AND octet_length('\\x01'::bytea) = 0) OR (1 >= 1 AND NULL >= 0 AND NULL < 1 AND octet_length('\\x01'::bytea) BETWEEN 1 AND 8192) ) IS NULL)::text || '|' || (( (NULL IS NULL AND NULL IS NULL AND octet_length('\\x01'::bytea) = 0) OR (1 >= 1 AND NULL >= 0 AND NULL < 1 AND octet_length('\\x01'::bytea) BETWEEN 1 AND 8192) ) IS TRUE)::text;\n", true);
        self.note("three-value-unknown-is-not-true", "require", flags.stdout.trim() == "true|false", &trim_detail(&flags.stdout));
    }

    fn fresh_named(&mut self, db: &str) {
        let named = [
            ("fresh-null-index", None, Some(1), Bmp::One),
            ("fresh-null-count", Some(0), None, Bmp::One),
            ("fresh-null-both", None, None, Bmp::One),
            ("fresh-absent", None, None, Bmp::Empty),
            ("fresh-one", Some(0), Some(1), Bmp::One),
            ("fresh-8192", Some(8191), Some(8192), Bmp::N8192),
            ("fresh-8193", Some(0), Some(1), Bmp::N8193),
            ("fresh-neg-index", Some(-1), Some(1), Bmp::One),
            ("fresh-neg-count", Some(0), Some(-1), Bmp::One),
            ("fresh-count0", Some(0), Some(0), Bmp::One),
            ("fresh-index-eq", Some(1), Some(1), Bmp::One),
            ("fresh-empty-present", Some(0), Some(1), Bmp::Empty),
            ("fresh-null-bitmap", None, None, Bmp::Null),
        ];
        for (name, index, count, bmp) in named {
            self.one_fresh(db, name, index, count, bmp);
        }
    }

    fn fresh_grid(&mut self, db: &str) {
        let indexes = [None, Some(0), Some(-1), Some(1), Some(8191), Some(8192)];
        let counts = [None, Some(0), Some(1), Some(-1), Some(8192), Some(8193)];
        let bmps = [Bmp::Null, Bmp::Empty, Bmp::One, Bmp::N8192, Bmp::N8193];
        let mut n = 0u32;
        let mut failed = 0u32;
        for index in indexes {
            for count in counts {
                for bmp in bmps {
                    n += 1;
                    let name = format!("grid-{n:03}");
                    if !self.one_fresh(db, &name, index, count, bmp) {
                        failed += 1;
                    }
                }
            }
        }
        self.note("grid-complete", "require", failed == 0, &format!("cases={n} failed={failed}"));
    }

    fn one_fresh(&mut self, db: &str, name: &str, index: Option<i32>, count: Option<i32>, bmp: Bmp) -> bool {
        let shard = format!("f{name}").replace('-', "");
        let run = self.call_receipt(db, "oo_a099_runtime", &shard, name, 1, index, count, bmp, 'e');
        let legal = closed_legal(index, count, bmp);
        let pass = if legal {
            run.exit == 0 && run.stdout.contains("RESULT|inserted")
        } else if matches!(bmp, Bmp::Null) {
            run.state.as_deref() == Some("23514") && run.stderr.contains("closed completion metadata") && !run.stdout.contains("RESULT|inserted")
        } else {
            run.state.as_deref() == Some("23514") && run.stderr.contains("closed segment metadata") && !run.stdout.contains("RESULT|inserted")
        };
        let detail = format!("legal={legal} state={} {}", run.state.unwrap_or_default(), first_error(&run.stderr));
        self.note(name, "require", pass, &detail);
        pass
    }

    fn fresh_stale(&mut self, db: &str) {
        let bad = self.script(db, "oo_a099_runtime", "stale-illegal", &format!("{}\n{}\n", adopt("a099stale", OWNER_A), receipt("a099stale", 99, "stale-bad", 'e', None, Some(1), Bmp::One)));
        self.note("stale-illegal-23514", "require", bad.state.as_deref() == Some("23514") && bad.stderr.contains("closed segment metadata"), &format!("state={}", bad.state.unwrap_or_default()));
        let good = self.script(db, "oo_a099_runtime", "stale-legal", &format!("{}\n{}\n", adopt("a099stalel", OWNER_A), receipt("a099stalel", 99, "stale-good", 'e', None, None, Bmp::Empty)));
        self.note("stale-legal-OO013", "require", good.state.as_deref() == Some("OO013"), &format!("state={}", good.state.unwrap_or_default()));
        let absent = self.receipt_count_batch(db, "stale-bad") == Some(0) && self.receipt_count_batch(db, "stale-good") == Some(0);
        self.note("stale-not-stored", "require", absent, "neither stale call left a receipt");
    }

    fn fresh_catalog(&mut self, db: &str) {
        let sql = r#"SELECT 'proc|' || p.proname || '|' || r.rolname || '|' || p.prosecdef::text || '|' || coalesce(p.proconfig::text, '')
FROM pg_proc p
JOIN pg_roles r ON r.oid = p.proowner
JOIN pg_namespace n ON n.oid = p.pronamespace
WHERE n.nspname = '_oo' AND p.proname IN ('commit_apply_receipt', 'segment_contract_ready')
ORDER BY p.proname;
SELECT 'check|' || pg_get_constraintdef(oid) FROM pg_constraint WHERE conname = 'apply_receipts_segment_check';
SELECT 'member_login|' || pg_has_role('oo_a099_runtime', 'oo_target_owner', 'member')::text;
SELECT 'member_runtime|' || pg_has_role('oo_pipeline_runtime', 'oo_target_owner', 'member')::text;
SELECT 'exec_commit|' || has_function_privilege('oo_a099_runtime', '_oo.commit_apply_receipt(text,uuid,bigint,text,text,text,text,text,text,text,text,text,text,integer,numeric,boolean,boolean,boolean,integer,integer,bytea)', 'EXECUTE')::text;
SELECT 'exec_ready|' || has_function_privilege('oo_a099_runtime', '_oo.segment_contract_ready(text)', 'EXECUTE')::text;
SELECT 'receipt_acl|' || has_table_privilege('oo_a099_runtime', '_oo.apply_receipts', 'SELECT')::text
  || '|' || has_table_privilege('oo_a099_runtime', '_oo.apply_receipts', 'INSERT')::text
  || '|' || has_table_privilege('oo_a099_runtime', '_oo.apply_receipts', 'UPDATE')::text
  || '|' || has_table_privilege('oo_a099_runtime', '_oo.apply_receipts', 'DELETE')::text;
SELECT 'readfile|' || position('pg_read_file' in pg_get_functiondef('_oo.segment_contract_ready(text)'::regprocedure))::text;
SELECT 'grant_in_body|' || position('GRANT oo_target_owner TO oo_pipeline_runtime' in pg_get_functiondef('_oo.commit_apply_receipt(text,uuid,bigint,text,text,text,text,text,text,text,text,text,text,integer,numeric,boolean,boolean,boolean,integer,integer,bytea)'::regprocedure))::text;
SELECT 'view|' || r.rolname || '|' || coalesce(c.reloptions::text, '')
FROM pg_class c
JOIN pg_roles r ON r.oid = c.relowner
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = '_oo' AND c.relname = 'segment_service_readiness';
"#;
        let run = self.psql("catalog", "oo_a099_admin", db, sql, true);
        let text = run.stdout.clone();
        let definer = text.contains("proc|commit_apply_receipt|oo_target_owner|true|") && text.contains("search_path=pg_catalog, _oo");
        let invoker = text.contains("proc|segment_contract_ready|oo_target_owner|false|") && text.contains("search_path=pg_catalog, _oo");
        let check_ok = text.contains("check|") && text.contains("IS TRUE");
        let members = text.contains("member_login|false") && text.contains("member_runtime|false");
        let exec_ok = text.contains("exec_commit|true") && text.contains("exec_ready|true");
        let no_dml = text.contains("receipt_acl|false|false|false|false");
        let no_read = text.contains("readfile|0") && text.contains("grant_in_body|0");
        let view_ok = text.contains("view|oo_target_owner|") && text.contains("security_invoker=true");
        self.note("catalog-owner-definer", "require", run.exit == 0 && definer && invoker && check_ok && exec_ok && no_read && view_ok, &trim_detail(&text));
        self.note("runtime-not-owner-member", "require", run.exit == 0 && members && no_dml, &trim_detail(&text));
    }

    fn fresh_direct(&mut self, db: &str) {
        let illegal = format!("BEGIN;\nSET ROLE oo_target_owner;\nSELECT pg_catalog.set_config('_oo.writer', 'apply-receipt', true);\n{}COMMIT;\n", direct_insert("direct-illegal", None, Some(1), Bmp::One));
        let run = self.psql("direct-illegal", "oo_a099_admin", db, &illegal, true);
        self.note("direct-check-23514", "require", run.state.as_deref() == Some("23514") && run.stderr.contains("apply_receipts_segment_check"), &first_error(&run.stderr));
        let null_bmp = format!("BEGIN;\nSET ROLE oo_target_owner;\nSELECT pg_catalog.set_config('_oo.writer', 'apply-receipt', true);\n{}COMMIT;\n", direct_insert("direct-null-bmp", None, None, Bmp::Null));
        let null_run = self.psql("direct-null-bitmap", "oo_a099_admin", db, &null_bmp, true);
        self.note("column-null-bitmap-23502", "require", null_run.state.as_deref() == Some("23502"), &format!("real column is NOT NULL so a NULL bitmap is not a stored history row; state={}", null_run.state.unwrap_or_default()));
        let legal = "BEGIN;\nSET ROLE oo_target_owner;\nSELECT pg_catalog.set_config('_oo.writer', 'apply-receipt', true);\nINSERT INTO _oo.apply_receipts (source, table_generation, txn_or_batch_id, config_version, config_digest, type_policy_version, type_digest, hook_digest, schema_digest, payload_digest, protocol_version, position_codec, position_lineage, source_epoch, applied_pos, partial_txn, cross_table_consistent, txn_complete, segment_index, segment_count, segment_bitmap, fence_shard, fence_generation) VALUES ('warehouse', 'orders_g1', 'direct-legal', 'config/v1', repeat('a', 64), 'type-policy/v1', repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'batch/v1', 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, NULL, NULL, ''::bytea, 'ownerlegal', 1);\nSELECT 'RESULT|owner-legal';\nROLLBACK;\n";
        let ok = self.psql("owner-legal-rollback", "oo_a099_admin", db, legal, true);
        let stored = self.receipt_count_batch(db, "direct-legal") == Some(0);
        self.note("owner-guc-insert-still-possible", "residual", ok.exit == 0 && ok.stdout.contains("RESULT|owner-legal") && stored, "A085-F2 remains: owner plus writer GUC can insert; this probe rolled it back");
        let runtime = format!("INSERT INTO _oo.apply_receipts (source, table_generation, txn_or_batch_id, config_version, config_digest, type_policy_version, type_digest, hook_digest, schema_digest, payload_digest, protocol_version, position_codec, position_lineage, source_epoch, applied_pos, partial_txn, cross_table_consistent, txn_complete, segment_bitmap, fence_shard, fence_generation) VALUES ('warehouse', 'orders_g1', 'runtime-direct', 'config/v1', repeat('a', 64), 'type-policy/v1', repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'batch/v1', 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, ''::bytea, 'runtimedirect', 1);\n");
        let denied = self.psql("runtime-insert", "oo_a099_runtime", db, &runtime, true);
        self.note("runtime-insert-42501", "require", denied.state.as_deref() == Some("42501"), &format!("state={}", denied.state.unwrap_or_default()));
    }

    fn fresh_poison(&mut self, db: &str) {
        let setup = "CREATE SCHEMA IF NOT EXISTS a099_evil;\nCREATE OR REPLACE FUNCTION a099_evil.clock_timestamp() RETURNS timestamptz LANGUAGE sql AS $$ SELECT '1999-01-01 00:00:00+00'::timestamptz $$;\nCREATE OR REPLACE FUNCTION _oo.clock_timestamp() RETURNS timestamptz LANGUAGE sql AS $$ SELECT '1998-01-01 00:00:00+00'::timestamptz $$;\nCREATE OR REPLACE FUNCTION _oo.octet_length(segment_bitmap bytea) RETURNS integer LANGUAGE sql AS $$ SELECT 0 $$;\n";
        let made = self.psql("poison-setup", "oo_a099_admin", db, setup, true);
        self.note("poison-setup", "harness", made.exit == 0, &trim_detail(&made.stderr));
        let sql = format!("SET search_path = a099_evil, public, pg_catalog;\n{}\n{}\n", adopt("a099poison", OWNER_A), receipt("a099poison", 1, "poison-batch", 'e', Some(0), Some(1), Bmp::One));
        let run = self.psql("poison-call", "oo_a099_runtime", db, &sql, true);
        let row = self.psql("poison-row", "oo_a099_admin", db, "SELECT target_commit_at::text FROM _oo.apply_receipts WHERE txn_or_batch_id = 'poison-batch';\n", true);
        let year_ok = row.stdout.contains("2026") && !row.stdout.contains("1999") && !row.stdout.contains("1998");
        self.note("search-path-poison", "require", run.exit == 0 && run.stdout.contains("RESULT|inserted") && year_ok, &trim_detail(&row.stdout));
        let _ = self.psql("poison-drop", "oo_a099_admin", db, "DROP FUNCTION _oo.clock_timestamp();\nDROP FUNCTION _oo.octet_length(bytea);\n", true);
    }

    fn fresh_acl(&mut self, db: &str) {
        let nobody = self.psql("nobody-connect", "oo_a099_nobody", db, "SELECT 1;\n", true);
        self.note("nobody-connect-refused", "require", nobody.exit != 0, &format!("state={} {}", nobody.state.unwrap_or_default(), first_error(&nobody.stderr)));
        let ready = self.psql("runtime-ready", "oo_a099_runtime", db, &format!("SELECT _oo.schema_ready('{SHA_0001}')::text || '|' || _oo.schema_ready('{SHA_0002}')::text || '|' || _oo.segment_contract_ready('{SHA_0002}')::text || '|' || (SELECT schema_version_present::text FROM _oo.segment_service_readiness);\n"), true);
        self.note("runtime-readiness-conjunction", "require", ready.stdout.contains("true|false|true|true"), &trim_detail(&ready.stdout));
        let codec = self.script(db, "oo_a099_runtime", "bad-codec", &format!("{}\nSELECT 'RESULT|' || _oo.commit_apply_receipt('a099codec', '{OWNER_A}'::uuid, 1, 'warehouse', 'orders_g1', 'bad-codec', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'not-a-codec', 'src_main', 1, 0, false, true, true, NULL, NULL, ''::bytea);\n", adopt("a099codec", OWNER_A)));
        self.note("bad-codec-OO015", "require", codec.state.as_deref() == Some("OO015"), &format!("state={}", codec.state.unwrap_or_default()));
    }

    fn fresh_dml_gap(&mut self, db: &str) {
        let setup = "CREATE TABLE public.fixture_future (id integer PRIMARY KEY, note text);\nGRANT INSERT, SELECT ON public.fixture_future TO oo_pipeline_runtime;\n";
        let made = self.psql("future-setup", "oo_a099_admin", db, setup, true);
        let ins = self.psql("future-insert", "oo_a099_runtime", db, "INSERT INTO public.fixture_future VALUES (1, 'unfenced');\nSELECT count(*)::text FROM public.fixture_future;\n", true);
        let pass = made.exit == 0 && ins.exit == 0 && ins.stdout.contains('1');
        self.note("arbitrary-dml-gap", "residual", pass, "A085-F3 remains: a runtime member inserted a user row with no adopt and no receipt");
        let pubs = self.psql("publications", "oo_a099_admin", db, "SELECT count(*)::text FROM pg_publication;\n", true);
        self.note("no-publication", "residual", pubs.stdout.trim() == "0", "receipt schema is not CDC");
    }

    fn phase_repeat(&mut self) {
        let db = "db_repeat";
        self.create_db(db);
        if !self.install(db, Install::V2) {
            return;
        }
        let first = self.applied_at(db);
        let again = self.psql("repeat-protocol", "oo_a099_admin", db, &format!("BEGIN;\nSET LOCAL lock_timeout = '3s';\nSELECT pg_catalog.pg_advisory_xact_lock(6404202605);\nSET ROLE oo_target_owner;\nSELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\nDO $check$ DECLARE recorded text; BEGIN SELECT checksum INTO recorded FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/2'; IF recorded IS DISTINCT FROM '{SHA_0002}' THEN RAISE EXCEPTION 'migration checksum mismatch' USING ERRCODE = 'OO001'; END IF; END $check$;\nRESET ROLE;\nCOMMIT;\n"), true);
        let second = self.applied_at(db);
        self.note("repeat-same-checksum", "require", again.exit == 0 && first.is_some() && first == second, &format!("applied_at stable {}", first.clone().unwrap_or_default()));
        let mismatch = self.psql("mismatch-protocol", "oo_a099_admin", db, &format!("BEGIN;\nSET LOCAL lock_timeout = '3s';\nSELECT pg_catalog.pg_advisory_xact_lock(6404202605);\nSET ROLE oo_target_owner;\nSELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\nDO $check$ DECLARE recorded text; BEGIN SELECT checksum INTO recorded FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/2'; IF recorded IS DISTINCT FROM '{LIE}' THEN RAISE EXCEPTION 'migration checksum mismatch' USING ERRCODE = 'OO001'; END IF; END $check$;\nRESET ROLE;\nCOMMIT;\n"), true);
        let still = self.applied_at(db);
        let body = self.function_has(db, "IS NOT TRUE");
        self.note("mismatch-OO001", "require", mismatch.state.as_deref() == Some("OO001") && still == first && body, "applicator protocol raised OO001 and did not execute 0002 again");
        let update = self.psql("checksum-immutable", "oo_a099_admin", db, &format!("UPDATE _oo.schema_migrations SET checksum = '{LIE}' WHERE version = 'target-fence-receipts/2';\n"), true);
        self.note("checksum-update-OO014", "require", update.state.as_deref() == Some("OO014"), &format!("state={}", update.state.unwrap_or_default()));
        let rerun = self.psql("reexec-file", "oo_a099_admin", db, &format!("BEGIN;\nSET ROLE oo_target_owner;\n{}\nCOMMIT;\n", self.segment), true);
        let after = self.applied_at(db);
        self.note("file-not-idempotent", "require", rerun.exit != 0 && after == first, &format!("state={} {}", rerun.state.unwrap_or_default(), first_error(&rerun.stderr)));
    }

    fn phase_rollback(&mut self) {
        let db = "db_roll";
        self.create_db(db);
        if !self.install(db, Install::V1) {
            return;
        }
        let run = self.call_receipt(db, "oo_a099_runtime", "a099roll", "roll-row", 1, None, None, Bmp::Empty, 'e');
        self.note("roll-plant", "require", run.exit == 0, &trim_detail(&run.stdout));
        let before = self.snapshot(db, "roll-before");
        let failed = self.install(db, Install::V2ThenFail);
        let after = self.snapshot(db, "roll-after");
        let restored = !self.function_has(db, "IS NOT TRUE") && !self.constraint_has(db, "IS TRUE") && self.marker_count(db) == Some(0) && before == after;
        self.note("forced-OO003-restores", "require", failed && restored, "constraint, function, marker, and history restored");
    }

    fn phase_lie(&mut self) {
        let db = "db_lie";
        self.create_db(db);
        if !self.install(db, Install::V2Lie) {
            return;
        }
        let view = self.psql("lie-view", "oo_a099_runtime", db, "SELECT checksum || '|' || schema_version_present::text FROM _oo.segment_service_readiness;\n", true);
        let file_hash_ready = view.stdout.contains(&format!("{LIE}|true"));
        self.note("lie-view-does-not-read-file", "require", file_hash_ready, &trim_detail(&view.stdout));
        self.note("external-conjunction-open", "residual", true, "segment_contract_ready compared the stored lie and returned true; the file hash is not read; the installer must hash 0002 outside the database");
    }

    fn phase_missing(&mut self) {
        let db = "db_miss";
        self.create_db(db);
        if !self.install(db, Install::BootstrapOnly) {
            return;
        }
        let sql = format!("BEGIN;\nSET ROLE oo_target_owner;\n{}\nCOMMIT;\n", self.segment);
        let run = self.psql("missing-v1", "oo_a099_admin", db, &sql, true);
        self.note("missing-v1-OO002", "require", run.state.as_deref() == Some("OO002"), &format!("state={}", run.state.unwrap_or_default()));
    }

    fn phase_shape(&mut self) {
        let db = "db_shape";
        self.create_db(db);
        if !self.install(db, Install::V1) {
            return;
        }
        let drop = self.psql("shape-drop", "oo_a099_admin", db, "SET ROLE oo_target_owner;\nALTER TABLE _oo.apply_receipts DROP CONSTRAINT apply_receipts_segment_check;\nRESET ROLE;\n", true);
        self.note("shape-drop", "harness", drop.exit == 0, &trim_detail(&drop.stderr));
        let sql = format!("BEGIN;\nSET LOCAL lock_timeout = '5s';\nSELECT pg_catalog.pg_advisory_xact_lock(6404202605);\nSET ROLE oo_target_owner;\nSELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\n{}\nRESET ROLE;\nCOMMIT;\n", self.segment);
        let run = self.psql("shape-forward", "oo_a099_admin", db, &sql, true);
        let shape_marker = self.marker_count(db);
        let shape_fn = self.function_has(db, "IS NOT TRUE");
        let shape_state = run.state.clone().unwrap_or_default();
        self.note("invalid-shape-OO002", "require", shape_state == "OO002" && shape_marker == Some(0) && !shape_fn, &format!("state={shape_state}"));
    }

    fn phase_regress(&mut self) {
        let db = "db_regress";
        self.create_db(db);
        if !self.install(db, Install::V2) {
            return;
        }
        let setup = "CREATE TABLE public.side_note (id integer PRIMARY KEY, note text);\nGRANT INSERT, SELECT ON public.side_note TO oo_pipeline_runtime;\n";
        let made = self.psql("side-setup", "oo_a099_admin", db, setup, true);
        self.note("side-setup", "harness", made.exit == 0, &trim_detail(&made.stderr));
        let holder_sql = format!(
            "SET application_name = 'a099replay';\n{}\n{}\nSELECT pg_sleep(8);\n{}\n",
            adopt("a099replay", OWNER_A),
            receipt("a099replay", 1, "replay-batch", 'e', Some(0), Some(1), Bmp::One),
            receipt("a099replay", 1, "replay-batch", 'e', Some(0), Some(1), Bmp::One)
        );
        let child = self.spawn(db, "oo_a099_runtime", &holder_sql);
        let sleeping = self.wait_query("a099replay", "pg_sleep", 15);
        let mid = self.psql("regress-mid", "oo_a099_admin", db, &stamp_select("replay-batch"), true);
        let mid_stamp = stamp_values(&mid.stdout).first().cloned().unwrap_or_default();
        let held_out = child.wait_with_output();
        let held_stdout = held_out.as_ref().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        let held_stderr = held_out.as_ref().map(|o| String::from_utf8_lossy(&o.stderr).to_string()).unwrap_or_default();
        let _ = fs::write(self.evidence.join("cases/regress-same.stdout"), &held_stdout);
        let _ = fs::write(self.evidence.join("cases/regress-same.stderr"), &held_stderr);
        let end = self.psql("regress-end", "oo_a099_admin", db, &stamp_select("replay-batch"), true);
        let end_stamp = stamp_values(&end.stdout).first().cloned().unwrap_or_default();
        let same_ok = sleeping
            && held_stdout.contains("RESULT|inserted")
            && held_stdout.contains("RESULT|replay")
            && !mid_stamp.is_empty()
            && mid_stamp == end_stamp;
        self.note("replay-immutable", "require", same_ok, &format!("mid={mid_stamp} end={end_stamp} sleeping={sleeping}"));
        let held = end_stamp;
        let cross = self.psql("regress-cross", "oo_a099_runtime", db, &receipt_at_adopted("a099replay", "replay-batch", 'e', Some(0), Some(1), Bmp::One), true);
        let cross_row = self.psql("regress-cross-row", "oo_a099_admin", db, &stamp_select("replay-batch"), true);
        let cross_stamp = stamp_values(&cross_row.stdout).first().cloned().unwrap_or_default();
        self.note(
            "replay-new-generation",
            "require",
            cross.stdout.contains("RESULT|replay") && cross_stamp == held && !held.is_empty(),
            &format!("new-backend stamp={cross_stamp} held={held} state={}", cross.state.unwrap_or_default()),
        );
        let conflict = format!(
            "SELECT _oo.adopt_target_generation('a099replay', '{OWNER_A}'::uuid) AS gen \\gset\nBEGIN;\nINSERT INTO public.side_note VALUES (1, 'side');\nSELECT 'RESULT|' || _oo.commit_apply_receipt('a099replay', '{OWNER_A}'::uuid, :gen, 'warehouse', 'orders_g1', 'replay-batch', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('f', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, 0, 1, '\\x01'::bytea);\nCOMMIT;\n"
        );
        let conf = self.psql("conflict", "oo_a099_runtime", db, &conflict, true);
        let side = self.psql("conflict-side", "oo_a099_admin", db, "SELECT count(*)::text FROM public.side_note;\n", true);
        let after = self.psql("conflict-stamp", "oo_a099_admin", db, &stamp_select("replay-batch"), true);
        self.note(
            "conflict-rolls-back-together",
            "require",
            conf.state.as_deref() == Some("OO010") && side.stdout.trim() == "0" && stamp_values(&after.stdout).first().map(String::as_str) == Some(held.as_str()),
            &format!("state={} side={}", conf.state.unwrap_or_default(), side.stdout.trim()),
        );
        self.locks_and_takeover(db);
        self.migration_wait(db);
        self.kills(db);
    }

    fn locks_and_takeover(&mut self, db: &str) {
        let holder_sql = format!("SET application_name = 'a099hold';\n{}\nSELECT pg_sleep(20);\n", adopt("a099lock", OWNER_A));
        let mut child = self.spawn(db, "oo_a099_runtime", &holder_sql);
        let visible = self.wait_app("a099hold", 20);
        let same = self.script(db, "oo_a099_runtime", "same-shard", &format!("{}\n", adopt("a099lock", OWNER_B)));
        let other = self.call_receipt(db, "oo_a099_runtime", "a099other", "other-shard", 1, None, None, Bmp::Empty, 'e');
        self.note("same-shard-55P03", "require", visible && same.state.as_deref() == Some("55P03"), &format!("visible={visible} state={}", same.state.unwrap_or_default()));
        self.note("different-shard-admitted", "require", other.exit == 0 && other.stdout.contains("RESULT|inserted"), &trim_detail(&other.stdout));
        let _ = self.psql("stop-holder", "oo_a099_admin", db, "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name = 'a099hold' AND pid <> pg_backend_pid();\n", true);
        let _ = child.wait();
        let old = self.call_receipt(db, "oo_a099_runtime", "a099dead", "dead-batch", 1, None, None, Bmp::Empty, 'e');
        thread::sleep(Duration::from_millis(200));
        let takeover_sql = format!("SET application_name = 'a099take';\n{}\nSELECT pg_sleep(12);\n", adopt("a099dead", OWNER_B));
        let mut take = self.spawn(db, "oo_a099_runtime", &takeover_sql);
        let take_visible = self.wait_app("a099take", 15);
        let blocked = self.script(db, "oo_a099_runtime", "old-writer", &format!("{}\n", adopt("a099dead", OWNER_A)));
        self.note("old-writer-blocked", "require", take_visible && blocked.state.as_deref() == Some("55P03"), &format!("state={}", blocked.state.unwrap_or_default()));
        let _ = self.psql("stop-take", "oo_a099_admin", db, "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name = 'a099take' AND pid <> pg_backend_pid();\n", true);
        let _ = take.wait();
        let gen = self.psql("take-gen", "oo_a099_admin", db, "SELECT generation::text || '|' || owner_token::text FROM _oo.target_generations WHERE shard_id = 'a099dead';\n", true);
        let original = self.psql("dead-row-before", "oo_a099_admin", db, "SELECT fence_generation::text || '|' || target_commit_at::text || '|' || payload_digest FROM _oo.apply_receipts WHERE txn_or_batch_id = 'dead-batch';\n", true);
        self.note("takeover-generation", "require", old.exit == 0 && gen.stdout.trim().starts_with("2|") && gen.stdout.contains(OWNER_B), &trim_detail(&(gen.stdout.clone() + &old.stdout)));
        let stale = format!("{}\n{}\n", adopt("a099dead", OWNER_B), receipt_owned("a099dead", OWNER_B, 1, "after-old", 'e', None, None, Bmp::Empty));
        let next = self.psql("after-takeover", "oo_a099_runtime", db, &stale, true);
        let after_old = self.receipt_count_batch(db, "after-old");
        let next_state = next.state.clone().unwrap_or_default();
        self.note("old-generation-OO013", "require", next_state == "OO013" && after_old == Some(0), &format!("state={next_state}"));
        let replay = self.psql("takeover-replay", "oo_a099_runtime", db, &receipt_at_adopted_owner("a099dead", OWNER_B, "dead-batch", 'e', None, None, Bmp::Empty), true);
        let row = self.psql("dead-row", "oo_a099_admin", db, "SELECT fence_generation::text || '|' || target_commit_at::text || '|' || payload_digest FROM _oo.apply_receipts WHERE txn_or_batch_id = 'dead-batch';\n", true);
        self.note("takeover-replay-keeps-original", "require", replay.stdout.contains("RESULT|replay") && row.stdout.trim() == original.stdout.trim() && original.stdout.trim().starts_with("1|"), &trim_detail(&(replay.stdout.clone() + &row.stdout)));
    }

    fn migration_wait(&mut self, db: &str) {
        let hold = "SET application_name = 'a099mig';\nBEGIN;\nSET LOCAL lock_timeout = '5s';\nSELECT pg_catalog.pg_advisory_xact_lock(6404202605);\nSELECT pg_sleep(12);\nCOMMIT;\n";
        let mut child = self.spawn(db, "oo_a099_admin", hold);
        let visible = self.wait_app("a099mig", 15);
        let start = Instant::now();
        let wait = self.psql("mig-wait", "oo_a099_admin", db, "BEGIN;\nSET LOCAL lock_timeout = '2s';\nSELECT pg_catalog.pg_advisory_xact_lock(6404202605);\nCOMMIT;\n", true);
        let elapsed = start.elapsed().as_millis();
        let _ = self.psql("stop-mig", "oo_a099_admin", db, "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name = 'a099mig' AND pid <> pg_backend_pid();\n", true);
        let _ = child.wait();
        let pass = visible && wait.state.as_deref() == Some("55P03") && (500..8000).contains(&elapsed);
        fs::write(self.evidence.join("cases/mig-wait.ms"), format!("{elapsed}\n")).ok();
        self.note("migration-lock-timeout", "require", pass, &format!("visible={visible} state={} elapsed_ms={elapsed}", wait.state.unwrap_or_default()));
    }

    fn kills(&mut self, db: &str) {
        let pre = format!("SET application_name = 'a099pre';\nBEGIN;\n{}\n{}\nSELECT pg_sleep(30);\nCOMMIT;\n", adopt("a099pre", OWNER_A), receipt("a099pre", 1, "pre-batch", 'e', None, None, Bmp::Empty));
        let pre_child = self.spawn(db, "oo_a099_runtime", &pre);
        let pre_seen = self.wait_query("a099pre", "pg_sleep", 20);
        let term_pre = self.psql("term-pre", "oo_a099_admin", db, "SELECT pg_terminate_backend(pid)::text FROM pg_stat_activity WHERE application_name = 'a099pre' AND pid <> pg_backend_pid();\n", true);
        let pre_out = pre_child.wait_with_output();
        let pre_err = pre_out.as_ref().map(|o| String::from_utf8_lossy(&o.stderr).to_string()).unwrap_or_default();
        let _ = fs::write(self.evidence.join("cases/kill-before.stderr"), &pre_err);
        let absent = self.receipt_count_batch(db, "pre-batch") == Some(0);
        let retry = self.psql("kill-before-retry", "oo_a099_runtime", db, &receipt_at_adopted("a099pre", "pre-batch", 'e', None, None, Bmp::Empty), true);
        self.note("kill-before-commit", "require", pre_seen && absent && pre_err.contains("57P01") && retry.stdout.contains("RESULT|inserted"), &format!("seen={pre_seen} absent={absent} term={} retry={}", trim_detail(&term_pre.stdout), trim_detail(&retry.stdout)));
        let post = format!("SET application_name = 'a099post';\nBEGIN;\n{}\n{}\nCOMMIT;\nSELECT pg_sleep(30);\n", adopt("a099post", OWNER_A), receipt("a099post", 1, "post-batch", 'e', None, None, Bmp::Empty));
        let post_child = self.spawn(db, "oo_a099_runtime", &post);
        let post_seen = self.wait_query("a099post", "pg_sleep", 20);
        let present_before_term = self.receipt_count_batch(db, "post-batch") == Some(1);
        let _ = self.psql("term-post", "oo_a099_admin", db, "SELECT pg_terminate_backend(pid)::text FROM pg_stat_activity WHERE application_name = 'a099post' AND pid <> pg_backend_pid();\n", true);
        let post_out = post_child.wait_with_output();
        let post_err = post_out.as_ref().map(|o| String::from_utf8_lossy(&o.stderr).to_string()).unwrap_or_default();
        let _ = fs::write(self.evidence.join("cases/kill-after.stderr"), &post_err);
        let present = self.receipt_count_batch(db, "post-batch") == Some(1);
        let kept = self.psql("kill-after-row", "oo_a099_admin", db, &stamp_select("post-batch"), true);
        let replay = self.psql("kill-after-replay", "oo_a099_runtime", db, &receipt_at_adopted("a099post", "post-batch", 'e', None, None, Bmp::Empty), true);
        let replay_row = self.psql("kill-after-replay-row", "oo_a099_admin", db, &stamp_select("post-batch"), true);
        let replay_stamps = stamp_values(&replay_row.stdout);
        let kept_stamps = stamp_values(&kept.stdout);
        self.note(
            "kill-after-commit",
            "require",
            post_seen && present_before_term && present && post_err.contains("57P01") && replay.stdout.contains("RESULT|replay") && replay_stamps == kept_stamps && !kept_stamps.is_empty(),
            &format!("seen={post_seen} present={present} kept={kept_stamps:?}"),
        );
        let drop_sql = format!("SET application_name = 'a099drop';\nBEGIN;\n{}\n{}\nSELECT pg_sleep(30);\n", adopt("a099drop", OWNER_A), receipt("a099drop", 1, "drop-batch", 'e', None, None, Bmp::Empty));
        let mut drop_child = self.spawn(db, "oo_a099_runtime", &drop_sql);
        let drop_seen = self.wait_query("a099drop", "pg_sleep", 20);
        let _ = drop_child.kill();
        let _ = drop_child.wait();
        let uncommitted = self.receipt_count_batch(db, "drop-batch") == Some(0);
        let leaked = self.psql("drop-leak", "oo_a099_admin", db, "SELECT count(*)::text FROM pg_stat_activity WHERE application_name = 'a099drop';\n", true);
        if leaked.stdout.contains('1') {
            let _ = self.psql("drop-hygiene", "oo_a099_admin", db, "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name = 'a099drop' AND pid <> pg_backend_pid();\n", true);
        }
        let still = self.receipt_count_batch(db, "drop-batch") == Some(0);
        self.note("disconnect-before-commit", "require", drop_seen && uncommitted && still, &format!("seen={drop_seen} leaked_backends={}", trim_detail(&leaked.stdout)));
    }

    fn forward_expect_oo021(&mut self, db: &str, name: &str) -> bool {
        let sql = format!("BEGIN;\nSET LOCAL lock_timeout = '5s';\nSELECT pg_catalog.pg_advisory_xact_lock(6404202605);\nSET ROLE oo_target_owner;\nSELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;\n{}\nRESET ROLE;\nCOMMIT;\n", self.segment);
        let run = self.psql(name, "oo_a099_admin", db, &sql, true);
        let pass = run.state.as_deref() == Some("OO021") && run.stderr.contains(OO021);
        self.note(name, "require", pass, &first_error(&run.stderr));
        pass
    }

    fn call_receipt(&mut self, db: &str, user: &str, shard: &str, batch: &str, gen: i64, index: Option<i32>, count: Option<i32>, bmp: Bmp, payload: char) -> Run {
        let sql = format!("{}\n{}\n", adopt(shard, OWNER_A), receipt(shard, gen, batch, payload, index, count, bmp));
        self.script(db, user, batch, &sql)
    }

    fn call_partial(&mut self, db: &str, shard: &str, batch: &str) -> Run {
        let sql = format!("{}\nSELECT 'RESULT|' || _oo.commit_apply_receipt('{shard}', '{OWNER_A}'::uuid, 1, 'warehouse', 'orders_g1', '{batch}', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, true, false, false, NULL, NULL, ''::bytea);\n", adopt(shard, OWNER_A));
        self.script(db, "oo_a099_runtime", batch, &sql)
    }

    fn script(&mut self, db: &str, user: &str, name: &str, sql: &str) -> Run {
        self.psql(name, user, db, sql, true)
    }

    fn snapshot(&mut self, db: &str, name: &str) -> String {
        let sql = "SELECT coalesce(string_agg(concat_ws('|', source, table_generation, txn_or_batch_id, payload_digest, fence_shard, fence_generation::text, coalesce(segment_index::text, 'null'), coalesce(segment_count::text, 'null'), octet_length(segment_bitmap)::text, encode(segment_bitmap, 'hex'), target_commit_at::text, partial_txn::text, cross_table_consistent::text, txn_complete::text), E'\\n' ORDER BY source, table_generation, txn_or_batch_id), '') FROM _oo.apply_receipts;\n";
        let run = self.psql(name, "oo_a099_admin", db, sql, true);
        let text = run.stdout.trim().to_string();
        if let Ok(sum) = sha256_text(&text) {
            let _ = fs::write(self.evidence.join(format!("cases/{name}.sha256")), sum + "\n");
        }
        text
    }

    fn marker_count(&mut self, db: &str) -> Option<i64> {
        let run = self.psql(&format!("marker-{db}"), "oo_a099_admin", db, "SELECT count(*)::text FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/2';\n", false);
        if run.exit != 0 {
            return None;
        }
        run.stdout.trim().parse().ok()
    }

    fn receipt_count(&mut self, db: &str) -> Option<i64> {
        let run = self.psql(&format!("count-{db}-{}", self.outcomes.len()), "oo_a099_admin", db, "SELECT count(*)::text FROM _oo.apply_receipts;\n", false);
        if run.exit != 0 { None } else { run.stdout.trim().parse().ok() }
    }

    fn receipt_count_batch(&mut self, db: &str, batch: &str) -> Option<i64> {
        let run = self.psql(&format!("count-{batch}"), "oo_a099_admin", db, &format!("SELECT count(*)::text FROM _oo.apply_receipts WHERE txn_or_batch_id = '{batch}';\n"), false);
        if run.exit != 0 { None } else { run.stdout.trim().parse().ok() }
    }

    fn function_has(&mut self, db: &str, needle: &str) -> bool {
        let run = self.psql(&format!("fn-{}", self.outcomes.len()), "oo_a099_admin", db, "SELECT pg_get_functiondef('_oo.commit_apply_receipt(text,uuid,bigint,text,text,text,text,text,text,text,text,text,text,integer,numeric,boolean,boolean,boolean,integer,integer,bytea)'::regprocedure);\n", false);
        run.exit == 0 && run.stdout.contains(needle)
    }

    fn constraint_has(&mut self, db: &str, needle: &str) -> bool {
        let run = self.psql(&format!("ck-{}", self.outcomes.len()), "oo_a099_admin", db, "SELECT pg_get_constraintdef(oid) FROM pg_constraint WHERE conname = 'apply_receipts_segment_check';\n", false);
        run.exit == 0 && run.stdout.contains(needle)
    }

    fn applied_at(&mut self, db: &str) -> Option<String> {
        let run = self.psql(&format!("applied-{db}-{}", self.outcomes.len()), "oo_a099_admin", db, "SELECT applied_at::text FROM _oo.schema_migrations WHERE version = 'target-fence-receipts/2';\n", false);
        if run.exit != 0 { None } else { Some(run.stdout.trim().to_string()) }
    }

    fn case_stderr(&self, name: &str) -> String {
        fs::read_to_string(self.evidence.join(format!("cases/{name}.stderr"))).unwrap_or_default()
    }

    fn wait_app(&mut self, app_name: &str, attempts: u32) -> bool {
        for _ in 0..attempts {
            let run = self.psql(&format!("poll-{app_name}"), "oo_a099_admin", "postgres", &format!("SELECT count(*)::text FROM pg_stat_activity WHERE application_name = '{app_name}' AND state = 'active';\n"), false);
            if run.stdout.trim().parse::<i64>().unwrap_or(0) > 0 {
                return true;
            }
            thread::sleep(Duration::from_secs(1));
        }
        false
    }

    fn wait_query(&mut self, app_name: &str, needle: &str, attempts: u32) -> bool {
        for _ in 0..attempts {
            let run = self.psql(&format!("pollq-{app_name}"), "oo_a099_admin", "postgres", &format!("SELECT count(*)::text FROM pg_stat_activity WHERE application_name = '{app_name}' AND query LIKE '%{needle}%' AND state = 'active';\n"), false);
            if run.stdout.trim().parse::<i64>().unwrap_or(0) > 0 {
                return true;
            }
            thread::sleep(Duration::from_secs(1));
        }
        false
    }

    fn spawn(&self, db: &str, user: &str, sql: &str) -> std::process::Child {
        let mut child = Command::new("docker")
            .args(["exec", "-i", "-e", "PGPASSFILE=/tmp/a099.pgpass", "-u", "postgres", &self.container, "psql", "-U", user, "-d", db, "-X", "-v", "ON_ERROR_STOP=1", "-v", "VERBOSITY=verbose"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn psql");
        child.stdin.as_mut().unwrap().write_all(sql.as_bytes()).ok();
        let _ = child.stdin.take();
        child
    }

    fn psql(&mut self, name: &str, user: &str, db: &str, sql: &str, save_sql: bool) -> Run {
        if save_sql {
            let _ = fs::write(self.evidence.join(format!("sql/{name}.sql")), self.scrub(sql));
        }
        let run = self.psql_raw(name, user, db, sql);
        let _ = fs::write(self.evidence.join(format!("cases/{name}.stdout")), self.scrub(&run.stdout));
        let _ = fs::write(self.evidence.join(format!("cases/{name}.stderr")), self.scrub(&run.stderr));
        let _ = fs::write(self.evidence.join(format!("cases/{name}.exit")), format!("{}\n", run.exit));
        run
    }

    fn psql_raw(&self, name: &str, user: &str, db: &str, sql: &str) -> Run {
        let mut child = Command::new("docker")
            .args(["exec", "-i", "-e", "PGPASSFILE=/tmp/a099.pgpass", "-u", "postgres", &self.container, "psql", "-U", user, "-d", db, "-X", "-v", "ON_ERROR_STOP=1", "-v", "VERBOSITY=verbose", "-A", "-t"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("psql");
        child.stdin.as_mut().unwrap().write_all(sql.as_bytes()).ok();
        drop(child.stdin.take());
        let out = child.wait_with_output().expect("wait psql");
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        let _ = name;
        Run { exit: out.status.code().unwrap_or(1), stdout, stderr: stderr.clone(), state: sqlstate(&stderr) }
    }

    fn scrub(&self, text: &str) -> String {
        text.replace(&self.admin_pw, "<redacted>")
            .replace(&self.runtime_pw, "<redacted>")
            .replace(&self.bootstrap_pw, "<redacted>")
            .replace(&self.nobody_pw, "<redacted>")
    }

    fn note(&mut self, name: &str, kind: &'static str, pass: bool, detail: &str) {
        let detail = self.scrub(detail);
        println!("{kind} {name} pass={pass} {detail}");
        self.outcomes.push(Outcome { name: name.to_string(), kind, pass, detail });
    }

    fn finish(&self) -> Result<i32, String> {
        let mut require_fail = 0;
        let mut harness_fail = 0;
        let mut residual = 0;
        let mut lines = String::from("[\n");
        for (i, item) in self.outcomes.iter().enumerate() {
            if !item.pass && item.kind == "require" { require_fail += 1; }
            if !item.pass && item.kind == "harness" { harness_fail += 1; }
            if item.kind == "residual" { residual += 1; }
            if i > 0 { lines.push_str(",\n"); }
            lines.push_str(&format!(
                "  {{\"name\": \"{}\", \"kind\": \"{}\", \"pass\": {}, \"detail\": \"{}\"}}",
                json_escape(&item.name), item.kind, item.pass, json_escape(&item.detail)
            ));
        }
        lines.push_str("\n]\n");
        fs::write(self.evidence.join("summary.json"), &lines).map_err(|e| e.to_string())?;
        let counts = format!("require_fail={require_fail}\nharness_fail={harness_fail}\nresidual={residual}\ncases={}\n", self.outcomes.len());
        fs::write(self.evidence.join("counts.txt"), &counts).map_err(|e| e.to_string())?;
        println!("{counts}");
        if harness_fail > 0 { Ok(2) } else if require_fail > 0 { Ok(1) } else { Ok(0) }
    }
}

struct Run {
    exit: i32,
    stdout: String,
    stderr: String,
    state: Option<String>,
}

#[derive(Clone, Copy)]
enum Install {
    BootstrapOnly,
    V1,
    V2,
    V2Upgrade,
    V2Lie,
    V2ThenFail,
}

fn adopt(shard: &str, owner: &str) -> String {
    format!("SELECT 'ADOPT|' || _oo.adopt_target_generation('{shard}', '{owner}'::uuid)::text;")
}

fn receipt(shard: &str, gen: i64, batch: &str, payload: char, index: Option<i32>, count: Option<i32>, bmp: Bmp) -> String {
    receipt_owned(shard, OWNER_A, gen, batch, payload, index, count, bmp)
}

fn receipt_owned(shard: &str, owner: &str, gen: i64, batch: &str, payload: char, index: Option<i32>, count: Option<i32>, bmp: Bmp) -> String {
    format!(
        "SELECT 'RESULT|' || _oo.commit_apply_receipt('{shard}', '{owner}'::uuid, {gen}, 'warehouse', 'orders_g1', '{batch}', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('{payload}', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, {index}, {count}, {bmp});",
        index = opt_i(index),
        count = opt_i(count),
        bmp = bmp_sql(bmp)
    )
}

fn receipt_at_adopted(shard: &str, batch: &str, payload: char, index: Option<i32>, count: Option<i32>, bmp: Bmp) -> String {
    receipt_at_adopted_owner(shard, OWNER_A, batch, payload, index, count, bmp)
}

fn receipt_at_adopted_owner(shard: &str, owner: &str, batch: &str, payload: char, index: Option<i32>, count: Option<i32>, bmp: Bmp) -> String {
    format!(
        "SELECT 'RESULT|' || _oo.commit_apply_receipt('{shard}', '{owner}'::uuid, _oo.adopt_target_generation('{shard}', '{owner}'::uuid), 'warehouse', 'orders_g1', '{batch}', repeat('a', 64), repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('{payload}', 64), 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, {index}, {count}, {bmp});",
        index = opt_i(index),
        count = opt_i(count),
        bmp = bmp_sql(bmp)
    )
}

fn stamp_select(batch: &str) -> String {
    format!("SELECT 'STAMP|' || target_commit_at::text || '|' || fence_generation::text || '|' || payload_digest FROM _oo.apply_receipts WHERE txn_or_batch_id = '{batch}';\n")
}

fn stamp_values(stdout: &str) -> Vec<String> {
    stdout.lines().filter(|line| line.starts_with("STAMP|")).map(|line| line.trim().to_string()).collect()
}

fn direct_insert(batch: &str, index: Option<i32>, count: Option<i32>, bmp: Bmp) -> String {
    format!(
        "INSERT INTO _oo.apply_receipts (source, table_generation, txn_or_batch_id, config_version, config_digest, type_policy_version, type_digest, hook_digest, schema_digest, payload_digest, protocol_version, position_codec, position_lineage, source_epoch, applied_pos, partial_txn, cross_table_consistent, txn_complete, segment_index, segment_count, segment_bitmap, fence_shard, fence_generation) VALUES ('warehouse', 'orders_g1', '{batch}', 'config/v1', repeat('a', 64), 'type-policy/v1', repeat('b', 64), repeat('c', 64), repeat('d', 64), repeat('e', 64), 'batch/v1', 'pg-commit-lsn/v1', 'src_main', 1, 0, false, true, true, {index}, {count}, {bmp}, '{batch}', 1);\n",
        index = opt_i(index),
        count = opt_i(count),
        bmp = bmp_sql(bmp)
    )
}

fn book_sql(rows: &[(&str, &str, &str)]) -> String {
    let values = rows.iter().map(|(v, c, f)| format!("  ('{v}', '{c}', 'expand', '{f}')")).collect::<Vec<_>>().join(",\n");
    format!("INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values};\n")
}

fn bootstrap_text(raw: &str, roles_exist: bool) -> String {
    if !roles_exist {
        return raw.to_string();
    }
    let mut out = String::new();
    let mut skipping = false;
    for line in raw.lines() {
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

fn closed_legal(index: Option<i32>, count: Option<i32>, bmp: Bmp) -> bool {
    let Some(len) = bmp_len(bmp) else { return false };
    match (index, count) {
        (None, None) => len == 0,
        (Some(i), Some(c)) => c >= 1 && i >= 0 && i < c && (1..=8192).contains(&len),
        _ => false,
    }
}

fn bmp_sql(bmp: Bmp) -> &'static str {
    match bmp {
        Bmp::Null => "NULL::bytea",
        Bmp::Empty => "''::bytea",
        Bmp::One => "'\\x01'::bytea",
        Bmp::N8192 => "decode(repeat('aa', 8192), 'hex')",
        Bmp::N8193 => "decode(repeat('aa', 8193), 'hex')",
    }
}

fn bmp_len(bmp: Bmp) -> Option<i32> {
    match bmp {
        Bmp::Null => None,
        Bmp::Empty => Some(0),
        Bmp::One => Some(1),
        Bmp::N8192 => Some(8192),
        Bmp::N8193 => Some(8193),
    }
}

fn opt_i(value: Option<i32>) -> String {
    match value { None => "NULL".into(), Some(n) => n.to_string() }
}

fn sqlstate(stderr: &str) -> Option<String> {
    for line in stderr.lines() {
        let rest = if let Some(idx) = line.find("ERROR:  ") {
            &line[idx + "ERROR:  ".len()..]
        } else if let Some(idx) = line.find("FATAL:  ") {
            &line[idx + "FATAL:  ".len()..]
        } else {
            continue;
        };
        let token = rest.split(':').next().unwrap_or("").trim();
        if token.len() == 5 && token.chars().all(|ch| ch.is_ascii_alphanumeric()) {
            return Some(token.to_string());
        }
    }
    None
}

fn first_error(stderr: &str) -> String {
    stderr.lines().find(|line| line.contains("ERROR:") || line.contains("FATAL:")).unwrap_or("").chars().take(400).collect()
}

fn trim_detail(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines().take(12) {
        out.push_str(line);
        out.push(' ');
        if out.len() > 500 { break; }
    }
    out
}

fn json_escape(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn read_string(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn expect_hash(path: &Path, expect: &str) -> Result<(), String> {
    let got = sha256_file(path)?;
    if got != expect {
        return Err(format!("hash {} got {got}", path.display()));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let out = Command::new("shasum").args(["-a", "256"]).arg(path).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("shasum {}", path.display()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).split_whitespace().next().unwrap_or("").to_string())
}

fn sha256_text(text: &str) -> Result<String, String> {
    let path = std::env::temp_dir().join(format!("a099-sha-{}", uuid_v4()));
    fs::write(&path, text).map_err(|e| e.to_string())?;
    let sum = sha256_file(&path);
    let _ = fs::remove_file(&path);
    sum
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    File::open("/dev/urandom").unwrap().read_exact(&mut buf).unwrap();
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn write_secret(path: &Path, value: &str) -> Result<(), String> {
    let mut file = OpenOptions::new().create(true).truncate(true).write(true).mode(0o600).open(path).map_err(|e| e.to_string())?;
    file.write_all(value.as_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    File::open("/dev/urandom").unwrap().read_exact(&mut b).unwrap();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

fn capacity_postgres() -> Result<(), String> {
    let out = docker_out(&["ps", "-a", "--filter", "label=oo.owner=a099", "--filter", "label=oo.kind=postgres", "--format", "{{.Names}}"])?;
    if !out.trim().is_empty() {
        return Err(format!("own postgres fixture already exists: {out}"));
    }
    Ok(())
}

fn docker_ok(args: &[&str]) -> Result<(), String> {
    let out = docker_out(args)?;
    let _ = out;
    Ok(())
}

fn docker_out(args: &[&str]) -> Result<String, String> {
    let out = Command::new("docker").args(args).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("docker {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn cleanup(evidence: &Path, root: &Path) -> i32 {
    let resources = fs::read_to_string(evidence.join("resources.txt")).unwrap_or_default();
    let mut container = String::new();
    let mut network = String::new();
    let mut volume = String::new();
    for line in resources.lines() {
        if let Some(v) = line.strip_prefix("container=") { container = v.to_string(); }
        if let Some(v) = line.strip_prefix("network=") { network = v.to_string(); }
        if let Some(v) = line.strip_prefix("volume=") { volume = v.to_string(); }
    }
    let mut report = String::new();
    for (kind, name) in [("rm", container.as_str()), ("network", network.as_str()), ("volume", volume.as_str())] {
        if name.is_empty() || !name.starts_with("oo-a099-") {
            report.push_str(&format!("{kind}=skip\n"));
            continue;
        }
        let args: Vec<&str> = if kind == "rm" { vec!["rm", "-f", name] } else { vec![kind, "rm", name] };
        let out = Command::new("docker").args(&args).output();
        match out {
            Ok(output) => report.push_str(&format!("{kind}_exit={}\n", output.status.code().unwrap_or(1))),
            Err(err) => report.push_str(&format!("{kind}_error={err}\n")),
        }
    }
    if root.starts_with("/tmp/oo-a099-eef831aa-dba4-4af2-a78b-541bc9e0a099") {
        let _ = fs::remove_dir_all(root);
        report.push_str(&format!("tmp_removed={} exists={}\n", !root.exists(), root.exists()));
    }
    let _ = fs::write(evidence.join("cleanup.txt"), &report);
    println!("{report}");
    0
}
