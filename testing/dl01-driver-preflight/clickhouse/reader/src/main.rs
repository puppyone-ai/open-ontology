mod audit;
mod catalog;
mod fail;
mod guard;
mod http;
mod ipc;

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use fail::{fail, Gate};
use serde_json::{json, Map, Value};

struct Opts {
    command: String,
    values: HashMap<String, Vec<String>>,
    send_forbidden: bool,
    close_session: bool,
    anonymous: bool,
}

fn main() -> ExitCode {
    ExitCode::from(tokio_main())
}

#[tokio::main(flavor = "current_thread")]
async fn tokio_main() -> u8 {
    match run().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

async fn run() -> Gate<u8> {
    let opts = parse_args()?;
    match opts.command.as_str() {
        "dba" | "read" => text_command(&opts).await,
        "arrow" => arrow_command(&opts).await,
        "expect-fail" => expect_fail(&opts).await,
        "cancel" => cancel_command(&opts).await,
        "judge-audit" => judge_audit(&opts),
        "judge-catalog" => judge_catalog(&opts),
        other => Err(fail(format!("unknown command {other}"))),
    }
}

async fn text_command(opts: &Opts) -> Gate<u8> {
    let outcome = send(opts, opts.command == "read").await?;
    save_body(opts, &outcome)?;
    let out_arg = required(opts, "out")?;
    let path = Path::new(&out_arg);
    http::write_json(path, &http::outcome_json(&opts.command, &optional(opts, "query-id"), &outcome, Map::new()))?;
    if outcome.status == 200 && outcome.error.is_none() {
        Ok(0)
    } else {
        Ok(3)
    }
}

async fn arrow_command(opts: &Opts) -> Gate<u8> {
    let outcome = send(opts, true).await?;
    save_body(opts, &outcome)?;
    let out_arg = required(opts, "out")?;
    let out = Path::new(&out_arg);
    let ipc_path = required(opts, "ipc-out")?;
    let row_cap = number(opts, "row-cap", 100)?;
    let byte_cap = number(opts, "byte-cap", 32_000_000)?;
    let mut extra = Map::new();
    let code = if outcome.status == 200 && outcome.error.is_none() {
        match ipc::inspect_ipc(&outcome.body, row_cap, byte_cap) {
            Ok(info) => {
                http::write_bytes(Path::new(&ipc_path), &outcome.body)?;
                extra.insert("rows".into(), json!(info.rows));
                extra.insert("batches".into(), json!(info.batches));
                extra.insert("fields".into(), json!(info.fields));
                extra.insert("ipc_sha256".into(), json!(http::sha256_hex(&outcome.body)));
                0
            }
            Err(error) => {
                extra.insert("ipc_error".into(), json!(error.0));
                2
            }
        }
    } else {
        extra.insert("ipc_written".into(), json!(false));
        3
    };
    extra.insert("ipc_written".into(), json!(code == 0));
    http::write_json(out, &http::outcome_json("arrow", &optional(opts, "query-id"), &outcome, extra))?;
    Ok(code)
}

async fn expect_fail(opts: &Opts) -> Gate<u8> {
    let outcome = send(opts, !opts.send_forbidden).await?;
    save_body(opts, &outcome)?;
    let out_arg = required(opts, "out")?;
    let out = Path::new(&out_arg);
    http::write_json(out, &http::outcome_json("expect-fail", &optional(opts, "query-id"), &outcome, Map::new()))?;
    if outcome.status == 200 {
        Ok(2)
    } else if outcome.status == 0 || (outcome.error.is_some() && outcome.exception_code.is_none()) {
        Ok(3)
    } else if outcome.exception_code.is_none() {
        Ok(4)
    } else {
        Ok(0)
    }
}

async fn cancel_command(opts: &Opts) -> Gate<u8> {
    let started_flag = required(opts, "started-flag")?;
    let abort_flag = required(opts, "abort-flag")?;
    let sql = load_sql(opts, true)?;
    let endpoint = required(opts, "endpoint")?;
    let url = http::endpoint_url(&endpoint, &query_params(opts)?)?;
    let headers = auth_headers(opts)?;
    let timeout = Duration::from_millis(millis(opts, "timeout-ms", 25_000)?);
    let cap = number(opts, "byte-cap", 32_000_000)?;
    let client = http::build_client(timeout)?;
    let task = tokio::spawn(async move {
        http::execute(&client, &url, headers, &sql, cap).await
    });
    if let Err(error) = fs::write(&started_flag, b"started\n") {
        task.abort();
        let _ignored = task.await;
        return Err(fail(format!("started flag: {error}")));
    }
    let abort = Path::new(&abort_flag);
    loop {
        if abort.exists() {
            task.abort();
            break;
        }
        if task.is_finished() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (outcome, client_closed) = match task.await {
        Ok(outcome) => (outcome, false),
        Err(error) if error.is_cancelled() => (
            http::HttpOutcome {
                status: 0,
                headers: Vec::new(),
                body: Vec::new(),
                elapsed_ms: 0,
                error: Some("client_aborted".into()),
                exception_code: None,
            },
            true,
        ),
        Err(error) => return Err(fail(format!("cancel task: {error}"))),
    };
    let mut extra = Map::new();
    extra.insert("client_closed".into(), json!(client_closed));
    http::write_json(
        Path::new(&required(opts, "out")?),
        &http::outcome_json("cancel", &optional(opts, "query-id"), &outcome, extra),
    )?;
    if outcome.status == 200 {
        Ok(4)
    } else {
        Ok(0)
    }
}

fn judge_audit(opts: &Opts) -> Gate<u8> {
    let reader_log = fs::read_to_string(required(opts, "reader-log")?).map_err(|error| fail(format!("reader log: {error}")))?;
    let dba_log = fs::read_to_string(required(opts, "dba-log")?).map_err(|error| fail(format!("dba log: {error}")))?;
    let expect = fs::read_to_string(required(opts, "expect")?).map_err(|error| fail(format!("expect: {error}")))?;
    let expect: Value = serde_json::from_str(&expect).map_err(|error| fail(format!("expect json: {error}")))?;
    let verdict = audit::judge(&reader_log, &dba_log, &expect)?;
    http::write_json(Path::new(&required(opts, "out")?), &verdict)?;
    Ok(exit_passed(&verdict))
}

fn judge_catalog(opts: &Opts) -> Gate<u8> {
    let columns = fs::read_to_string(required(opts, "columns")?).map_err(|error| fail(format!("columns: {error}")))?;
    let table = fs::read_to_string(required(opts, "table")?).map_err(|error| fail(format!("table: {error}")))?;
    let parts = fs::read_to_string(required(opts, "parts")?).map_err(|error| fail(format!("parts: {error}")))?;
    let gold = fs::read_to_string(required(opts, "nofinal-gold")?).map_err(|error| fail(format!("gold: {error}")))?;
    let gold: Value = serde_json::from_str(&gold).map_err(|error| fail(format!("gold json: {error}")))?;
    let verdict = catalog::judge(&columns, &table, &parts, &gold)?;
    http::write_json(Path::new(&required(opts, "out")?), &verdict)?;
    Ok(exit_passed(&verdict))
}

fn save_body(opts: &Opts, outcome: &http::HttpOutcome) -> Gate<()> {
    let Some(path) = optional_some(opts, "body-out") else {
        return Ok(());
    };
    http::write_bytes(Path::new(&path), &outcome.body)
}

fn exit_passed(verdict: &Value) -> u8 {
    if verdict.get("passed").and_then(Value::as_bool) == Some(true) {
        0
    } else {
        2
    }
}

async fn send(opts: &Opts, guard_sql: bool) -> Gate<http::HttpOutcome> {
    let sql = load_sql(opts, guard_sql)?;
    let endpoint = required(opts, "endpoint")?;
    let url = http::endpoint_url(&endpoint, &query_params(opts)?)?;
    let headers = auth_headers(opts)?;
    let timeout = Duration::from_millis(millis(opts, "timeout-ms", 15_000)?);
    let cap = number(opts, "byte-cap", 32_000_000)?;
    let client = http::build_client(timeout)?;
    Ok(http::execute(&client, &url, headers, &sql, cap).await)
}

fn load_sql(opts: &Opts, guard_sql: bool) -> Gate<String> {
    let sql = if let Some(path) = optional_some(opts, "body") {
        fs::read_to_string(&path).map_err(|error| fail(format!("body: {error}")))?
    } else if let Some(text) = optional_some(opts, "sql") {
        text
    } else {
        return Err(fail("missing --body or --sql"));
    };
    let guarded = guard_sql || (!opts.send_forbidden && opts.command != "dba");
    if guarded {
        guard::assert_reader_sql(&sql)?;
    }
    if extra_statement(&sql) {
        return Err(fail("refusing more than one statement"));
    }
    Ok(sql)
}

fn extra_statement(sql: &str) -> bool {
    let trimmed = sql.trim().trim_end_matches(';').trim_end();
    trimmed.contains(';')
}

fn auth_headers(opts: &Opts) -> Gate<reqwest::header::HeaderMap> {
    if opts.anonymous {
        return Ok(reqwest::header::HeaderMap::new());
    }
    let user = http::read_trimmed(Path::new(&required(opts, "user-file")?))?;
    let password = http::password_header(Path::new(&required(opts, "password-file")?))?;
    http::reader_headers(&user, &password)
}

fn query_params(opts: &Opts) -> Gate<Vec<(String, String)>> {
    let mut params = Vec::new();
    if let Some(query_id) = optional_some(opts, "query-id") {
        params.push(("query_id".into(), query_id));
    }
    if let Some(session_id) = optional_some(opts, "session-id") {
        params.push(("session_id".into(), session_id));
    }
    if let Some(timeout) = optional_some(opts, "session-timeout") {
        params.push(("session_timeout".into(), timeout));
    }
    if opts.close_session {
        params.push(("close_session".into(), "1".into()));
    }
    if let Some(values) = opts.values.get("param") {
        for value in values {
            let Some((key, raw)) = value.split_once('=') else {
                return Err(fail(format!("--param {value} is not key=value")));
            };
            params.push((key.to_string(), raw.to_string()));
        }
    }
    Ok(params)
}

fn parse_args() -> Gate<Opts> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or_else(|| fail("missing command"))?;
    let mut values: HashMap<String, Vec<String>> = HashMap::new();
    let mut send_forbidden = false;
    let mut close_session = false;
    let mut anonymous = false;
    let rest: Vec<String> = args.collect();
    let mut index = 0;
    while index < rest.len() {
        let item = &rest[index];
        let Some(key) = item.strip_prefix("--") else {
            return Err(fail(format!("unexpected argument {item}")));
        };
        if matches!(key, "send-forbidden" | "close-session" | "anonymous") {
            match key {
                "send-forbidden" => send_forbidden = true,
                "close-session" => close_session = true,
                "anonymous" => anonymous = true,
                _ => {}
            }
            index += 1;
            continue;
        }
        let Some(value) = rest.get(index + 1) else {
            return Err(fail(format!("missing value for --{key}")));
        };
        if value.starts_with("--") {
            return Err(fail(format!("missing value for --{key}")));
        }
        values.entry(key.to_string()).or_default().push(value.clone());
        index += 2;
    }
    Ok(Opts {
        command,
        values,
        send_forbidden,
        close_session,
        anonymous,
    })
}

fn required(opts: &Opts, key: &str) -> Gate<String> {
    let values = opts.values.get(key).ok_or_else(|| fail(format!("missing --{key}")))?;
    if values.len() != 1 {
        return Err(fail(format!("--{key} must be given once")));
    }
    Ok(values[0].clone())
}

fn optional(opts: &Opts, key: &str) -> String {
    optional_some(opts, key).unwrap_or_default()
}

fn optional_some(opts: &Opts, key: &str) -> Option<String> {
    opts.values.get(key).and_then(|values| values.first().cloned())
}

fn millis(opts: &Opts, key: &str, default: u64) -> Gate<u64> {
    match optional_some(opts, key) {
        None => Ok(default),
        Some(text) => text.parse().map_err(|_| fail(format!("--{key} is not an integer"))),
    }
}

fn number(opts: &Opts, key: &str, default: usize) -> Gate<usize> {
    match optional_some(opts, key) {
        None => Ok(default),
        Some(text) => text.parse().map_err(|_| fail(format!("--{key} is not an integer"))),
    }
}
