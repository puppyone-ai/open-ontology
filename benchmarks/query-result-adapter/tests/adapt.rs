use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

use oo_query_result_adapter::run;
use serde_json::{Value, json};

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

const EMPTY_ROWS: &str =
    "{\"kind\":\"rows\",\"completion\":\"complete\",\"warnings\":[],\"columns\":[],\"rows\":[]}\n";
const NOT_FOUND: &str = "{\"kind\":\"not_found\",\"completion\":\"complete\",\"warnings\":[]}\n";

fn header_field(columns: &str, field: Option<Value>) -> String {
    let mut value = json!({
        "columns": serde_json::from_str::<Value>(columns).unwrap(),
        "max_result_bytes": 4096,
        "max_rows": 20,
        "record": "header",
        "turn": 0,
        "version": "oo.benchmark.query-result-adapter/v1"
    });
    if let Some(field) = field {
        value
            .as_object_mut()
            .unwrap()
            .insert("empty_result".to_owned(), field);
    }
    serde_json::to_string(&value).unwrap()
}

struct Flags {
    status: &'static str,
    truncated: bool,
    rollback: bool,
    socket: bool,
    end_ok: bool,
    stop: &'static str,
    sqlstate: &'static str,
}

fn summary_flags(flags: Flags) -> String {
    format!(
        r#"{{"record":"summary","turn":0,"status":"{status}","truncated":{truncated},"returned_rows":0,"result_bytes":0,"rollback_confirmed":{rollback},"socket_dropped":{socket},"end_audit_ok":{end_ok},"original_sha256":"{SHA}","executed_sha256":"{SHA}","stop":{stop},"sqlstate":{sqlstate}}}"#,
        status = flags.status,
        truncated = flags.truncated,
        rollback = flags.rollback,
        socket = flags.socket,
        end_ok = flags.end_ok,
        stop = flags.stop,
        sqlstate = flags.sqlstate,
    )
}

fn stdout_of(input: &str) -> String {
    let (code, stdout, stderr) = run_text(input);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty());
    stdout
}

#[test]
fn omitted_and_explicit_rows_keep_the_empty_table_bytes() {
    let clean = summary("succeeded", 0, 0, true, false);
    let omitted = stdout_of(&stream(&header("[]"), &[], &clean));
    let explicit = stdout_of(&stream(
        &header_field("[]", Some(json!("rows"))),
        &[],
        &clean,
    ));
    assert_eq!(omitted, EMPTY_ROWS);
    assert_eq!(explicit, EMPTY_ROWS);

    let row = r#"{"record":"row","turn":0,"cells":[{"kind":"bool","value":true}]}"#;
    let columns = r#"[{"name":"flag","type":"bool","ordinal":0}]"#;
    let done = summary("succeeded", 1, 4, true, false);
    let plain = stdout_of(&stream(&header(columns), &[row], &done));
    let named = stdout_of(&stream(
        &header_field(columns, Some(json!("rows"))),
        &[row],
        &done,
    ));
    let opted = stdout_of(&stream(
        &header_field(columns, Some(json!("not_found_if_complete_empty"))),
        &[row],
        &done,
    ));
    assert_eq!(plain, named);
    assert_eq!(plain, opted);
    assert!(plain.contains("\"kind\":\"rows\""));
    assert!(!plain.contains("not_found"));
}

#[test]
fn opt_in_emits_not_found_only_for_a_clean_zero_row_result() {
    let opt = header_field("[]", Some(json!("not_found_if_complete_empty")));
    let clean = summary_flags(Flags {
        status: "succeeded",
        truncated: false,
        rollback: true,
        socket: false,
        end_ok: true,
        stop: "null",
        sqlstate: "null",
    });
    let found = stdout_of(&stream(&opt, &[], &clean));
    assert_eq!(found, NOT_FOUND);
    let parsed: Value = serde_json::from_str(found.trim_end()).unwrap();
    assert!(parsed.get("columns").is_none());
    assert!(parsed.get("rows").is_none());
    let keys: Vec<&str> = parsed
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["completion", "kind", "warnings"]);

    let with_column = header_field(
        r#"[{"name":"id","type":"text","ordinal":0}]"#,
        Some(json!("not_found_if_complete_empty")),
    );
    assert_eq!(stdout_of(&stream(&with_column, &[], &clean)), NOT_FOUND);

    let cases = [
        Flags {
            status: "failed",
            truncated: false,
            rollback: true,
            socket: false,
            end_ok: true,
            stop: "null",
            sqlstate: "null",
        },
        Flags {
            status: "partial",
            truncated: false,
            rollback: true,
            socket: false,
            end_ok: true,
            stop: "null",
            sqlstate: "null",
        },
        Flags {
            status: "succeeded",
            truncated: true,
            rollback: true,
            socket: false,
            end_ok: true,
            stop: "null",
            sqlstate: "null",
        },
        Flags {
            status: "succeeded",
            truncated: false,
            rollback: false,
            socket: false,
            end_ok: true,
            stop: "null",
            sqlstate: "null",
        },
        Flags {
            status: "succeeded",
            truncated: false,
            rollback: true,
            socket: true,
            end_ok: true,
            stop: "null",
            sqlstate: "null",
        },
        Flags {
            status: "succeeded",
            truncated: false,
            rollback: true,
            socket: false,
            end_ok: true,
            stop: "\"timeout\"",
            sqlstate: "null",
        },
        Flags {
            status: "succeeded",
            truncated: false,
            rollback: true,
            socket: false,
            end_ok: true,
            stop: "null",
            sqlstate: "\"P0002\"",
        },
    ];
    for flags in cases {
        let done = summary_flags(flags);
        let text = stdout_of(&stream(&opt, &[], &done));
        let value: Value = serde_json::from_str(text.trim_end()).unwrap();
        assert_eq!(value["kind"], "rows", "{text}");
        assert_eq!(value["rows"], json!([]), "{text}");
        assert!(value.get("columns").is_some(), "{text}");
        assert!(!text.contains("not_found"), "{text}");
    }
    let truncated = stdout_of(&stream(
        &opt,
        &[],
        &summary_flags(Flags {
            status: "succeeded",
            truncated: true,
            rollback: true,
            socket: false,
            end_ok: true,
            stop: "null",
            sqlstate: "null",
        }),
    ));
    assert_eq!(
        serde_json::from_str::<Value>(truncated.trim_end()).unwrap()["completion"],
        "complete"
    );

    let mut zero_budget = header_field("[]", Some(json!("not_found_if_complete_empty")));
    zero_budget = zero_budget.replacen("\"max_rows\":20", "\"max_rows\":0", 1);
    let budget = stdout_of(&stream(
        &zero_budget,
        &[],
        &summary("succeeded", 0, 0, true, false),
    ));
    assert_eq!(
        budget,
        "{\"kind\":\"rows\",\"completion\":\"complete\",\"warnings\":[],\"columns\":[],\"rows\":[]}\n"
    );

    let refused = stream(
        &opt,
        &[],
        &summary_flags(Flags {
            status: "succeeded",
            truncated: false,
            rollback: true,
            socket: false,
            end_ok: false,
            stop: "null",
            sqlstate: "null",
        }),
    );
    assert_eq!(
        err(&refused).1,
        "query-result-adapter: completion /summary/end_audit_ok\n"
    );
}

#[test]
fn invalid_empty_result_is_schema() {
    let done = summary("succeeded", 0, 0, true, false);
    for field in [
        json!(null),
        json!(1),
        json!(true),
        json!({}),
        json!([]),
        json!(""),
        json!("not_found"),
        json!("ROWS"),
    ] {
        let input = stream(&header_field("[]", Some(field)), &[], &done);
        assert_eq!(
            err(&input).1,
            "query-result-adapter: schema /0/empty_result\n"
        );
    }
}

#[test]
fn current_fixture_headers_stay_empty_tables() {
    for name in [
        "desk-header.jsonl",
        "peek-header.jsonl",
        "count-header.jsonl",
    ] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name);
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("empty_result"), "{name}");
        let head = text.lines().next().unwrap();
        let input = stream(head, &[], &summary("succeeded", 0, 0, true, false));
        let stdout = stdout_of(&input);
        let value: Value = serde_json::from_str(stdout.trim_end()).unwrap();
        assert_eq!(value["kind"], "rows", "{name}");
        assert_eq!(value["rows"], json!([]), "{name}");
        assert!(!stdout.contains("not_found"), "{name}");
    }
}
