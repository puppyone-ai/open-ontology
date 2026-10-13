use serde_json::{json, Value};

use crate::fail::{fail, Gate};

pub fn parse_json_each_row(text: &str) -> Gate<Vec<Value>> {
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("Code:") {
            let snippet = &line[..line.len().min(300)];
            return Err(fail(format!("query_log body is an exception: {snippet}")));
        }
        let line_no = index + 1;
        let value: Value = serde_json::from_str(line).map_err(|error| {
            fail(format!("query_log line {line_no} is not json: {error}"))
        })?;
        rows.push(value);
    }
    Ok(rows)
}

pub fn judge(reader_log: &str, dba_log: &str, expect: &Value) -> Gate<Value> {
    let reader_rows = parse_json_each_row(reader_log)?;
    let dba_rows = parse_json_each_row(dba_log)?;
    let reader_user = expect_str(expect, "reader_user")?;
    let cancel_id = expect_str(expect, "cancel_query_id")?;
    let deadline_id = expect_str(expect, "deadline_query_id")?;
    let cancel_max_ms = expect_u64(expect, "cancel_max_ms")?;
    let deadline_min_ms = expect_u64(expect, "deadline_min_ms")?;
    let deadline_max_ms = expect_u64(expect, "deadline_max_ms")?;
    let own_kill_id = expect
        .get("own_cancel_kill_query_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut failures = Vec::new();
    let mut cancel_hit = false;
    let mut deadline_hit = false;
    let mut own_kill_hit = false;
    let mut operational_selects = 0u64;
    let mut reader_kill_denied = 0u64;
    let mut reader_kill_ok = 0u64;

    for row in &reader_rows {
        let user = text_field(row, "user");
        if user != reader_user {
            failures.push(format!("reader log row is for user {user}"));
            continue;
        }
        let kind = text_field(row, "query_kind");
        let code = number_field(row, "exception_code");
        let written_rows = number_field(row, "written_rows");
        let written_bytes = number_field(row, "written_bytes");
        let duration = number_field(row, "query_duration_ms");
        let query_id = text_field(row, "query_id");
        let comment = text_field(row, "log_comment");
        // A code-0 KillQuery is the reader's own cancel only when the expect
        // names that statement. Every other successful non-select fails.
        let bound_own_kill =
            kind == "KillQuery" && code == 0 && !own_kill_id.is_empty() && query_id == own_kill_id;
        if bound_own_kill {
            own_kill_hit = true;
        }
        if code == 0 && kind != "Select" && !bound_own_kill {
            failures.push(format!("successful reader {kind} query_id {query_id}"));
        }
        if code == 0 && (written_rows != 0 || written_bytes != 0) {
            failures.push(format!("reader query_id {query_id} wrote rows={written_rows} bytes={written_bytes}"));
        }
        if kind == "KillQuery" {
            if code == 0 {
                reader_kill_ok += 1;
                if !bound_own_kill {
                    failures.push(format!("reader KillQuery succeeded query_id {query_id}"));
                }
            } else {
                reader_kill_denied += 1;
            }
        }
        if comment.starts_with("oo-dl01w1-40:op:") && code == 0 {
            if kind != "Select" && !bound_own_kill {
                failures.push(format!("operational {query_id} kind {kind}"));
            } else if kind == "Select" {
                operational_selects += 1;
            }
        }
        if query_id == cancel_id {
            cancel_hit = true;
            if code != 394 {
                failures.push(format!("cancel query_id {query_id} exception {code}, want 394"));
            }
            if duration >= cancel_max_ms {
                failures.push(format!("cancel duration {duration} ms is not under {cancel_max_ms}"));
            }
            if user != reader_user {
                failures.push("cancel user binding does not match the reader".into());
            }
        }
        if query_id == deadline_id {
            deadline_hit = true;
            if code != 159 {
                failures.push(format!("deadline query_id {query_id} exception {code}, want 159"));
            }
            if duration < deadline_min_ms || duration > deadline_max_ms {
                failures.push(format!(
                    "deadline duration {duration} ms is outside {deadline_min_ms}..={deadline_max_ms}"
                ));
            }
        }
    }
    if !cancel_hit {
        failures.push(format!("query_log has no reader row for cancel query_id {cancel_id}"));
    }
    if !deadline_hit {
        failures.push(format!("query_log has no reader row for deadline query_id {deadline_id}"));
    }
    if operational_selects == 0 {
        failures.push("reader query_log has no successful operational Select".into());
    }
    if !own_kill_id.is_empty() && !own_kill_hit {
        failures.push(format!(
            "query_log has no successful own-cancel KillQuery {own_kill_id}"
        ));
    }
    if let Some(minimum) = expect.get("reader_kill_denied_min").and_then(Value::as_u64)
        && reader_kill_denied < minimum
    {
        failures.push(format!(
            "reader denied KillQuery count {reader_kill_denied} is below {minimum}"
        ));
    }

    let mut dba_kill = 0u64;
    for row in &dba_rows {
        if text_field(row, "query_kind") == "KillQuery" {
            dba_kill += number_field(row, "n").max(1);
        }
    }
    if dba_kill != 0 {
        failures.push(format!("DBA query_log contains {dba_kill} KillQuery rows"));
    }

    Ok(json!({
        "passed": failures.is_empty(),
        "failures": failures,
        "reader_rows": reader_rows.len(),
        "operational_selects": operational_selects,
        "cancel_query_id": cancel_id,
        "deadline_query_id": deadline_id,
        "reader_kill_denied": reader_kill_denied,
        "reader_kill_ok": reader_kill_ok,
        "dba_kill_query": dba_kill,
        "zero_write": failures.iter().all(|item| !item.contains("wrote") && !item.contains("successful reader")),
    }))
}

fn expect_str<'a>(value: &'a Value, key: &str) -> Gate<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| fail(format!("audit expect missing {key}")))
}

fn expect_u64(value: &Value, key: &str) -> Gate<u64> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| fail(format!("audit expect missing {key}")))
}

fn text_field(row: &Value, key: &str) -> String {
    match row.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn number_field(row: &Value, key: &str) -> u64 {
    match row.get(key) {
        Some(Value::Number(number)) => number.as_u64().unwrap_or(u64::MAX),
        Some(Value::String(text)) => text.parse().unwrap_or(u64::MAX),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::judge;
    use serde_json::json;

    #[test]
    fn zero_write_cancel_and_deadline_pass() {
        let reader = r#"
{"query_id":"c1","user":"reader","type":"ExceptionWhileProcessing","query_kind":"Select","exception_code":394,"query_duration_ms":900,"written_rows":"0","written_bytes":"0","log_comment":"oo-dl01w1-40:op:cancel"}
{"query_id":"d1","user":"reader","type":"ExceptionWhileProcessing","query_kind":"Select","exception_code":159,"query_duration_ms":2100,"written_rows":0,"written_bytes":0,"log_comment":"oo-dl01w1-40:op:deadline"}
{"query_id":"r1","user":"reader","type":"QueryFinish","query_kind":"Select","exception_code":0,"query_duration_ms":20,"written_rows":0,"written_bytes":0,"log_comment":"oo-dl01w1-40:op:arrow-final"}
{"query_id":"n1","user":"reader","type":"ExceptionBeforeStart","query_kind":"Insert","exception_code":497,"query_duration_ms":1,"written_rows":0,"written_bytes":0,"log_comment":"oo-dl01w1-40:neg:insert"}
{"query_id":"k1","user":"reader","type":"ExceptionBeforeStart","query_kind":"KillQuery","exception_code":497,"query_duration_ms":1,"written_rows":0,"written_bytes":0,"log_comment":"oo-dl01w1-40:neg:kill"}
"#;
        let dba = r#"{"query_kind":"Insert","exception_code":0,"n":4}
{"query_kind":"Select","exception_code":0,"n":3}
"#;
        let expect = json!({
            "reader_user": "reader",
            "cancel_query_id": "c1",
            "deadline_query_id": "d1",
            "cancel_max_ms": 5000,
            "deadline_min_ms": 1500,
            "deadline_max_ms": 4500,
            "reader_kill_denied_min": 1
        });
        let verdict = judge(reader, dba, &expect).unwrap();
        assert_eq!(verdict["passed"], true, "{verdict}");
        assert_eq!(verdict["reader_kill_ok"], 0);
    }

    #[test]
    fn dba_kill_and_slow_cancel_fail() {
        let reader = r#"{"query_id":"c1","user":"reader","query_kind":"Select","exception_code":394,"query_duration_ms":9000,"written_rows":0,"written_bytes":0,"log_comment":"oo-dl01w1-40:op:cancel"}
{"query_id":"d1","user":"reader","query_kind":"Select","exception_code":159,"query_duration_ms":2000,"written_rows":0,"written_bytes":0,"log_comment":"oo-dl01w1-40:op:deadline"}
{"query_id":"r1","user":"reader","query_kind":"Select","exception_code":0,"query_duration_ms":1,"written_rows":0,"written_bytes":0,"log_comment":"oo-dl01w1-40:op:arrow-final"}
"#;
        let dba = r#"{"query_kind":"KillQuery","exception_code":0,"n":1}"#;
        let expect = json!({
            "reader_user": "reader",
            "cancel_query_id": "c1",
            "deadline_query_id": "d1",
            "cancel_max_ms": 5000,
            "deadline_min_ms": 1500,
            "deadline_max_ms": 4500
        });
        let verdict = judge(reader, dba, &expect).unwrap();
        assert_eq!(verdict["passed"], false);
        let text = verdict["failures"].to_string();
        assert!(text.contains("KillQuery"), "{text}");
        assert!(text.contains("cancel duration"), "{text}");
    }

    #[test]
    fn own_cancel_kill_is_bound_and_other_success_fails() {
        let reader = r#"
{"query_id":"c1","user":"reader","type":"ExceptionWhileProcessing","query_kind":"Select","exception_code":394,"query_duration_ms":1007,"written_rows":0,"written_bytes":0,"log_comment":"oo-dl01w1-40:op:profile"}
{"query_id":"ck","user":"reader","type":"QueryFinish","query_kind":"KillQuery","exception_code":0,"query_duration_ms":916,"written_rows":0,"written_bytes":0,"log_comment":"oo-dl01w1-40:op:profile"}
{"query_id":"d1","user":"reader","type":"ExceptionWhileProcessing","query_kind":"Select","exception_code":159,"query_duration_ms":2100,"written_rows":0,"written_bytes":0,"log_comment":"oo-dl01w1-40:op:profile"}
{"query_id":"r1","user":"reader","type":"QueryFinish","query_kind":"Select","exception_code":0,"query_duration_ms":20,"written_rows":0,"written_bytes":0,"log_comment":"oo-dl01w1-40:op:profile"}
{"query_id":"fk","user":"reader","type":"ExceptionBeforeStart","query_kind":"KillQuery","exception_code":497,"query_duration_ms":1,"written_rows":0,"written_bytes":0,"log_comment":"oo-dl01w1-40:op:profile"}
"#;
        let dba = r#"{"query_kind":"Select","exception_code":0,"n":2}"#;
        let expect = json!({
            "reader_user": "reader",
            "cancel_query_id": "c1",
            "deadline_query_id": "d1",
            "own_cancel_kill_query_id": "ck",
            "cancel_max_ms": 5000,
            "deadline_min_ms": 1500,
            "deadline_max_ms": 4500,
            "reader_kill_denied_min": 1
        });
        let verdict = judge(reader, dba, &expect).unwrap();
        assert_eq!(verdict["passed"], true, "{verdict}");
        assert_eq!(verdict["reader_kill_ok"], 1);
        assert_eq!(verdict["reader_kill_denied"], 1);
    }
}
