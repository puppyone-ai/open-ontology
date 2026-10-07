use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use query_result_export::{Completeness, Fault, export_to, render, sha256_hex};

const CLERK_SHA: &str = "54cabda641bf1a2128e0246a78eb1a761895a0aa5a4226d136ad8aec399ad6eb";
const CLERK_CSV_SHA: &str = "9c7c1562db84359e654e50f7f836465062759af12d84f36d52aa6f845c1607f1";
const CLERK: &[&str] = &[
    "order_id",
    "customer_id",
    "memo",
    "posted",
    "amount",
    "clerk_note",
];

#[test]
fn sha256_matches_the_empty_and_abc_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn compiled_clerk_stdout_matches_the_hand_csv_and_sidecar() {
    let source = include_bytes!("fixtures/query-clerk-order_doc-star.stdout");
    let golden = include_bytes!("fixtures/golden-clerk.csv");
    assert_eq!(source.len(), 1027);
    assert_eq!(golden.len(), 171);
    assert_eq!(sha256_hex(source), CLERK_SHA);
    assert_eq!(sha256_hex(golden), CLERK_CSV_SHA);
    let export = render(source, CLERK).expect("clerk export");
    assert!(export.complete);
    assert_eq!(export.csv, golden);
    assert_eq!(
        String::from_utf8(export.sidecar).unwrap(),
        clerk_sidecar(source, golden)
    );
    assert!(!export.csv.windows(2).any(|pair| pair == b"'-"));
    assert!(export.csv.windows(20).any(|window| window == b"-9223372036854775808"));
}

#[test]
fn hand_formula_row_keeps_canonical_ints_and_prefixes_text() {
    let source = include_bytes!("fixtures/hand-formula.jsonl");
    let columns = [
        "lo", "hi", "zero", "neg", "eq", "plus", "minus", "at", "sp", "tab", "cr", "lf", "soh",
        "nbsp", "ideo", "quote", "nl", "uni", "empty", "nil", "flag", "safe", "apos", "digits",
    ];
    let export = render(source, &columns).expect("formula export");
    assert!(export.complete);
    assert_eq!(export.csv, formula_csv());
    let side = String::from_utf8(export.sidecar).unwrap();
    assert!(side.contains("\"formula_prefixed\":11"));
    assert!(side.contains("\"formula_cells\":[[0,4],[0,5],[0,6],[0,7],[0,8],[0,9],[0,10],[0,11],[0,12],[0,13],[0,14]]"));
    assert!(side.contains("\"null_cells\":[[0,19]]"));
    assert!(side.contains("\"empty_text_cells\":[[0,18]]"));
    assert!(side.contains("\"text\":\"-9223372036854775808\""));
    assert!(side.contains("\"text\":\"-1\""));
    assert!(side.contains("\"xlsx\":\"unsupported\""));
    assert!(side.contains("\"http_auth\":false"));
    assert!(side.contains("\"query_id\":null"));
    assert!(side.contains("\"in_source\":false"));
    assert!(std::str::from_utf8(&export.csv).unwrap().contains("0,-1,'=1+1"));
}

#[test]
fn truncated_consistent_source_is_partial_and_keeps_the_csv() {
    let source = include_bytes!("fixtures/query-clerk-order_doc-star.stdout");
    let golden = include_bytes!("fixtures/golden-clerk.csv");
    let text = String::from_utf8(source.to_vec()).unwrap();
    let partial = text.replacen("\"truncated\":false", "\"truncated\":true", 1);
    let export = render(partial.as_bytes(), CLERK).expect("partial");
    assert!(!export.complete);
    assert_eq!(export.csv, golden);
    let side = String::from_utf8(export.sidecar).unwrap();
    assert!(side.contains("\"complete\":false"));
    assert!(side.contains("\"partial\":true"));
    assert!(side.contains("\"truncated\":true"));
}

#[test]
fn inconsistent_row_count_writes_nothing_in_render() {
    let source = include_bytes!("fixtures/query-clerk-order_doc-star.stdout");
    let text = String::from_utf8(source.to_vec()).unwrap();
    let bad = text.replacen("\"returned_rows\":3", "\"returned_rows\":2", 1);
    let err = render(bad.as_bytes(), CLERK).expect_err("mismatch");
    assert_eq!(err, Fault::source());
}

#[test]
fn decimal_kind_and_noncanonical_ints_are_source_failures() {
    assert_eq!(render(&one_cell("decimal", "\"1.50\""), &["n"]).unwrap_err(), Fault::source());
    assert_eq!(render(&one_cell("int", "\"01\""), &["n"]).unwrap_err(), Fault::source());
    assert_eq!(render(&one_cell("int", "\"-0\""), &["n"]).unwrap_err(), Fault::source());
    assert_eq!(render(&one_cell("int", "\"+1\""), &["n"]).unwrap_err(), Fault::source());
    assert_eq!(render(&one_cell("numeric", "\"1\""), &["n"]).unwrap_err(), Fault::source());
}

#[test]
fn duplicate_key_and_raw_cr_are_refused() {
    let dup = b"{\"record\":\"row\",\"record\":\"row\",\"turn\":0,\"cells\":[{\"kind\":\"null\"}]}\n";
    assert_eq!(render(dup, &["n"]).unwrap_err(), Fault::source());
    let cr = b"{\"record\":\"summary\",\"turn\":0}\r\n";
    assert_eq!(render(cr, &["n"]).unwrap_err(), Fault::source());
}

#[test]
fn row_and_column_budgets_refuse_without_a_complete_flag() {
    let mut wide = String::from("{\"record\":\"row\",\"turn\":0,\"cells\":[");
    for index in 0..513 {
        if index > 0 {
            wide.push(',');
        }
        wide.push_str("{\"kind\":\"null\"}");
    }
    wide.push_str("]}\n");
    wide.push_str(&summary(1, 0, false));
    assert_eq!(render(wide.as_bytes(), &["n"]).unwrap_err(), Fault::budget());

    let mut tall = String::new();
    for _ in 0..2001 {
        tall.push_str("{\"record\":\"row\",\"turn\":0,\"cells\":[{\"kind\":\"int\",\"value\":\"1\"}]}\n");
    }
    tall.push_str(&summary(2001, 2001, false));
    assert_eq!(render(tall.as_bytes(), &["n"]).unwrap_err(), Fault::budget());
}

#[test]
fn source_over_the_byte_budget_is_budget() {
    let mut bytes = vec![b'x'; 32 * 1024 * 1024 + 1];
    bytes[0] = b'{';
    assert_eq!(render(&bytes, &["n"]).unwrap_err(), Fault::budget());
}

#[test]
fn caller_width_mismatch_is_a_request_failure() {
    let source = include_bytes!("fixtures/query-clerk-order_doc-star.stdout");
    assert_eq!(render(source, &["only"]).unwrap_err(), Fault::request());
}

#[test]
fn zero_rows_write_the_header_only() {
    let source = summary(0, 0, false);
    let export = render(source.as_bytes(), &["lane"]).expect("empty");
    assert!(export.complete);
    assert_eq!(export.csv, b"lane\r\n");
    let side = String::from_utf8(export.sidecar).unwrap();
    assert!(side.contains("\"kind\":\"none\""));
    assert!(side.contains("\"parsed_rows\":0"));
}

#[test]
fn formula_prone_header_is_prefixed_and_not_counted_as_a_cell() {
    let source = format!(
        "{}\n{}",
        r#"{"record":"row","turn":0,"cells":[{"kind":"text","value":"x"}]}"#,
        summary(1, 1, false)
    );
    let export = render(source.as_bytes(), &["=cmd"]).expect("header");
    assert_eq!(export.csv, b"'=cmd\r\nx\r\n");
    let side = String::from_utf8(export.sidecar).unwrap();
    assert!(side.contains("\"formula_prefixed\":0"));
}

#[test]
fn failed_status_with_matching_counts_is_not_complete() {
    let mut source = summary(0, 0, false);
    source = source.replacen("\"status\":\"succeeded\"", "\"status\":\"failed\"", 1);
    let export = render(source.as_bytes(), &["lane"]).expect("failed status");
    assert!(!export.complete);
}

#[test]
fn export_to_refuses_symlink_and_existing_names() {
    let source = summary(0, 0, false).into_bytes();
    let dir = scratch();
    fs::write(dir.join("export.csv"), b"keep").unwrap();
    let err = export_to(&source, &["lane"], &dir).unwrap_err();
    assert_eq!(err, Fault::path());
    assert_eq!(fs::read(dir.join("export.csv")).unwrap(), b"keep");
    assert!(!dir.join("export.sidecar.json").exists());
    assert!(!dir.join("export.source.jsonl").exists());

    let real = scratch();
    let parent = scratch();
    let link = parent.join("linked");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert_eq!(
        export_to(&source, &["lane"], &link).unwrap_err(),
        Fault::path()
    );
    assert!(fs::read_dir(&real).unwrap().next().is_none());
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&real);
    let _ = fs::remove_dir_all(&parent);
}

#[test]
fn export_to_writes_the_exact_source_bytes() {
    let source = include_bytes!("fixtures/query-clerk-order_doc-star.stdout");
    let dir = scratch();
    let done = export_to(source, CLERK, &dir).expect("write");
    assert_eq!(done, Completeness::Complete);
    assert_eq!(fs::read(dir.join("export.source.jsonl")).unwrap(), source);
    assert_eq!(
        fs::read(dir.join("export.csv")).unwrap(),
        include_bytes!("fixtures/golden-clerk.csv")
    );
    let again = export_to(source, CLERK, &dir).unwrap_err();
    assert_eq!(again, Fault::path());
    let _ = fs::remove_dir_all(&dir);
}

fn clerk_sidecar(source: &[u8], csv: &[u8]) -> String {
    let mut out = String::new();
    out.push_str("{\"version\":\"oo.query-result-export/v1\",\"format\":\"csv\",\"xlsx\":\"unsupported\",\"http_auth\":false,\"authenticated\":false,\"complete\":true,\"partial\":false,\"source_sha256\":\"");
    out.push_str(&sha256_hex(source));
    out.push_str("\",\"source_bytes\":1027,\"turn\":0,\"status\":\"succeeded\",\"truncated\":false,\"stop\":null,\"sqlstate\":null,\"end_audit_ok\":true,\"rollback_confirmed\":true,\"socket_dropped\":false,\"original_sha256\":\"2a3ab40de0b44170f24ad916604478c76c2e3b67bb59b280105d56e9893f7750\",\"executed_sha256\":\"30de27abc8df667f41d05241c326f3f4cd09eefd5961caa81d431d3a2bc4e762\",\"citation\":{\"in_source\":false,\"query_id\":null},\"returned_rows\":3,\"result_bytes\":98,\"parsed_rows\":3,\"parsed_result_bytes\":98,\"formula_transformation\":\"apostrophe-prefix-after-leading-whitespace-or-control\",\"formula_prefixed\":0,\"formula_cells\":[],\"null_count\":2,\"empty_text_count\":0,\"null_cells\":[[1,2],[1,5]],\"empty_text_cells\":[],\"columns\":[");
    out.push_str("{\"name\":\"order_id\",\"kind\":\"int\",\"nulls\":0,\"empty_text\":0},");
    out.push_str("{\"name\":\"customer_id\",\"kind\":\"int\",\"nulls\":0,\"empty_text\":0},");
    out.push_str("{\"name\":\"memo\",\"kind\":\"text\",\"nulls\":1,\"empty_text\":0},");
    out.push_str("{\"name\":\"posted\",\"kind\":\"bool\",\"nulls\":0,\"empty_text\":0},");
    out.push_str("{\"name\":\"amount\",\"kind\":\"int\",\"nulls\":0,\"empty_text\":0},");
    out.push_str("{\"name\":\"clerk_note\",\"kind\":\"text\",\"nulls\":1,\"empty_text\":0}");
    out.push_str("],\"int_count\":9,\"int_cells\":[");
    out.push_str("{\"row\":0,\"column\":0,\"text\":\"701\"},");
    out.push_str("{\"row\":0,\"column\":1,\"text\":\"41\"},");
    out.push_str("{\"row\":0,\"column\":4,\"text\":\"-9223372036854775808\"},");
    out.push_str("{\"row\":1,\"column\":0,\"text\":\"702\"},");
    out.push_str("{\"row\":1,\"column\":1,\"text\":\"41\"},");
    out.push_str("{\"row\":1,\"column\":4,\"text\":\"9223372036854775807\"},");
    out.push_str("{\"row\":2,\"column\":0,\"text\":\"703\"},");
    out.push_str("{\"row\":2,\"column\":1,\"text\":\"43\"},");
    out.push_str("{\"row\":2,\"column\":4,\"text\":\"0\"}");
    out.push_str("],\"csv_sha256\":\"");
    out.push_str(&sha256_hex(csv));
    out.push_str("\",\"csv_bytes\":171}\n");
    out
}

fn formula_csv() -> Vec<u8> {
    let mut out = Vec::new();
    push_row(
        &mut out,
        &[b"lo,hi,zero,neg,eq,plus,minus,at,sp,tab,cr,lf,soh,nbsp,ideo,quote,nl,uni,empty,nil,flag,safe,apos,digits"],
    );
    push_row(
        &mut out,
        &[
            b"-9223372036854775808",
            b"9223372036854775807",
            b"0",
            b"-1",
            b"'=1+1",
            b"'+1",
            b"'-2+3",
            b"'@SUM(A1)",
            b"' =1",
            b"'\t=1",
            b"\"'\r=1\"",
            b"\"'\n=1\"",
            &[b'\'', 0x01, b'=', b'1'],
            &[b'\'', 0xC2, 0xA0, b'=', b'1'],
            &[b'\'', 0xE3, 0x80, 0x80, b'=', b'1'],
            b"\"say \"\"hi\"\", next\"",
            b"\"line1\nline2\"",
            "雪".as_bytes(),
            b"",
            b"",
            b"false",
            b"hello",
            b"'=1",
            b"123",
        ],
    );
    out
}

fn push_row(out: &mut Vec<u8>, fields: &[&[u8]]) {
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            out.push(b',');
        }
        out.extend_from_slice(field);
    }
    out.extend_from_slice(b"\r\n");
}

fn one_cell(kind: &str, value_json: &str) -> Vec<u8> {
    let mut body = format!(
        "{{\"record\":\"row\",\"turn\":0,\"cells\":[{{\"kind\":\"{kind}\",\"value\":{value_json}}}]}}\n"
    );
    body.push_str(&summary(1, 1, false));
    body.into_bytes()
}

fn summary(rows: u32, bytes: u64, truncated: bool) -> String {
    let flag = if truncated { "true" } else { "false" };
    format!(
        "{{\"record\":\"summary\",\"turn\":0,\"status\":\"succeeded\",\"truncated\":{flag},\"returned_rows\":{rows},\"result_bytes\":{bytes},\"rollback_confirmed\":true,\"socket_dropped\":false,\"end_audit_ok\":true,\"original_sha256\":\"{}\",\"executed_sha256\":\"{}\",\"stop\":null,\"sqlstate\":null}}\n",
        "a".repeat(64),
        "b".repeat(64)
    )
}

fn scratch() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let root = fs::canonicalize(std::env::temp_dir()).unwrap();
    let dir = root.join(format!(
        "oo-qre-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
