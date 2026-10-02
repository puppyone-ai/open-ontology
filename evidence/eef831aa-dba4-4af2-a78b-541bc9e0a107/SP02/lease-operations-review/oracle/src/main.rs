#![forbid(unsafe_code)]

mod pg;
mod sha256;

use pg::{cell, q, Outcome, Pg};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LIE: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const CANARY: &str = "CANARY_a107_SECRET_VALUE";
const BOOT_H: &str = "445d7248aa252a96a4103ae1b0be0000744eef2c0603037c19df6a0f8eeb0b52";
const EXPAND_H: &str = "b6181331d68360e6eec5395b4400ba0141276aebffe21abc15c8b4b8f0310b75";
const EVENT_H: &str = "4ede04e8f848eff002da7e3a12628ac731c7bca02ba345daa6d036790e894fee";
const LEASE_H: &str = "5cb2e9030a479903ecc57ce32fc3ee851f00d146e02ab3902fcbe7ab09501034";
const CONTRACT_H: &str = "63009790833468ddb4affa766a8310893469f72895e958119ecc837b92a1365b";
const CONTROL_CONTRACT_H: &str = "d352c86dbf2836360e541a3511af23674ce9692e687a6ac7041d99118dc8c2ca";

const P_MAIN: &str = "11111111-1111-4111-8111-111111111111";
const P_OTHER: &str = "22222222-2222-4222-8222-222222222222";
const P_CAP: &str = "33333333-3333-4333-8333-333333333333";
const P_HB: &str = "44444444-4444-4444-8444-444444444444";
const P_RE: &str = "55555555-5555-4555-8555-555555555555";
const P_CX: &str = "66666666-6666-4666-8666-666666666666";
const P_OV: &str = "77777777-7777-4777-8777-777777777777";
const P_RACE: &str = "88888888-8888-4888-8888-888888888888";
const P_BATCH: &str = "99999999-9999-4999-8999-999999999999";
const P_BAD: &str = "abababab-abab-4aba-8aba-abababababab";
const P_LIFE: &str = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";
const P_RETRY: &str = "ffffffff-ffff-4fff-8fff-ffffffffffff";
const P_LOCK: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const P_KILL: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const P_TERM: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const P_HOLD: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const P_DEAD: &str = "d0d0d0d0-d0d0-4d0d-8d0d-d0d0d0d0d0d0";
const LEGAL_TASK: &str = "12121212-1212-4121-8121-121212121212";

struct Rec {
    path: PathBuf,
    passed: u32,
    failed: u32,
}

impl Rec {
    fn open(dir: &Path) -> Self {
        fs::create_dir_all(dir).ok();
        let path = dir.join("cases.jsonl");
        Self {
            path,
            passed: 0,
            failed: 0,
        }
    }

    fn add(&mut self, id: &str, ok: bool, expect: &str, actual: &str, sqlstate: &str, note: &str) {
        if ok {
            self.passed += 1;
        } else {
            self.failed += 1;
        }
        println!("CASE {id} {}", if ok { "ok" } else { "fail" });
        let line = format!(
            "{{\"id\":\"{}\",\"ok\":{},\"expect\":\"{}\",\"actual\":\"{}\",\"sqlstate\":\"{}\",\"note\":\"{}\"}}\n",
            esc(id),
            if ok { "true" } else { "false" },
            esc(expect),
            esc(actual),
            esc(sqlstate),
            esc(note)
        );
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = f.write_all(line.as_bytes());
            let _ = f.flush();
        }
    }
}

fn esc(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            c if c.is_control() => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

struct Ctx {
    host: String,
    port: u16,
    out: PathBuf,
    boot: String,
    expand: String,
    event: String,
    lease: String,
}

fn phase(out: &Path, n: u32, name: &str) {
    println!("PHASE {n} {name}");
    let _ = fs::write(out.join(format!("phase-{n}.txt")), format!("{n} {name}\n"));
    let _ = fs::write(out.join("phase-last.txt"), format!("{n} {name}\n"));
}

fn connect(host: &str, port: u16, user: &str, db: &str) -> Result<Pg, String> {
    Pg::connect(host, port, user, db)
}

fn su(ctx: &Ctx) -> Result<Pg, String> {
    connect(&ctx.host, ctx.port, "postgres", "a107")
}

fn admin(ctx: &Ctx) -> Result<Pg, String> {
    connect(&ctx.host, ctx.port, "postgres", "postgres")
}

fn rt(ctx: &Ctx) -> Result<Pg, String> {
    connect(&ctx.host, ctx.port, "a107_runtime", "a107")
}

fn own(ctx: &Ctx) -> Result<Pg, String> {
    connect(&ctx.host, ctx.port, "a107_owner", "a107")
}

fn finish(pg: &mut Pg, sql: &str) -> Outcome {
    let o = pg.exec(sql);
    if o.sqlstate.is_some() {
        let _ = pg.exec("ROLLBACK");
    }
    o
}

fn row(o: &Outcome) -> Vec<Option<String>> {
    o.rows.first().cloned().unwrap_or_default()
}

fn show(o: &Outcome) -> String {
    if !o.rows.is_empty() {
        o.rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|c| c.clone().unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .collect::<Vec<_>>()
            .join(";")
    } else {
        format!("{} {}", o.state(), o.message)
    }
}

fn dollar(tag: &str, s: &str) -> String {
    format!("${tag}${s}${tag}$")
}

fn enq(project: &str, kind: &str, dedupe: &str, max: i32, run_at: &str, payload_sql: &str) -> String {
    format!(
        "SELECT status, COALESCE(task_id::text,''), COALESCE(state,''), COALESCE(attempt::text,''), COALESCE(generation::text,''), COALESCE(lease_owner,''), COALESCE(EXTRACT(EPOCH FROM (lease_until - clock_timestamp()))::text,''), COALESCE(token,''), COALESCE(EXTRACT(EPOCH FROM (run_at - clock_timestamp()))::text,''), COALESCE(invalidated_owner,''), COALESCE(invalidated_generation::text,''), COALESCE(invalidated_until::text,'') FROM jobs.enqueue({}::uuid, {}, 1, {}, 'cfg-1', '{}', {}, {}, {})",
        q(project),
        q(kind),
        payload_sql,
        DIGEST,
        q(dedupe),
        run_at,
        max
    )
}

fn claim(project: &str, kinds_sql: &str, worker: &str) -> String {
    format!(
        "SELECT status, COALESCE(task_id::text,''), COALESCE(state,''), COALESCE(attempt::text,''), COALESCE(generation::text,''), COALESCE(lease_owner,''), COALESCE(EXTRACT(EPOCH FROM (lease_until - clock_timestamp()))::text,''), COALESCE(token,''), COALESCE(EXTRACT(EPOCH FROM (run_at - clock_timestamp()))::text,''), COALESCE(invalidated_owner,''), COALESCE(invalidated_generation::text,''), COALESCE(invalidated_until::text,''), pg_backend_pid()::text FROM jobs.claim({}::uuid, {}, {})",
        q(project),
        kinds_sql,
        q(worker)
    )
}

fn cols(fun: &str, project: &str, task: &str, worker: &str, gen: &str, extra: &str) -> String {
    format!(
        "SELECT status, COALESCE(task_id::text,''), COALESCE(state,''), COALESCE(attempt::text,''), COALESCE(generation::text,''), COALESCE(lease_owner,''), COALESCE(EXTRACT(EPOCH FROM (lease_until - clock_timestamp()))::text,''), COALESCE(token,''), COALESCE(EXTRACT(EPOCH FROM (run_at - clock_timestamp()))::text,''), COALESCE(invalidated_owner,''), COALESCE(invalidated_generation::text,''), COALESCE(invalidated_until::text,'') FROM jobs.{fun}({}::uuid, {}::uuid, {}, {gen}{extra})",
        q(project),
        q(task),
        q(worker)
    )
}

fn insert_rows(rows: &[(&str, &str, &str)]) -> String {
    let mut s = String::from(
        "INSERT INTO jobs.schema_migrations (version, checksum, phase, filename) VALUES\n",
    );
    for (i, (version, hash, file)) in rows.iter().enumerate() {
        if i > 0 {
            s.push_str(",\n");
        }
        s.push_str(&format!(
            "({}, {}, 'expand', {})",
            q(version),
            q(hash),
            q(file)
        ));
    }
    s.push_str(";\n");
    s
}

fn hashes_ok(boot: &str, expand: &str, event: &str, lease: &str, contract: &str, control: &str) -> bool {
    boot == BOOT_H && expand == EXPAND_H && event == EVENT_H && lease == LEASE_H && contract == CONTRACT_H
        && control == CONTROL_CONTRACT_H
}

fn load(ctx_out: &Path) -> Result<Ctx, String> {
    sha256::self_check()?;
    let host = env::var("PGHOST").map_err(|_| "PGHOST missing")?;
    let port: u16 = env::var("PGPORT").unwrap_or_else(|_| "5432".into()).parse().map_err(|_| "PGPORT")?;
    let sql = PathBuf::from(env::var("SQLDIR").map_err(|_| "SQLDIR missing")?);
    let src = PathBuf::from(env::var("SRCDIR").map_err(|_| "SRCDIR missing")?);
    let out = PathBuf::from(env::var("OUTDIR").map_err(|_| "OUTDIR missing")?);
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let boot_b = fs::read(sql.join("0000_bootstrap_roles.sql")).map_err(|e| e.to_string())?;
    let expand_b = fs::read(sql.join("0001_jobs_control_expand.sql")).map_err(|e| e.to_string())?;
    let event_b = fs::read(sql.join("0002_event_project_binding_expand.sql")).map_err(|e| e.to_string())?;
    let lease_b = fs::read(sql.join("0003_job_lease_operations_expand.sql")).map_err(|e| e.to_string())?;
    let contract_b = fs::read(src.join("contracts/jobs/lease-operations/jobs-lease-operations-v1.json")).map_err(|e| e.to_string())?;
    let control_b = fs::read(src.join("contracts/jobs/control-schema/jobs-control-v1.json")).map_err(|e| e.to_string())?;
    let boot_h = sha256::hex(&boot_b);
    let expand_h = sha256::hex(&expand_b);
    let event_h = sha256::hex(&event_b);
    let lease_h = sha256::hex(&lease_b);
    let contract_h = sha256::hex(&contract_b);
    let control_h = sha256::hex(&control_b);
    fs::write(
        out.join("identity.txt"),
        format!("boot={boot_h}\nexpand={expand_h}\nevent={event_h}\nlease={lease_h}\ncontract={contract_h}\ncontrol={control_h}\n"),
    ).ok();
    if !hashes_ok(&boot_h, &expand_h, &event_h, &lease_h, &contract_h, &control_h) {
        return Err(format!("hash mismatch boot={boot_h} expand={expand_h} event={event_h} lease={lease_h} contract={contract_h} control={control_h}"));
    }
    let contract = String::from_utf8_lossy(&contract_b);
    if !contract.contains(LEASE_H) || !contract.contains("\"already_applied\"") {
        return Err("contract text missing published lease hash or status list".into());
    }
    let _ = ctx_out;
    Ok(Ctx {
        host,
        port,
        out,
        boot: String::from_utf8(boot_b).map_err(|e| e.to_string())?,
        expand: String::from_utf8(expand_b).map_err(|e| e.to_string())?,
        event: String::from_utf8(event_b).map_err(|e| e.to_string())?,
        lease: String::from_utf8(lease_b).map_err(|e| e.to_string())?,
    })
}

fn reset(ctx: &Ctx) -> Result<(), String> {
    let mut a = admin(ctx)?;
    let _ = a.exec("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = 'a107' AND pid <> pg_backend_pid()");
    let drop_db = a.exec("DROP DATABASE IF EXISTS a107");
    if drop_db.sqlstate.is_some() {
        return Err(format!("drop db {} {}", drop_db.state(), drop_db.message));
    }
    for role in ["a107_runtime", "a107_owner", "a107_public", "oo_jobs_runtime", "oo_jobs_owner"] {
        let o = a.exec(&format!("DROP ROLE IF EXISTS {role}"));
        if o.sqlstate.is_some() {
            return Err(format!("drop role {role} {} {}", o.state(), o.message));
        }
    }
    let created = a.exec("CREATE DATABASE a107");
    if created.sqlstate.is_some() {
        return Err(format!("create db {} {}", created.state(), created.message));
    }
    Ok(())
}

fn logins(ctx: &Ctx) -> Result<(), String> {
    let mut a = su(ctx)?;
    let sql = "\
CREATE ROLE a107_runtime LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION INHERIT;
GRANT oo_jobs_runtime TO a107_runtime;
CREATE ROLE a107_owner LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION INHERIT;
GRANT oo_jobs_owner TO a107_owner;
CREATE ROLE a107_public LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION NOINHERIT;
GRANT CONNECT ON DATABASE a107 TO a107_runtime;
GRANT CONNECT ON DATABASE a107 TO a107_owner;
GRANT CONNECT ON DATABASE a107 TO a107_public;
GRANT USAGE ON SCHEMA jobs TO a107_public;
";
    let o = a.exec(sql);
    if o.sqlstate.is_some() {
        return Err(format!("logins {} {}", o.state(), o.message));
    }
    Ok(())
}

fn apply_fresh(ctx: &Ctx, lease_hash: &str) -> Outcome {
    let mut pg = match su(ctx) {
        Ok(v) => v,
        Err(e) => return Outcome { rows: vec![], sqlstate: Some("CONN".into()), message: e, detail: String::new(), status: b'E' },
    };
    let mut sql = String::from("BEGIN;\nSET LOCAL lock_timeout = '20s';\nSELECT pg_advisory_xact_lock(6401202601);\n");
    sql.push_str(&ctx.boot);
    sql.push_str("\nSET ROLE oo_jobs_owner;\n");
    sql.push_str(&ctx.expand);
    sql.push_str("\n");
    sql.push_str(&ctx.event);
    sql.push_str("\n");
    sql.push_str(&ctx.lease);
    sql.push_str("\nRESET ROLE;\nSELECT id FROM jobs.migration_lock WHERE id = 1 FOR UPDATE;\n");
    sql.push_str(&insert_rows(&[
        ("jobs-bootstrap/1", BOOT_H, "0000_bootstrap_roles.sql"),
        ("jobs-control/1", EXPAND_H, "0001_jobs_control_expand.sql"),
        ("jobs-event-project/1", EVENT_H, "0002_event_project_binding_expand.sql"),
        ("jobs-lease-operations/1", lease_hash, "0003_job_lease_operations_expand.sql"),
    ]));
    sql.push_str("COMMIT;\n");
    finish(&mut pg, &sql)
}

fn apply_control(ctx: &Ctx) -> Outcome {
    let mut pg = match su(ctx) {
        Ok(v) => v,
        Err(e) => return Outcome { rows: vec![], sqlstate: Some("CONN".into()), message: e, detail: String::new(), status: b'E' },
    };
    let mut sql = String::from("BEGIN;\nSET LOCAL lock_timeout = '20s';\nSELECT pg_advisory_xact_lock(6401202601);\n");
    sql.push_str(&ctx.boot);
    sql.push_str("\nSET ROLE oo_jobs_owner;\n");
    sql.push_str(&ctx.expand);
    sql.push_str("\nRESET ROLE;\nSELECT id FROM jobs.migration_lock WHERE id = 1 FOR UPDATE;\n");
    sql.push_str(&insert_rows(&[
        ("jobs-bootstrap/1", BOOT_H, "0000_bootstrap_roles.sql"),
        ("jobs-control/1", EXPAND_H, "0001_jobs_control_expand.sql"),
    ]));
    sql.push_str("COMMIT;\n");
    finish(&mut pg, &sql)
}

fn apply_owner_file(ctx: &Ctx, body: &str, insert: &str, tail: &str) -> Outcome {
    let mut pg = match su(ctx) {
        Ok(v) => v,
        Err(e) => return Outcome { rows: vec![], sqlstate: Some("CONN".into()), message: e, detail: String::new(), status: b'E' },
    };
    let mut sql = String::from("BEGIN;\nSET LOCAL lock_timeout = '20s';\nSELECT pg_advisory_xact_lock(6401202601);\nSELECT id FROM jobs.migration_lock WHERE id = 1 FOR UPDATE;\nSET ROLE oo_jobs_owner;\n");
    sql.push_str(body);
    sql.push_str("\nRESET ROLE;\n");
    sql.push_str(tail);
    sql.push_str(insert);
    sql.push_str("COMMIT;\n");
    finish(&mut pg, &sql)
}

fn one_cell(ctx: &Ctx, sql: &str) -> Outcome {
    match rt(ctx) {
        Ok(mut pg) => pg.exec(sql),
        Err(e) => Outcome { rows: vec![], sqlstate: Some("CONN".into()), message: e, detail: String::new(), status: b'E' },
    }
}

fn task_hash(ctx: &Ctx) -> String {
    match su(ctx) {
        Ok(mut pg) => {
            let o = pg.exec("SELECT md5(COALESCE(string_agg(id::text||'|'||project_id::text||'|'||kind||'|'||payload::text||'|'||state||'|'||attempt::text||'|'||generation::text||'|'||dedupe_key||'|'||COALESCE(lease_owner,'')||'|'||COALESCE(last_error,''), E'\\n' ORDER BY id), '')) FROM jobs.tasks");
            cell(&row(&o), 0)
        }
        Err(e) => e,
    }
}

fn event_hash(ctx: &Ctx) -> String {
    match su(ctx) {
        Ok(mut pg) => {
            let o = pg.exec("SELECT md5(COALESCE(string_agg(event_id::text||'|'||project_id::text||'|'||task_id::text||'|'||COALESCE(from_state,'')||'|'||to_state||'|'||generation::text||'|'||attempt::text||'|'||COALESCE(lease_owner,''), E'\\n' ORDER BY event_id), '')) FROM jobs.task_events");
            cell(&row(&o), 0)
        }
        Err(e) => e,
    }
}

fn count_sql(ctx: &Ctx, sql: &str) -> String {
    match su(ctx) {
        Ok(mut pg) => cell(&row(&pg.exec(sql)), 0),
        Err(_) => "err".into(),
    }
}

fn gap_ok_30(s: &str) -> bool {
    let v: f64 = s.parse().unwrap_or(-1.0);
    v > 27.0 && v <= 30.5
}

fn delay_for(attempt: i32) -> f64 {
    if attempt >= 7 {
        60.0
    } else {
        f64::from(1i32 << (attempt - 1))
    }
}

fn delay_ok(actual: &str, attempt: i32) -> bool {
    let v: f64 = actual.parse().unwrap_or(-999.0);
    let expect = delay_for(attempt);
    v > expect - 1.5 && v <= expect + 0.35
}

fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Result<u8, String> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err("bad-b64".into()),
        }
    }
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return Err("b64-len".into());
    }
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let (a, b, c, d) = (chunk[0], chunk[1], chunk[2], chunk[3]);
        let av = val(a)?;
        let bv = val(b)?;
        let cv = if c == b'=' { 0 } else { val(c)? };
        let dv = if d == b'=' { 0 } else { val(d)? };
        out.push((av << 2) | (bv >> 4));
        if c != b'=' {
            out.push((bv << 4) | (cv >> 2));
        }
        if d != b'=' {
            out.push((cv << 6) | dv);
        }
    }
    Ok(out)
}

fn token_matches(token: &str, task: &str, project: &str, worker: &str, gen: &str, until_utc: &str) -> bool {
    let Some(rest) = token.strip_prefix("jlt1.") else {
        return false;
    };
    let cleaned: String = rest.chars().filter(|c| !c.is_whitespace()).collect();
    let Ok(bytes) = b64_decode(&cleaned) else {
        return false;
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return false;
    };
    text == format!("{task}|{project}|{worker}|{gen}|{until_utc}")
}

fn main() {
    let phase_name = env::args().nth(1).unwrap_or_else(|| "full".into());
    let ctx = match load(Path::new(".")) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("identity_fail {e}");
            std::process::exit(2);
        }
    };
    let mut rec = Rec::open(&ctx.out);
    let code = if phase_name == "restart" {
        phase_restart(&ctx, &mut rec)
    } else {
        phase_full(&ctx, &mut rec)
    };
    let summary = format!("passed={} failed={} exit={code}\n", rec.passed, rec.failed);
    let _ = fs::write(ctx.out.join(format!("summary-{phase_name}.txt")), &summary);
    let _ = fs::write(ctx.out.join("summary.txt"), &summary);
    println!("SUMMARY passed={} failed={} exit={code}", rec.passed, rec.failed);
    std::process::exit(code);
}

fn phase_restart(ctx: &Ctx, rec: &mut Rec) -> i32 {
    phase(&ctx.out, 7, "restart");
    let queued = fs::read_to_string(ctx.out.join("marker-queued.txt")).unwrap_or_default();
    let running = fs::read_to_string(ctx.out.join("marker-running.txt")).unwrap_or_default();
    let open = fs::read_to_string(ctx.out.join("marker-open.txt")).unwrap_or_default();
    let qid = queued.trim();
    let parts: Vec<&str> = running.trim().split('|').collect();
    let rid = parts.first().copied().unwrap_or("");
    let rgen = parts.get(1).copied().unwrap_or("");
    let rattempt = parts.get(2).copied().unwrap_or("");
    let oid = open.trim();
    let mut pg = match su(ctx) {
        Ok(v) => v,
        Err(e) => {
            rec.add("restart_connect", false, "connect", &e, "CONN", "after container restart");
            return 1;
        }
    };
    let ver = pg.exec("SHOW server_version");
    rec.add("restart_version", cell(&row(&ver), 0).starts_with("17.10"), "17.10", &cell(&row(&ver), 0), &ver.state(), "durability session");
    let queued_row = pg.exec(&format!("SELECT state, attempt::text, generation::text FROM jobs.tasks WHERE id = {}::uuid", q(qid)));
    let qrow = row(&queued_row);
    rec.add("restart_queued_durable", queued_row.ok() && cell(&qrow, 0) == "queued" && cell(&qrow, 1) == "0", "queued|0", &show(&queued_row), &queued_row.state(), "committed before restart");
    let running_row = pg.exec(&format!("SELECT state, attempt::text, generation::text, lease_owner FROM jobs.tasks WHERE id = {}::uuid", q(rid)));
    let rrow = row(&running_row);
    rec.add(
        "restart_running_durable",
        running_row.ok() && cell(&rrow, 0) == "running" && cell(&rrow, 1) == rattempt && cell(&rrow, 2) == rgen,
        &format!("running|{rattempt}|{rgen}"),
        &show(&running_row),
        &running_row.state(),
        "committed running row kept across restart; expiry does not rewrite it until a later claim",
    );
    let gone = pg.exec(&format!("SELECT count(*)::text FROM jobs.tasks WHERE id = {}::uuid", q(oid)));
    rec.add("restart_open_tx_absent", cell(&row(&gone), 0) == "0", "0", &show(&gone), &gone.state(), "uncommitted enqueue rolled back by disconnect and restart");
    let mig = pg.exec("SELECT version||'='||checksum FROM jobs.schema_migrations ORDER BY version");
    rec.add("restart_checksum_rows", mig.ok() && show(&mig).contains(LEASE_H), "stored real lease hash", &show(&mig), &mig.state(), "bookkeeping survived restart");
    if rec.failed == 0 { 0 } else { 1 }
}

fn phase_full(ctx: &Ctx, rec: &mut Rec) -> i32 {
    phase(&ctx.out, 1, "identity");
    let lib = fs::read_to_string(env::var("SRCDIR").unwrap() + "/crates/oo-jobs/src/lib.rs").unwrap_or_default();
    rec.add("driver_stub_unsigned", lib.contains("not implemented"), "src/lib.rs has no driver", if lib.contains("not implemented") { "stub" } else { "unexpected" }, "", "driver remains unsigned");
    let live = fs::read_to_string(env::var("SRCDIR").unwrap() + "/crates/oo-jobs/tests/lease_operations_live.rs").unwrap_or_default();
    rec.add("author_fixture_hardcoded", live.contains("oo-a102-pg1710-jobs"), "foreign container constant present", if live.contains("oo-a102-pg1710-jobs") { "present_not_executed" } else { "absent" }, "", "scoped finding; this oracle uses PGHOST");
    let jitter_src = ctx.lease.matches("jitter").count();
    rec.add("sql_source_has_no_jitter_word", jitter_src == 0, "0", &jitter_src.to_string(), "", "docs17 jitter is a scoped omission, not a silent waiver");

    phase(&ctx.out, 2, "lie");
    if let Err(e) = reset(ctx) {
        rec.add("reset_lie", false, "reset", &e, "", "");
        return 1;
    }
    let lie = apply_fresh(ctx, LIE);
    rec.add("lie_install", lie.ok(), "commit stored lie", &format!("{} {}", lie.state(), lie.message), &lie.state(), "caller-supplied checksum is not a file hash");
    if lie.ok() {
        if let Err(e) = logins(ctx) {
            rec.add("lie_logins", false, "logins", &e, "", "");
        } else {
            let ready = one_cell(ctx, "SELECT schema_version_present::text, checksum FROM jobs.lease_operations_readiness");
            let rr = row(&ready);
            rec.add("lie_readiness_present", cell(&rr, 0) == "true" && cell(&rr, 1) == LIE, "true|lie", &show(&ready), &ready.state(), "view does not open the release file");
            let file_proof = cell(&rr, 1) == LEASE_H;
            rec.add("lie_is_not_file_proof", !file_proof, "stored checksum differs from file", &cell(&rr, 1), "", "applicator must compare bytes");
            let en = one_cell(ctx, &enq(P_MAIN, "alpha", "lie-one", 3, "clock_timestamp()", "'{\"ok\":true}'"));
            rec.add("lie_enqueue_still_works", cell(&row(&en), 0) == "enqueued", "enqueued", &show(&en), &en.state(), "not file proof");
            let mut pg = su(ctx).unwrap();
            let rep = finish(&mut pg, &repeat_sql(LEASE_H));
            rec.add("lie_repeat_real_hash_oo001", rep.state() == "OO001", "OO001", &format!("{} {}", rep.state(), rep.message), &rep.state(), "presented file hash does not match stored lie");
            let still = pg.exec("SELECT checksum FROM jobs.schema_migrations WHERE version = 'jobs-lease-operations/1'");
            rec.add("lie_checksum_unchanged", cell(&row(&still), 0) == LIE, LIE, &show(&still), &still.state(), "OO001 rolls back");
        }
    }

    phase(&ctx.out, 3, "upgrade-history");
    if let Err(e) = reset(ctx) {
        rec.add("reset_upgrade", false, "reset", &e, "", "");
        return 1;
    }
    let control = apply_control(ctx);
    rec.add("install_0000_0001", control.ok(), "commit", &format!("{} {}", control.state(), control.message), &control.state(), "same transaction as checksum insert");
    if !control.ok() {
        return 1;
    }
    if let Err(e) = logins(ctx) {
        rec.add("logins", false, "logins", &e, "", "");
        return 1;
    }
    let v1 = one_cell(ctx, &claim(P_MAIN, "ARRAY['alpha']::text[]", "worker-a"));
    rec.add("v1_claim_absent", v1.state() == "42883", "42883", &format!("{} {}", v1.state(), v1.message), &v1.state(), "jobs-control/1 does not install claim");
    let mut owner = match own(ctx) {
        Ok(v) => v,
        Err(e) => {
            rec.add("owner_connect", false, "connect", &e, "CONN", "");
            return 1;
        }
    };
    let _ = owner.exec("SET ROLE oo_jobs_owner");
    let seed = owner.exec(&format!(
        "INSERT INTO jobs.tasks (id, project_id, kind, payload_version, payload, config_version, artifact_digest, dedupe_key, max_attempts, run_at) VALUES ({}::uuid, {}::uuid, 'legal', 1, '{{\"ok\":true}}'::jsonb, 'cfg-legal', '{}', 'dedupe-legal', 3, clock_timestamp())",
        q(LEGAL_TASK), q(P_MAIN), DIGEST
    ));
    rec.add("seed_legal", seed.ok(), "insert", &format!("{} {}", seed.state(), seed.message), &seed.state(), "owner seed before 0003, not a public API call");
    let hash_tasks_before = task_hash(ctx);
    let hash_events_before = event_hash(ctx);
    let bad = owner.exec(&format!(
        "INSERT INTO jobs.task_events (project_id, task_id, to_state, generation, attempt) VALUES ({}::uuid, {}::uuid, 'queued', 0, 0)",
        q(P_BAD), q(LEGAL_TASK)
    ));
    rec.add("seed_bad_history", bad.ok(), "insert mismatched event", &format!("{} {}", bad.state(), bad.message), &bad.state(), "pre-0002 history");
    let bad_n = count_sql(ctx, &format!("SELECT count(*)::text FROM jobs.task_events WHERE project_id = {}::uuid", q(P_BAD)));
    let bind = apply_owner_file(ctx, &ctx.event, "", "");
    rec.add("bad_history_oo004", bind.state() == "OO004", "OO004", &format!("{} {}", bind.state(), bind.message), &bind.state(), "migration must not rewrite history");
    let bad_after = count_sql(ctx, &format!("SELECT count(*)::text FROM jobs.task_events WHERE project_id = {}::uuid", q(P_BAD)));
    rec.add("bad_history_not_rewritten", bad_after == bad_n && bad_after == "1", "1", &bad_after, "", "failed 0002 left the bad row");
    let fk = count_sql(ctx, "SELECT count(*)::text FROM pg_constraint WHERE conname = 'task_events_task_project_fkey'");
    rec.add("bad_history_no_fk", fk == "0", "0", &fk, "", "OO004 rolled the constraint back");
    let del = owner.exec(&format!("DELETE FROM jobs.task_events WHERE project_id = {}::uuid", q(P_BAD)));
    rec.add("owner_delete_event_rejected", del.state() == "23514", "23514", &format!("{} {}", del.state(), del.message), &del.state(), "task events are immutable; the migration is not asked to delete this row");
    let mut fixer = su(ctx).unwrap();
    let fix = fixer.exec(&format!("BEGIN; SET LOCAL session_replication_role = replica; DELETE FROM jobs.task_events WHERE project_id = {}::uuid; COMMIT;", q(P_BAD)));
    rec.add("fixture_remove_bad_row_not_migration", fix.ok() && count_sql(ctx, &format!("SELECT count(*)::text FROM jobs.task_events WHERE project_id = {}::uuid", q(P_BAD))) == "0", "superuser replica-role delete", &format!("{} {}", fix.state(), fix.message), &fix.state(), "fixture cleanup so 0002 can apply; session_replication_role is not a product API");
    rec.add("legal_hash_restored", task_hash(ctx) == hash_tasks_before && event_hash(ctx) == hash_events_before, "same md5", &format!("tasks {} events {}", task_hash(ctx), event_hash(ctx)), "", "before 0002");
    let up2 = apply_owner_file(ctx, &ctx.event, &insert_rows(&[("jobs-event-project/1", EVENT_H, "0002_event_project_binding_expand.sql")]), "");
    rec.add("install_0002", up2.ok(), "commit", &format!("{} {}", up2.state(), up2.message), &up2.state(), "");
    rec.add("hash_after_0002", task_hash(ctx) == hash_tasks_before && event_hash(ctx) == hash_events_before, "unchanged", &format!("{} {}", task_hash(ctx), event_hash(ctx)), "", "0002 does not rewrite legal rows");

    let mut as_rt = match su(ctx) {
        Ok(v) => v,
        Err(e) => {
            rec.add("su_for_oo013", false, "connect", &e, "", "");
            return 1;
        }
    };
    let oo013 = finish(&mut as_rt, &format!("BEGIN; SET LOCAL lock_timeout = '20s'; SELECT pg_advisory_xact_lock(6401202601); SELECT id FROM jobs.migration_lock WHERE id = 1 FOR UPDATE; SET ROLE a107_runtime;\n{}\nCOMMIT;\n", ctx.lease));
    rec.add("runtime_migration_oo013", oo013.state() == "OO013", "OO013", &format!("{} {}", oo013.state(), oo013.message), &oo013.state(), "runtime is not the owner");

    let stub = owner.exec("CREATE FUNCTION jobs.enqueue(uuid, text, integer, text, text, text, text, timestamptz, integer) RETURNS integer LANGUAGE sql AS $$ SELECT 1 $$");
    rec.add("plant_stub", stub.ok(), "create stub", &format!("{} {}", stub.state(), stub.message), &stub.state(), "existing name");
    let oo014 = apply_owner_file(ctx, &ctx.lease, "", "");
    rec.add("refuse_adopt_oo014", oo014.state() == "OO014", "OO014", &format!("{} {}", oo014.state(), oo014.message), &oo014.state(), "does not replace a planted definer");
    let stub_left = count_sql(ctx, "SELECT count(*)::text FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='jobs' AND p.proname='enqueue'");
    rec.add("stub_not_replaced", stub_left == "1", "1", &stub_left, "", "OO014 rollback");
    let drop_stub = owner.exec("DROP FUNCTION jobs.enqueue(uuid, text, integer, text, text, text, text, timestamptz, integer)");
    rec.add("drop_stub", drop_stub.ok(), "drop", &format!("{} {}", drop_stub.state(), drop_stub.message), &drop_stub.state(), "fixture cleanup");

    let planted = owner.exec("CREATE FUNCTION jobs.planted_a107() RETURNS integer LANGUAGE sql SECURITY DEFINER AS $$ SELECT 1 $$");
    rec.add("plant_other_definer", planted.ok(), "create other name", &format!("{} {}", planted.state(), planted.message), &planted.state(), "A068-F5 outside the checked names");
    let forced = apply_owner_file(ctx, &ctx.lease, "", "DO $$ BEGIN RAISE EXCEPTION 'forced migration failure' USING ERRCODE = 'OO003'; END $$;\n");
    rec.add("forced_oo003", forced.state() == "OO003", "OO003", &format!("{} {}", forced.state(), forced.message), &forced.state(), "bookkeeping and functions roll back together");
    let no_fun = count_sql(ctx, "SELECT count(*)::text FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='jobs' AND p.proname='claim'");
    let no_ver = count_sql(ctx, "SELECT count(*)::text FROM jobs.schema_migrations WHERE version = 'jobs-lease-operations/1'");
    rec.add("forced_no_function", no_fun == "0" && no_ver == "0", "0|0", &format!("{no_fun}|{no_ver}"), "", "OO003 rollback");
    let planted_left = count_sql(ctx, "SELECT count(*)::text FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='jobs' AND p.proname='planted_a107'");
    rec.add("planted_definer_survives_0003_guard", planted_left == "1", "1", &planted_left, "", "A068-F5 remains open");

    let real = apply_owner_file(ctx, &ctx.lease, &insert_rows(&[("jobs-lease-operations/1", LEASE_H, "0003_job_lease_operations_expand.sql")]), "");
    rec.add("install_0003", real.ok(), "commit real hash", &format!("{} {}", real.state(), real.message), &real.state(), "checksum inserted in the same transaction");
    if !real.ok() {
        return 1;
    }
    rec.add("hash_after_0003", task_hash(ctx) == hash_tasks_before && event_hash(ctx) == hash_events_before, "unchanged", &format!("{} {}", task_hash(ctx), event_hash(ctx)), "", "0003 does not rewrite legal rows");
    let oid_before = count_sql(ctx, "SELECT p.oid::text FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='jobs' AND p.proname='claim'");
    let mut pg = su(ctx).unwrap();
    let rep = finish(&mut pg, &repeat_sql(LEASE_H));
    let oid_after = count_sql(ctx, "SELECT p.oid::text FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='jobs' AND p.proname='claim'");
    rec.add("repeat_same_checksum", rep.ok() && oid_before == oid_after && !oid_before.is_empty(), "commit and same oid", &format!("{} oid {oid_before} -> {oid_after}", rep.state()), &rep.state(), "repeat does not recreate the function");
    let bad_rep = finish(&mut pg, &repeat_sql(LIE));
    rec.add("repeat_mismatch_oo001", bad_rep.state() == "OO001", "OO001", &format!("{} {}", bad_rep.state(), bad_rep.message), &bad_rep.state(), "");
    let stored = pg.exec("SELECT checksum FROM jobs.schema_migrations WHERE version = 'jobs-lease-operations/1'");
    rec.add("repeat_mismatch_keeps_hash", cell(&row(&stored), 0) == LEASE_H, LEASE_H, &show(&stored), &stored.state(), "OO001 does not rewrite the stored checksum");

    let hold_a = thread::spawn({
        let host = ctx.host.clone();
        let port = ctx.port;
        move || {
            let mut a = connect(&host, port, "postgres", "a107").unwrap();
            a.exec("BEGIN; SELECT pg_advisory_xact_lock(6401202601); SELECT pg_sleep(6); COMMIT;")
        }
    });
    thread::sleep(Duration::from_secs(1));
    let wait = finish(&mut pg, "BEGIN; SET LOCAL lock_timeout = '2s'; SELECT pg_advisory_xact_lock(6401202601); COMMIT;");
    let _ = hold_a.join();
    rec.add("concurrent_migration_55P03", wait.state() == "55P03", "55P03", &format!("{} {}", wait.state(), wait.message), &wait.state(), "advisory waiter; prior 40P01 is a different cycle");

    let hold_rt = thread::spawn({
        let host = ctx.host.clone();
        let port = ctx.port;
        move || {
            let mut a = connect(&host, port, "a107_runtime", "a107").unwrap();
            a.exec("BEGIN; SELECT pg_advisory_xact_lock(6401202601); SELECT pg_sleep(6); COMMIT;")
        }
    });
    thread::sleep(Duration::from_secs(1));
    let wait_rt = finish(&mut pg, "BEGIN; SET ROLE oo_jobs_owner; SET LOCAL lock_timeout = '2s'; SELECT pg_advisory_xact_lock(6401202601); COMMIT;");
    let rt_hold = hold_rt.join();
    let rt_hold_ok = rt_hold.as_ref().map(|o| o.ok()).unwrap_or(false);
    rec.add("runtime_can_hold_advisory_6401202601", rt_hold_ok && wait_rt.state() == "55P03", "runtime acquired and migrator 55P03", &format!("waiter {} {}", wait_rt.state(), wait_rt.message), &wait_rt.state(), "A068-F4 remains open");

    phase(&ctx.out, 4, "behavior");
    behavior(ctx, rec);
    phase(&ctx.out, 5, "expiry");
    expiry(ctx, rec);
    phase(&ctx.out, 6, "hold");
    hold(ctx, rec);
    if rec.failed == 0 { 0 } else { 1 }
}

fn repeat_sql(lease_hash: &str) -> String {
    format!(
        "BEGIN; SET LOCAL lock_timeout = '5s'; SELECT pg_advisory_xact_lock(6401202601); SELECT id FROM jobs.migration_lock WHERE id = 1 FOR UPDATE; DO $check$ DECLARE b text; e text; v text; l text; BEGIN SELECT checksum INTO b FROM jobs.schema_migrations WHERE version = 'jobs-bootstrap/1'; SELECT checksum INTO e FROM jobs.schema_migrations WHERE version = 'jobs-control/1'; SELECT checksum INTO v FROM jobs.schema_migrations WHERE version = 'jobs-event-project/1'; SELECT checksum INTO l FROM jobs.schema_migrations WHERE version = 'jobs-lease-operations/1'; IF b IS NULL OR e IS NULL OR v IS NULL OR l IS NULL THEN RAISE EXCEPTION 'migration bookkeeping incomplete' USING ERRCODE = 'OO002'; END IF; IF b <> {b} OR e <> {e} OR v <> {v} OR l <> {l} THEN RAISE EXCEPTION 'migration checksum mismatch' USING ERRCODE = 'OO001'; END IF; END $check$; COMMIT;",
        b = q(BOOT_H),
        e = q(EXPAND_H),
        v = q(EVENT_H),
        l = q(lease_hash)
    )
}

fn behavior(ctx: &Ctx, rec: &mut Rec) {
    let mut rtpg = match rt(ctx) {
        Ok(v) => v,
        Err(e) => {
            rec.add("runtime_connect", false, "connect", &e, "CONN", "");
            return;
        }
    };
    let empty = rtpg.exec(&claim(P_MAIN, "ARRAY['alpha']::text[]", "worker-a"));
    rec.add("empty_claim_none", cell(&row(&empty), 0) == "none", "none", &show(&empty), &empty.state(), "");
    let who = rtpg.exec("SELECT current_user, session_user");
    rec.add("runtime_identity", cell(&row(&who), 0) == "a107_runtime", "a107_runtime", &show(&who), &who.state(), "trusted runtime filter, not RBAC and not an agent credential");

    let catalog = su(ctx).unwrap().exec("SELECT p.proname||':'||p.prosecdef::text||':'||r.rolname||':'||COALESCE(array_to_string(p.proconfig, ','), '') FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace JOIN pg_roles r ON r.oid = p.proowner WHERE n.nspname = 'jobs' AND p.proname IN ('enqueue','claim','heartbeat','complete_success','complete_failure','retry_failure','cancel','_require_lease_install','_lease_token','_lease_result') ORDER BY 1");
    let cat = show(&catalog);
    let definers = ["enqueue", "claim", "heartbeat", "complete_success", "complete_failure", "retry_failure", "cancel"];
    let mut definer_ok = catalog.ok();
    for name in definers {
        definer_ok &= cat.contains(&format!("{name}:true:oo_jobs_owner:search_path=pg_catalog, jobs"));
    }
    definer_ok &= cat.contains("_lease_token:false:oo_jobs_owner:search_path=pg_catalog, jobs");
    definer_ok &= cat.contains("_require_lease_install:false:oo_jobs_owner:search_path=pg_catalog, jobs");
    rec.add("definer_owner_search_path", definer_ok, "seven definers and invoker helpers", &cat, &catalog.state(), "");

    let acl = su(ctx).unwrap().exec("SELECT has_function_privilege('a107_runtime','jobs.claim(uuid,text[],text)','EXECUTE')::text, has_function_privilege('public','jobs.claim(uuid,text[],text)','EXECUTE')::text, has_function_privilege('a107_runtime','jobs._lease_token(uuid,uuid,text,bigint,timestamptz)','EXECUTE')::text, has_table_privilege('a107_runtime','jobs.tasks','INSERT')::text, has_table_privilege('a107_runtime','jobs.tasks','UPDATE')::text, has_table_privilege('a107_runtime','jobs.tasks','DELETE')::text, has_table_privilege('a107_runtime','jobs.tasks','SELECT')::text, has_table_privilege('a107_runtime','jobs.task_events','INSERT')::text, has_sequence_privilege('a107_runtime','jobs.task_events_event_id_seq','USAGE')::text, has_column_privilege('a107_runtime','jobs.schedules','kind','INSERT')::text, has_column_privilege('a107_runtime','jobs.leases','owner','UPDATE')::text, has_column_privilege('a107_runtime','jobs.leases','target_fence_certified','UPDATE')::text");
    rec.add(
        "acl_shape",
        show(&acl) == "true|false|false|false|false|false|true|false|false|true|true|false",
        "true|false|false|false|false|false|true|false|false|true|true|false",
        &show(&acl),
        &acl.state(),
        "direct task and event DML revoked; schedule and leases grants remain; fence column not granted",
    );
    let dml = rtpg.exec("INSERT INTO jobs.tasks (project_id, kind, payload_version, payload, config_version, artifact_digest, dedupe_key, max_attempts, run_at) VALUES ('11111111-1111-4111-8111-111111111111', 'alpha', 1, '{\"a\":1}', 'cfg-1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'direct-no', 3, clock_timestamp())");
    rec.add("direct_task_insert_42501", dml.state() == "42501" && !dml.detail.contains(CANARY), "42501", &format!("{} {}", dml.state(), dml.message), &dml.state(), "");
    let dml_e = rtpg.exec("INSERT INTO jobs.task_events (project_id, task_id, to_state, generation, attempt) VALUES ('11111111-1111-4111-8111-111111111111', '12121212-1212-4121-8121-121212121212', 'queued', 0, 0)");
    rec.add("direct_event_insert_42501", dml_e.state() == "42501", "42501", &format!("{} {}", dml_e.state(), dml_e.message), &dml_e.state(), "");
    let upd = rtpg.exec("UPDATE jobs.tasks SET state = 'cancelled' WHERE dedupe_key = 'dedupe-legal'");
    rec.add("direct_task_update_42501", upd.state() == "42501", "42501", &format!("{} {}", upd.state(), upd.message), &upd.state(), "");
    let seq = rtpg.exec("SELECT nextval('jobs.task_events_event_id_seq')");
    rec.add("sequence_usage_42501", seq.state() == "42501", "42501", &format!("{} {}", seq.state(), seq.message), &seq.state(), "");
    let mut public = connect(&ctx.host, ctx.port, "a107_public", "a107").unwrap();
    let pub_exec = public.exec(&claim(P_MAIN, "ARRAY['alpha']::text[]", "worker-a"));
    rec.add("public_execute_42501", pub_exec.state() == "42501", "42501", &format!("{} {}", pub_exec.state(), pub_exec.message), &pub_exec.state(), "");
    let helper = rtpg.exec("SELECT jobs._lease_token('12121212-1212-4121-8121-121212121212', '11111111-1111-4111-8111-111111111111', 'worker-a', 1, clock_timestamp())");
    rec.add("helper_execute_42501", helper.state() == "42501", "42501", &format!("{} {}", helper.state(), helper.message), &helper.state(), "");

    let poison = su(ctx).unwrap().exec("CREATE FUNCTION public.clock_timestamp() RETURNS timestamptz LANGUAGE sql AS $$ SELECT NULL::timestamptz $$");
    let poisoned = rtpg.exec(&claim(P_HB, "ARRAY['alpha']::text[]", "worker-a"));
    let _ = su(ctx).unwrap().exec("DROP FUNCTION public.clock_timestamp()");
    rec.add("search_path_ignores_public_clock", poison.ok() && poisoned.state() != "42804" && cell(&row(&poisoned), 0) == "none", "none using pg_catalog clock", &format!("create {} claim {}", poison.state(), show(&poisoned)), &poisoned.state(), "definer search_path is pg_catalog, jobs");

    let sched = rtpg.exec(&format!("INSERT INTO jobs.schedules (project_id, kind, config_version, artifact_digest, dedupe_prefix, next_run_at) VALUES ({}::uuid, 'sched', 'cfg-1', '{}', 'pfx-a107', clock_timestamp())", q(P_MAIN), DIGEST));
    rec.add("schedule_insert_still_granted", sched.ok(), "insert", &format!("{} {}", sched.state(), sched.message), &sched.state(), "legacy grant remains explicit and unsigned as a replacement API");
    let fence_ins = rtpg.exec(&format!("INSERT INTO jobs.leases (project_id, target_key) VALUES ({}::uuid, 'fence-a107')", q(P_MAIN)));
    let fence_up = rtpg.exec("UPDATE jobs.leases SET target_fence_certified = true WHERE target_key = 'fence-a107'");
    let fence_read = rtpg.exec("SELECT target_fence_certified::text FROM jobs.leases WHERE target_key = 'fence-a107'");
    rec.add("fence_not_certified", fence_ins.ok() && fence_up.state() == "42501" && cell(&row(&fence_read), 0) == "false", "insert ok, update 42501, value false", &format!("ins {} up {} read {}", fence_ins.state(), fence_up.state(), show(&fence_read)), &fence_up.state(), "control lease is not a target fence");
    let owner_fence = own(ctx).unwrap().exec("UPDATE jobs.leases SET target_fence_certified = true WHERE target_key = 'fence-a107'");
    rec.add("fence_check_rejects_owner", owner_fence.state() == "23514", "23514", &format!("{} {}", owner_fence.state(), owner_fence.message), &owner_fence.state(), "column is constrained false");

    let ready = rtpg.exec("SELECT version, schema_version_present::text, checksum FROM jobs.lease_operations_readiness");
    rec.add("readiness_64_hex", cell(&row(&ready), 0) == "jobs-lease-operations/1" && cell(&row(&ready), 1) == "true" && cell(&row(&ready), 2) == LEASE_H && cell(&row(&ready), 2).len() == 64, "version present and file hash", &show(&ready), &ready.state(), "caller sees 64 hex; file compare is external");
    let svc = rtpg.exec("SELECT version, schema_version_present::text FROM jobs.service_readiness");
    rec.add("service_readiness_is_control_only", show(&svc) == "jobs-control/1|true", "jobs-control/1|true", &show(&svc), &svc.state(), "control readiness does not authorize lease functions");
    let trig = su(ctx).unwrap().exec("SELECT string_agg(tgname||':'||tgenabled::text, ',' ORDER BY tgname) FROM pg_trigger WHERE NOT tgisinternal AND tgname IN ('tasks_enforce_update','tasks_audit_insert','tasks_audit_update','task_events_reject_update','task_events_reject_delete')");
    let trig_s = show(&trig);
    rec.add("triggers_enabled", trig.ok() && trig_s.matches(":O").count() == 5, "five origin-enabled triggers", &trig_s, &trig.state(), "");

    let future = rtpg.exec(&enq(P_MAIN, "alpha", "future-1", 3, "clock_timestamp() + interval '1 day'", "'{\"ok\":true}'"));
    let beta = rtpg.exec(&enq(P_MAIN, "beta", "beta-1", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    let early = rtpg.exec(&enq(P_MAIN, "alpha", "ord-early", 3, "clock_timestamp() - interval '30 seconds'", "'{\"ok\":true}'"));
    let late = rtpg.exec(&enq(P_MAIN, "alpha", "ord-late", 3, "clock_timestamp() - interval '5 seconds'", "'{\"ok\":true}'"));
    let c1 = rtpg.exec(&claim(P_MAIN, "ARRAY['alpha']::text[]", "worker-a"));
    let c1r = row(&c1);
    rec.add("claim_due_order", cell(&c1r, 0) == "claimed" && cell(&c1r, 1) == cell(&row(&early), 1) && gap_ok_30(&cell(&c1r, 6)) && cell(&c1r, 2) == "running" && cell(&c1r, 3) == "1" && cell(&c1r, 4) == "1" && cell(&c1r, 5) == "worker-a", "earliest alpha claimed for 30s", &show(&c1), &c1.state(), "DB clock, not caller TTL");
    let c_beta = rtpg.exec(&claim(P_MAIN, "ARRAY['alpha']::text[]", "worker-a"));
    rec.add("nonallowed_not_picked_before_later_alpha", cell(&row(&c_beta), 1) == cell(&row(&late), 1), "second alpha", &show(&c_beta), &c_beta.state(), "beta remains");
    let c_none = rtpg.exec(&claim(P_MAIN, "ARRAY['alpha']::text[]", "worker-a"));
    rec.add("future_not_picked", cell(&row(&c_none), 0) == "none", "none", &show(&c_none), &c_none.state(), "future and empty selection");
    let c_beta2 = rtpg.exec(&claim(P_MAIN, "ARRAY['beta']::text[]", "worker-b"));
    rec.add("allowed_kind_picks_beta", cell(&row(&c_beta2), 1) == cell(&row(&beta), 1) && cell(&row(&c_beta2), 0) == "claimed", "beta claimed", &show(&c_beta2), &c_beta2.state(), "");
    let _ = future;

    let same_a = rtpg.exec(&enq(P_MAIN, "alpha", "same-a", 3, "'2020-01-01 00:00:00+00'::timestamptz", "'{\"ok\":true}'"));
    let same_b = rtpg.exec(&enq(P_MAIN, "alpha", "same-b", 3, "'2020-01-01 00:00:00+00'::timestamptz", "'{\"ok\":true}'"));
    let expect_id = count_sql(ctx, "SELECT id::text FROM jobs.tasks WHERE dedupe_key IN ('same-a','same-b') ORDER BY run_at, id LIMIT 1");
    let same = rtpg.exec(&claim(P_MAIN, "ARRAY['alpha']::text[]", "worker-a"));
    rec.add("tie_break_id", cell(&row(&same), 1) == expect_id && cell(&row(&same), 0) == "claimed", &expect_id, &show(&same), &same.state(), "order run_at, id");
    let _ = (same_a, same_b);

    let other = rtpg.exec(&enq(P_OTHER, "alpha", "ord-early", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    rec.add("dedupe_other_project", cell(&row(&other), 0) == "enqueued", "enqueued", &show(&other), &other.state(), "project is part of identity");
    let cross = rtpg.exec(&claim(P_MAIN, "ARRAY['alpha']::text[]", "worker-a"));
    rec.add("claim_does_not_cross_project", cell(&row(&cross), 0) == "none" || cell(&row(&cross), 1) != cell(&row(&other), 1), "not the other project", &show(&cross), &cross.state(), "there may be a remaining main-project tie");

    let replay = rtpg.exec(&enq(P_MAIN, "alpha", "replay-1", 3, "clock_timestamp() + interval '2 days'", "'{\"ok\":true}'"));
    let replay2 = rtpg.exec(&enq(P_MAIN, "alpha", "replay-1", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    let run_at = count_sql(ctx, "SELECT count(*)::text FROM jobs.tasks WHERE dedupe_key = 'replay-1'");
    let replay_gap: f64 = cell(&row(&replay2), 8).parse().unwrap_or(0.0);
    rec.add("replay_ignores_run_at", cell(&row(&replay), 0) == "enqueued" && cell(&row(&replay2), 0) == "replayed" && cell(&row(&replay2), 1) == cell(&row(&replay), 1) && run_at == "1" && replay_gap > 170000.0, "replayed same id and original run_at", &format!("{} ; {} count {run_at} gap {replay_gap}", show(&replay), show(&replay2)), &replay2.state(), "max_attempts stays 3; run_at is not dedupe identity");
    let before_conflict = count_sql(ctx, "SELECT count(*)::text FROM jobs.tasks");
    let before_ev = count_sql(ctx, "SELECT count(*)::text FROM jobs.task_events");
    let conflict = rtpg.exec(&enq(P_MAIN, "alpha", "replay-1", 3, "clock_timestamp()", "'{\"ok\":false}'"));
    let after_conflict = count_sql(ctx, "SELECT count(*)::text FROM jobs.tasks");
    let after_ev = count_sql(ctx, "SELECT count(*)::text FROM jobs.task_events");
    rec.add("conflict_side_effect_free", conflict.state() == "OO011" && before_conflict == after_conflict && before_ev == after_ev && !conflict.detail.contains(CANARY), "OO011 and counts unchanged", &format!("{} {} counts {before_conflict}/{after_conflict} events {before_ev}/{after_ev}", conflict.state(), conflict.message), &conflict.state(), "");

    negatives(ctx, rec, &mut rtpg);
    lifecycle(ctx, rec, &mut rtpg);
    concurrency(ctx, rec);
    locks_and_rollback(ctx, rec);
    deadlock(ctx, rec);
}

fn negatives(ctx: &Ctx, rec: &mut Rec, rtpg: &mut Pg) {
    let long_payload = format!("{{\"k\":\"{}\"}}", "x".repeat(16380));
    let stored_exact = format!("{{\"k\":\"{}\"}}", "x".repeat(16375));
    let input_over = format!("{{\"k\":\"{}\"}}", "x".repeat(16376));
    let cases = [
        ("neg_null_project", enq_raw("NULL", "'alpha'", "'n1'", "3", "clock_timestamp()", "'{\"a\":1}'"), true),
        ("neg_null_kinds", claim(P_MAIN, "NULL::text[]", "worker-a"), true),
        ("neg_empty_kinds", claim(P_MAIN, "ARRAY[]::text[]", "worker-a"), true),
        ("neg_null_kind_element", claim(P_MAIN, "ARRAY['alpha', NULL]::text[]", "worker-a"), true),
        ("neg_dup_kinds", claim(P_MAIN, "ARRAY['alpha','alpha']::text[]", "worker-a"), true),
        ("neg_65", claim(P_MAIN, &kind_array(65), "worker-a"), true),
        ("neg_worker_space", claim(P_MAIN, "ARRAY['alpha']::text[]", &format!("x {CANARY}")), true),
        ("neg_worker_unicode", claim(P_MAIN, "ARRAY['alpha']::text[]", "wörker"), true),
        ("neg_worker_long", claim(P_MAIN, "ARRAY['alpha']::text[]", &"w".repeat(129)), true),
        ("neg_max_0", enq(P_MAIN, "alpha", "max0", 0, "clock_timestamp()", "'{\"a\":1}'"), true),
        ("neg_max_33", enq(P_MAIN, "alpha", "max33", 33, "clock_timestamp()", "'{\"a\":1}'"), true),
        ("neg_bad_digest", enq_raw(&q(P_MAIN), "'alpha'", "'bad-digest'", "3", "clock_timestamp()", "'{\"a\":1}'").replace(DIGEST, "abcd"), true),
        ("neg_array_payload", enq(P_MAIN, "alpha", "arr", 3, "clock_timestamp()", "'[1,2]'"), true),
        ("neg_bad_json", enq(P_MAIN, "alpha", "badjson", 3, "clock_timestamp()", &dollar("j", &format!("{{{CANARY}"))), true),
        ("neg_password", enq(P_MAIN, "alpha", "pw", 3, "clock_timestamp()", &dollar("p", &format!("{{\"password\":\"{CANARY}\"}}"))), true),
        ("neg_api_key", enq(P_MAIN, "alpha", "api", 3, "clock_timestamp()", &dollar("a", &format!("{{\"api_key\":\"{CANARY}\"}}"))), true),
        ("neg_secret", enq(P_MAIN, "alpha", "sec", 3, "clock_timestamp()", &dollar("s", &format!("{{\"secret\":\"{CANARY}\"}}"))), true),
        ("neg_kind_canary", enq(P_MAIN, &format!("x{CANARY}"), "kindc", 3, "clock_timestamp()", "'{\"a\":1}'"), true),
        ("neg_infinity", enq(P_MAIN, "alpha", "inf", 3, "'infinity'::timestamptz", "'{\"a\":1}'"), true),
        ("neg_oversize", enq(P_MAIN, "alpha", "over", 3, "clock_timestamp()", &dollar("o", &long_payload)), true),
    ];
    for (id, sql, _) in cases {
        let o = rtpg.exec(&sql);
        let leaked = o.message.contains(CANARY) || o.detail.contains(CANARY);
        rec.add(id, o.state() == "OO010" && o.message == "invalid lease operation input" && !leaked, "OO010 static message", &format!("{} {}", o.state(), o.message), &o.state(), "canary must not be echoed");
    }
    let stored_len = count_sql(ctx, &format!("SELECT octet_length({}::jsonb::text)::text", dollar("e", &stored_exact)));
    let exact = rtpg.exec(&enq(P_MAIN, "alpha", "exact-bytes", 3, "clock_timestamp()", &dollar("e", &stored_exact)));
    let exact_status = cell(&row(&exact), 0);
    rec.add("payload_stored_16384_enqueued", stored_len == "16384" && exact.ok() && exact_status == "enqueued", "enqueued when stored jsonb text is 16384", &format!("{} stored {stored_len} input {}", show(&exact), stored_exact.len()), &exact.state(), "contract caps the stored jsonb object; PostgreSQL inserts one space after the colon");
    let over_canon = rtpg.exec(&enq(P_MAIN, "alpha", "canon-over", 3, "clock_timestamp()", &dollar("c", &input_over)));
    rec.add("payload_input_16384_stored_16385_oo010", over_canon.state() == "OO010" && input_over.len() == 16384, "OO010", &format!("{} input {}", over_canon.state(), input_over.len()), &over_canon.state(), "compact input is 16384 and canonical jsonb text is 16385, over the stored cap");
    let nested = rtpg.exec(&enq(P_MAIN, "alpha", "nested-secret", 3, "clock_timestamp()", &dollar("n", &format!("{{\"outer\":{{\"password\":\"{CANARY}\"}}}}"))));
    rec.add("nested_secret_not_scanned", cell(&row(&nested), 0) == "enqueued", "enqueued", &show(&nested), &nested.state(), "contract says nested keys are not scanned; scoped residual, not a waiver of a secret scanner");
    let kinds64 = rtpg.exec(&enq(P_MAIN, "k00", "k64-row", 3, "clock_timestamp()", "'{\"a\":1}'"));
    let claim64 = rtpg.exec(&claim(P_MAIN, &kind_array(64), "worker-64"));
    rec.add("kinds_64_allowed", cell(&row(&kinds64), 0) == "enqueued" && cell(&row(&claim64), 0) == "claimed" && cell(&row(&claim64), 1) == cell(&row(&kinds64), 1), "claimed k00", &show(&claim64), &claim64.state(), "");
    let gen0 = rtpg.exec(&cols("heartbeat", P_MAIN, LEGAL_TASK, "worker-a", "0", ""));
    rec.add("heartbeat_generation_below_1_oo010", gen0.state() == "OO010", "OO010", &format!("{} {}", gen0.state(), gen0.message), &gen0.state(), "not lost_lease");
}

fn enq_raw(project: &str, kind: &str, dedupe: &str, max: &str, run_at: &str, payload: &str) -> String {
    format!(
        "SELECT status FROM jobs.enqueue({project}, {kind}, 1, {payload}, 'cfg-1', '{DIGEST}', {dedupe}, {run_at}, {max})"
    )
}

fn kind_array(n: usize) -> String {
    let mut parts = Vec::new();
    for i in 0..n {
        parts.push(format!("'k{i:02}'"));
    }
    format!("ARRAY[{}]::text[]", parts.join(","))
}

fn lifecycle(ctx: &Ctx, rec: &mut Rec, rtpg: &mut Pg) {
    let en = rtpg.exec(&enq(P_LIFE, "alpha", "life-1", 4, "clock_timestamp()", "'{\"ok\":true}'"));
    let id = cell(&row(&en), 1);
    let events_before = count_sql(ctx, &format!("SELECT count(*)::text FROM jobs.task_events WHERE task_id = {}::uuid", q(&id)));
    let cl = rtpg.exec(&claim(P_LIFE, "ARRAY['alpha']::text[]", "worker-life"));
    let cr = row(&cl);
    let until = count_sql(ctx, &format!("SELECT to_char(lease_until AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') FROM jobs.tasks WHERE id = {}::uuid", q(&id)));
    let token = cell(&cr, 7);
    let newlines = token.chars().filter(|c| *c == '\n').count();
    let decoded_ok = token_matches(&token, &cell(&cr, 1), P_LIFE, "worker-life", &cell(&cr, 4), &until);
    rec.add("claim_token_unauthenticated_binding", cell(&cr, 0) == "claimed" && cell(&cr, 1) == id && token.starts_with("jlt1.") && decoded_ok, "jlt1 base64 of task|project|owner|generation|utc deadline", &format!("newlines={newlines} prefix={} decoded={decoded_ok}", token.starts_with("jlt1.")), &cl.state(), "callers do not present the token; CAS uses columns");
    let mut lock_probe = su(ctx).unwrap();
    let free = lock_probe.exec(&format!("BEGIN; SELECT id::text FROM jobs.tasks WHERE id = {}::uuid FOR UPDATE NOWAIT; ROLLBACK;", q(&id)));
    rec.add("autocommit_claim_no_leftover_tuple_lock", cell(&cr, 0) == "claimed" && cell(&cr, 1) == id && free.ok() && cell(&row(&free), 0) == id, "superuser NOWAIT succeeds while the runtime session is still open", &format!("{} {}", free.state(), show(&free)), &free.state(), "runtime lacks UPDATE, so it cannot probe FOR UPDATE; granted tuple locks are not listed in pg_locks");
    let presented = rtpg.exec(&cols("complete_success", P_LIFE, &id, &token, &cell(&cr, 4), ""));
    rec.add("token_not_accepted_as_worker", presented.state() == "OO010" || cell(&row(&presented), 0) == "lost_lease", "rejected", &format!("{} {}", presented.state(), show(&presented)), &presented.state(), "no token argument on complete_success");
    let beat_events_before = count_sql(ctx, &format!("SELECT count(*)::text FROM jobs.task_events WHERE task_id = {}::uuid", q(&id)));
    let beat = rtpg.exec(&cols("heartbeat", P_LIFE, &id, "worker-life", &cell(&cr, 4), ""));
    let beat_events_after = count_sql(ctx, &format!("SELECT count(*)::text FROM jobs.task_events WHERE task_id = {}::uuid", q(&id)));
    rec.add("heartbeat_extends_30_without_event", cell(&row(&beat), 0) == "extended" && gap_ok_30(&cell(&row(&beat), 6)) && beat_events_before == beat_events_after && cell(&row(&beat), 3) == cell(&cr, 3) && cell(&row(&beat), 4) == cell(&cr, 4), "extended ~30s and same event count", &format!("{} events {beat_events_before}->{beat_events_after}", show(&beat)), &beat.state(), "heartbeat poll of 10s is driver scope and is not this lease length");
    let wrong_worker = rtpg.exec(&cols("complete_success", P_LIFE, &id, "worker-other", &cell(&cr, 4), ""));
    let wrong_gen = rtpg.exec(&cols("complete_success", P_LIFE, &id, "worker-life", "99", ""));
    let wrong_project = rtpg.exec(&cols("complete_success", P_OTHER, &id, "worker-life", &cell(&cr, 4), ""));
    rec.add("cas_wrong_worker_gen_project", cell(&row(&wrong_worker), 0) == "lost_lease" && cell(&row(&wrong_gen), 0) == "lost_lease" && cell(&row(&wrong_project), 0) == "lost_lease", "lost_lease three ways", &format!("{} | {} | {}", show(&wrong_worker), show(&wrong_gen), show(&wrong_project)), "", "no resurrection");
    let bad_kind = rtpg.exec(&format!("SELECT status FROM jobs.complete_failure({}::uuid, {}::uuid, 'worker-life', {}, {})", q(P_LIFE), q(&id), cell(&cr, 4), dollar("k", CANARY)));
    rec.add("unsupported_error_not_stored", bad_kind.state() == "OO010" && !bad_kind.detail.contains(CANARY) && count_sql(ctx, &format!("SELECT state FROM jobs.tasks WHERE id = {}::uuid", q(&id))) == "running", "OO010 and still running", &format!("{} {}", bad_kind.state(), bad_kind.message), &bad_kind.state(), "");
    let _ = events_before;

    let mut tx = rt(ctx).unwrap();
    let _ = tx.exec("BEGIN");
    let done = tx.exec(&cols("complete_success", P_LIFE, &id, "worker-life", &cell(&cr, 4), ""));
    let ev_in = tx.exec(&format!("SELECT count(*)::text FROM jobs.task_events WHERE task_id = {}::uuid AND to_state = 'succeeded'", q(&id)));
    let _ = tx.exec("ROLLBACK");
    let state_after = count_sql(ctx, &format!("SELECT state FROM jobs.tasks WHERE id = {}::uuid", q(&id)));
    let ev_after = count_sql(ctx, &format!("SELECT count(*)::text FROM jobs.task_events WHERE task_id = {}::uuid AND to_state = 'succeeded'", q(&id)));
    rec.add("success_event_atomic_rollback", cell(&row(&done), 0) == "succeeded" && cell(&row(&ev_in), 0) == "1" && state_after == "running" && ev_after == "0", "rollback restores task and event", &format!("in {} state {state_after} events {ev_after}", show(&done)), &done.state(), "");

    let ok = rtpg.exec(&cols("complete_success", P_LIFE, &id, "worker-life", &cell(&cr, 4), ""));
    let again = rtpg.exec(&cols("complete_success", P_LIFE, &id, "worker-life", &cell(&cr, 4), ""));
    let anygen = rtpg.exec(&cols("complete_success", P_LIFE, &id, "worker-life", "1", ""));
    let blob = format!("{} {} {}", show(&ok), show(&again), show(&anygen));
    rec.add("terminal_not_already_applied", cell(&row(&ok), 0) == "succeeded" && cell(&row(&again), 0) == "terminal" && cell(&row(&anygen), 0) == "terminal" && !blob.contains("already_applied"), "terminal, never already_applied", &blob, "", "no guessed successful replay");

    let q = rtpg.exec(&enq(P_LIFE, "alpha", "cancel-q", 3, "clock_timestamp() + interval '3 days'", "'{\"ok\":true}'"));
    let qid = cell(&row(&q), 1);
    let cq = rtpg.exec(&cols("cancel", P_LIFE, &qid, "someone-else", "0", ""));
    let cq2 = rtpg.exec(&cols("cancel", P_LIFE, &qid, "someone-else", "0", ""));
    rec.add("cancel_queued_generation", cell(&row(&cq), 0) == "cancelled" && cell(&row(&cq), 9).is_empty() && cell(&row(&cq2), 0) == "terminal", "cancelled then terminal", &format!("{} ; {}", show(&cq), show(&cq2)), &cq.state(), "queued cancel checks generation; worker is format-only");
    let stale_q = rtpg.exec(&cols("complete_success", P_LIFE, &qid, "someone-else", "1", ""));
    rec.add("queued_complete_lost", cell(&row(&stale_q), 0) == "terminal" && !show(&stale_q).contains("already_applied"), "terminal", &show(&stale_q), &stale_q.state(), "generation 0 is OO010 before the row is read; generation 1 reaches the cancelled row and returns terminal");

    retry_curve(ctx, rec, rtpg);
    business(ctx, rec);
}

fn retry_curve(ctx: &Ctx, rec: &mut Rec, rtpg: &mut Pg) {
    let en = rtpg.exec(&enq(P_RETRY, "alpha", "retry-1", 8, "clock_timestamp()", "'{\"ok\":true}'"));
    let id = cell(&row(&en), 1);
    let mut gaps = Vec::new();
    for attempt in 1..=7 {
        let cl = rtpg.exec(&claim(P_RETRY, "ARRAY['alpha']::text[]", "worker-retry"));
        let cr = row(&cl);
        let picked = cell(&cr, 1) == id && cell(&cr, 3) == attempt.to_string();
        let ry = rtpg.exec(&format!(
            "SELECT status, COALESCE(attempt::text,''), COALESCE(generation::text,''), COALESCE(state,''), COALESCE(EXTRACT(EPOCH FROM (run_at - clock_timestamp()))::text,''), COALESCE(lease_owner,'') FROM jobs.retry_failure({}::uuid, {}::uuid, 'worker-retry', {}, 'worker_failed')",
            q(P_RETRY), q(&id), cell(&cr, 4)
        ));
        let rr = row(&ry);
        let ok = picked && cell(&rr, 0) == "retried" && cell(&rr, 1) == attempt.to_string() && cell(&rr, 3) == "queued" && cell(&rr, 5).is_empty() && delay_ok(&cell(&rr, 4), attempt);
        gaps.push(cell(&rr, 4));
        rec.add(&format!("retry_attempt_{attempt}"), ok, &format!("retried delay {}", delay_for(attempt)), &format!("claim {} retry {}", show(&cl), show(&ry)), &ry.state(), "attempt increments on the next claim only; delay is server-side and has no jitter");
        if attempt < 7 {
            let pull = own(ctx).unwrap().exec(&format!("UPDATE jobs.tasks SET run_at = clock_timestamp() WHERE id = {}::uuid AND state = 'queued'", q(&id)));
            rec.add(&format!("owner_pull_run_at_{attempt}"), pull.ok(), "owner sets run_at to now", &format!("{} {}", pull.state(), pull.message), &pull.state(), "clock_source=owner_seed_not_public_api");
        }
    }
    let gap_diff = gaps.first().and_then(|g| g.parse::<f64>().ok()).unwrap_or(0.0)
        - gaps.get(0).and_then(|g| g.parse::<f64>().ok()).unwrap_or(0.0);
    let _ = gap_diff;
    let cl = rtpg.exec(&claim(P_RETRY, "ARRAY['alpha']::text[]", "worker-retry"));
    let pull = own(ctx).unwrap().exec(&format!("UPDATE jobs.tasks SET run_at = clock_timestamp() WHERE id = {}::uuid AND state = 'queued'", q(&id)));
    let _ = pull;
    let cl8 = rtpg.exec(&claim(P_RETRY, "ARRAY['alpha']::text[]", "worker-retry"));
    let exhausted = rtpg.exec(&format!(
        "SELECT status FROM jobs.retry_failure({}::uuid, {}::uuid, 'worker-retry', {}, 'dependency_unavailable')",
        q(P_RETRY), q(&id), cell(&row(&cl8), 4)
    ));
    let err = count_sql(ctx, &format!("SELECT state||'|'||COALESCE(last_error,'') FROM jobs.tasks WHERE id = {}::uuid", q(&id)));
    rec.add("retry_cap_exhausted", cell(&row(&cl8), 3) == "8" && cell(&row(&exhausted), 0) == "failed" && err == "failed|attempt_exhausted", "failed|attempt_exhausted", &format!("{} row {err}", show(&exhausted)), &exhausted.state(), "unsupported text is a different case; this kind is allowlisted and the stored error is the internal cap");
    let again = rtpg.exec(&format!(
        "SELECT status FROM jobs.retry_failure({}::uuid, {}::uuid, 'worker-retry', {}, 'worker_failed')",
        q(P_RETRY), q(&id), cell(&row(&cl8), 4)
    ));
    rec.add("retry_terminal_no_replay", cell(&row(&again), 0) == "terminal", "terminal", &show(&again), &again.state(), "");
    let resurrect = rtpg.exec(&claim(P_RETRY, "ARRAY['alpha']::text[]", "worker-retry"));
    rec.add("failed_not_resurrected", cell(&row(&resurrect), 0) == "none", "none", &show(&resurrect), &resurrect.state(), "attempt cap failure is terminal");
    let _ = cl;
    let second = rtpg.exec(&enq(P_RETRY, "alpha", "retry-2", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    let id2 = cell(&row(&second), 1);
    let cl2 = rtpg.exec(&claim(P_RETRY, "ARRAY['alpha']::text[]", "worker-retry"));
    let ry2 = rtpg.exec(&format!(
        "SELECT COALESCE(EXTRACT(EPOCH FROM (run_at - clock_timestamp()))::text,'') FROM jobs.retry_failure({}::uuid, {}::uuid, 'worker-retry', {}, 'external_timeout')",
        q(P_RETRY), q(&id2), cell(&row(&cl2), 4)
    ));
    let g1: f64 = gaps.first().and_then(|g| g.parse().ok()).unwrap_or(-1.0);
    let g2: f64 = cell(&row(&ry2), 0).parse().unwrap_or(-2.0);
    rec.add("retry_delay_deterministic", delay_ok(&format!("{g1}"), 1) && delay_ok(&format!("{g2}"), 1) && (g1 - g2).abs() < 0.5, "both about 1s", &format!("{g1} {g2}"), "", "no jitter sample; docs17 jitter remains unsigned");
}

fn business(ctx: &Ctx, rec: &mut Rec) {
    let mut su = su(ctx).unwrap();
    let mk = su.exec("CREATE TABLE public.a107_biz (id integer PRIMARY KEY, note text NOT NULL); INSERT INTO public.a107_biz VALUES (1, 'before'); GRANT SELECT, UPDATE ON public.a107_biz TO a107_runtime;");
    rec.add("business_table", mk.ok(), "created", &format!("{} {}", mk.state(), mk.message), &mk.state(), "caller business table, not part of jobs schema");
    let mut tx = rt(ctx).unwrap();
    let _ = tx.exec("BEGIN");
    let _ = tx.exec("UPDATE public.a107_biz SET note = 'after' WHERE id = 1");
    let en = tx.exec(&enq(P_MAIN, "alpha", "biz-rollback", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    let _ = tx.exec("ROLLBACK");
    let note = count_sql(ctx, "SELECT note FROM public.a107_biz WHERE id = 1");
    let n = count_sql(ctx, "SELECT count(*)::text FROM jobs.tasks WHERE dedupe_key = 'biz-rollback'");
    let ev = count_sql(ctx, "SELECT count(*)::text FROM jobs.task_events e JOIN jobs.tasks t ON t.id = e.task_id WHERE t.dedupe_key = 'biz-rollback'");
    rec.add("business_enqueue_rollback", cell(&row(&en), 0) == "enqueued" && note == "before" && n == "0" && ev == "0", "neither business write nor task nor event", &format!("note {note} tasks {n} events {ev}"), &en.state(), "");
    let _ = tx.exec("BEGIN");
    let _ = tx.exec("UPDATE public.a107_biz SET note = 'committed' WHERE id = 1");
    let en2 = tx.exec(&enq(P_MAIN, "alpha", "biz-commit", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    let _ = tx.exec("COMMIT");
    let note2 = count_sql(ctx, "SELECT note FROM public.a107_biz WHERE id = 1");
    let n2 = count_sql(ctx, "SELECT count(*)::text FROM jobs.tasks WHERE dedupe_key = 'biz-commit'");
    rec.add("business_enqueue_commit", cell(&row(&en2), 0) == "enqueued" && note2 == "committed" && n2 == "1", "both remain", &format!("note {note2} tasks {n2}"), &en2.state(), "function does not commit on its own");
}

fn concurrency(ctx: &Ctx, rec: &mut Rec) {
    let mut rtpg = rt(ctx).unwrap();
    for i in 0..100 {
        let o = rtpg.exec(&enq(P_BATCH, "batch", &format!("batch-{i:03}"), 3, "clock_timestamp() - interval '1 second'", "'{\"n\":1}'"));
        if cell(&row(&o), 0) != "enqueued" {
            rec.add("batch_enqueue", false, "100 enqueued", &format!("{i} {}", show(&o)), &o.state(), "");
            return;
        }
    }
    rec.add("batch_enqueue", true, "100 enqueued", "100", "", "serial setup before the concurrent claims");
    let barrier = Arc::new(Barrier::new(100));
    let claimed_b = Arc::new(Barrier::new(101));
    let release_b = Arc::new(Barrier::new(101));
    let rows = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();
    for i in 0..100 {
        let barrier = Arc::clone(&barrier);
        let claimed_b = Arc::clone(&claimed_b);
        let release_b = Arc::clone(&release_b);
        let rows = Arc::clone(&rows);
        let host = ctx.host.clone();
        let port = ctx.port;
        handles.push(thread::spawn(move || {
            let conn = connect(&host, port, "a107_runtime", "a107");
            barrier.wait();
            let mut pg = match conn {
                Ok(v) => v,
                Err(e) => {
                    rows.lock().unwrap().push(format!("ERR|CONN|{e}"));
                    claimed_b.wait();
                    release_b.wait();
                    return;
                }
            };
            let o = pg.exec(&claim(P_BATCH, "ARRAY['batch']::text[]", &format!("w{i:03}")));
            let r = row(&o);
            rows.lock().unwrap().push(format!("{}|{}|{}|{}|{}|{}|{}", cell(&r, 0), cell(&r, 1), cell(&r, 12), cell(&r, 3), cell(&r, 4), cell(&r, 5), o.state()));
            claimed_b.wait();
            release_b.wait();
            drop(pg);
        }));
    }
    claimed_b.wait();
    let got = rows.lock().unwrap().clone();
    let mut ids = Vec::new();
    let mut pids = Vec::new();
    let mut bad = 0u32;
    let mut lock_bad = 0u32;
    let mut su_probe = su(ctx).unwrap();
    for line in &got {
        let p: Vec<&str> = line.split('|').collect();
        if p.first() != Some(&"claimed") || p.get(6) != Some(&"") {
            bad += 1;
        }
        if p.first() == Some(&"claimed") {
            let id = p.get(1).copied().unwrap_or("").to_string();
            ids.push(id.clone());
            pids.push(p.get(2).copied().unwrap_or("").to_string());
            let probe = su_probe.exec(&format!("BEGIN; SELECT id::text FROM jobs.tasks WHERE id = {}::uuid FOR UPDATE NOWAIT; ROLLBACK;", q(&id)));
            if !(probe.ok() && cell(&row(&probe), 0) == id) {
                lock_bad += 1;
                let _ = su_probe.exec("ROLLBACK");
            }
        }
    }
    release_b.wait();
    for h in handles {
        let _ = h.join();
    }
    ids.sort();
    ids.dedup();
    pids.sort();
    pids.dedup();
    let db = count_sql(ctx, "SELECT count(*)::text||'|'||count(*) FILTER (WHERE state='running')::text||'|'||count(*) FILTER (WHERE attempt=1)::text||'|'||count(*) FILTER (WHERE generation=1)::text||'|'||count(DISTINCT lease_owner)::text FROM jobs.tasks WHERE dedupe_key LIKE 'batch-%'");
    let ev = count_sql(ctx, "SELECT count(*) FILTER (WHERE e.to_state='running')::text||'|'||count(*) FILTER (WHERE e.project_id <> t.project_id)::text FROM jobs.task_events e JOIN jobs.tasks t ON t.id = e.task_id WHERE t.dedupe_key LIKE 'batch-%'");
    let ok = got.len() == 100 && ids.len() == 100 && pids.len() == 100 && bad == 0 && lock_bad == 0 && db == "100|100|100|100|100" && ev.starts_with("100|0");
    rec.add("claim_100_sessions", ok, "100 distinct pids, tasks, owners; attempt 1 generation 1; events match project; no leftover tuple locks", &format!("lines {} ids {} pids {} bad {bad} lock_bad {lock_bad} db {db} events {ev}", got.len(), ids.len(), pids.len()), "", "primitive sessions, not 100 whole workers");
    let mut none = 0;
    for i in 0..20 {
        let o = rtpg.exec(&claim(P_BATCH, "ARRAY['batch']::text[]", &format!("extra-{i}")));
        if cell(&row(&o), 0) == "none" {
            none += 1;
        }
    }
    rec.add("second_wave_none", none == 20, "20 none", &none.to_string(), "", "each identity once");

    let one = rtpg.exec(&enq(P_BATCH, "batch", "one-job", 3, "clock_timestamp()", "'{\"n\":1}'"));
    let one_id = cell(&row(&one), 1);
    let barrier = Arc::new(Barrier::new(10));
    let rows = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();
    for i in 0..10 {
        let barrier = Arc::clone(&barrier);
        let rows = Arc::clone(&rows);
        let host = ctx.host.clone();
        let port = ctx.port;
        handles.push(thread::spawn(move || {
            let conn = connect(&host, port, "a107_runtime", "a107");
            barrier.wait();
            let mut pg = match conn {
                Ok(v) => v,
                Err(e) => {
                    rows.lock().unwrap().push(format!("ERR|{e}"));
                    return;
                }
            };
            let o = pg.exec(&claim(P_BATCH, "ARRAY['batch']::text[]", &format!("solo-{i}")));
            rows.lock().unwrap().push(cell(&row(&o), 0) + "|" + &cell(&row(&o), 1));
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    let got = rows.lock().unwrap().clone();
    let wins = got.iter().filter(|s| s.starts_with("claimed|") && s.ends_with(&one_id)).count();
    let nones = got.iter().filter(|s| s.starts_with("none|")).count();
    rec.add("ten_sessions_one_job", wins == 1 && nones == 9, "1 claimed and 9 none", &format!("{wins} {nones} {}", got.join(",")), "", "SKIP LOCKED");

    let race_en = rtpg.exec(&enq(P_RACE, "alpha", "race-1", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    let race_id = cell(&row(&race_en), 1);
    let race_cl = rtpg.exec(&claim(P_RACE, "ARRAY['alpha']::text[]", "worker-race"));
    let gen = cell(&row(&race_cl), 4);
    let barrier = Arc::new(Barrier::new(2));
    let left_b = Arc::clone(&barrier);
    let right_b = Arc::clone(&barrier);
    let host_l = ctx.host.clone();
    let host_r = ctx.host.clone();
    let port = ctx.port;
    let id_l = race_id.clone();
    let id_r = race_id.clone();
    let gen_l = gen.clone();
    let gen_r = gen.clone();
    let left = thread::spawn(move || {
        let conn = connect(&host_l, port, "a107_runtime", "a107");
        left_b.wait();
        let mut pg = match conn {
            Ok(v) => v,
            Err(e) => return format!("ERR {e}"),
        };
        let o = pg.exec(&cols("complete_success", P_RACE, &id_l, "worker-race", &gen_l, ""));
        format!("{} {}", cell(&row(&o), 0), o.state())
    });
    let right = thread::spawn(move || {
        let conn = connect(&host_r, port, "a107_runtime", "a107");
        right_b.wait();
        let mut pg = match conn {
            Ok(v) => v,
            Err(e) => return format!("ERR {e}"),
        };
        let o = pg.exec(&cols("cancel", P_RACE, &id_r, "worker-race", &gen_r, ""));
        format!("{} {}", cell(&row(&o), 0), o.state())
    });
    let l = left.join().unwrap_or_else(|_| "join".into());
    let r = right.join().unwrap_or_else(|_| "join".into());
    let final_state = count_sql(ctx, &format!("SELECT state FROM jobs.tasks WHERE id = {}::uuid", q(&race_id)));
    let term_events = count_sql(ctx, &format!("SELECT count(*)::text FROM jobs.task_events WHERE task_id = {}::uuid AND to_state IN ('succeeded','cancelled')", q(&race_id)));
    let winners = [&l, &r].iter().filter(|s| s.starts_with("succeeded") || s.starts_with("cancelled")).count();
    rec.add("cancel_complete_race", winners == 1 && term_events == "1" && (final_state == "succeeded" || final_state == "cancelled"), "one terminal transition", &format!("left {l} right {r} state {final_state} events {term_events}"), "", "later CAS is lost");
}

fn locks_and_rollback(ctx: &Ctx, rec: &mut Rec) {
    let mut tx = rt(ctx).unwrap();
    let en = tx.exec(&enq(P_LOCK, "alpha", "lock-1", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    let en_id = cell(&row(&en), 1);
    let _ = tx.exec("BEGIN");
    let cl = tx.exec(&claim(P_LOCK, "ARRAY['alpha']::text[]", "worker-lock"));
    let mut other = su(ctx).unwrap();
    let blocked = other.exec(&format!("SELECT id::text FROM jobs.tasks WHERE id = {}::uuid FOR UPDATE NOWAIT", q(&en_id)));
    let _ = other.exec("ROLLBACK");
    let _ = tx.exec("COMMIT");
    let after = other.exec(&format!("BEGIN; SELECT id::text FROM jobs.tasks WHERE id = {}::uuid FOR UPDATE NOWAIT; ROLLBACK;", q(&en_id)));
    rec.add("locks_released_after_caller_commit", cell(&row(&cl), 0) == "claimed" && cell(&row(&cl), 1) == en_id && blocked.state() == "55P03" && after.ok() && cell(&row(&after), 0) == en_id, "55P03 while the caller transaction is open, then NOWAIT succeeds", &format!("during {} after {}", blocked.state(), show(&after)), &cl.state(), "the function does not commit; granted tuple locks are not listed in pg_locks");

    let mut kill = rt(ctx).unwrap();
    let en = kill.exec(&enq(P_KILL, "alpha", "kill-rollback", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    let id = cell(&row(&en), 1);
    let _ = kill.exec("BEGIN");
    let cl = kill.exec(&claim(P_KILL, "ARRAY['alpha']::text[]", "worker-kill"));
    let _ = kill.exec("ROLLBACK");
    let state = count_sql(ctx, &format!("SELECT state||'|'||attempt::text||'|'||generation::text FROM jobs.tasks WHERE id = {}::uuid", q(&id)));
    rec.add("claim_rollback", cell(&row(&cl), 0) == "claimed" && cell(&row(&cl), 1) == id && state == "queued|0|0", "queued|0|0", &format!("claim {} row {state}", show(&cl)), &cl.state(), "");

    let mut kill2 = rt(ctx).unwrap();
    let en = kill2.exec(&enq(P_TERM, "alpha", "kill-terminate", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    let id = cell(&row(&en), 1);
    let _ = kill2.exec("BEGIN");
    let cl = kill2.exec(&claim(P_TERM, "ARRAY['alpha']::text[]", "worker-kill"));
    let pid = kill2.exec("SELECT pg_backend_pid()::text");
    let pid_s = cell(&row(&pid), 0);
    let term = su(ctx).unwrap().exec(&format!("SELECT pg_terminate_backend({pid_s})"));
    let mut state = String::new();
    for _ in 0..10 {
        state = count_sql(ctx, &format!("SELECT state||'|'||attempt::text FROM jobs.tasks WHERE id = {}::uuid", q(&id)));
        if state == "queued|0" {
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    rec.add("terminate_claim_backend_rolls_back", cell(&row(&cl), 0) == "claimed" && cell(&row(&cl), 1) == id && term.ok() && state == "queued|0", "queued|0", &format!("terminate {} row {state}", show(&term)), &term.state(), "backend kill is the test harness, not cancel");
}

fn deadlock(ctx: &Ctx, rec: &mut Rec) {
    let mut setup = rt(ctx).unwrap();
    let en = setup.exec(&enq(P_DEAD, "alpha", "dead-1", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    let id = cell(&row(&en), 1);
    let host = ctx.host.clone();
    let port = ctx.port;
    let owner_h = thread::spawn(move || {
        let mut pg = connect(&host, port, "a107_owner", "a107").unwrap();
        pg.exec("BEGIN; SET ROLE oo_jobs_owner; LOCK TABLE jobs.task_events IN SHARE ROW EXCLUSIVE MODE; SELECT pg_sleep(6); LOCK TABLE jobs.tasks IN SHARE ROW EXCLUSIVE MODE; COMMIT;")
    });
    thread::sleep(Duration::from_secs(1));
    let claim_o = setup.exec(&claim(P_DEAD, "ARRAY['alpha']::text[]", "worker-dead"));
    let owner_o = owner_h.join();
    let owner_state = owner_o.as_ref().map(|o| o.state()).unwrap_or_else(|_| "join".into());
    let owner_msg = owner_o.as_ref().map(|o| o.message.clone()).unwrap_or_default();
    let saw = owner_state == "40P01" || claim_o.state() == "40P01";
    let state = count_sql(ctx, &format!("SELECT state||'|'||attempt::text FROM jobs.tasks WHERE id = {}::uuid", q(&id)));
    let bad_events = count_sql(ctx, &format!("SELECT count(*)::text FROM jobs.task_events WHERE task_id = {}::uuid AND project_id <> {}::uuid", q(&id), q(P_DEAD)));
    let sqlstate = if owner_state == "40P01" {
        owner_state.clone()
    } else {
        claim_o.state()
    };
    rec.add("lock_order_40P01", saw && bad_events == "0" && (state == "running|1" || state == "queued|0"), "40P01 observed and row consistent", &format!("owner {owner_state} {owner_msg} claim {} row {state}", claim_o.state()), &sqlstate, "a079 cycle remains; this DDL does not close it");
}

fn expiry(ctx: &Ctx, rec: &mut Rec) {
    let mut rtpg = rt(ctx).unwrap();
    let hb = rtpg.exec(&enq(P_HB, "alpha", "hb-expire", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    let hb_id = cell(&row(&hb), 1);
    let hb_cl = rtpg.exec(&claim(P_HB, "ARRAY['alpha']::text[]", "worker-hb"));
    let hb_gen = cell(&row(&hb_cl), 4);
    let re = rtpg.exec(&enq(P_RE, "alpha", "reclaim-1", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    let re_id = cell(&row(&re), 1);
    let re_cl = rtpg.exec(&claim(P_RE, "ARRAY['alpha']::text[]", "worker-same"));
    let re_gen = cell(&row(&re_cl), 4);
    let re_tok = cell(&row(&re_cl), 7);
    for i in 0..9 {
        let o = rtpg.exec(&enq(P_CAP, "alpha", &format!("cap-{i}"), 1, "clock_timestamp() - interval '1 minute'", "'{\"ok\":true}'"));
        if cell(&row(&o), 0) != "enqueued" {
            rec.add("cap_seed", false, "enqueued", &show(&o), &o.state(), "");
            return;
        }
        let c = rtpg.exec(&claim(P_CAP, "ARRAY['alpha']::text[]", "worker-cap"));
        if cell(&row(&c), 0) != "claimed" {
            rec.add("cap_seed_claim", false, "claimed", &show(&c), &c.state(), "");
            return;
        }
    }
    let queued = rtpg.exec(&enq(P_CAP, "alpha", "cap-queued", 3, "clock_timestamp() + interval '1 second'", "'{\"ok\":true}'"));
    let cx = rtpg.exec(&enq(P_CX, "alpha", "cancel-expire", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    let cx_id = cell(&row(&cx), 1);
    let cx_cl = rtpg.exec(&claim(P_CX, "ARRAY['alpha']::text[]", "worker-cx"));
    let cx_gen = cell(&row(&cx_cl), 4);
    println!("CASE expiry_sleep start");
    thread::sleep(Duration::from_secs(32));
    let hb2 = rtpg.exec(&cols("heartbeat", P_HB, &hb_id, "worker-hb", &hb_gen, ""));
    let hb_state = count_sql(ctx, &format!("SELECT state FROM jobs.tasks WHERE id = {}::uuid", q(&hb_id)));
    rec.add("genuine_expiry_heartbeat_lost", cell(&row(&hb2), 0) == "lost_lease" && hb_state == "running", "lost_lease and still running", &format!("{} state {hb_state}", show(&hb2)), &hb2.state(), "clock_source=elapsed_db_32s; not revived");
    let re2 = rtpg.exec(&claim(P_RE, "ARRAY['alpha']::text[]", "worker-same"));
    let re2r = row(&re2);
    let old = rtpg.exec(&cols("complete_success", P_RE, &re_id, "worker-same", &re_gen, ""));
    let old_b = rtpg.exec(&cols("heartbeat", P_RE, &re_id, "worker-same", &re_gen, ""));
    rec.add(
        "genuine_expiry_same_worker_reclaim",
        cell(&re2r, 0) == "claimed" && cell(&re2r, 1) == re_id && cell(&re2r, 3) == "2" && cell(&re2r, 4) == "2" && cell(&re2r, 5) == "worker-same" && cell(&re2r, 7) != re_tok && cell(&row(&old), 0) == "lost_lease" && cell(&row(&old_b), 0) == "lost_lease",
        "fresh attempt and generation; old token generation rejected",
        &format!("new {} old {} beat {}", show(&re2), show(&old), show(&old_b)),
        &re2.state(),
        "clock_source=elapsed_db_32s",
    );
    let new_ok = rtpg.exec(&cols("complete_success", P_RE, &re_id, "worker-same", &cell(&re2r, 4), ""));
    rec.add("reclaim_new_generation_completes", cell(&row(&new_ok), 0) == "succeeded", "succeeded", &show(&new_ok), &new_ok.state(), "");
    let cap1 = rtpg.exec(&claim(P_CAP, "ARRAY['alpha']::text[]", "worker-cap"));
    let failed_n = count_sql(ctx, "SELECT count(*)::text FROM jobs.tasks WHERE project_id = '33333333-3333-4333-8333-333333333333' AND state = 'failed' AND last_error = 'lease_expired_attempt_cap'");
    let running_n = count_sql(ctx, "SELECT count(*)::text FROM jobs.tasks WHERE project_id = '33333333-3333-4333-8333-333333333333' AND state = 'running'");
    let queued_state = count_sql(ctx, "SELECT state FROM jobs.tasks WHERE dedupe_key = 'cap-queued'");
    rec.add("cap8_returns_none_and_leaves_queued", cell(&row(&cap1), 0) == "none" && failed_n == "8" && running_n == "1" && queued_state == "queued", "none, 8 failed, 1 running, queued remains", &format!("claim {} failed {failed_n} running {running_n} queued {queued_state}", show(&cap1)), &cap1.state(), "clock_source=elapsed_db_32s; budget is 8 finalizations");
    let cap2 = rtpg.exec(&claim(P_CAP, "ARRAY['alpha']::text[]", "worker-cap"));
    let failed_n2 = count_sql(ctx, "SELECT count(*)::text FROM jobs.tasks WHERE project_id = '33333333-3333-4333-8333-333333333333' AND state = 'failed' AND last_error = 'lease_expired_attempt_cap'");
    rec.add("cap_ninth_then_claim_queued", cell(&row(&cap2), 0) == "claimed" && cell(&row(&cap2), 1) == cell(&row(&queued), 1) && failed_n2 == "9", "ninth finalized and queued claimed in the later call", &format!("{} failed {failed_n2}", show(&cap2)), &cap2.state(), "the first call stopped even though a due row existed after the +1 second boundary");
    let cx2 = rtpg.exec(&cols("cancel", P_CX, &cx_id, "worker-cx", &cx_gen, ""));
    let cxr = row(&cx2);
    rec.add("cancel_expired_running", cell(&cxr, 0) == "cancelled" && cell(&cxr, 9) == "worker-cx" && cell(&cxr, 10) == cx_gen, "cancelled with invalidated binding", &show(&cx2), &cx2.state(), "clock_source=elapsed_db_32s; does not terminate a backend");
    let mut extra = rt(ctx).unwrap();
    let pid = cell(&row(&extra.exec("SELECT pg_backend_pid()::text")), 0);
    let still = count_sql(ctx, &format!("SELECT count(*)::text FROM pg_stat_activity WHERE pid = {pid}"));
    rec.add("cancel_does_not_terminate_backend", still == "1", "1", &still, "", "the extra session is still present");

    let ov1 = rtpg.exec(&enq(P_OV, "alpha", "ov-head", 3, "clock_timestamp() - interval '1 minute'", "'{\"ok\":true}'"));
    let ov2 = rtpg.exec(&enq(P_OV, "alpha", "ov-next", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    let ov_id = cell(&row(&ov1), 1);
    let bump = own(ctx).unwrap().exec(&format!("UPDATE jobs.tasks SET generation = 9223372036854775807 WHERE id = {}::uuid", q(&ov_id)));
    let ov_claim = rtpg.exec(&claim(P_OV, "ARRAY['alpha']::text[]", "worker-ov"));
    let ov_state = count_sql(ctx, &format!("SELECT generation::text||'|'||state FROM jobs.tasks WHERE id = {}::uuid", q(&ov_id)));
    let next_state = count_sql(ctx, &format!("SELECT state FROM jobs.tasks WHERE id = {}::uuid", q(&cell(&row(&ov2), 1))));
    rec.add("generation_overflow_oo012", bump.ok() && ov_claim.state() == "OO012" && ov_state.ends_with("|queued") && next_state == "queued", "OO012 and neither row claimed", &format!("bump {} claim {} {} head {ov_state} next {next_state}", bump.state(), ov_claim.state(), ov_claim.message), &ov_claim.state(), "clock_source=owner_seed_not_public_api; head blocks the call");
    let move_out = own(ctx).unwrap().exec(&format!("UPDATE jobs.tasks SET run_at = clock_timestamp() + interval '1 day' WHERE id = {}::uuid", q(&ov_id)));
    let next = rtpg.exec(&claim(P_OV, "ARRAY['alpha']::text[]", "worker-ov"));
    rec.add("overflow_head_moved_by_owner", move_out.ok() && cell(&row(&next), 1) == cell(&row(&ov2), 1) && cell(&row(&next), 0) == "claimed", "later row claimed", &show(&next), &next.state(), "clock_source=owner_seed_not_public_api");
}

fn hold(ctx: &Ctx, rec: &mut Rec) {
    let mut rtpg = match rt(ctx) {
        Ok(v) => v,
        Err(e) => {
            rec.add("hold_connect", false, "connect", &e, "CONN", "");
            return;
        }
    };
    let durable_q = rtpg.exec(&enq(P_HOLD, "alpha", "durable-queued", 3, "clock_timestamp() + interval '2 days'", "'{\"ok\":true}'"));
    let durable_r = rtpg.exec(&enq(P_HOLD, "alpha", "durable-running", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    let cl = rtpg.exec(&claim(P_HOLD, "ARRAY['alpha']::text[]", "worker-durable"));
    let qid = cell(&row(&durable_q), 1);
    let rid = cell(&row(&cl), 1);
    if cell(&row(&durable_q), 0) != "enqueued" || cell(&row(&cl), 0) != "claimed" || rid != cell(&row(&durable_r), 1) {
        rec.add("durability_markers", false, "queued and running markers", &format!("{} {}", show(&durable_q), show(&cl)), "", "");
        return;
    }
    let _ = fs::write(ctx.out.join("marker-queued.txt"), format!("{qid}\n"));
    let _ = fs::write(ctx.out.join("marker-running.txt"), format!("{}|{}|{}\n", rid, cell(&row(&cl), 4), cell(&row(&cl), 3)));
    let _ = rtpg.exec("BEGIN");
    let open = rtpg.exec(&enq(P_HOLD, "alpha", "durable-open", 3, "clock_timestamp()", "'{\"ok\":true}'"));
    let oid = cell(&row(&open), 1);
    let _ = fs::write(ctx.out.join("marker-open.txt"), format!("{oid}\n"));
    let _ = fs::write(ctx.out.join("hold-ready.txt"), format!("{oid}\n"));
    println!("CASE hold_ready {oid}");
    let mut killed = false;
    let mut detail = String::new();
    for _ in 0..50 {
        thread::sleep(Duration::from_secs(1));
        let ping = rtpg.exec("SELECT 1");
        if !ping.ok() {
            killed = true;
            detail = format!("{} {}", ping.state(), ping.message);
            break;
        }
    }
    rec.add("hold_disconnected_for_restart", killed, "connection dropped while transaction open", &detail, "", "host restarts postgres during this window");
    let _ = SystemTime::now().duration_since(UNIX_EPOCH);
}
