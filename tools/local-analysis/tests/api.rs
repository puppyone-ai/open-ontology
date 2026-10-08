use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use local_analysis::{Fault, Written, run, sha256_hex};

const QUERY_ID: &str = "q_40326e01dcdd4ae7";
const SOURCE_SHA: &str = "20623e7dc575b618223ac383ceb1b2e9a9e3f9efc1695c58ea2373bcaff4b3eb";
const CSV_SHA: &str = "f5b287fff0d197d55a1d67893178ff7e68ed5a1338830d24ea8fe0f11db2ca3d";
const SIDECAR_SHA: &str = "407dc0647b7d271fd1d77d14450a7ecb2b2ff06f6506c3803d17c6a29aa343c8";

fn bin() -> &'static str {
    option_env!("CARGO_BIN_EXE_local-analysis")
        .or(option_env!("CARGO_BIN_EXE_local_analysis"))
        .expect("local-analysis bin")
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oo-a386-api-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
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

fn field(doc: &str, key: &str) -> String {
    let pat = format!("\"{key}\":\"");
    let start = doc.find(&pat).unwrap() + pat.len();
    let rest = &doc[start..];
    let end = rest.find('"').unwrap();
    rest[..end].to_string()
}

struct Ask<'a> {
    export_dir: &'a Path,
    lineage: &'a Path,
    output: &'a Path,
    operation: &'a str,
    group_column: usize,
    value_column: usize,
    group_header: &'a str,
    value_header: &'a str,
    chart_title: &'a str,
    cancel_file: Option<&'a str>,
    engine: &'a str,
    xlsx: bool,
    parquet: bool,
    duckdb_cli: Option<&'a str>,
    font_file: Option<&'a str>,
}

fn write_request(dir: &Path, ask: &Ask<'_>) -> PathBuf {
    let cancel = match ask.cancel_file {
        Some(path) => esc(path),
        None => "null".to_string(),
    };
    let body = format!(
        r#"{{"version":"oo.local-analysis/v1","export_dir":{},"lineage_file":{},"output_dir":{},"operation":"{}","group_column":{},"value_column":{},"group_header":{},"value_header":{},"chart_title":{},"cancel_file":{},"engine":"{}","xlsx":{},"parquet":{}}}"#,
        esc(&ask.export_dir.display().to_string()),
        esc(&ask.lineage.display().to_string()),
        esc(&ask.output.display().to_string()),
        ask.operation,
        ask.group_column,
        ask.value_column,
        esc(ask.group_header),
        esc(ask.value_header),
        esc(ask.chart_title),
        cancel,
        ask.engine,
        if ask.xlsx { "true" } else { "false" },
        if ask.parquet { "true" } else { "false" },
    );
    let cli = match ask.duckdb_cli {
        Some(path) => format!(r#","duckdb_cli":{}"#, esc(path)),
        None => String::new(),
    };
    let font = match ask.font_file {
        Some(path) => format!(r#","font_file":{}"#, esc(path)),
        None => String::new(),
    };
    let mut body = body;
    body.pop();
    body.push_str(&cli);
    body.push_str(&font);
    body.push('}');
    let path = dir.join("request.json");
    fs::write(&path, body).unwrap();
    path
}

fn write_lineage(dir: &Path, body: &str, query_id: &str) -> PathBuf {
    let sha = sha256_hex(body.as_bytes());
    let json = format!(
        r#"{{"version":"oo.local-analysis-lineage/v1","trust":"local_operator_declared","query_id":{},"receipt_sha256":"{sha}","receipt_body":{}}}"#,
        esc(query_id),
        esc(body),
    );
    let path = dir.join("lineage.json");
    fs::write(&path, json).unwrap();
    path
}

fn produced(export_name: &str, audit_name: &str, work_name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let export_dir = fixture(export_name);
    let body = fs::read_to_string(export_dir.join(audit_name)).unwrap();
    let work = scratch(work_name);
    let output = work.join("out");
    fs::create_dir(&output).unwrap();
    let lineage = write_lineage(&work, &body, QUERY_ID);
    (work, output, lineage)
}

fn assert_closed(dir: &Path) {
    for name in [
        "analysis.csv",
        "analysis.svg",
        "analysis.manifest.json",
        "analysis.csv.partial",
        "analysis.svg.partial",
        "analysis.manifest.json.partial",
        "analysis.parquet",
        "analysis.arrow",
        "analysis.xlsx",
        "NotoSansCJKsc-Regular.otf",
    ] {
        assert!(!dir.join(name).exists(), "{name}");
    }
}

#[derive(Clone)]
enum Cell {
    Null,
    Int(&'static str),
    Text(String),
    Dec {
        negative: bool,
        coefficient: &'static str,
        scale: i32,
        text: &'static str,
    },
}

fn weight(cell: &Cell) -> u64 {
    match cell {
        Cell::Null => 0,
        Cell::Int(text) | Cell::Dec { text, .. } => text.len() as u64,
        Cell::Text(text) => text.len() as u64,
    }
}

fn cell_json(cell: &Cell) -> String {
    match cell {
        Cell::Null => r#"{"kind":"null"}"#.to_string(),
        Cell::Int(text) => format!(r#"{{"kind":"int","value":{}}}"#, esc(text)),
        Cell::Text(text) => format!(r#"{{"kind":"text","value":{}}}"#, esc(text)),
        Cell::Dec {
            negative,
            coefficient,
            scale,
            ..
        } => format!(
            r#"{{"kind":"decimal","negative":{},"coefficient":{},"scale":{}}}"#,
            if *negative { "true" } else { "false" },
            esc(coefficient),
            scale,
        ),
    }
}

struct Synthetic {
    work: PathBuf,
    export_dir: PathBuf,
    lineage: PathBuf,
    output: PathBuf,
}

fn synthetic(name: &str, rows: &[Vec<Cell>], truncated: bool) -> Synthetic {
    let work = scratch(name);
    let export_dir = work.join("export");
    let output = work.join("out");
    fs::create_dir_all(&export_dir).unwrap();
    fs::create_dir(&output).unwrap();
    let mut source = String::new();
    let mut total = 0_u64;
    for row in rows {
        let cells = row.iter().map(cell_json).collect::<Vec<_>>().join(",");
        source.push_str(&format!(
            r#"{{"record":"row","turn":0,"cells":[{cells}]}}"#
        ));
        source.push('\n');
        total += row.iter().map(weight).sum::<u64>();
    }
    let original = "11".repeat(32);
    let executed = "22".repeat(32);
    let trunc = if truncated { "true" } else { "false" };
    source.push_str(&format!(
        r#"{{"record":"summary","turn":0,"status":"succeeded","truncated":{trunc},"returned_rows":{},"result_bytes":{total},"rollback_confirmed":true,"socket_dropped":false,"end_audit_ok":true,"original_sha256":"{original}","executed_sha256":"{executed}","stop":null,"sqlstate":null}}"#,
        rows.len()
    ));
    source.push('\n');
    fs::write(export_dir.join("export.source.jsonl"), &source).unwrap();
    let csv = b"synthetic\r\n";
    fs::write(export_dir.join("export.csv"), csv).unwrap();
    let sidecar = format!(
        r#"{{"version":"oo.query-result-export/v1","format":"csv","xlsx":"unsupported","http_auth":false,"authenticated":false,"complete":{},"partial":{},"source_sha256":"{}","source_bytes":{},"turn":0,"status":"succeeded","truncated":{trunc},"stop":null,"sqlstate":null,"end_audit_ok":true,"rollback_confirmed":true,"socket_dropped":false,"original_sha256":"{original}","executed_sha256":"{executed}","citation":{{"in_source":false,"query_id":null}},"returned_rows":{},"result_bytes":{total},"parsed_rows":{},"parsed_result_bytes":{total},"columns":[{{"name":"g"}},{{"name":"v"}}],"csv_sha256":"{}","csv_bytes":{}}}"#,
        if truncated { "false" } else { "true" },
        if truncated { "true" } else { "false" },
        sha256_hex(source.as_bytes()),
        source.len(),
        rows.len(),
        rows.len(),
        sha256_hex(csv),
        csv.len(),
    );
    fs::write(export_dir.join("export.sidecar.json"), sidecar).unwrap();
    let policy = "33".repeat(32);
    let fingerprint = "abc123def4567890";
    let shared = format!(
        r#""protocol":"oo.gateway.query-pg-audit/v1","original_sha256":"{original}","executed_sha256":"{executed}","fingerprint_hex":"{fingerprint}","role":"oo_r_clerk","project":"synth","policy_id":"{policy}","backend_pid":7,"backend_start":"1""#
    );
    let admit = format!(r#"{{{shared},"record":"admit"}}"#);
    let complete = format!(
        r#"{{{shared},"record":"complete","status":"succeeded","truncated":{trunc},"returned_rows":{},"result_bytes":{total},"rollback_confirmed":true,"socket_dropped":false,"stop":null,"sqlstate":null}}"#,
        rows.len()
    );
    let body = format!("{admit}\n{complete}\n");
    let lineage = write_lineage(&work, &body, &format!("q_{fingerprint}"));
    Synthetic {
        work,
        export_dir,
        lineage,
        output,
    }
}

fn ask<'a>(prep: &'a Synthetic, operation: &'a str, title: &'a str) -> Ask<'a> {
    Ask {
        export_dir: &prep.export_dir,
        lineage: &prep.lineage,
        output: &prep.output,
        operation,
        group_column: 0,
        value_column: 1,
        group_header: "g",
        value_header: "v",
        chart_title: title,
        cancel_file: None,
        engine: "rust_exact",
        xlsx: false,
        parquet: false,
        duckdb_cli: None,
        font_file: None,
    }
}

#[test]
fn reviewed_amount_export_sums_exact_decimals_and_draws_chinese() {
    let export_dir = fixture("a344-complete");
    assert_eq!(
        sha256_hex(&fs::read(export_dir.join("export.source.jsonl")).unwrap()),
        SOURCE_SHA
    );
    assert_eq!(
        sha256_hex(&fs::read(export_dir.join("export.csv")).unwrap()),
        CSV_SHA
    );
    assert_eq!(
        sha256_hex(&fs::read(export_dir.join("export.sidecar.json")).unwrap()),
        SIDECAR_SHA
    );
    let (work, output, lineage) = produced("a344-complete", "amounts.jsonl", "amounts");
    let request = write_request(
        &work,
        &Ask {
            export_dir: &export_dir,
            lineage: &lineage,
            output: &output,
            operation: "sum",
            group_column: 0,
            value_column: 1,
            group_header: "entry_id",
            value_header: "amount",
            chart_title: "金额",
            cancel_file: None,
            engine: "rust_exact",
            xlsx: false,
            parquet: false,
        duckdb_cli: None,
        font_file: None,
        },
    );
    let output_run = Command::new(bin())
        .arg("--request")
        .arg(&request)
        .output()
        .unwrap();
    assert_eq!(output_run.status.code(), Some(0));
    assert!(output_run.stdout.is_empty());
    assert!(output_run.stderr.is_empty());
    assert_eq!(run(&request).unwrap_err().token, "path");
    let csv = fs::read(output.join("analysis.csv")).unwrap();
    let expected = "\
entry_id,amount\r\n\
1,12345678901234.5678\r\n\
2,-12345678901234.5678\r\n\
3,0.0001\r\n\
4,-0.0001\r\n\
5,0.0000\r\n";
    assert_eq!(std::str::from_utf8(&csv).unwrap(), expected);
    assert!(!csv.contains(&b'\''));
    assert!(!csv.starts_with(&[0xEF, 0xBB, 0xBF]));
    let svg = fs::read_to_string(output.join("analysis.svg")).unwrap();
    assert!(svg.contains("金额"));
    assert!(svg.contains(QUERY_ID));
    assert!(svg.contains("数值已核对"));
    assert!(svg.contains("本运行未调用DuckDB"));
    assert!(svg.contains("字体未随包提供"));
    assert!(!svg.contains("不可引用"));
    assert_eq!(svg.matches("http").count(), 1);
    assert!(svg.contains("http://www.w3.org/2000/svg"));
    assert!(!svg.to_ascii_lowercase().contains("doctype"));
    assert!(!svg.contains("<script"));
    assert!(!svg.contains("foreignObject"));
    assert!(!svg.contains("href"));
    assert!(svg.contains("Noto Sans CJK SC"));
    assert!(svg.contains("Source Han Sans SC"));
    assert!(svg.contains("width=\"400\""));
    assert!(svg.contains("width=\"1\""));
    assert!(svg.contains("width=\"0\""));
    let manifest = fs::read_to_string(output.join("analysis.manifest.json")).unwrap();
    assert!(manifest.ends_with('\n'));
    assert!(manifest.starts_with(
        "{\"version\":\"oo.local-analysis/v1\",\"closes_sp09_w05\":false,\"audited\":false,\"citable\":true,\"authorization\":\"not_claimed\",\"authenticated\":false,\"trust\":\"local_operator_declared\",\"lineage\":\"bound\""
    ));
    assert!(manifest.contains(&format!("\"query_id\":\"{QUERY_ID}\"")));
    assert!(manifest.contains("\"role\":\"oo_r_clerk\""));
    assert!(manifest.contains("\"project\":\"a327num\""));
    assert!(manifest.contains("\"engine\":\"rust_exact\""));
    assert!(manifest.contains("\"duckdb\":\"not_invoked\""));
    assert!(manifest.contains("\"arrow\":\"not_written\""));
    assert!(manifest.contains("\"parquet\":\"not_written\""));
    assert!(manifest.contains("\"xlsx\":\"not_written\""));
    assert!(manifest.contains("\"svg\":\"rust_native\""));
    assert!(manifest.contains("\"cjk_font\":\"not_bundled\""));
    assert!(manifest.contains(&format!("\"source_sha256\":\"{SOURCE_SHA}\"")));
    assert!(manifest.contains(&format!("\"csv_sha256\":\"{CSV_SHA}\"")));
    assert!(manifest.contains(&format!("\"sidecar_sha256\":\"{SIDECAR_SHA}\"")));
    assert_eq!(field(&manifest, "analysis_csv_sha256"), sha256_hex(&csv));
    assert_eq!(
        field(&manifest, "svg_sha256"),
        sha256_hex(svg.as_bytes())
    );
    assert!(manifest.contains(
        "\"export_complete\":true,\"truncated\":false,\"operation\":\"sum\",\"group_column\":0,\"value_column\":1,\"group_count\":5,\"null_groups\":0,\"null_values\":1"
    ));
    assert!(manifest.contains(
        "\"warnings\":[\"admission_not_audited\",\"duckdb_not_invoked\",\"arrow_not_written\",\"parquet_not_written\",\"xlsx_not_written\",\"cjk_font_not_bundled\",\"null_value\"]"
    ));
    assert!(manifest.contains(
        "\"key\":\"2\",\"text\":\"-12345678901234.5678\",\"negative\":true,\"coefficient\":\"123456789012345678\",\"scale\":4"
    ));
    assert!(manifest.contains(
        "\"key\":\"5\",\"text\":\"0.0000\",\"negative\":false,\"coefficient\":\"0\",\"scale\":4"
    ));
    assert!(!manifest.contains("\"closes_sp09_w05\":true"));
    let _ = fs::remove_dir_all(&work);
}

#[test]
fn wide_column_keeps_thirty_eight_digits() {
    let (work, output, lineage) = produced("a344-complete", "amounts.jsonl", "wide");
    let request = write_request(
        &work,
        &Ask {
            export_dir: &fixture("a344-complete"),
            lineage: &lineage,
            output: &output,
            operation: "sum",
            group_column: 0,
            value_column: 2,
            group_header: "entry_id",
            value_header: "wide",
            chart_title: "金额",
            cancel_file: None,
            engine: "rust_exact",
            xlsx: false,
            parquet: false,
        duckdb_cli: None,
        font_file: None,
        },
    );
    assert_eq!(run(&request).unwrap(), Written::Citable);
    let csv = fs::read_to_string(output.join("analysis.csv")).unwrap();
    assert_eq!(
        csv,
        "\
entry_id,wide\r\n\
1,1234567890123456789012345678901234.5678\r\n\
2,-1234567890123456789012345678901234.5678\r\n\
3,0.0001\r\n\
4,-0.0001\r\n\
5,0.0000\r\n"
    );
    assert!(!csv.contains('\''));
    let svg = fs::read_to_string(output.join("analysis.svg")).unwrap();
    assert!(svg.contains("width=\"400\""));
    assert!(svg.contains("width=\"1\""));
    assert!(svg.contains("width=\"0\""));
    assert!(svg.contains("1234567890123456789012345678901234.5678"));
    assert!(svg.contains("-1234567890123456789012345678901234.5678"));
    let _ = fs::remove_dir_all(&work);
}

#[test]
fn truncated_producer_export_is_written_uncited() {
    let (work, output, lineage) = produced("a344-partial", "partial.jsonl", "partial");
    let request = write_request(
        &work,
        &Ask {
            export_dir: &fixture("a344-partial"),
            lineage: &lineage,
            output: &output,
            operation: "sum",
            group_column: 0,
            value_column: 1,
            group_header: "entry_id",
            value_header: "amount",
            chart_title: "金额",
            cancel_file: None,
            engine: "rust_exact",
            xlsx: false,
            parquet: false,
        duckdb_cli: None,
        font_file: None,
        },
    );
    let output_run = Command::new(bin())
        .arg("--request")
        .arg(&request)
        .output()
        .unwrap();
    assert_eq!(output_run.status.code(), Some(1));
    assert!(output_run.stdout.is_empty());
    assert_eq!(output_run.stderr, b"local-analysis: uncited\n");
    let svg = fs::read_to_string(output.join("analysis.svg")).unwrap();
    assert!(svg.contains("不可引用"));
    assert!(!svg.contains("数值已核对"));
    let manifest = fs::read_to_string(output.join("analysis.manifest.json")).unwrap();
    assert!(manifest.contains("\"citable\":false"));
    assert!(manifest.contains("\"closes_sp09_w05\":false"));
    assert!(manifest.contains("\"uncited\""));
    assert!(!manifest.contains("\"query_id\":null"));
    let _ = fs::remove_dir_all(&work);
}

#[test]
fn hash_query_and_source_disagreements_write_nothing() {
    let src = fixture("a344-complete");
    let body = fs::read_to_string(src.join("amounts.jsonl")).unwrap();
    let work = scratch("mutations");
    let export_dir = work.join("export");
    fs::create_dir_all(&export_dir).unwrap();
    for name in ["export.source.jsonl", "export.csv", "export.sidecar.json"] {
        fs::copy(src.join(name), export_dir.join(name)).unwrap();
    }
    let mut csv = fs::read(export_dir.join("export.csv")).unwrap();
    let last = csv.len() - 1;
    csv[last] ^= 0x01;
    fs::write(export_dir.join("export.csv"), &csv).unwrap();
    let output = work.join("out-hash");
    fs::create_dir(&output).unwrap();
    let lineage = write_lineage(&work, &body, QUERY_ID);
    let request = write_request(
        &work,
        &Ask {
            export_dir: &export_dir,
            lineage: &lineage,
            output: &output,
            operation: "sum",
            group_column: 0,
            value_column: 1,
            group_header: "entry_id",
            value_header: "amount",
            chart_title: "金额",
            cancel_file: None,
            engine: "rust_exact",
            xlsx: false,
            parquet: false,
        duckdb_cli: None,
        font_file: None,
        },
    );
    let err = run(&request).unwrap_err();
    assert_eq!(err, Fault::hash());
    assert_closed(&output);
    let fresh = work.join("export-id");
    fs::create_dir(&fresh).unwrap();
    for name in ["export.source.jsonl", "export.csv", "export.sidecar.json"] {
        fs::copy(src.join(name), fresh.join(name)).unwrap();
    }
    let output_id = work.join("out-id");
    fs::create_dir(&output_id).unwrap();
    let lineage_id = work.join("lineage-id.json");
    fs::copy(&lineage, &lineage_id).unwrap();
    let mut text = fs::read_to_string(&lineage_id).unwrap();
    text = text.replace(QUERY_ID, "q_0000000000000000");
    fs::write(&lineage_id, text).unwrap();
    let request_id = write_request(
        &work,
        &Ask {
            export_dir: &fresh,
            lineage: &lineage_id,
            output: &output_id,
            operation: "sum",
            group_column: 0,
            value_column: 1,
            group_header: "g",
            value_header: "v",
            chart_title: "金额",
            cancel_file: None,
            engine: "rust_exact",
            xlsx: false,
            parquet: false,
        duckdb_cli: None,
        font_file: None,
        },
    );
    assert_eq!(run(&request_id).unwrap_err(), Fault::lineage());
    assert_closed(&output_id);
    let lineage_good = work.join("lineage-good.json");
    fs::copy(&lineage, &lineage_good).unwrap();
    let disagree = body.replace("\"returned_rows\":6", "\"returned_rows\":7");
    let output_line = work.join("out-line");
    fs::create_dir(&output_line).unwrap();
    let lineage_bad = write_lineage(&work, &disagree, QUERY_ID);
    let request_line = write_request(
        &work,
        &Ask {
            export_dir: &fresh,
            lineage: &lineage_bad,
            output: &output_line,
            operation: "sum",
            group_column: 0,
            value_column: 1,
            group_header: "g",
            value_header: "v",
            chart_title: "t",
            cancel_file: None,
            engine: "rust_exact",
            xlsx: false,
            parquet: false,
        duckdb_cli: None,
        font_file: None,
        },
    );
    assert_eq!(run(&request_line).unwrap_err(), Fault::lineage());
    assert_closed(&output_line);
    let flipped = work.join("export-flip");
    fs::create_dir(&flipped).unwrap();
    for name in ["export.source.jsonl", "export.csv", "export.sidecar.json"] {
        fs::copy(src.join(name), flipped.join(name)).unwrap();
    }
    let sidecar = fs::read_to_string(flipped.join("export.sidecar.json")).unwrap();
    let sidecar = sidecar
        .replace("\"complete\":true", "\"complete\":false")
        .replace("\"partial\":false", "\"partial\":true");
    fs::write(flipped.join("export.sidecar.json"), sidecar).unwrap();
    let output_src = work.join("out-src");
    fs::create_dir(&output_src).unwrap();
    let request_src = write_request(
        &work,
        &Ask {
            export_dir: &flipped,
            lineage: &lineage_good,
            output: &output_src,
            operation: "sum",
            group_column: 0,
            value_column: 1,
            group_header: "g",
            value_header: "v",
            chart_title: "t",
            cancel_file: None,
            engine: "rust_exact",
            xlsx: false,
            parquet: false,
        duckdb_cli: None,
        font_file: None,
        },
    );
    assert_eq!(run(&request_src).unwrap_err(), Fault::source());
    assert_closed(&output_src);
    let proc = Command::new(bin())
        .arg("--request")
        .arg(&request_src)
        .output()
        .unwrap();
    assert_eq!(proc.status.code(), Some(5));
    assert_eq!(proc.stderr, b"local-analysis: source\n");
    assert!(proc.stdout.is_empty());
    let _ = fs::remove_dir_all(&work);
}

#[test]
fn scale_alignment_formula_prefix_count_and_budgets() {
    let aligned = synthetic(
        "align",
        &[
            vec![
                Cell::Text("g".to_string()),
                Cell::Dec {
                    negative: false,
                    coefficient: "150",
                    scale: 2,
                    text: "1.50",
                },
            ],
            vec![
                Cell::Text("g".to_string()),
                Cell::Dec {
                    negative: false,
                    coefficient: "25",
                    scale: 1,
                    text: "2.5",
                },
            ],
        ],
        false,
    );
    let request = write_request(&aligned.work, &ask(&aligned, "sum", "金额"));
    assert_eq!(run(&request).unwrap(), Written::Citable);
    assert_eq!(
        fs::read_to_string(aligned.output.join("analysis.csv")).unwrap(),
        "g,v\r\ng,4.00\r\n"
    );
    let mixed = synthetic(
        "mixed",
        &[
            vec![Cell::Text("g".to_string()), Cell::Int("1")],
            vec![
                Cell::Text("g".to_string()),
                Cell::Dec {
                    negative: false,
                    coefficient: "1",
                    scale: 0,
                    text: "1",
                },
            ],
        ],
        false,
    );
    let request = write_request(&mixed.work, &ask(&mixed, "sum", "t"));
    assert_eq!(run(&request).unwrap_err(), Fault::source());
    assert_closed(&mixed.output);
    let counted = synthetic(
        "count",
        &[vec![
            Cell::Text("g".to_string()),
            Cell::Text("=#".to_string()),
        ]],
        false,
    );
    let mut count_ask = ask(&counted, "count", "金额");
    count_ask.group_header = "=id";
    count_ask.value_header = "+v";
    let request = write_request(&counted.work, &count_ask);
    assert_eq!(run(&request).unwrap(), Written::Citable);
    assert_eq!(
        fs::read(counted.output.join("analysis.csv")).unwrap(),
        b"'=id,'+v\r\ng,1\r\n"
    );
    let prefixed = synthetic(
        "feff",
        &[vec![Cell::Text("k".to_string()), Cell::Int("1")]],
        false,
    );
    let mut feff = ask(&prefixed, "sum", "金额");
    feff.group_header = "\u{feff}=id";
    let request = write_request(&prefixed.work, &feff);
    assert_eq!(run(&request).unwrap(), Written::Citable);
    let csv = fs::read(prefixed.output.join("analysis.csv")).unwrap();
    assert_eq!(&csv[..6], &[b'\'', 0xEF, 0xBB, 0xBF, b'=', b'i']);
    let minimum = synthetic(
        "min",
        &[vec![
            Cell::Text("g".to_string()),
            Cell::Int("-9223372036854775808"),
        ]],
        false,
    );
    let request = write_request(&minimum.work, &ask(&minimum, "sum", "金额"));
    assert_eq!(run(&request).unwrap(), Written::Citable);
    let csv = fs::read_to_string(minimum.output.join("analysis.csv")).unwrap();
    assert_eq!(csv, "g,v\r\ng,-9223372036854775808\r\n");
    assert!(!csv.contains('\''));
    let empty = synthetic("empty", &[], false);
    let request = write_request(&empty.work, &ask(&empty, "sum", "金额"));
    assert_eq!(run(&request).unwrap(), Written::Citable);
    assert!(
        fs::read_to_string(empty.output.join("analysis.svg"))
            .unwrap()
            .contains("无分组")
    );
    let missing = synthetic(
        "null-group",
        &[vec![Cell::Null, Cell::Int("1")]],
        false,
    );
    let request = write_request(&missing.work, &ask(&missing, "sum", "金额"));
    assert_eq!(run(&request).unwrap(), Written::Citable);
    let manifest = fs::read_to_string(missing.output.join("analysis.manifest.json")).unwrap();
    assert!(manifest.contains("\"null_groups\":1"));
    assert!(manifest.contains("\"null_group\""));
    assert_eq!(
        fs::read_to_string(missing.output.join("analysis.csv")).unwrap(),
        "g,v\r\n"
    );
    let mut rows = Vec::new();
    for index in 0..65 {
        rows.push(vec![
            Cell::Text(format!("k{index:02}")),
            Cell::Int("1"),
        ]);
    }
    let budget = synthetic("groups", &rows, false);
    let request = write_request(&budget.work, &ask(&budget, "sum", "t"));
    assert_eq!(run(&request).unwrap_err(), Fault::budget());
    assert_closed(&budget.output);
    let omitted = synthetic(
        "omit",
        &[vec![Cell::Text("k".to_string()), Cell::Int("1")]],
        false,
    );
    let mut omit_ask = ask(&omitted, "sum", "\u{1}金额");
    omit_ask.group_header = "k";
    let request = write_request(&omitted.work, &omit_ask);
    assert_eq!(run(&request).unwrap(), Written::Citable);
    let svg = fs::read(omitted.output.join("analysis.svg")).unwrap();
    assert!(!svg.contains(&1u8));
    assert!(fs::read_to_string(omitted.output.join("analysis.manifest.json"))
        .unwrap()
        .contains("\"svg_label_omitted\""));
    let _ = fs::remove_dir_all(&aligned.work);
    let _ = fs::remove_dir_all(&mixed.work);
    let _ = fs::remove_dir_all(&counted.work);
    let _ = fs::remove_dir_all(&prefixed.work);
    let _ = fs::remove_dir_all(&minimum.work);
    let _ = fs::remove_dir_all(&empty.work);
    let _ = fs::remove_dir_all(&missing.work);
    let _ = fs::remove_dir_all(&budget.work);
    let _ = fs::remove_dir_all(&omitted.work);
}

#[test]
fn cancel_beats_a_missing_export_and_an_existing_output_is_kept() {
    let work = scratch("cancel");
    let stop = work.join("stop");
    fs::write(&stop, b"do-not-read").unwrap();
    let output = work.join("out");
    fs::create_dir(&output).unwrap();
    let lineage = work.join("missing-lineage.json");
    let stop_text = stop.display().to_string();
    let request = write_request(
        &work,
        &Ask {
            export_dir: &work.join("missing-export"),
            lineage: &lineage,
            output: &output,
            operation: "sum",
            group_column: 0,
            value_column: 1,
            group_header: "g",
            value_header: "v",
            chart_title: "t",
            cancel_file: Some(&stop_text),
            engine: "rust_exact",
            xlsx: false,
            parquet: false,
        duckdb_cli: None,
        font_file: None,
        },
    );
    let err = run(&request).unwrap_err();
    assert_eq!(err, Fault::canceled());
    assert_eq!(fs::read(&stop).unwrap(), b"do-not-read");
    assert_closed(&output);
    let kept = synthetic(
        "kept",
        &[vec![Cell::Text("g".to_string()), Cell::Int("1")]],
        false,
    );
    fs::write(kept.output.join("analysis.csv"), b"keep").unwrap();
    let request = write_request(&kept.work, &ask(&kept, "sum", "t"));
    assert_eq!(run(&request).unwrap_err(), Fault::path());
    assert_eq!(fs::read(kept.output.join("analysis.csv")).unwrap(), b"keep");
    assert!(!kept.output.join("analysis.svg").exists());
    assert!(!kept.output.join("analysis.csv.partial").exists());
    let link_parent = scratch("link");
    let link = link_parent.join("export-link");
    std::os::unix::fs::symlink(fixture("a344-complete"), &link).unwrap();
    let output = link_parent.join("out");
    fs::create_dir(&output).unwrap();
    let lineage = write_lineage(
        &link_parent,
        &fs::read_to_string(fixture("a344-complete").join("amounts.jsonl")).unwrap(),
        QUERY_ID,
    );
    let request = write_request(
        &link_parent,
        &Ask {
            export_dir: &link,
            lineage: &lineage,
            output: &output,
            operation: "sum",
            group_column: 0,
            value_column: 1,
            group_header: "g",
            value_header: "v",
            chart_title: "t",
            cancel_file: None,
            engine: "rust_exact",
            xlsx: false,
            parquet: false,
        duckdb_cli: None,
        font_file: None,
        },
    );
    assert_eq!(run(&request).unwrap_err(), Fault::path());
    assert_closed(&output);
    let _ = fs::remove_dir_all(&work);
    let _ = fs::remove_dir_all(&kept.work);
    let _ = fs::remove_dir_all(&link_parent);
}

#[test]
fn parquet_arrow_and_xlsx_are_real_files_and_excel_has_no_formula() {
    let (work, output, lineage) = produced("a344-complete", "amounts.jsonl", "columns");
    let request = write_request(
        &work,
        &Ask {
            export_dir: &fixture("a344-complete"),
            lineage: &lineage,
            output: &output,
            operation: "sum",
            group_column: 0,
            value_column: 1,
            group_header: "=金额",
            value_header: "amount",
            chart_title: "金额",
            cancel_file: None,
            engine: "rust_exact",
            xlsx: true,
            parquet: true,
            duckdb_cli: None,
            font_file: None,
        },
    );
    assert_eq!(run(&request).unwrap(), Written::Citable);
    let parquet = fs::read(output.join("analysis.parquet")).unwrap();
    let arrow = fs::read(output.join("analysis.arrow")).unwrap();
    let xlsx = fs::read(output.join("analysis.xlsx")).unwrap();
    assert!(parquet.starts_with(b"PAR1"));
    assert!(arrow.windows(6).any(|window| window == b"ARROW1"));
    let xml = xlsx_xml(&xlsx);
    assert!(!xml.contains("<f>"));
    assert!(!xml.contains("<f "));
    assert!(xml.contains("'=金额"));
    assert!(xml.contains("-12345678901234.5678"));
    assert!(!xml.contains(">=金额"));
    let manifest = fs::read_to_string(output.join("analysis.manifest.json")).unwrap();
    assert!(manifest.contains("\"audited\":false"));
    assert!(manifest.contains("\"closes_sp09_w05\":false"));
    assert!(manifest.contains("\"engine\":\"rust_exact\""));
    assert!(manifest.contains("\"duckdb\":\"not_invoked\""));
    assert!(manifest.contains("\"parquet\":\"pinned_unaudited\""));
    assert!(manifest.contains("\"arrow\":\"pinned_unaudited\""));
    assert!(manifest.contains("\"xlsx\":\"pinned_unaudited\""));
    assert!(!manifest.contains("\"parquet_not_written\""));
    let _ = fs::remove_dir_all(&work);
}

fn xlsx_xml(bytes: &[u8]) -> String {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = String::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        if !file.name().ends_with(".xml") {
            continue;
        }
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut file, &mut xml).unwrap();
        out.push_str(&xml);
    }
    out
}

#[test]
fn pinned_duckdb_matches_the_exact_sum_and_cancel_kills_the_child() {
    let Some(cli) = std::env::var_os("OO_A386_DUCKDB") else {
        assert!(std::env::var_os("OO_A386_REQUIRE_DUCKDB").is_none());
        return;
    };
    let Some(font) = std::env::var_os("OO_A386_FONT") else {
        panic!("OO_A386_FONT");
    };
    let cli = PathBuf::from(cli);
    let font = PathBuf::from(font);
    let (work, output, lineage) = produced("a344-complete", "amounts.jsonl", "duck");
    let request = write_request(
        &work,
        &Ask {
            export_dir: &fixture("a344-complete"),
            lineage: &lineage,
            output: &output,
            operation: "sum",
            group_column: 0,
            value_column: 1,
            group_header: "entry_id",
            value_header: "amount",
            chart_title: "金额",
            cancel_file: None,
            engine: "duckdb",
            xlsx: true,
            parquet: true,
            duckdb_cli: cli.to_str(),
            font_file: font.to_str(),
        },
    );
    assert_eq!(run(&request).unwrap(), Written::Citable);
    let csv = fs::read_to_string(output.join("analysis.csv")).unwrap();
    assert_eq!(
        csv,
        "\
entry_id,amount\r\n\
1,12345678901234.5678\r\n\
2,-12345678901234.5678\r\n\
3,0.0001\r\n\
4,-0.0001\r\n\
5,0.0000\r\n"
    );
    let svg = fs::read_to_string(output.join("analysis.svg")).unwrap();
    assert!(svg.contains("数值已核对"));
    assert!(svg.contains("DuckDB合计与精确十进制一致"));
    assert!(svg.contains("字体已校验"));
    assert!(svg.contains("NotoSansCJKsc-Regular.otf"));
    assert_eq!(svg.matches("http").count(), 1);
    assert!(output.join("NotoSansCJKsc-Regular.otf").is_file());
    let manifest = fs::read_to_string(output.join("analysis.manifest.json")).unwrap();
    assert!(manifest.contains("\"engine\":\"duckdb\""));
    assert!(manifest.contains("\"duckdb\":\"pinned_unaudited\""));
    assert!(manifest.contains("\"audited\":false"));
    assert!(manifest.contains("\"closes_sp09_w05\":false"));
    assert!(manifest.contains("\"cjk_font\":\"pinned_unaudited\""));
    let cancel_dir = scratch("duck-cancel");
    let cancel = cancel_dir.join("stop");
    let cancel_write = cancel.clone();
    let waiter = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        fs::write(cancel_write, b"stop").unwrap();
    });
    local_analysis::prove_native_cancel(&cli, &cancel).unwrap();
    waiter.join().unwrap();
    let _ = fs::remove_dir_all(&work);
    let _ = fs::remove_dir_all(&cancel_dir);
}
