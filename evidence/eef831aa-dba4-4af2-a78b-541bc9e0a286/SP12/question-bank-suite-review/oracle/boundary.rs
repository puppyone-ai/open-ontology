//! Drive the integrated adapter and the pre-seam adapter with the same bytes.
//! Std only. No product overlay.

#![forbid(unsafe_code)]

use std::env;
use std::io::Write;
use std::process::{Command, ExitCode, Stdio};

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NOT_FOUND: &str = "{\"kind\":\"not_found\",\"completion\":\"complete\",\"warnings\":[]}\n";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("boundary: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let new_bin = env::var("OO_A286_NEW_ADAPTER").map_err(|_| "OO_A286_NEW_ADAPTER unset")?;
    let old_bin = env::var("OO_A286_OLD_ADAPTER").map_err(|_| "OO_A286_OLD_ADAPTER unset")?;
    let clean = summary("succeeded", 0, false);
    let omitted = stream(&header(20, None), &[], &clean);
    let explicit = stream(&header(20, Some("rows")), &[], &clean);
    let opted = stream(&header(20, Some("not_found_if_complete_empty")), &[], &clean);
    let old_omitted = drive(&old_bin, &omitted)?;
    let new_omitted = drive(&new_bin, &omitted)?;
    expect_ok("old_omitted", &old_omitted)?;
    expect_ok("new_omitted", &new_omitted)?;
    same("omitted_matches_old", &new_omitted.stdout, &old_omitted.stdout)?;
    empty_table("omitted", &new_omitted.stdout)?;
    let new_rows = drive(&new_bin, &explicit)?;
    expect_ok("explicit_rows", &new_rows)?;
    same("rows_matches_omitted", &new_rows.stdout, &new_omitted.stdout)?;
    let old_opt = drive(&old_bin, &opted)?;
    if old_opt.code != 2 || !old_opt.stderr.contains("empty_result") {
        return Err(format!(
            "old adapter did not refuse the new key code={} stderr={}",
            old_opt.code, old_opt.stderr
        ));
    }
    println!("old_refuses_empty_result=1");
    let new_opt = drive(&new_bin, &opted)?;
    expect_ok("opt_in_clean", &new_opt)?;
    if new_opt.stdout != NOT_FOUND {
        return Err(format!("opt-in clean stdout={}", new_opt.stdout));
    }
    println!("opt_in_clean=not_found");

    let truncated_summary = summary("succeeded", 0, true);
    compare_break(&new_bin, &old_bin, "truncated", 20, &truncated_summary)?;
    compare_break(&new_bin, &old_bin, "failed", 20, &summary("failed", 0, false))?;
    compare_break(&new_bin, &old_bin, "max_rows_0", 0, &clean)?;

    let row = r#"{"record":"row","turn":0,"cells":[{"kind":"text","value":"kept-row"}]}"#;
    let columns = r#"[{"name":"label","ordinal":0,"type":"text"}]"#;
    let done = summary("succeeded", 1, false);
    let plain = stream(&header_columns(columns, 20, None), &[row], &done);
    let named = stream(&header_columns(columns, 20, Some("rows")), &[row], &done);
    let opted_row = stream(
        &header_columns(columns, 20, Some("not_found_if_complete_empty")),
        &[row],
        &done,
    );
    let old_row = drive(&old_bin, &plain)?;
    let new_plain = drive(&new_bin, &plain)?;
    let new_named = drive(&new_bin, &named)?;
    let new_opted = drive(&new_bin, &opted_row)?;
    expect_ok("old_row", &old_row)?;
    expect_ok("new_plain", &new_plain)?;
    expect_ok("new_named", &new_named)?;
    expect_ok("new_opted_row", &new_opted)?;
    same("contrary_omitted_matches_old", &new_plain.stdout, &old_row.stdout)?;
    same("contrary_rows_matches_omitted", &new_named.stdout, &new_plain.stdout)?;
    same("contrary_opt_in_stays_rows", &new_opted.stdout, &new_plain.stdout)?;
    if !new_opted.stdout.contains("kept-row") || new_opted.stdout.contains("not_found") {
        return Err(format!("contrary row stdout={}", new_opted.stdout));
    }
    println!("contrary_rows=preserved");
    println!("boundary_ok");
    Ok(())
}

fn compare_break(new_bin: &str, old_bin: &str, name: &str, max_rows: u64, summary: &str) -> Result<(), String> {
    let old_input = stream(&header(max_rows, None), &[], summary);
    let new_input = stream(&header(max_rows, Some("not_found_if_complete_empty")), &[], summary);
    let old = drive(old_bin, &old_input)?;
    let new = drive(new_bin, &new_input)?;
    expect_ok(&format!("old_{name}"), &old)?;
    expect_ok(&format!("new_{name}"), &new)?;
    same(&format!("{name}_matches_old"), &new.stdout, &old.stdout)?;
    empty_table(name, &new.stdout)?;
    println!("{name}_never_not_found=1");
    Ok(())
}

fn header(max_rows: u64, empty: Option<&str>) -> String {
    header_columns("[]", max_rows, empty)
}

fn header_columns(columns: &str, max_rows: u64, empty: Option<&str>) -> String {
    let mut text = format!(
        "{{\"columns\":{columns},\"max_result_bytes\":4096,\"max_rows\":{max_rows},\"record\":\"header\",\"turn\":0,\"version\":\"oo.benchmark.query-result-adapter/v1\""
    );
    if let Some(empty) = empty {
        text.push_str(&format!(",\"empty_result\":\"{empty}\""));
    }
    text.push('}');
    text
}

fn summary(status: &str, rows: u64, truncated: bool) -> String {
    let bytes = if rows == 0 { 0 } else { 4 };
    format!(
        "{{\"end_audit_ok\":true,\"executed_sha256\":\"{SHA}\",\"original_sha256\":\"{SHA}\",\"record\":\"summary\",\"result_bytes\":{bytes},\"returned_rows\":{rows},\"rollback_confirmed\":true,\"socket_dropped\":false,\"sqlstate\":null,\"status\":\"{status}\",\"stop\":null,\"truncated\":{truncated},\"turn\":0}}"
    )
}

fn stream(header: &str, rows: &[&str], summary: &str) -> String {
    let mut out = String::new();
    out.push_str(header);
    out.push('\n');
    for row in rows {
        out.push_str(row);
        out.push('\n');
    }
    out.push_str(summary);
    out.push('\n');
    out
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn drive(bin: &str, input: &str) -> Result<Out, String> {
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
        .write_all(input.as_bytes())
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

fn same(name: &str, left: &str, right: &str) -> Result<(), String> {
    if left != right {
        return Err(format!("{name} left={left} right={right}"));
    }
    println!("{name}=1");
    Ok(())
}

fn empty_table(name: &str, text: &str) -> Result<(), String> {
    if !text.contains("\"kind\":\"rows\"") || !text.contains("\"rows\":[]") || text.contains("not_found") {
        return Err(format!("{name} is not an empty table: {text}"));
    }
    Ok(())
}
