//! Compare omitted and explicit rows headers on one captured query-pg stdout
//! with the pre-seam adapter. The opt-in header is checked on the new adapter.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::Write;
use std::process::{Command, ExitCode, Stdio};

const NOT_FOUND: &str = "{\"kind\":\"not_found\",\"completion\":\"complete\",\"warnings\":[]}\n";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("live_bytes: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let new_bin = args.next().ok_or("usage: live_bytes NEW OLD QUERY_STDOUT")?;
    let old_bin = args.next().ok_or("usage: live_bytes NEW OLD QUERY_STDOUT")?;
    let query_path = args.next().ok_or("usage: live_bytes NEW OLD QUERY_STDOUT")?;
    if args.next().is_some() {
        return Err("usage: live_bytes NEW OLD QUERY_STDOUT".to_owned());
    }
    let query = fs::read(&query_path).map_err(|err| format!("read query: {err}"))?;
    if query.is_empty() {
        return Err("query stdout is empty".to_owned());
    }
    let omitted = drive(&old_bin, &prefixed(&header(None), &query))?;
    let new_omitted = drive(&new_bin, &prefixed(&header(None), &query))?;
    let new_rows = drive(&new_bin, &prefixed(&header(Some("rows")), &query))?;
    let new_opt = drive(&new_bin, &prefixed(&header(Some("not_found_if_complete_empty")), &query))?;
    expect_ok("old_omitted", &omitted)?;
    expect_ok("new_omitted", &new_omitted)?;
    expect_ok("new_rows", &new_rows)?;
    expect_ok("new_opt", &new_opt)?;
    if new_omitted.stdout != omitted.stdout {
        return Err(format!(
            "omitted bytes differ\nold={}\nnew={}",
            omitted.stdout, new_omitted.stdout
        ));
    }
    if new_rows.stdout != omitted.stdout {
        return Err(format!(
            "rows bytes differ from old omitted\nold={}\nrows={}",
            omitted.stdout, new_rows.stdout
        ));
    }
    if new_opt.stdout != NOT_FOUND {
        return Err(format!("opt-in stdout={}", new_opt.stdout));
    }
    if omitted.stdout.contains("not_found") {
        return Err(format!("old omitted contains not_found: {}", omitted.stdout));
    }
    println!("live_omitted_matches_old=1");
    println!("live_rows_matches_old=1");
    println!("live_opt_in=not_found");
    println!("live_bytes_ok");
    Ok(())
}

fn header(empty: Option<&str>) -> String {
    let mut text = String::from(
        "{\"columns\":[{\"name\":\"v\",\"ordinal\":0,\"type\":\"int64\"}],\"max_result_bytes\":4096,\"max_rows\":20,\"record\":\"header\",\"turn\":0,\"version\":\"oo.benchmark.query-result-adapter/v1\"",
    );
    if let Some(empty) = empty {
        text.push_str(&format!(",\"empty_result\":\"{empty}\""));
    }
    text.push_str("}\n");
    text
}

fn prefixed(header: &str, query: &[u8]) -> Vec<u8> {
    let mut bytes = header.as_bytes().to_vec();
    bytes.extend_from_slice(query);
    bytes
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn drive(bin: &str, input: &[u8]) -> Result<Out, String> {
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("spawn {bin}: {err}"))?;
    child
        .stdin
        .take()
        .ok_or("stdin")?
        .write_all(input)
        .map_err(|err| format!("write {bin}: {err}"))?;
    let output = child.wait_with_output().map_err(|err| format!("wait {bin}: {err}"))?;
    Ok(Out {
        code: output.status.code().unwrap_or(127),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn expect_ok(name: &str, out: &Out) -> Result<(), String> {
    if out.code != 0 || !out.stderr.is_empty() {
        return Err(format!("{name} code={} stderr={}", out.code, out.stderr));
    }
    Ok(())
}
