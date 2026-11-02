//! Live parity test for `mysql-preview source-sql` against an owned MySQL 5.7
//! fixture. `#[ignore]` by default; run with `--ignored` and the environment below.
//!
//! Each case `NN.sql` in `OO_CB_P11_CASES` goes through the producer. Its stdout must
//! equal `NN.cli.out`, the bytes that `mysql --batch --raw --skip-column-names`
//! printed for the same SQL on the same fixture.
//!
//! Environment: OO_CB_P11_HOST, OO_CB_P11_PORT, OO_CB_P11_DB, OO_CB_P11_USER,
//! OO_CB_P11_PW_FILE (0600 reader password file), OO_CB_P11_CA, OO_CB_P11_TLS_NAME,
//! OO_CB_P11_CASES.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde_json::json;

const BIN: &str = env!("CARGO_BIN_EXE_mysql-preview");

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("set {key}"))
}

fn request_with(
    sql: &str,
    tls: &str,
    password_file: &Path,
    statement_ms: u64,
    max_rows: u64,
    max_bytes: u64,
) -> Vec<u8> {
    json!({
        "protocol": "oo.mysql.source-sql/v1",
        "connection": {
            "host": env("OO_CB_P11_HOST"),
            "port": env("OO_CB_P11_PORT").parse::<u64>().unwrap(),
            "database": env("OO_CB_P11_DB"),
            "user": env("OO_CB_P11_USER"),
            "password_file": password_file,
            "ca_file": env("OO_CB_P11_CA"),
            "tls_server_name": tls,
        },
        "statement_ms": statement_ms,
        "max_rows": max_rows,
        "max_bytes": max_bytes,
        "sql": sql,
    })
    .to_string()
    .into_bytes()
}

fn request(sql: &str) -> Vec<u8> {
    request_with(
        sql,
        &env("OO_CB_P11_TLS_NAME"),
        &password_file(),
        60_000,
        100_000,
        67_108_864,
    )
}

fn password_file() -> PathBuf {
    PathBuf::from(env("OO_CB_P11_PW_FILE"))
}

struct Outcome {
    success: bool,
    stdout: Vec<u8>,
    stderr: String,
}

fn run(request: &[u8]) -> Outcome {
    let mut child = Command::new(BIN)
        .arg("source-sql")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn producer");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(request).expect("write request");
    }
    let output = child.wait_with_output().expect("wait");
    Outcome {
        success: output.status.success(),
        stdout: output.stdout,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

#[test]
#[ignore = "live: owned MySQL 5.7 fixture (OO_CB_P11_*)"]
fn cli_parity_for_every_case() {
    let dir = PathBuf::from(env("OO_CB_P11_CASES"));
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("cases dir")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
        .collect();
    names.sort();
    assert!(
        names.len() >= 10,
        "expected the reference cases in {}",
        dir.display()
    );
    let mut failures = Vec::new();
    for sql_path in &names {
        let sql = std::fs::read_to_string(sql_path).expect("sql");
        let expected = std::fs::read(sql_path.with_extension("cli.out")).expect("cli.out");
        let outcome = run(&request(&sql));
        if !outcome.success || outcome.stdout != expected {
            failures.push(format!(
                "{}: success={} stdout_bytes={} expected_bytes={} stderr={}",
                sql_path.display(),
                outcome.success,
                outcome.stdout.len(),
                expected.len(),
                outcome.stderr.trim()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "parity failures:\n{}",
        failures.join("\n")
    );
}

#[test]
#[ignore = "live: owned MySQL 5.7 fixture (OO_CB_P11_*)"]
fn the_session_is_read_only_and_writes_are_refused_before_connect() {
    let read_only = run(&request("SELECT @@session.transaction_read_only"));
    assert!(read_only.success, "{}", read_only.stderr);
    assert_eq!(read_only.stdout, b"1\n");
    for sql in [
        "DELETE FROM profile_t",
        "SELECT 1; DELETE FROM profile_t",
        "SELECT * FROM profile_t FOR UPDATE",
        "SELECT 1 /* x */",
        "SELECT 1 INTO OUTFILE '/tmp/p11-should-not-exist'",
    ] {
        let outcome = run(&request(sql));
        assert!(!outcome.success, "{sql}");
        assert!(outcome.stdout.is_empty(), "{sql}");
        assert!(
            outcome.stderr.contains("config"),
            "{sql}: {}",
            outcome.stderr
        );
    }
}

#[test]
#[ignore = "live: owned MySQL 5.7 fixture (OO_CB_P11_*)"]
fn wrong_certificate_name_is_refused_and_prints_nothing() {
    let outcome = run(&request_with(
        "SELECT 1",
        "wrong.example",
        &password_file(),
        60_000,
        100,
        1_000_000,
    ));
    assert!(!outcome.success);
    assert!(outcome.stdout.is_empty());
}

#[test]
#[ignore = "live: owned MySQL 5.7 fixture (OO_CB_P11_*)"]
fn row_and_byte_budgets_fail_without_partial_output() {
    let rows = run(&request_with(
        "SELECT id FROM big_t ORDER BY id",
        &env("OO_CB_P11_TLS_NAME"),
        &password_file(),
        60_000,
        10,
        1_000_000,
    ));
    assert!(!rows.success);
    assert!(rows.stdout.is_empty());
    assert!(rows.stderr.contains("max_rows"), "{}", rows.stderr);

    let bytes = run(&request_with(
        "SELECT payload FROM big_t ORDER BY id",
        &env("OO_CB_P11_TLS_NAME"),
        &password_file(),
        60_000,
        100_000,
        1_000,
    ));
    assert!(!bytes.success);
    assert!(bytes.stdout.is_empty());
    assert!(bytes.stderr.contains("max_bytes"), "{}", bytes.stderr);
}

#[test]
#[ignore = "live: owned MySQL 5.7 fixture (OO_CB_P11_*)"]
fn statement_budget_stops_a_long_select() {
    let started = Instant::now();
    let outcome = run(&request_with(
        "SELECT COUNT(*) FROM big_t a JOIN big_t b ON b.id > a.id",
        &env("OO_CB_P11_TLS_NAME"),
        &password_file(),
        2_000,
        100,
        1_000_000,
    ));
    assert!(!outcome.success, "a 2 s budget must stop a 134M-pair join");
    assert!(outcome.stdout.is_empty());
    assert!(started.elapsed().as_secs() < 120, "{:?}", started.elapsed());
}

#[test]
#[ignore = "live: owned MySQL 5.7 fixture (OO_CB_P11_*)"]
fn group_or_world_readable_password_file_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let copy = std::env::temp_dir().join(format!("oo-cb-p11-pw-{}.pw", std::process::id()));
    std::fs::copy(password_file(), &copy).expect("copy");
    std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    let outcome = run(&request_with(
        "SELECT 1",
        &env("OO_CB_P11_TLS_NAME"),
        &copy,
        60_000,
        100,
        1_000_000,
    ));
    std::fs::remove_file(&copy).ok();
    assert!(!outcome.success);
    assert!(
        outcome.stderr.contains("password_file"),
        "{}",
        outcome.stderr
    );
}

#[test]
fn explicit_plaintext_full_transport_on_an_isolated_mysql_fixture() {
    use std::fs;
    use std::io::Read;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let suffix = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(format!("oo-source-sql-{suffix}"));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let mut random = [0u8; 24];
    fs::File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut random)
        .unwrap();
    let password: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let password_file = root.join("reader");
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&password_file)
        .unwrap()
        .write_all(password.as_bytes())
        .unwrap();
    let name = format!("oo-yjcap-full-source-sql-{suffix}");
    let started = Command::new("docker")
        .args([
            "run",
            "-d",
            "--platform",
            "linux/amd64",
            "--name",
            &name,
            "-p",
            "127.0.0.1::3306",
            "-v",
            &format!("{}:/run/secrets:ro", root.display()),
            "-e",
            "MYSQL_ROOT_PASSWORD_FILE=/run/secrets/reader",
            "mysql:5.7.44",
            "--skip-log-bin",
            "--ssl=OFF",
            "--general-log=ON",
            "--log-output=TABLE",
        ])
        .output()
        .unwrap();
    assert!(started.status.success(), "isolated fixture startup");
    let mysql = |sql: &str| {
        let mut child = Command::new("docker").args([
            "exec", "-i", &name, "sh", "-c",
            "MYSQL_PWD=\"$(cat /run/secrets/reader)\" exec mysql --protocol=TCP -h127.0.0.1 -uroot --default-character-set=utf8 -N -B --raw",
        ]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(sql.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let ready = Instant::now();
    while !mysql("SELECT 1").status.success() {
        assert!(
            ready.elapsed() < Duration::from_secs(120),
            "isolated fixture readiness"
        );
        std::thread::sleep(Duration::from_secs(1));
    }
    let setup = mysql(&format!(
        "CREATE DATABASE sample; CREATE TABLE sample.items(id int PRIMARY KEY,label varchar(100)); INSERT INTO sample.items VALUES(1,'owned fixture'),(2,NULL); CREATE USER 'reader'@'%' IDENTIFIED BY '{password}'; GRANT SELECT ON sample.* TO 'reader'@'%'; CREATE USER 'writer'@'%' IDENTIFIED BY '{password}'; GRANT SELECT,INSERT ON sample.* TO 'writer'@'%';"
    ));
    assert!(setup.status.success(), "isolated fixture setup");
    let port = Command::new("docker")
        .args([
            "inspect",
            "--format",
            "{{(index (index .NetworkSettings.Ports \"3306/tcp\") 0).HostPort}}",
            &name,
        ])
        .output()
        .unwrap();
    assert!(port.status.success());
    let port: u16 = String::from_utf8(port.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let mut request = json!({
        "protocol":"oo.mysql.source-sql/v1",
        "connection":{"host":"127.0.0.1","port":port,"database":"sample","user":"reader","password_file":password_file,"tls":"disabled","plaintext_authorized":true,"ca_file":null,"tls_server_name":null},
        "statement_ms":1000,"max_rows":100,"max_bytes":1048576,
        "sql":"SELECT id,label FROM sample.items ORDER BY id"
    });
    let outcome = run(&serde_json::to_vec(&request).unwrap());
    assert!(outcome.success, "{}", outcome.stderr);
    assert_eq!(
        outcome.stdout,
        mysql(request["sql"].as_str().unwrap()).stdout
    );
    assert_eq!(outcome.stdout, b"1\towned fixture\n2\tNULL\n");
    request["sql"] = json!("SELECT @@session.transaction_read_only");
    let outcome = run(&serde_json::to_vec(&request).unwrap());
    assert!(outcome.success, "{}", outcome.stderr);
    assert_eq!(outcome.stdout, b"1\n");
    request["connection"]["user"] = json!("writer");
    let outcome = run(&serde_json::to_vec(&request).unwrap());
    assert!(!outcome.success && outcome.stdout.is_empty());
    assert!(
        outcome.stderr.contains("full_read_grants"),
        "{}",
        outcome.stderr
    );
    request["connection"]["user"] = json!("reader");
    let mode = mysql("SELECT @@GLOBAL.sql_mode");
    assert!(mode.status.success());
    assert!(
        mysql("SET GLOBAL sql_mode='NO_BACKSLASH_ESCAPES'")
            .status
            .success()
    );
    let outcome = run(&serde_json::to_vec(&request).unwrap());
    assert!(!outcome.success && outcome.stdout.is_empty());
    assert!(
        outcome.stderr.contains("source_facts"),
        "{}",
        outcome.stderr
    );
    let mode = String::from_utf8(mode.stdout).unwrap();
    assert!(
        mysql(&format!("SET GLOBAL sql_mode='{}'", mode.trim()))
            .status
            .success()
    );
    for (key, value, fact) in [
        ("max_rows", json!(1), "max_rows"),
        ("max_bytes", json!(1), "max_bytes"),
        ("sql", json!("SELECT 1; DELETE FROM sample.items"), "config"),
    ] {
        let mut bad = request.clone();
        bad["sql"] = json!("SELECT id,label FROM sample.items ORDER BY id");
        bad[key] = value;
        let outcome = run(&serde_json::to_vec(&bad).unwrap());
        assert!(!outcome.success && outcome.stdout.is_empty());
        assert!(outcome.stderr.contains(fact), "{}", outcome.stderr);
    }
    let mut tls = request.clone();
    tls["connection"].as_object_mut().unwrap().remove("tls");
    tls["connection"]
        .as_object_mut()
        .unwrap()
        .remove("plaintext_authorized");
    let outcome = run(&serde_json::to_vec(&tls).unwrap());
    assert!(!outcome.success && outcome.stdout.is_empty());
    assert!(outcome.stderr.contains("ca_file"), "{}", outcome.stderr);
    let dummy_ca = root.join("dummy-ca.pem");
    fs::write(&dummy_ca, "not a CA certificate").unwrap();
    tls["connection"]["ca_file"] = json!(dummy_ca);
    let outcome = run(&serde_json::to_vec(&tls).unwrap());
    assert!(!outcome.success && outcome.stdout.is_empty());
    request["sql"] = json!("SELECT id,SLEEP(30) FROM sample.items");
    request["statement_ms"] = json!(1);
    let started = Instant::now();
    let outcome = run(&serde_json::to_vec(&request).unwrap());
    assert!(!outcome.success && outcome.stdout.is_empty());
    assert!(
        outcome.stderr.contains("protocol query"),
        "{}",
        outcome.stderr
    );
    assert!(started.elapsed() < Duration::from_secs(25));
    let log = mysql(
        "SELECT thread_id,argument FROM mysql.general_log WHERE command_type='Query' AND (argument='SELECT id,SLEEP(30) FROM sample.items' OR argument LIKE 'KILL CONNECTION %') ORDER BY event_time",
    );
    assert!(log.status.success());
    let log = String::from_utf8(log.stdout).unwrap();
    let rows: Vec<_> = log
        .lines()
        .map(|line| line.split_once('\t').unwrap())
        .collect();
    let source_id = rows
        .iter()
        .find(|(_, sql)| *sql == "SELECT id,SLEEP(30) FROM sample.items")
        .unwrap()
        .0;
    assert!(
        rows.iter()
            .any(|(id, sql)| *id != source_id && *sql == format!("KILL CONNECTION {source_id}")),
        "{log}"
    );
    let gone = mysql(&format!(
        "SELECT COUNT(*) FROM information_schema.PROCESSLIST WHERE ID={source_id}"
    ));
    assert!(gone.status.success());
    assert_eq!(gone.stdout, b"0\n");
    assert!(
        Command::new("docker")
            .args(["stop", &name])
            .output()
            .unwrap()
            .status
            .success()
    );
}
