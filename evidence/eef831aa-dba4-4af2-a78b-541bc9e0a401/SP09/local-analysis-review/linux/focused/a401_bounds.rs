//! Independent boundary checks for the fixed local-analysis archive.
//! The citable sums read the private PostgreSQL export. The 39-digit case is a
//! refusal oracle: numeric(38,4) cannot emit it. It is not business evidence.
#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use local_analysis::{Fault, Written, prove_native_cancel, run, sha256_hex};

fn env_path(key: &str) -> PathBuf {
    let value = std::env::var(key).unwrap_or_else(|_| panic!("missing {key}"));
    PathBuf::from(value)
}

fn scratch(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("a401-{name}-{nanos}"));
    fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn esc(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                let code = ch as u32;
                out.push_str(&format!("\\u{code:04x}"));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn assert_empty(dir: &Path) {
    let mut names = fs::read_dir(dir).expect("read out").collect::<Result<Vec<_>, _>>().expect("entries");
    names.retain(|entry| entry.file_name() != ".gitignore");
    assert!(names.is_empty(), "output was not empty");
}

#[allow(clippy::too_many_arguments)]
fn write_request(
    dir: &Path,
    export_dir: &Path,
    lineage: &Path,
    output: &Path,
    operation: &str,
    group_column: usize,
    value_column: usize,
    group_header: &str,
    value_header: &str,
    title: &str,
    engine: &str,
    xlsx: bool,
    parquet: bool,
    duckdb_cli: Option<&str>,
    font_file: Option<&str>,
) -> PathBuf {
    let xlsx_text = if xlsx { "true" } else { "false" };
    let parquet_text = if parquet { "true" } else { "false" };
    let mut body = format!(
        "{{\"version\":\"oo.local-analysis/v1\",\"export_dir\":{},\"lineage_file\":{},\"output_dir\":{},\"operation\":\"{operation}\",\"group_column\":{group_column},\"value_column\":{value_column},\"group_header\":{},\"value_header\":{},\"chart_title\":{},\"cancel_file\":null,\"engine\":\"{engine}\",\"xlsx\":{xlsx_text},\"parquet\":{parquet_text}",
        esc(&export_dir.display().to_string()),
        esc(&lineage.display().to_string()),
        esc(&output.display().to_string()),
        esc(group_header),
        esc(value_header),
        esc(title),
    );
    if let Some(path) = duckdb_cli {
        body.push_str(",\"duckdb_cli\":");
        body.push_str(&esc(path));
    }
    if let Some(path) = font_file {
        body.push_str(",\"font_file\":");
        body.push_str(&esc(path));
    }
    body.push('}');
    let path = dir.join("request.json");
    fs::write(&path, body).expect("request");
    path
}

struct Built {
    export_dir: PathBuf,
    lineage: PathBuf,
    output: PathBuf,
    work: PathBuf,
}

fn build_export(name: &str, rows: &[(&str, &str)], truncated: bool) -> Built {
    let work = scratch(name);
    let export_dir = work.join("export");
    let output = work.join("out");
    fs::create_dir_all(&export_dir).expect("export");
    fs::create_dir(&output).expect("out");
    let mut source = String::new();
    let mut total = 0_u64;
    for (group, value) in rows {
        let cells = format!("{},{}", group, value);
        source.push_str(&format!(
            "{{\"record\":\"row\",\"turn\":0,\"cells\":[{cells}]}}\n"
        ));
        total += cell_weight(group) + cell_weight(value);
    }
    let original = "11".repeat(32);
    let executed = "22".repeat(32);
    let trunc = if truncated { "true" } else { "false" };
    let complete = if truncated { "false" } else { "true" };
    let partial = if truncated { "true" } else { "false" };
    source.push_str(&format!(
        "{{\"record\":\"summary\",\"turn\":0,\"status\":\"succeeded\",\"truncated\":{trunc},\"returned_rows\":{},\"result_bytes\":{total},\"rollback_confirmed\":true,\"socket_dropped\":false,\"end_audit_ok\":true,\"original_sha256\":\"{original}\",\"executed_sha256\":\"{executed}\",\"stop\":null,\"sqlstate\":null}}\n",
        rows.len()
    ));
    fs::write(export_dir.join("export.source.jsonl"), &source).expect("source");
    let csv = b"boundary\r\n";
    fs::write(export_dir.join("export.csv"), csv).expect("csv");
    let sidecar = format!(
        "{{\"version\":\"oo.query-result-export/v1\",\"format\":\"csv\",\"xlsx\":\"unsupported\",\"http_auth\":false,\"authenticated\":false,\"complete\":{complete},\"partial\":{partial},\"source_sha256\":\"{}\",\"source_bytes\":{},\"turn\":0,\"status\":\"succeeded\",\"truncated\":{trunc},\"stop\":null,\"sqlstate\":null,\"end_audit_ok\":true,\"rollback_confirmed\":true,\"socket_dropped\":false,\"original_sha256\":\"{original}\",\"executed_sha256\":\"{executed}\",\"citation\":{{\"in_source\":false,\"query_id\":null}},\"returned_rows\":{},\"result_bytes\":{total},\"parsed_rows\":{},\"parsed_result_bytes\":{total},\"columns\":[{{\"name\":\"g\"}},{{\"name\":\"v\"}}],\"csv_sha256\":\"{}\",\"csv_bytes\":{}}}",
        sha256_hex(source.as_bytes()),
        source.len(),
        rows.len(),
        rows.len(),
        sha256_hex(csv),
        csv.len(),
    );
    fs::write(export_dir.join("export.sidecar.json"), sidecar).expect("sidecar");
    let policy = "33".repeat(32);
    let fingerprint = "abc123def4567890";
    let shared = format!(
        "\"protocol\":\"oo.gateway.query-pg-audit/v1\",\"original_sha256\":\"{original}\",\"executed_sha256\":\"{executed}\",\"fingerprint_hex\":\"{fingerprint}\",\"role\":\"oo_r_clerk\",\"project\":\"synth\",\"policy_id\":\"{policy}\",\"backend_pid\":7,\"backend_start\":\"1\""
    );
    let body = format!(
        "{{{},\"record\":\"admit\"}}\n{{{},\"record\":\"complete\",\"status\":\"succeeded\",\"truncated\":{trunc},\"returned_rows\":{},\"result_bytes\":{total},\"rollback_confirmed\":true,\"socket_dropped\":false,\"stop\":null,\"sqlstate\":null}}\n",
        shared,
        shared,
        rows.len()
    );
    let lineage = work.join("lineage.json");
    let json = format!(
        "{{\"version\":\"oo.local-analysis-lineage/v1\",\"trust\":\"local_operator_declared\",\"query_id\":{},\"receipt_sha256\":\"{}\",\"receipt_body\":{}}}\n",
        esc(&format!("q_{fingerprint}")),
        sha256_hex(body.as_bytes()),
        esc(&body),
    );
    fs::write(&lineage, json).expect("lineage");
    Built {
        export_dir,
        lineage,
        output,
        work,
    }
}

fn cell_weight(cell: &str) -> u64 {
    if cell.contains("\"kind\":\"null\"") {
        return 0;
    }
    if let Some(rest) = cell.split("\"value\":\"").nth(1) {
        let end = rest.find('"').expect("value end");
        return end as u64;
    }
    let coefficient = between(cell, "\"coefficient\":\"", "\"");
    let scale: i32 = between(cell, "\"scale\":", "}").parse().expect("scale");
    let negative = cell.contains("\"negative\":true");
    plain_len(negative, coefficient, scale)
}

fn between<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let from = text.find(start).expect("start") + start.len();
    let rest = &text[from..];
    let to = rest.find(end).expect("end");
    &rest[..to]
}

fn plain_len(negative: bool, coefficient: &str, scale: i32) -> u64 {
    let mut len = coefficient.len();
    if negative {
        len += 1;
    }
    if scale == 0 {
        return len as u64;
    }
    if scale < 0 {
        return (len + scale.unsigned_abs() as usize) as u64;
    }
    let scale_us = scale as usize;
    let total = if coefficient.len() > scale_us {
        len + 1
    } else {
        len + 2 + (scale_us - coefficient.len())
    };
    u64::try_from(total).expect("weight")
}

fn text_cell(text: &str) -> String {
    format!("{{\"kind\":\"text\",\"value\":{}}}", esc(text))
}

fn int_cell(text: &str) -> String {
    format!("{{\"kind\":\"int\",\"value\":{}}}", esc(text))
}

fn dec_cell(negative: bool, coefficient: &str, scale: i32) -> String {
    let flag = if negative { "true" } else { "false" };
    format!(
        "{{\"kind\":\"decimal\",\"negative\":{flag},\"coefficient\":{},\"scale\":{scale}}}",
        esc(coefficient)
    )
}

fn null_cell() -> String {
    "{\"kind\":\"null\"}".to_string()
}

#[test]
fn thirty_nine_digits_are_engine_and_write_nothing() {
    let coefficient = "123456789012345678901234567890123456789";
    assert_eq!(coefficient.len(), 39);
    let rows = [(
        int_cell("1"),
        dec_cell(false, coefficient, 0),
    )];
    let row_refs: Vec<(&str, &str)> = rows.iter().map(|(group, value)| (group.as_str(), value.as_str())).collect();
    let built = build_export("digits39", &row_refs, false);
    let request = write_request(
        &built.work,
        &built.export_dir,
        &built.lineage,
        &built.output,
        "sum",
        0,
        1,
        "g",
        "v",
        "边界",
        "rust_exact",
        false,
        true,
        None,
        None,
    );
    let err = run(&request).expect_err("39 digits");
    assert_eq!(err, Fault::engine());
    assert_empty(&built.output);
}

#[test]
fn formula_text_is_prefixed_and_typed_minus_is_not() {
    let rows = [
        (text_cell("=总额"), int_cell("4")),
        (int_cell("-8"), int_cell("1")),
    ];
    let row_refs: Vec<(&str, &str)> = rows.iter().map(|(group, value)| (group.as_str(), value.as_str())).collect();
    let built = build_export("formula", &row_refs, false);
    let request = write_request(
        &built.work,
        &built.export_dir,
        &built.lineage,
        &built.output,
        "sum",
        0,
        1,
        "=组",
        "v",
        "公式",
        "rust_exact",
        true,
        false,
        None,
        None,
    );
    assert_eq!(run(&request).expect("formula"), Written::Citable);
    let csv = fs::read_to_string(built.output.join("analysis.csv")).expect("csv");
    assert!(csv.starts_with("'=组,v\r\n"), "{csv}");
    assert!(csv.contains("'=总额,4\r\n"), "{csv}");
    assert!(csv.contains("-8,1\r\n"), "{csv}");
    assert!(!csv.contains("'-8"), "{csv}");
    let manifest = fs::read_to_string(built.output.join("analysis.manifest.json")).expect("manifest");
    assert!(manifest.contains("\"audited\":false"));
    assert!(manifest.contains("\"closes_sp09_w05\":false"));
    assert!(manifest.contains("\"citable\":true"));
}

#[test]
fn null_group_and_null_value_are_excluded() {
    let rows = [
        (text_cell("甲"), null_cell()),
        (text_cell("甲"), int_cell("2")),
        (null_cell(), int_cell("5")),
    ];
    let row_refs: Vec<(&str, &str)> = rows.iter().map(|(group, value)| (group.as_str(), value.as_str())).collect();
    let built = build_export("nulls", &row_refs, false);
    let request = write_request(
        &built.work,
        &built.export_dir,
        &built.lineage,
        &built.output,
        "sum",
        0,
        1,
        "组",
        "值",
        "空值",
        "rust_exact",
        false,
        false,
        None,
        None,
    );
    assert_eq!(run(&request).expect("nulls"), Written::Citable);
    let csv = fs::read_to_string(built.output.join("analysis.csv")).expect("csv");
    assert_eq!(csv, "组,值\r\n甲,2\r\n");
    let manifest = fs::read_to_string(built.output.join("analysis.manifest.json")).expect("manifest");
    assert!(manifest.contains("\"null_values\":1"));
    assert!(manifest.contains("\"null_groups\":1"));
    assert!(manifest.contains("\"group_count\":1"));
    assert!(!csv.contains('5'));
}

#[test]
fn truncated_synthetic_is_uncited_and_not_checked() {
    let rows = [(int_cell("1"), int_cell("9"))];
    let row_refs: Vec<(&str, &str)> = rows.iter().map(|(group, value)| (group.as_str(), value.as_str())).collect();
    let built = build_export("trunc", &row_refs, true);
    let request = write_request(
        &built.work,
        &built.export_dir,
        &built.lineage,
        &built.output,
        "sum",
        0,
        1,
        "g",
        "v",
        "截断",
        "rust_exact",
        false,
        false,
        None,
        None,
    );
    assert_eq!(run(&request).expect("trunc"), Written::Uncited);
    let svg = fs::read_to_string(built.output.join("analysis.svg")).expect("svg");
    assert!(svg.contains("不可引用"), "{svg}");
    assert!(!svg.contains("数值已核对"), "{svg}");
    let manifest = fs::read_to_string(built.output.join("analysis.manifest.json")).expect("manifest");
    assert!(manifest.contains("\"citable\":false"));
    assert!(manifest.contains("\"truncated\":true"));
    assert!(manifest.contains("\"audited\":false"));
}

#[test]
fn tampered_query_id_is_lineage_and_writes_nothing() {
    let rows = [(int_cell("1"), int_cell("2"))];
    let row_refs: Vec<(&str, &str)> = rows.iter().map(|(group, value)| (group.as_str(), value.as_str())).collect();
    let built = build_export("lineage", &row_refs, false);
    let text = fs::read_to_string(&built.lineage).expect("lineage");
    let marker = "\"query_id\":\"q_";
    let start = text.find(marker).expect("query id") + marker.len();
    let end = start + text[start..].find('"').expect("end");
    let mut tampered = text.clone();
    tampered.replace_range(start..end, "0000000000000000");
    fs::write(&built.lineage, tampered).expect("rewrite");
    let request = write_request(
        &built.work,
        &built.export_dir,
        &built.lineage,
        &built.output,
        "sum",
        0,
        1,
        "g",
        "v",
        "谱系",
        "rust_exact",
        false,
        false,
        None,
        None,
    );
    let err = run(&request).expect_err("lineage");
    assert_eq!(err, Fault::lineage());
    assert_empty(&built.output);
}

#[allow(clippy::too_many_lines)]
#[test]
fn real_export_keeps_precision_null_formula_and_uncited() {
    let complete = env_path("OO_A401_COMPLETE");
    let lineage = env_path("OO_A401_LINEAGE");
    let partial = env_path("OO_A401_PARTIAL");
    let partial_lineage = env_path("OO_A401_PARTIAL_LINEAGE");
    let formula = env_path("OO_A401_FORMULA");
    let duckdb = env_path("OO_A401_DUCKDB");
    let font = env_path("OO_A401_FONT");
    let work = scratch("real");
    let amount_out = work.join("amount");
    let wide_out = work.join("wide");
    let partial_out = work.join("partial");
    let formula_out = work.join("formula");
    let tamper_out = work.join("tamper");
    for dir in [&amount_out, &wide_out, &partial_out, &formula_out, &tamper_out] {
        fs::create_dir(dir).expect("out");
    }
    let amount = write_request(
        &work,
        &complete,
        &lineage,
        &amount_out,
        "sum",
        0,
        1,
        "entry_id",
        "amount",
        "金额",
        "duckdb",
        true,
        true,
        duckdb.to_str(),
        font.to_str(),
    );
    assert_eq!(run(&amount).expect("amount"), Written::Citable);
    let csv = fs::read(amount_out.join("analysis.csv")).expect("amount csv");
    assert_eq!(
        csv,
        b"entry_id,amount\r\n1,12345678901234.5678\r\n2,-12345678901234.5678\r\n3,0.0001\r\n4,-0.0001\r\n5,0.0000\r\n"
    );
    let svg = fs::read_to_string(amount_out.join("analysis.svg")).expect("svg");
    assert!(svg.contains("数值已核对"));
    assert!(svg.contains("DuckDB合计与精确十进制一致"));
    assert!(svg.contains("字体已校验"));
    assert!(!svg.contains("不可引用"));
    let manifest = fs::read_to_string(amount_out.join("analysis.manifest.json")).expect("manifest");
    assert!(manifest.contains("\"audited\":false"));
    assert!(manifest.contains("\"closes_sp09_w05\":false"));
    assert!(manifest.contains("\"engine\":\"duckdb\""));
    assert!(manifest.contains("\"duckdb\":\"pinned_unaudited\""));
    assert!(manifest.contains("\"arrow\":\"pinned_unaudited\""));
    assert!(manifest.contains("\"parquet\":\"pinned_unaudited\""));
    assert!(manifest.contains("\"xlsx\":\"pinned_unaudited\""));
    assert!(manifest.contains("\"null_values\":1"));
    assert!(manifest.contains("\"citable\":true"));
    assert!(amount_out.join("analysis.parquet").is_file());
    assert!(amount_out.join("analysis.arrow").is_file());
    assert!(amount_out.join("analysis.xlsx").is_file());
    assert_eq!(
        sha256_hex(&fs::read(amount_out.join("NotoSansCJKsc-Regular.otf")).expect("font")),
        "2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b"
    );

    let wide = write_request(
        &work,
        &complete,
        &lineage,
        &wide_out,
        "sum",
        0,
        2,
        "entry_id",
        "wide",
        "宽金额",
        "rust_exact",
        false,
        true,
        None,
        None,
    );
    assert_eq!(run(&wide).expect("wide"), Written::Citable);
    let wide_csv = fs::read_to_string(wide_out.join("analysis.csv")).expect("wide csv");
    assert!(wide_csv.contains("1234567890123456789012345678901234.5678\r\n"));
    assert!(wide_csv.contains("-1234567890123456789012345678901234.5678\r\n"));
    assert!(!wide_csv.contains("'-1234567890123456789012345678901234.5678"));
    assert!(wide_out.join("analysis.parquet").is_file());

    let partial_request = write_request(
        &work,
        &partial,
        &partial_lineage,
        &partial_out,
        "sum",
        0,
        1,
        "entry_id",
        "amount",
        "金额",
        "rust_exact",
        false,
        false,
        None,
        None,
    );
    assert_eq!(run(&partial_request).expect("partial"), Written::Uncited);
    let partial_svg = fs::read_to_string(partial_out.join("analysis.svg")).expect("partial svg");
    assert!(partial_svg.contains("不可引用"));
    assert!(!partial_svg.contains("数值已核对"));
    let partial_manifest = fs::read_to_string(partial_out.join("analysis.manifest.json")).expect("partial manifest");
    assert!(partial_manifest.contains("\"citable\":false"));
    assert!(partial_manifest.contains("\"truncated\":true"));

    let formula_request = write_request(
        &work,
        &formula,
        &lineage,
        &formula_out,
        "sum",
        0,
        2,
        "entry_id",
        "-wide",
        "公式",
        "rust_exact",
        true,
        false,
        None,
        None,
    );
    assert_eq!(run(&formula_request).expect("formula header"), Written::Citable);
    let formula_csv = fs::read_to_string(formula_out.join("analysis.csv")).expect("formula csv");
    assert!(formula_csv.starts_with("entry_id,'-wide\r\n"), "{formula_csv}");
    assert!(formula_csv.contains("2,-1234567890123456789012345678901234.5678\r\n"));
    assert!(!formula_csv.contains("'-1234567890123456789012345678901234.5678"));

    let mut bad = fs::read_to_string(&lineage).expect("lineage");
    let marker = "\"query_id\":\"q_";
    let start = bad.find(marker).expect("qid") + marker.len();
    let end = start + bad[start..].find('"').expect("qid end");
    bad.replace_range(start..end, "0000000000000000");
    let bad_path = work.join("bad-lineage.json");
    fs::write(&bad_path, bad).expect("bad lineage");
    let tamper = write_request(
        &work,
        &complete,
        &bad_path,
        &tamper_out,
        "sum",
        0,
        1,
        "entry_id",
        "amount",
        "金额",
        "rust_exact",
        false,
        false,
        None,
        None,
    );
    assert_eq!(run(&tamper).expect_err("tamper"), Fault::lineage());
    assert_empty(&tamper_out);
}

#[test]
fn native_cancel_kills_the_child_and_leaves_no_temp_dir() {
    let cli = env_path("OO_A401_DUCKDB");
    let work = scratch("cancel");
    let cancel = work.join("stop");
    let before = duck_temps();
    let cancel_write = cancel.clone();
    let waiter = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        fs::write(cancel_write, b"stop").expect("stop");
    });
    prove_native_cancel(&cli, &cancel).expect("cancel");
    waiter.join().expect("waiter");
    let after = duck_temps();
    for path in &after {
        assert!(before.contains(path), "temp residue {}", path.display());
    }
    assert!(duckdb_pids().is_empty(), "duckdb still running");
}

fn duck_temps() -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(std::env::temp_dir()).expect("temp").flatten() {
        if entry.file_name().to_string_lossy().starts_with("oo-a386-duck-") {
            found.push(entry.path());
        }
    }
    found
}

fn duckdb_pids() -> Vec<String> {
    let mut pids = Vec::new();
    for entry in fs::read_dir("/proc").expect("proc").flatten() {
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if !text.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let comm = fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
        if comm.trim() == "duckdb" {
            pids.push(text.into_owned());
        }
    }
    pids
}
