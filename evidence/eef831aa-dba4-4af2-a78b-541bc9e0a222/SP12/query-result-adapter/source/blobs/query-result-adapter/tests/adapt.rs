use std::io::Cursor;

use oo_query_result_adapter::run;
use serde_json::Value;

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn run_text(input: &str) -> (i32, String, String) {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(
        Vec::<String>::new(),
        Cursor::new(input.as_bytes()),
        &mut stdout,
        &mut stderr,
    );
    (
        code,
        String::from_utf8(stdout).unwrap(),
        String::from_utf8(stderr).unwrap(),
    )
}

fn header(columns: &str) -> String {
    format!(
        r#"{{"record":"header","version":"oo.benchmark.query-result-adapter/v1","turn":0,"max_rows":20,"max_result_bytes":4096,"columns":{columns}}}"#
    )
}

fn summary(status: &str, rows: u32, bytes: u64, end_ok: bool, truncated: bool) -> String {
    format!(
        r#"{{"record":"summary","turn":0,"status":"{status}","truncated":{truncated},"returned_rows":{rows},"result_bytes":{bytes},"rollback_confirmed":true,"socket_dropped":false,"end_audit_ok":{end_ok},"original_sha256":"{SHA}","executed_sha256":"{SHA}","stop":null,"sqlstate":null}}"#
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

fn ok(input: &str) -> Value {
    let (code, stdout, stderr) = run_text(input);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty());
    assert!(stdout.ends_with('\n'));
    serde_json::from_str(stdout.trim_end()).unwrap()
}

fn err(input: &str) -> (i32, String) {
    let (code, stdout, stderr) = run_text(input);
    assert_eq!(code, 2, "{stdout}");
    assert!(stdout.is_empty());
    (code, stderr)
}

fn token(stderr: &str) -> &str {
    stderr
        .trim_end()
        .strip_prefix("query-result-adapter: ")
        .unwrap()
        .split_once(' ')
        .map(|(token, _)| token)
        .unwrap_or(
            stderr
                .trim_end()
                .strip_prefix("query-result-adapter: ")
                .unwrap(),
        )
}

#[test]
fn maps_gateway_cells_and_keeps_exact_int_text() {
    let input = stream(
        &header(
            r#"[{"name":"flag","type":"bool","ordinal":0},{"name":"qty","type":"int64","ordinal":1},{"name":"note","type":"text","ordinal":2}]"#,
        ),
        &[
            r#"{"record":"row","turn":0,"cells":[{"kind":"bool","value":true},{"kind":"int","value":"-9223372036854775808"},{"kind":"text","value":"雪の票́"}]}"#,
            r#"{"record":"row","turn":0,"cells":[{"kind":"bool","value":false},{"kind":"int","value":"9223372036854775807"},{"kind":"null"}]}"#,
        ],
        &summary("succeeded", 2, 40, true, false),
    );
    let value = ok(&input);
    assert_eq!(value["kind"], "rows");
    assert_eq!(value["completion"], "complete");
    assert_eq!(value["warnings"], serde_json::json!([]));
    assert_eq!(value["rows"][0][1]["value"], "-9223372036854775808");
    assert_eq!(value["rows"][0][2]["value"], "雪の票́");
    assert!(
        value["rows"][0][2]["value"]
            .as_str()
            .unwrap()
            .contains('\u{0301}')
    );
    assert_eq!(value["rows"][1][2], serde_json::json!({"presence":"null"}));
    assert_ne!(value["rows"][1][2]["presence"], "missing");
}

#[test]
fn missing_ordinal_is_not_sql_null() {
    let input = stream(
        &header(
            r#"[{"name":"note","type":"text","ordinal":0},{"name":"seal","type":"text","ordinal":null}]"#,
        ),
        &[r#"{"record":"row","turn":0,"cells":[{"kind":"null"}]}"#],
        &summary("succeeded", 1, 0, true, false),
    );
    let value = ok(&input);
    assert_eq!(value["rows"][0][0]["presence"], "null");
    assert_eq!(value["rows"][0][1]["presence"], "missing");
}

#[test]
fn column_order_follows_the_header() {
    let input = stream(
        &header(
            r#"[{"name":"note","type":"text","ordinal":1},{"name":"lane","type":"int64","ordinal":0}]"#,
        ),
        &[
            r#"{"record":"row","turn":0,"cells":[{"kind":"int","value":"7"},{"kind":"text","value":"beta"}]}"#,
        ],
        &summary("succeeded", 1, 5, true, false),
    );
    let value = ok(&input);
    assert_eq!(value["columns"][0]["name"], "note");
    assert_eq!(value["rows"][0][0]["value"], "beta");
    assert_eq!(value["rows"][0][1]["value"], "7");
}

#[test]
fn decimal_comes_only_from_the_binding() {
    let input = stream(
        &header(
            r#"[{"name":"amount","type":"decimal","ordinal":0},{"name":"whole","type":"decimal","ordinal":1}]"#,
        ),
        &[
            r#"{"record":"row","turn":0,"cells":[{"kind":"text","value":"1.20"},{"kind":"int","value":"-12"}]}"#,
        ],
        &summary("succeeded", 1, 6, true, false),
    );
    let value = ok(&input);
    assert_eq!(value["rows"][0][0]["coefficient"], "120");
    assert_eq!(value["rows"][0][0]["scale"], 2);
    assert_eq!(value["rows"][0][0]["negative"], false);
    assert_eq!(value["rows"][0][1]["coefficient"], "12");
    assert_eq!(value["rows"][0][1]["scale"], 0);
    assert_eq!(value["rows"][0][1]["negative"], true);

    let invented = stream(
        &header(r#"[{"name":"amount","type":"decimal","ordinal":0}]"#),
        &[r#"{"record":"row","turn":0,"cells":[{"kind":"decimal","value":"1.20"}]}"#],
        &summary("succeeded", 1, 4, true, false),
    );
    let (_, stderr) = err(&invented);
    assert_eq!(token(&stderr), "protocol");

    let text_as_int = stream(
        &header(r#"[{"name":"qty","type":"int64","ordinal":0}]"#),
        &[r#"{"record":"row","turn":0,"cells":[{"kind":"text","value":"12"}]}"#],
        &summary("succeeded", 1, 2, true, false),
    );
    let (_, stderr) = err(&text_as_int);
    assert_eq!(token(&stderr), "type");
}

#[test]
fn partial_and_failed_keep_their_words() {
    let row = r#"{"record":"row","turn":0,"cells":[{"kind":"bool","value":true}]}"#;
    let head = header(r#"[{"name":"flag","type":"bool","ordinal":0}]"#);
    let partial = ok(&stream(
        &head,
        &[row],
        &summary("partial", 1, 4, false, false),
    ));
    assert_eq!(partial["completion"], "partial");
    let failed = ok(&stream(
        &head,
        &[row],
        &summary("failed", 1, 4, true, false),
    ));
    assert_eq!(failed["completion"], "failed");
    let truncated = ok(&stream(
        &head,
        &[row],
        &summary("succeeded", 1, 4, true, true),
    ));
    assert_eq!(truncated["completion"], "complete");
}

#[test]
fn succeeded_without_end_audit_is_refused() {
    let input = stream(
        &header(r#"[{"name":"flag","type":"bool","ordinal":0}]"#),
        &[r#"{"record":"row","turn":0,"cells":[{"kind":"bool","value":true}]}"#],
        &summary("succeeded", 1, 4, false, false),
    );
    let (_, stderr) = err(&input);
    assert_eq!(
        stderr,
        "query-result-adapter: completion /summary/end_audit_ok\n"
    );
}

#[test]
fn eof_without_summary_is_not_complete() {
    let input = format!(
        "{}\n{}\n",
        header(r#"[{"name":"flag","type":"bool","ordinal":0}]"#),
        r#"{"record":"row","turn":0,"cells":[{"kind":"bool","value":true}]}"#
    );
    let (_, stderr) = err(&input);
    assert_eq!(token(&stderr), "protocol");
    assert_ne!(token(&stderr), "completion");
}

#[test]
fn duplicate_trailer_and_unknown_record_refuse() {
    let head = header(r#"[{"name":"flag","type":"bool","ordinal":0}]"#);
    let row = r#"{"record":"row","turn":0,"cells":[{"kind":"bool","value":true}]}"#;
    let done = summary("succeeded", 1, 4, true, false);
    let trailer = format!("{head}\n{row}\n{done}\n{row}\n");
    assert_eq!(token(&err(&trailer).1), "protocol");
    let duplicate = format!("{head}\n{row}\n{done}\n{done}\n");
    assert_eq!(token(&err(&duplicate).1), "protocol");
    let unknown = format!("{head}\n{{\"record\":\"page\",\"turn\":0}}\n{done}\n");
    assert_eq!(token(&err(&unknown).1), "protocol");
    let missing_header = format!("{row}\n{done}\n");
    assert_eq!(token(&err(&missing_header).1), "protocol");
    let unterminated = format!("{head}\n{row}\n{done}");
    assert_eq!(token(&err(&unterminated).1), "protocol");
}

#[test]
fn rowcount_and_budget_bind_the_summary() {
    let head = header(r#"[{"name":"flag","type":"bool","ordinal":0}]"#);
    let row = r#"{"record":"row","turn":0,"cells":[{"kind":"bool","value":true}]}"#;
    let mismatch = stream(&head, &[row], &summary("succeeded", 2, 4, true, false));
    assert_eq!(token(&err(&mismatch).1), "rowcount");
    let over = stream(
        &header(r#"[{"name":"flag","type":"bool","ordinal":0}]"#)
            .replace("\"max_rows\":20", "\"max_rows\":0"),
        &[],
        &summary("succeeded", 0, 0, true, false),
    );
    let empty = ok(&over);
    assert_eq!(empty["rows"], serde_json::json!([]));
    assert_eq!(empty["kind"], "rows");
    let bytes = stream(&head, &[row], &summary("succeeded", 1, 5000, true, false));
    assert_eq!(token(&err(&bytes).1), "budget");
}

#[test]
fn duplicate_key_and_bad_int_refuse() {
    let head = header(r#"[{"name":"qty","type":"int64","ordinal":0}]"#);
    let dup = format!(
        "{head}\n{{\"record\":\"row\",\"record\":\"row\",\"turn\":0,\"cells\":[]}}\n{}\n",
        summary("succeeded", 0, 0, true, false)
    );
    assert_eq!(token(&err(&dup).1), "json");
    let bad = stream(
        &head,
        &[r#"{"record":"row","turn":0,"cells":[{"kind":"int","value":"01"}]}"#],
        &summary("succeeded", 1, 2, true, false),
    );
    assert_eq!(token(&err(&bad).1), "int64");
    let negative_zero = stream(
        &header(r#"[{"name":"amount","type":"decimal","ordinal":0}]"#),
        &[r#"{"record":"row","turn":0,"cells":[{"kind":"text","value":"-0.00"}]}"#],
        &summary("succeeded", 1, 5, true, false),
    );
    assert_eq!(token(&err(&negative_zero).1), "decimal");
}

#[test]
fn refusal_does_not_echo_the_cell() {
    let input = stream(
        &header(r#"[{"name":"qty","type":"int64","ordinal":0}]"#),
        &[r#"{"record":"row","turn":0,"cells":[{"kind":"text","value":"beta-secret-cell"}]}"#],
        &summary("succeeded", 1, 16, true, false),
    );
    let (_, stderr) = err(&input);
    assert_eq!(token(&stderr), "type");
    assert!(!stderr.contains("beta-secret-cell"));
}
