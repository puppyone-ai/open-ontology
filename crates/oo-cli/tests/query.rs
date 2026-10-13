//! Spawned `oo` against a loopback HTTP/TLS oracle.
//!
//! Expected bytes live under `contracts/cli/query`. The SDK fixtures are the
//! response bodies. This file does not edit `oo-sdk`.

#[path = "common/mod.rs"]
mod common;

use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt;

use common::{
    Action, CANARY, Door, Oracle, Scratch, Spec, TOKEN, assert_secret_free, json_response,
    pem_block, run, sdk_tls,
};
use oo_core::Diagnostic;
use oo_core::api::{QueryFormat, QueryRequest, QueryResponse, parse_api_json};
use oo_core::text::QuerySql;

const STANDARD: &str =
    include_str!("../../../contracts/sdk/json-query/fixtures/query-response.json");
const EMPTY: &str =
    include_str!("../../../contracts/sdk/json-query/fixtures/query-response-empty.json");
const TRUNCATED: &str =
    include_str!("../../../contracts/sdk/json-query/fixtures/query-response-truncated.json");
const RATE: &str =
    include_str!("../../../contracts/sdk/json-query/fixtures/error-rate-limited.json");
const UNAUTH: &str =
    include_str!("../../../contracts/sdk/json-query/fixtures/error-unauthenticated.json");
const OVERLOADED: &str =
    include_str!("../../../contracts/sdk/json-query/fixtures/error-overloaded.json");
const TIMEOUT_BODY: &str =
    include_str!("../../../contracts/sdk/json-query/fixtures/error-timeout.json");
const INTERNAL: &str =
    include_str!("../../../contracts/sdk/json-query/fixtures/error-internal.json");
const SQL_ERR: &str =
    include_str!("../../../contracts/sdk/json-query/fixtures/error-sql-parse.json");
const DUPLICATE: &str =
    include_str!("../../../contracts/sdk/json-query/fixtures/reject-duplicate-key.json");
const UNKNOWN: &str =
    include_str!("../../../contracts/sdk/json-query/fixtures/reject-unknown-field.json");

fn quiet<T>(_: &T) -> Vec<Diagnostic> {
    Vec::new()
}

fn args(scratch: &Scratch, url: &str, transport: &str, extra: &[&str]) -> Vec<String> {
    let mut out = vec![
        "query".to_owned(),
        "--auth".to_owned(),
        "external".to_owned(),
        "--token-file".to_owned(),
        scratch.token_path.display().to_string(),
        "--gateway".to_owned(),
        url.to_owned(),
        "--transport".to_owned(),
        transport.to_owned(),
    ];
    out.extend(extra.iter().map(|item| (*item).to_owned()));
    out
}

fn omit_nulls(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.retain(|_, child| !child.is_null());
            for child in map.values_mut() {
                omit_nulls(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                omit_nulls(item);
            }
        }
        _ => {}
    }
}

fn canonical_request(sql: &str, max_rows: Option<u32>, timeout_ms: Option<u32>) -> Vec<u8> {
    let request = QueryRequest {
        sql: QuerySql::new(sql).expect("sql"),
        max_rows,
        timeout_ms,
        format: QueryFormat::Json,
        client_context: None,
    };
    let mut value = serde_json::to_value(&request).expect("value");
    omit_nulls(&mut value);
    serde_json::to_vec(&value).expect("bytes")
}

fn assert_query(hit: &common::Hit, sql: &str, max_rows: Option<u32>, timeout_ms: Option<u32>) {
    assert_eq!(hit.method, "POST");
    assert_eq!(hit.path, "/v1/query");
    let authorization = format!("Bearer {TOKEN}");
    assert_eq!(hit.one("authorization"), Some(authorization.as_str()));
    assert_eq!(hit.one("content-type"), Some("application/json"));
    assert_eq!(hit.one("accept"), Some("application/json"));
    assert_eq!(hit.one("user-agent"), Some("oo-sdk/0.1"));
    assert!(hit.one("proxy-authorization").is_none());
    let expected = canonical_request(sql, max_rows, timeout_ms);
    assert_eq!(hit.body, expected);
    let text = std::str::from_utf8(&hit.body).expect("utf8");
    let parsed = parse_api_json::<QueryRequest>(text, quiet).expect("request door");
    assert_eq!(parsed.sql.as_str(), sql);
    assert_eq!(parsed.max_rows, max_rows);
    assert_eq!(parsed.timeout_ms, timeout_ms);
    assert!(parsed.client_context.is_none());
    assert!(
        !hit.body
            .windows(CANARY.len())
            .any(|window| window == CANARY.as_bytes())
    );
}

async fn finish(scratch: &Scratch, spec: Spec, door: Option<&Door>) -> common::Outcome {
    let outcome = run(spec).await;
    assert!(!scratch.home.exists(), "home was created");
    assert_secret_free(&outcome.stdout, &[]);
    assert_secret_free(&outcome.stderr, &[]);
    if let Some(door) = door {
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(door.hits(), 0, "unexpected accept");
    }
    outcome
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn help_and_usage_open_no_socket() {
    let scratch = Scratch::new();
    let door = Door::open();
    let url = door.url();
    let help = finish(
        &scratch,
        Spec::new(&scratch, vec!["--help".to_owned()]),
        Some(&door),
    )
    .await;
    assert_eq!(help.code, 0);
    assert!(help.stderr.is_empty());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("oo query"));
    assert!(text.contains("--auth external"));
    assert!(text.contains("oo tables"));
    assert!(text.contains("oo knowledge cat"));
    assert!(text.contains("export, config, and mcp are not implemented"));

    let query_help = finish(
        &scratch,
        Spec::new(
            &scratch,
            vec![
                "query".to_owned(),
                "--help".to_owned(),
                "--token-file".to_owned(),
                scratch.dir.join("does-not-exist").display().to_string(),
                "--bogus".to_owned(),
            ],
        ),
        Some(&door),
    )
    .await;
    assert_eq!(query_help.code, 0);
    let text = String::from_utf8(query_help.stdout).unwrap();
    assert!(text.contains("--max-rows"));
    assert!(text.contains("1..=2000"));
    assert!(text.contains("Sandbox mode"));
    assert!(text.contains("--json"));
    assert!(query_help.stderr.is_empty());

    let cases = [
        (vec!["query", "--json"], 2, "sql is missing"),
        (
            vec!["query", "--bogus", "select 1"],
            2,
            "unknown flag: --bogus",
        ),
        (vec!["query", "select 1", "extra"], 2, "extra argument"),
        (
            vec!["query", "--max-rows", "0", "select 1"],
            2,
            "invalid max-rows",
        ),
        (
            vec!["query", "--max-rows", "01", "select 1"],
            2,
            "invalid max-rows",
        ),
        (
            vec!["query", "--max-rows", "2001", "select 1"],
            2,
            "invalid max-rows",
        ),
        (
            vec!["query", "--max-rows", "-1", "select 1"],
            2,
            "invalid max-rows",
        ),
        (
            vec!["query", "--timeout-ms", "120001", "select 1"],
            2,
            "invalid timeout-ms",
        ),
        (
            vec!["query", "--json", "--json", "select 1"],
            2,
            "duplicate flag",
        ),
        (
            vec!["query", "--token-file=SECRETVALUEA095", "select 1"],
            2,
            "flags do not take equals values",
        ),
        (
            vec!["query", "--SECRETVALUEA095", "select 1"],
            2,
            "unknown flag",
        ),
        (
            vec!["query", "-f", "-", "ignored"],
            2,
            "sql stdin is not supported",
        ),
        (vec!["export"], 2, "not implemented"),
        (vec!["tables"], 4, "token source is missing"),
        (vec!["describe"], 2, "table is missing"),
        (vec!["validate"], 2, "sql is missing"),
        (vec!["config"], 2, "not implemented"),
    ];
    for (argv, code, needle) in cases {
        let mut full: Vec<String> = argv.iter().map(|item| (*item).to_owned()).collect();
        full.extend([
            "--gateway".to_owned(),
            url.clone(),
            "--auth".to_owned(),
            "external".to_owned(),
            "--transport".to_owned(),
            "local-development".to_owned(),
        ]);
        let outcome = finish(&scratch, Spec::new(&scratch, full), Some(&door)).await;
        assert_eq!(outcome.code, code, "{argv:?} {needle}");
        let combined = [outcome.stdout.clone(), outcome.stderr.clone()].concat();
        let shown = String::from_utf8_lossy(&combined);
        assert!(shown.contains(needle), "{argv:?} showed {shown}");
        assert!(!shown.contains("SECRETVALUEA095"), "{shown}");
        if argv.contains(&"--json") && argv[0] == "query" {
            assert!(outcome.stderr.is_empty(), "{argv:?}");
            assert!(outcome.stdout.ends_with(b"\n"));
        }
    }
    let usage = include_bytes!("../../../contracts/cli/query/fixtures/error-usage.stdout.json");
    let missing = finish(
        &scratch,
        Spec::new(&scratch, vec!["query".to_owned(), "--json".to_owned()]),
        Some(&door),
    )
    .await;
    assert_eq!(missing.stdout, usage);
    assert!(missing.stderr.is_empty());

    let text_missing = finish(
        &scratch,
        Spec::new(&scratch, vec!["query".to_owned()]),
        Some(&door),
    )
    .await;
    assert_eq!(text_missing.code, 2);
    assert!(text_missing.stdout.is_empty());
    assert_eq!(
        String::from_utf8(text_missing.stderr).unwrap(),
        "error: usage sql is missing\nretryable: false\nretry_outcome: not_applicable\nretry_policy: max_extra_attempts=3 max_wait_ms=2000 nested_cli_retries=0 attempt_count=unknown\n"
    );

    let bare = finish(&scratch, Spec::new(&scratch, Vec::new()), Some(&door)).await;
    assert_eq!(bare.code, 2);
    assert!(String::from_utf8_lossy(&bare.stderr).contains("command is missing"));

    let export_help = finish(
        &scratch,
        Spec::new(&scratch, vec!["export".to_owned(), "--help".to_owned()]),
        Some(&door),
    )
    .await;
    assert_eq!(export_help.code, 0);
    assert!(String::from_utf8_lossy(&export_help.stdout).contains("not implemented"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_bytes_and_stdin_sql_do_not_block() {
    let scratch = Scratch::new();
    let door = Door::open();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_oo"));
    cmd.arg(std::ffi::OsStr::from_bytes(&[0xff]))
        .env_clear()
        .env("OO_TOKEN", CANARY)
        .env("HOME", &scratch.home)
        .current_dir(&scratch.dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let output = cmd.output().expect("spawn");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("argument is not utf-8"));
    assert!(!scratch.home.exists());
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(door.hits(), 0);

    let mut spec = Spec::new(
        &scratch,
        vec!["query".to_owned(), "-f".to_owned(), "-".to_owned()],
    );
    spec.stdin = b"select 1".to_vec();
    spec.timeout = Duration::from_secs(2);
    let outcome = finish(&scratch, spec, Some(&door)).await;
    assert_eq!(outcome.code, 2);
    assert!(String::from_utf8_lossy(&outcome.stderr).contains("sql stdin is not supported"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sandbox_and_missing_auth_do_not_read_or_connect() {
    let scratch = Scratch::new();
    let door = Door::open();
    let fifo = scratch.dir.join("token-fifo");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo")
            .success()
    );
    let mut sandbox = Spec::new(
        &scratch,
        vec![
            "query".to_owned(),
            "--auth".to_owned(),
            "sandbox".to_owned(),
            "--token-file".to_owned(),
            fifo.display().to_string(),
            "--gateway".to_owned(),
            door.url(),
            "--transport".to_owned(),
            "local-development".to_owned(),
            "select 1".to_owned(),
        ],
    );
    sandbox.timeout = Duration::from_secs(2);
    let outcome = finish(&scratch, sandbox, Some(&door)).await;
    assert_eq!(outcome.code, 1);
    let shown = String::from_utf8_lossy(&outcome.stderr);
    assert!(shown.contains("from_sandbox_env"));
    assert!(shown.contains("did not read a token"));

    let mut missing = Spec::new(
        &scratch,
        vec![
            "query".to_owned(),
            "--token-file".to_owned(),
            fifo.display().to_string(),
            "--gateway".to_owned(),
            door.url(),
            "--transport".to_owned(),
            "local-development".to_owned(),
            "select 1".to_owned(),
        ],
    );
    missing.timeout = Duration::from_secs(2);
    let outcome = finish(&scratch, missing, Some(&door)).await;
    assert_eq!(outcome.code, 4);
    assert!(String::from_utf8_lossy(&outcome.stderr).contains("authentication source is missing"));

    let mut no_source = Spec::new(
        &scratch,
        vec![
            "query".to_owned(),
            "--auth".to_owned(),
            "external".to_owned(),
            "--gateway".to_owned(),
            door.url(),
            "--transport".to_owned(),
            "local-development".to_owned(),
            "-f".to_owned(),
            scratch.dir.join("missing.sql").display().to_string(),
        ],
    );
    no_source.timeout = Duration::from_secs(2);
    let outcome = finish(&scratch, no_source, Some(&door)).await;
    assert_eq!(outcome.code, 4);
    assert!(String::from_utf8_lossy(&outcome.stderr).contains("token source is missing"));

    let mut no_gateway = Spec::new(
        &scratch,
        vec![
            "query".to_owned(),
            "--auth".to_owned(),
            "external".to_owned(),
            "--token-file".to_owned(),
            fifo.display().to_string(),
            "--transport".to_owned(),
            "local-development".to_owned(),
            "select 1".to_owned(),
        ],
    );
    no_gateway.timeout = Duration::from_secs(2);
    let outcome = finish(&scratch, no_gateway, Some(&door)).await;
    assert_eq!(outcome.code, 2);
    assert!(String::from_utf8_lossy(&outcome.stderr).contains("gateway is required"));
}

fn replace_token(scratch: &Scratch, url: &str, path: &Path) -> Vec<String> {
    let mut argv = args(scratch, url, "local-development", &["select 1"]);
    let pos = argv.iter().position(|item| item == "--token-file").unwrap();
    argv[pos + 1] = path.display().to_string();
    argv
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn token_and_sql_files_fail_before_connect() {
    let scratch = Scratch::new();
    let door = Door::open();
    let url = door.url();

    let mode = scratch.dir.join("wide-token");
    std::fs::write(&mode, format!("{TOKEN}\n")).unwrap();
    std::fs::set_permissions(&mode, std::fs::Permissions::from_mode(0o644)).unwrap();
    let link = scratch.dir.join("token-link");
    symlink(&scratch.token_path, &link).unwrap();
    let dir = scratch.dir.join("token-dir");
    std::fs::create_dir(&dir).unwrap();
    let broken = scratch.dir.join("two-newlines");
    std::fs::write(&broken, format!("{TOKEN}\n\n")).unwrap();
    std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o600)).unwrap();
    let absent = scratch.dir.join("absent-token");
    let token_cases = [
        (mode.as_path(), "token file rejected: mode"),
        (link.as_path(), "token file rejected: symlink"),
        (dir.as_path(), "token file rejected: not-file"),
        (absent.as_path(), "token file rejected: missing"),
        (broken.as_path(), "token rejected"),
    ];
    for (path, needle) in token_cases {
        let outcome = finish(
            &scratch,
            Spec::new(&scratch, replace_token(&scratch, &url, path)),
            Some(&door),
        )
        .await;
        assert_eq!(outcome.code, 4, "{needle}");
        assert!(outcome.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&outcome.stderr).contains(needle),
            "{needle} in {}",
            String::from_utf8_lossy(&outcome.stderr)
        );
    }

    let both = args(
        &scratch,
        &url,
        "local-development",
        &["--token-stdin", "select 1"],
    );
    let outcome = finish(&scratch, Spec::new(&scratch, both), Some(&door)).await;
    assert_eq!(outcome.code, 2);
    assert!(String::from_utf8_lossy(&outcome.stderr).contains("mutually exclusive"));

    let sql_link = scratch.dir.join("sql-link");
    let sql_real = scratch.dir.join("real.sql");
    std::fs::write(&sql_real, "select 1").unwrap();
    symlink(&sql_real, &sql_link).unwrap();
    let locked = scratch.dir.join("locked.sql");
    std::fs::write(&locked, "select 1").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let big = scratch.dir.join("big.sql");
    std::fs::write(&big, vec![b'a'; 65_537]).unwrap();
    let empty = scratch.dir.join("empty.sql");
    std::fs::write(&empty, "").unwrap();
    let bad = scratch.dir.join("bad.sql");
    std::fs::write(&bad, [0xff, 0xfe]).unwrap();
    let missing_sql = scratch.dir.join("missing.sql");
    let sql_cases = [
        (sql_link.as_path(), 1, "sql file rejected: symlink"),
        (locked.as_path(), 1, "sql file rejected: permission"),
        (big.as_path(), 1, "sql file rejected: budget"),
        (empty.as_path(), 2, "sql is missing"),
        (bad.as_path(), 1, "sql file rejected: utf8"),
        (missing_sql.as_path(), 1, "sql file rejected: missing"),
    ];
    for (path, code, needle) in sql_cases {
        let argv = args(
            &scratch,
            &url,
            "local-development",
            &["-f", path.to_str().unwrap()],
        );
        let outcome = finish(&scratch, Spec::new(&scratch, argv), Some(&door)).await;
        assert_eq!(outcome.code, code, "{needle}");
        let shown = String::from_utf8_lossy(&outcome.stderr);
        assert!(shown.contains(needle), "{needle} in {shown}");
    }

    let mixed = args(
        &scratch,
        &url,
        "local-development",
        &[
            "select 1",
            "-f",
            scratch.dir.join("missing.sql").to_str().unwrap(),
        ],
    );
    let outcome = finish(&scratch, Spec::new(&scratch, mixed), Some(&door)).await;
    assert_eq!(outcome.code, 2);
    assert!(String::from_utf8_lossy(&outcome.stderr).contains("mutually exclusive"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn renders_sdk_fixtures_and_independent_bytes() {
    let scratch = Scratch::new();
    let min = include_bytes!("../../../contracts/cli/query/fixtures/request-min.wire.json");
    let limits = include_bytes!("../../../contracts/cli/query/fixtures/request-limits.wire.json");
    assert_eq!(canonical_request("select 1", None, None), min);
    assert_eq!(canonical_request("select 1", Some(10), Some(1000)), limits);

    let cases = [
        (
            STANDARD,
            include_bytes!("../../../contracts/cli/query/fixtures/response-standard.stdout.json")
                .as_slice(),
            include_bytes!("../../../contracts/cli/query/fixtures/response-standard.md").as_slice(),
        ),
        (
            EMPTY,
            b"".as_slice(),
            include_bytes!("../../../contracts/cli/query/fixtures/response-empty.md").as_slice(),
        ),
        (
            TRUNCATED,
            b"".as_slice(),
            include_bytes!("../../../contracts/cli/query/fixtures/response-truncated.md")
                .as_slice(),
        ),
    ];
    for (body, json_golden, markdown) in cases {
        let typed = parse_api_json::<QueryResponse>(body, quiet).expect("sdk fixture");
        for json in [false, true] {
            let oracle = Oracle::spawn({
                let payload = json_response(200, "OK", body, &[]);
                move |_, _| Action::Raw(payload.clone())
            })
            .await;
            let mut extra = vec!["select 1"];
            if json {
                extra.insert(0, "--json");
            }
            let outcome = finish(
                &scratch,
                Spec::new(
                    &scratch,
                    args(&scratch, &oracle.origin(), "local-development", &extra),
                ),
                None,
            )
            .await;
            assert_eq!(
                outcome.code,
                0,
                "{}",
                String::from_utf8_lossy(&outcome.stderr)
            );
            assert!(outcome.stderr.is_empty());
            let hits = oracle.hits();
            assert_eq!(hits.len(), 1);
            assert_query(&hits[0], "select 1", None, None);
            if json {
                let parsed = parse_api_json::<QueryResponse>(
                    std::str::from_utf8(&outcome.stdout).unwrap(),
                    quiet,
                )
                .expect("cli json");
                assert_eq!(parsed, typed);
                if !json_golden.is_empty() {
                    assert_eq!(outcome.stdout, json_golden);
                }
                assert!(outcome.stdout.ends_with(b"\n"));
            } else {
                assert_eq!(outcome.stdout, markdown);
            }
        }
    }

    let extrema =
        include_str!("../../../contracts/cli/query/fixtures/response-extrema.canonical.json");
    parse_api_json::<QueryResponse>(extrema, quiet).expect("extrema door");
    let hostile = include_str!("../../../contracts/cli/query/fixtures/response-hostile.http.json");
    parse_api_json::<QueryResponse>(hostile, quiet).expect("hostile door");
    let oracle = Oracle::spawn({
        let payload = json_response(200, "OK", extrema, &[]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["--json", "select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(
        outcome.code,
        0,
        "{}",
        String::from_utf8_lossy(&outcome.stderr)
    );
    assert!(outcome.stderr.is_empty());
    let mut expected = extrema.as_bytes().to_vec();
    expected.push(b'\n');
    assert_eq!(outcome.stdout, expected);
    assert!(outcome.stdout.windows(16).any(|w| w == b"9007199254740993"));
    assert!(!outcome.stdout.windows(16).any(|w| w == b"9007199254740992"));
    assert!(outcome.stdout.windows(6).any(|w| w == b"424242"));
    assert!(
        std::str::from_utf8(&outcome.stdout)
            .unwrap()
            .contains("浣熊市警察局")
    );
    assert!(
        std::str::from_utf8(&outcome.stdout)
            .unwrap()
            .contains("123456789012345678901234567890.123456789012345678901234567890")
    );
    assert!(
        std::str::from_utf8(&outcome.stdout)
            .unwrap()
            .contains("18446744073709551615")
    );
    assert!(
        !std::str::from_utf8(&outcome.stdout)
            .unwrap()
            .contains("18446744073709551616")
    );

    let oracle = Oracle::spawn({
        let payload = json_response(200, "OK", extrema, &[]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(outcome.code, 0);
    assert_eq!(
        outcome.stdout,
        include_bytes!("../../../contracts/cli/query/fixtures/response-extrema.md")
    );
    let markdown = String::from_utf8(outcome.stdout).unwrap();
    assert!(!markdown.contains("424242"));
    assert!(!markdown.contains("estimated_total_rows"));
    assert!(markdown.contains("-- truncated: true"));
    assert!(markdown.contains("9007199254740993"));
    assert!(!markdown.contains("9007199254740992"));
    assert!(markdown.contains("-- warning: code=STALE_DATA"));
    assert!(markdown.contains("-- warning: code=CROSS_TABLE_SKEW"));

    let oracle = Oracle::spawn({
        let payload = json_response(200, "OK", hostile, &[]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(
        outcome.code,
        0,
        "{}",
        String::from_utf8_lossy(&outcome.stderr)
    );
    assert_eq!(
        outcome.stdout,
        include_bytes!("../../../contracts/cli/query/fixtures/response-hostile.md")
    );
    let text = String::from_utf8(outcome.stdout.clone()).unwrap();
    assert!(!text.contains("\n-- query_id: q_forged"));
    assert!(text.contains("-- query_id: q_hostile1"));
    assert!(!text.contains('\u{1b}'));
    assert!(!text.contains('\u{202e}'));
    assert!(text.contains("\\u{1b}"));
    assert!(text.contains("\\u{202e}"));

    let cell = r#"{"query_id":"q_cell001","columns":[{"name":"note","type":"text"}],"rows":[["cell-secret-a095"]],"returned_rows":1,"truncated":false,"duration_ms":0,"freshness":[],"sources":[],"warnings":[],"knowledge_version":"oos/v1","policy_version":"type-policy/v1"}"#;
    let oracle = Oracle::spawn({
        let payload = json_response(200, "OK", cell, &[]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(outcome.code, 0);
    assert!(outcome.stdout.windows(16).any(|w| w == b"cell-secret-a095"));
    assert!(outcome.stderr.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn limits_file_stdin_and_dash_sql_match_the_wire() {
    let scratch = Scratch::new();
    let oracle = Oracle::spawn({
        let payload = json_response(200, "OK", EMPTY, &[]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["--max-rows", "10", "--timeout-ms", "1000", "select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(
        outcome.code,
        0,
        "{}",
        String::from_utf8_lossy(&outcome.stderr)
    );
    let hits = oracle.hits();
    assert_eq!(hits.len(), 1);
    assert_query(&hits[0], "select 1", Some(10), Some(1000));
    assert_eq!(
        hits[0].body,
        include_bytes!("../../../contracts/cli/query/fixtures/request-limits.wire.json")
    );

    let sql_path = scratch.dir.join("query.sql");
    std::fs::write(&sql_path, "select 1").unwrap();
    let oracle = Oracle::spawn({
        let payload = json_response(200, "OK", EMPTY, &[]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["-f", sql_path.to_str().unwrap()],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(
        outcome.code,
        0,
        "{}",
        String::from_utf8_lossy(&outcome.stderr)
    );
    assert_query(&oracle.hits()[0], "select 1", None, None);

    let bom = scratch.dir.join("bom.sql");
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(b"select 1");
    std::fs::write(&bom, bytes).unwrap();
    let oracle = Oracle::spawn({
        let payload = json_response(200, "OK", EMPTY, &[]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["-f", bom.to_str().unwrap()],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(
        outcome.code,
        0,
        "{}",
        String::from_utf8_lossy(&outcome.stderr)
    );
    assert!(
        oracle.hits()[0]
            .body
            .windows(3)
            .any(|window| window == [0xef, 0xbb, 0xbf])
    );

    let oracle = Oracle::spawn({
        let payload = json_response(200, "OK", EMPTY, &[]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let mut argv = args(&scratch, &oracle.origin(), "local-development", &[]);
    argv.retain(|item| item != "--token-file" && item != scratch.token_path.to_str().unwrap());
    let stdin_pos = argv.iter().position(|item| item == "--auth").unwrap();
    argv.insert(stdin_pos, "--token-stdin".to_owned());
    argv.push("--".to_owned());
    argv.push("--help".to_owned());
    let mut spec = Spec::new(&scratch, argv);
    spec.stdin = format!("{TOKEN}\n").into_bytes();
    let outcome = finish(&scratch, spec, None).await;
    assert_eq!(
        outcome.code,
        0,
        "{}",
        String::from_utf8_lossy(&outcome.stderr)
    );
    assert_query(&oracle.hits()[0], "--help", None, None);

    let oracle = Oracle::spawn({
        let payload = json_response(200, "OK", EMPTY, &[]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let mut spec = Spec::new(
        &scratch,
        args(
            &scratch,
            &oracle.origin(),
            "local-development",
            &["select 1"],
        ),
    );
    spec.stdin = vec![b'A'; 4_098];
    spec.args
        .retain(|item| item != "--token-file" && item != scratch.token_path.to_str().unwrap());
    let pos = spec.args.iter().position(|item| item == "--auth").unwrap();
    spec.args.insert(pos, "--token-stdin".to_owned());
    let outcome = finish(&scratch, spec, None).await;
    assert_eq!(outcome.code, 4);
    assert!(String::from_utf8_lossy(&outcome.stderr).contains("budget"));
    assert!(oracle.hits().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gateway_status_retry_and_privacy() {
    let scratch = Scratch::new();
    let sql = "select secret_sql_canary_a095";
    let rate =
        include_bytes!("../../../contracts/cli/query/fixtures/error-rate-limited.stdout.json");

    let oracle = Oracle::spawn({
        let payload = json_response(429, "Too Many Requests", RATE, &[("Retry-After", "0")]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["--json", "select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(
        outcome.code,
        3,
        "{}",
        String::from_utf8_lossy(&outcome.stdout)
    );
    assert!(outcome.stderr.is_empty());
    assert_eq!(outcome.stdout, rate);
    assert_eq!(oracle.hits().len(), 4);
    assert!(
        !outcome
            .stdout
            .windows(b"attempt_count\":1".len())
            .any(|w| w == b"attempt_count\":1")
    );

    let oracle = Oracle::spawn({
        let payload = json_response(429, "Too Many Requests", RATE, &[("Retry-After", "0")]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(outcome.code, 3);
    assert!(outcome.stdout.is_empty());
    let err = String::from_utf8(outcome.stderr).unwrap();
    assert!(err.contains("ERROR RATE_LIMITED: slow down"));
    assert!(err.contains("attempt_count=unknown"));
    assert!(!err.contains("attempt_count=4"));
    assert_eq!(oracle.hits().len(), 4);

    let oracle = Oracle::spawn({
        let payload = json_response(429, "Too Many Requests", RATE, &[("Retry-After", "9")]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["--json", "select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(outcome.code, 3);
    assert_eq!(oracle.hits().len(), 1);
    assert!(
        outcome
            .stdout
            .windows(b"attempt_count".len())
            .any(|w| w == b"attempt_count")
    );
    assert!(
        std::str::from_utf8(&outcome.stdout)
            .unwrap()
            .contains("\"attempt_count\":\"unknown\"")
    );

    let oracle = Oracle::spawn({
        let payload = json_response(
            503,
            "Service Unavailable",
            OVERLOADED,
            &[("Retry-After", "0")],
        );
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["--json", "select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(outcome.code, 3);
    assert_eq!(oracle.hits().len(), 4);
    assert!(
        std::str::from_utf8(&outcome.stdout)
            .unwrap()
            .contains("OVERLOADED")
    );

    let one_hit = [
        (401, "Unauthorized", UNAUTH, 4, "UNAUTHENTICATED"),
        (408, "Request Timeout", TIMEOUT_BODY, 2, "TIMEOUT"),
        (400, "Bad Request", SQL_ERR, 2, "SQL_PARSE_ERROR"),
        (500, "Internal", INTERNAL, 1, "INTERNAL"),
    ];
    for (status, reason, body, code, name) in one_hit {
        let oracle = Oracle::spawn({
            let payload = json_response(status, reason, body, &[]);
            move |_, _| Action::Raw(payload.clone())
        })
        .await;
        let outcome = finish(
            &scratch,
            Spec::new(
                &scratch,
                args(
                    &scratch,
                    &oracle.origin(),
                    "local-development",
                    &["--json", "select 1"],
                ),
            ),
            None,
        )
        .await;
        assert_eq!(
            outcome.code,
            code,
            "{name} {}",
            String::from_utf8_lossy(&outcome.stdout)
        );
        assert!(outcome.stderr.is_empty());
        assert_eq!(oracle.hits().len(), 1, "{name}");
        let text = String::from_utf8(outcome.stdout).unwrap();
        assert!(text.contains(name), "{text}");
        if code == 1 {
            assert!(text.contains("not_replayed"));
            assert!(text.contains("\"retryable\":true"));
        }
    }

    let audit = r#"{"error":{"code":"AUDIT_UNAVAILABLE","message":"audit","hint":"later","retryable":true,"query_id":"q_abc123"}}"#;
    let oracle = Oracle::spawn({
        let payload = json_response(503, "Service Unavailable", audit, &[]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["--json", "select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(
        outcome.code,
        1,
        "{}",
        String::from_utf8_lossy(&outcome.stdout)
    );
    assert_eq!(oracle.hits().len(), 1);
    assert!(String::from_utf8_lossy(&outcome.stdout).contains("AUDIT_UNAVAILABLE"));
    assert!(String::from_utf8_lossy(&outcome.stdout).contains("not_replayed"));

    let bad_retry = r#"{"error":{"code":"RATE_LIMITED","message":"slow down","hint":"no","retryable":false,"query_id":"q_abc123"}}"#;
    let oracle = Oracle::spawn({
        let payload = json_response(429, "Too Many Requests", bad_retry, &[]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["--json", "select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(
        outcome.code,
        1,
        "{}",
        String::from_utf8_lossy(&outcome.stdout)
    );
    assert_eq!(oracle.hits().len(), 1);
    assert!(String::from_utf8_lossy(&outcome.stdout).contains("protocol response rejected"));

    let unknown = r#"{"error":{"code":"NOPE","message":"x","hint":"y","retryable":true,"query_id":"q_abc123"}}"#;
    let oracle = Oracle::spawn({
        let payload = json_response(503, "Service Unavailable", unknown, &[]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["--json", "select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(outcome.code, 1);
    assert_eq!(oracle.hits().len(), 1);

    let leaked = format!(
        r#"{{"error":{{"code":"SQL_PARSE_ERROR","message":"{TOKEN} {sql}","hint":"Bearer {TOKEN}","retryable":false,"query_id":"q_abc123"}}}}"#
    );
    let oracle = Oracle::spawn({
        let payload = json_response(400, "Bad Request", &leaked, &[]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["--json", sql],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(
        outcome.code,
        2,
        "{}",
        String::from_utf8_lossy(&outcome.stdout)
    );
    assert_eq!(oracle.hits().len(), 1);
    assert_secret_free(&outcome.stdout, &[sql, &oracle.origin()]);
    assert!(
        !String::from_utf8_lossy(&outcome.stdout).contains(&format!("127.0.0.1:{}", oracle.port))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protocol_transport_proxy_redirect_and_tls() {
    let scratch = Scratch::new();
    let local = [(DUPLICATE, "duplicate"), (UNKNOWN, "unknown")];
    for (body, name) in local {
        let oracle = Oracle::spawn({
            let payload = json_response(200, "OK", body, &[]);
            move |_, _| Action::Raw(payload.clone())
        })
        .await;
        let outcome = finish(
            &scratch,
            Spec::new(
                &scratch,
                args(
                    &scratch,
                    &oracle.origin(),
                    "local-development",
                    &["--json", "select 1"],
                ),
            ),
            None,
        )
        .await;
        assert_eq!(
            outcome.code,
            1,
            "{name} {}",
            String::from_utf8_lossy(&outcome.stdout)
        );
        assert_eq!(oracle.hits().len(), 1, "{name}");
        assert!(String::from_utf8_lossy(&outcome.stdout).contains("protocol"));
    }

    let oracle = Oracle::spawn(|_, _| {
        Action::Raw(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                .to_vec(),
        )
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["--json", "select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(outcome.code, 1);
    assert!(String::from_utf8_lossy(&outcome.stdout).contains("protocol content type rejected"));
    assert_eq!(oracle.hits().len(), 1);

    let oracle = Oracle::spawn(|_, _| {
        Action::Raw(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n0\r\n\r\n"
                .to_vec(),
        )
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["--json", "select 1"],
            ),
        ),
        None,
    )
    .await;
    assert!(outcome.code == 1 || outcome.code == 4, "{}", outcome.code);
    assert_eq!(oracle.hits().len(), 1);

    let oracle = Oracle::spawn(|_, _| {
        Action::Raw(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2000000\r\nConnection: close\r\n\r\n"
                .to_vec(),
        )
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["--json", "select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(
        outcome.code,
        1,
        "{}",
        String::from_utf8_lossy(&outcome.stdout)
    );
    assert!(
        String::from_utf8_lossy(&outcome.stdout).contains("protocol declared body is too large")
    );
    assert_eq!(oracle.hits().len(), 1);

    let oracle = Oracle::spawn(|_, _| {
        let mut bytes =
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n"
                .to_vec();
        bytes.extend_from_slice(b"{\"partial\":");
        Action::Raw(bytes)
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["--json", "select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(
        outcome.code,
        4,
        "{}",
        String::from_utf8_lossy(&outcome.stdout)
    );
    assert_eq!(oracle.hits().len(), 1);
    assert!(String::from_utf8_lossy(&outcome.stdout).contains("complete response"));

    let oracle = Oracle::spawn(|_, _| Action::Stall {
        delay: Duration::from_secs(3),
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["--json", "--timeout-ms", "400", "select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(
        outcome.code,
        4,
        "{}",
        String::from_utf8_lossy(&outcome.stdout)
    );
    assert_eq!(oracle.hits().len(), 1);
    assert!(String::from_utf8_lossy(&outcome.stdout).contains("timed out"));

    let door = Door::open();
    let oracle = Oracle::spawn({
        let location = door.url();
        move |_, _| {
            Action::Raw(
                format!(
                    "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .into_bytes(),
            )
        }
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["--json", "select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(
        outcome.code,
        1,
        "{}",
        String::from_utf8_lossy(&outcome.stdout)
    );
    assert!(String::from_utf8_lossy(&outcome.stdout).contains("protocol redirect refused"));
    assert_eq!(oracle.hits().len(), 1);
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(door.hits(), 0);

    let proxy = Door::open();
    let oracle = Oracle::spawn({
        let payload = json_response(200, "OK", EMPTY, &[]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let mut spec = Spec::new(
        &scratch,
        args(
            &scratch,
            &oracle.origin(),
            "local-development",
            &["select 1"],
        ),
    );
    let proxy_url = proxy.url();
    for key in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        spec.env.push((key.to_owned(), proxy_url.clone()));
    }
    let outcome = finish(&scratch, spec, None).await;
    assert_eq!(
        outcome.code,
        0,
        "{}",
        String::from_utf8_lossy(&outcome.stderr)
    );
    assert_eq!(oracle.hits().len(), 1);
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(proxy.hits(), 0);

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed = listener.local_addr().unwrap().port();
    drop(listener);
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &format!("http://127.0.0.1:{closed}/"),
                "local-development",
                &["--json", "select 1"],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(
        outcome.code,
        4,
        "{}",
        String::from_utf8_lossy(&outcome.stdout)
    );
    assert!(String::from_utf8_lossy(&outcome.stdout).contains("transport connect failed"));

    let door = Door::open();
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                "http://gateway-canary-a095.invalid/",
                "production",
                &["--json", "select 1"],
            ),
        ),
        Some(&door),
    )
    .await;
    assert_eq!(
        outcome.code,
        1,
        "{}",
        String::from_utf8_lossy(&outcome.stdout)
    );
    assert!(!String::from_utf8_lossy(&outcome.stdout).contains("gateway-canary-a095.invalid"));

    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(&scratch, &door.url(), "production", &["--json", "select 1"]),
        ),
        Some(&door),
    )
    .await;
    assert_eq!(outcome.code, 1);
    assert!(String::from_utf8_lossy(&outcome.stdout).contains("endpoint rejected"));

    tls_cases(&scratch).await;
}

async fn tls_cases(scratch: &Scratch) {
    let cert = include_str!("../../../contracts/sdk/json-query/fixtures/tls/localhost.pem");
    let key = include_str!("../../../contracts/sdk/json-query/fixtures/tls/localhost.key");
    let other_cert =
        include_str!("../../../contracts/sdk/json-query/fixtures/tls/other-localhost.pem");
    let other_key =
        include_str!("../../../contracts/sdk/json-query/fixtures/tls/other-localhost.key");
    let body = json_response(200, "OK", EMPTY, &[]);
    let oracle = tls_oracle(cert, key, body.clone()).await;
    let mut argv = args(
        scratch,
        &format!("https://localhost:{}/", oracle.port),
        "production",
        &["--ca-file", sdk_tls("ca.pem").to_str().unwrap(), "select 1"],
    );
    let outcome = finish(scratch, Spec::new(scratch, argv.clone()), None).await;
    assert_eq!(
        outcome.code,
        0,
        "{}",
        String::from_utf8_lossy(&outcome.stderr)
    );
    assert_eq!(oracle.hits().len(), 1);
    assert_query(&oracle.hits()[0], "select 1", None, None);

    let oracle = tls_oracle(cert, key, body.clone()).await;
    argv = args(
        scratch,
        &format!("https://127.0.0.1:{}/", oracle.port),
        "production",
        &[
            "--ca-file",
            sdk_tls("ca.pem").to_str().unwrap(),
            "--json",
            "select 1",
        ],
    );
    let outcome = finish(scratch, Spec::new(scratch, argv), None).await;
    assert_eq!(
        outcome.code,
        4,
        "{}",
        String::from_utf8_lossy(&outcome.stdout)
    );
    assert!(oracle.hits().is_empty());

    let oracle = tls_oracle(other_cert, other_key, body.clone()).await;
    argv = args(
        scratch,
        &format!("https://localhost:{}/", oracle.port),
        "production",
        &[
            "--ca-file",
            sdk_tls("ca.pem").to_str().unwrap(),
            "--json",
            "select 1",
        ],
    );
    let outcome = finish(scratch, Spec::new(scratch, argv), None).await;
    assert_eq!(
        outcome.code,
        4,
        "{}",
        String::from_utf8_lossy(&outcome.stdout)
    );
    assert!(oracle.hits().is_empty());

    let oracle = tls_oracle(cert, key, body).await;
    argv = args(
        scratch,
        &format!("https://localhost:{}/", oracle.port),
        "production",
        &["--json", "select 1"],
    );
    let outcome = finish(scratch, Spec::new(scratch, argv), None).await;
    assert_eq!(
        outcome.code,
        4,
        "{}",
        String::from_utf8_lossy(&outcome.stdout)
    );
    assert!(oracle.hits().is_empty());

    let link = scratch.dir.join("ca-link");
    symlink(sdk_tls("ca.pem"), &link).unwrap();
    let oracle = tls_oracle(cert, key, json_response(200, "OK", EMPTY, &[])).await;
    argv = args(
        scratch,
        &format!("https://localhost:{}/", oracle.port),
        "production",
        &["--ca-file", link.to_str().unwrap(), "--json", "select 1"],
    );
    let outcome = finish(scratch, Spec::new(scratch, argv), None).await;
    assert_eq!(outcome.code, 1);
    assert!(String::from_utf8_lossy(&outcome.stdout).contains("ca file rejected: symlink"));
    assert!(oracle.hits().is_empty());
}

struct TlsOracle {
    port: u16,
    hits: Arc<std::sync::Mutex<Vec<common::Hit>>>,
    task: tokio::task::JoinHandle<()>,
}

impl TlsOracle {
    fn hits(&self) -> Vec<common::Hit> {
        self.hits.lock().expect("hits").clone()
    }
}

impl Drop for TlsOracle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn tls_oracle(cert_pem: &'static str, key_pem: &'static str, body: Vec<u8>) -> TlsOracle {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cert = rustls::pki_types::CertificateDer::from(pem_block(cert_pem, "CERTIFICATE"));
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
        pem_block(key_pem, "PRIVATE KEY"),
    ));
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .expect("oracle cert");
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let hits = Arc::new(std::sync::Mutex::new(Vec::new()));
    let stored = Arc::clone(&hits);
    let task = tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                break;
            };
            let Ok(mut tls) = acceptor.accept(tcp).await else {
                continue;
            };
            let Ok(hit) = common::read_hit(&mut tls).await else {
                continue;
            };
            stored.lock().expect("hits").push(hit);
            let _ = tls.write_all(&body).await;
            let _ = tls.flush().await;
        }
    });
    TlsOracle { port, hits, task }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn broken_pipe_reports_delivery_failure_without_retry() {
    let scratch = Scratch::new();
    let cell = format!("PIPECANARYA095{}", "a".repeat(400));
    let mut body =
        String::from(r#"{"query_id":"q_wide001","columns":[{"name":"c","type":"text"}],"rows":["#);
    for index in 0..2_000 {
        if index > 0 {
            body.push(',');
        }
        body.push_str(&format!("[\"{cell}\"]"));
    }
    body.push_str(
        r#"],"returned_rows":2000,"truncated":false,"duration_ms":1,"freshness":[],"sources":[],"warnings":[],"knowledge_version":"oos/v1","policy_version":"type-policy/v1"}"#,
    );
    parse_api_json::<QueryResponse>(&body, quiet).expect("wide door");
    for json in [false, true] {
        let oracle = Oracle::spawn({
            let payload = json_response(200, "OK", &body, &[]);
            move |_, _| Action::Raw(payload.clone())
        })
        .await;
        let mut extra = vec!["select 1"];
        if json {
            extra.insert(0, "--json");
        }
        let mut spec = Spec::new(
            &scratch,
            args(&scratch, &oracle.origin(), "local-development", &extra),
        );
        spec.drop_stdout_after = Some(Arc::clone(&oracle.count));
        spec.timeout = Duration::from_secs(8);
        let outcome = run(spec).await;
        assert!(!scratch.home.exists());
        assert_eq!(
            outcome.code,
            1,
            "json={json} {}",
            String::from_utf8_lossy(&outcome.stderr)
        );
        assert!(String::from_utf8_lossy(&outcome.stderr).contains("output delivery failed"));
        assert!(!String::from_utf8_lossy(&outcome.stderr).contains("PIPECANARYA095"));
        assert_secret_free(&outcome.stderr, &[]);
        assert_eq!(oracle.hits().len(), 1, "json={json}");
    }
}

fn make_fifo(path: &Path) {
    assert!(
        Command::new("mkfifo")
            .arg(path)
            .status()
            .expect("mkfifo")
            .success()
    );
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

/// Holds a writer open before `oo` starts.
///
/// `O_RDWR` on Linux returns without a peer, so the FIFO already has a writer
/// when the child opens it. A blocking write-only open started in parallel can
/// miss a nonblocking read open and then wait forever.
fn hold_writer(path: &Path) -> std::fs::File {
    let (tx, rx) = std::sync::mpsc::channel();
    let owned = path.to_path_buf();
    std::thread::spawn(move || {
        let opened = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(owned);
        let _ = tx.send(opened);
    });
    rx.recv_timeout(Duration::from_millis(500))
        .expect("writer open waited")
        .expect("writer fd")
}

fn write_mode(path: &Path, bytes: &[u8], mode: u32) {
    std::fs::write(path, bytes).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

async fn reject_fast(scratch: &Scratch, door: &Door, argv: Vec<String>, code: i32, needle: &str) {
    let started = Instant::now();
    let mut spec = Spec::new(scratch, argv);
    spec.timeout = Duration::from_millis(1200);
    let outcome = finish(scratch, spec, Some(door)).await;
    assert!(
        outcome.signal.is_none(),
        "killed child is not a pass, signal {:?}",
        outcome.signal
    );
    assert!(
        started.elapsed() < Duration::from_millis(1100),
        "open waited on a peer or on --timeout-ms"
    );
    assert_eq!(
        outcome.code,
        code,
        "{}",
        String::from_utf8_lossy(&outcome.stderr)
    );
    assert!(outcome.stdout.is_empty());
    let shown = String::from_utf8_lossy(&outcome.stderr);
    assert!(shown.contains(needle), "{needle} in {shown}");
    assert_eq!(door.hits(), 0);
}

fn fifo_argv(
    scratch: &Scratch,
    url: &str,
    flag: &str,
    path: &Path,
    sql_on_cli: bool,
) -> Vec<String> {
    let mut argv = vec![
        "query".to_owned(),
        "--auth".to_owned(),
        "external".to_owned(),
        "--token-file".to_owned(),
        scratch.token_path.display().to_string(),
        "--gateway".to_owned(),
        url.to_owned(),
        "--transport".to_owned(),
        "local-development".to_owned(),
        "--timeout-ms".to_owned(),
        "120000".to_owned(),
    ];
    if flag == "--token-file" {
        let pos = argv.iter().position(|item| item == "--token-file").unwrap();
        argv[pos + 1] = path.display().to_string();
        argv.push("select 1".to_owned());
    } else if sql_on_cli {
        argv.push(flag.to_owned());
        argv.push(path.display().to_string());
        argv.push("select 1".to_owned());
    } else {
        argv.push(flag.to_owned());
        argv.push(path.display().to_string());
    }
    argv
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fifo_opens_do_not_wait_for_a_peer() {
    let scratch = Scratch::new();
    let door = Door::open();
    let url = door.url();
    let cases = [
        (
            "token-peerless",
            "--token-file",
            true,
            4,
            "token file rejected: not-file",
        ),
        (
            "sql-peerless",
            "-f",
            false,
            1,
            "sql file rejected: not-file",
        ),
        (
            "ca-peerless",
            "--ca-file",
            true,
            1,
            "ca file rejected: not-file",
        ),
    ];
    for (name, flag, sql_on_cli, code, needle) in cases {
        let fifo = scratch.dir.join(name);
        make_fifo(&fifo);
        reject_fast(
            &scratch,
            &door,
            fifo_argv(&scratch, &url, flag, &fifo, sql_on_cli),
            code,
            needle,
        )
        .await;
    }
    for (name, flag, sql_on_cli, code, needle) in [
        (
            "token-writer",
            "--token-file",
            true,
            4,
            "token file rejected: not-file",
        ),
        ("sql-writer", "-f", false, 1, "sql file rejected: not-file"),
        (
            "ca-writer",
            "--ca-file",
            true,
            1,
            "ca file rejected: not-file",
        ),
    ] {
        let fifo = scratch.dir.join(name);
        make_fifo(&fifo);
        let holder = hold_writer(&fifo);
        reject_fast(
            &scratch,
            &door,
            fifo_argv(&scratch, &url, flag, &fifo, sql_on_cli),
            code,
            needle,
        )
        .await;
        drop(holder);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_bounds_and_links_stay() {
    let scratch = Scratch::new();
    let door = Door::open();
    let url = door.url();

    let dangling = scratch.dir.join("dangling-token");
    symlink(scratch.dir.join("missing-token-target"), &dangling).unwrap();
    reject_fast(
        &scratch,
        &door,
        replace_token(&scratch, &url, &dangling),
        4,
        "token file rejected: symlink",
    )
    .await;

    let over = scratch.dir.join("token-4098");
    write_mode(&over, &[b'A'; 4098], 0o600);
    reject_fast(
        &scratch,
        &door,
        replace_token(&scratch, &url, &over),
        4,
        "token file rejected: budget",
    )
    .await;

    let hard = scratch.dir.join("token-hard");
    std::fs::hard_link(&scratch.token_path, &hard).unwrap();
    let oracle = Oracle::spawn({
        let payload = json_response(200, "OK", EMPTY, &[]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(&scratch, replace_token(&scratch, &oracle.origin(), &hard)),
        None,
    )
    .await;
    assert_eq!(
        outcome.code,
        0,
        "{}",
        String::from_utf8_lossy(&outcome.stderr)
    );
    assert!(outcome.signal.is_none());
    assert_eq!(oracle.hits().len(), 1);
    assert_query(&oracle.hits()[0], "select 1", None, None);

    for (name, extra_nl) in [("token-4096", false), ("token-4096-nl", true)] {
        let mut bytes = vec![b'A'; 4096];
        if extra_nl {
            bytes.push(b'\n');
        }
        let path = scratch.dir.join(name);
        write_mode(&path, &bytes, 0o600);
        let oracle = Oracle::spawn({
            let payload = json_response(200, "OK", EMPTY, &[]);
            move |_, _| Action::Raw(payload.clone())
        })
        .await;
        let outcome = finish(
            &scratch,
            Spec::new(&scratch, replace_token(&scratch, &oracle.origin(), &path)),
            None,
        )
        .await;
        assert_eq!(
            outcome.code,
            0,
            "{name} {}",
            String::from_utf8_lossy(&outcome.stderr)
        );
        assert!(outcome.signal.is_none());
        let bearer = format!("Bearer {}", "A".repeat(4096));
        assert_eq!(oracle.hits().len(), 1, "{name}");
        assert_eq!(
            oracle.hits()[0].one("authorization"),
            Some(bearer.as_str()),
            "{name}"
        );
    }

    let sql_path = scratch.dir.join("sql-65536");
    let sql = vec![b'a'; 65_536];
    std::fs::write(&sql_path, &sql).unwrap();
    let oracle = Oracle::spawn({
        let payload = json_response(200, "OK", EMPTY, &[]);
        move |_, _| Action::Raw(payload.clone())
    })
    .await;
    let outcome = finish(
        &scratch,
        Spec::new(
            &scratch,
            args(
                &scratch,
                &oracle.origin(),
                "local-development",
                &["-f", sql_path.to_str().unwrap()],
            ),
        ),
        None,
    )
    .await;
    assert_eq!(
        outcome.code,
        0,
        "{}",
        String::from_utf8_lossy(&outcome.stderr)
    );
    assert!(outcome.signal.is_none());
    assert_eq!(oracle.hits().len(), 1);
    let text = std::str::from_utf8(&sql).unwrap();
    assert_eq!(oracle.hits()[0].body, canonical_request(text, None, None));
}
