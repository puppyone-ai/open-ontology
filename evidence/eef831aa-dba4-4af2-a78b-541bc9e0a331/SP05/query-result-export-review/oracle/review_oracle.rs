//! Independent review oracle for query-result-export.
//! Safe Rust. No crate dependencies. It drives the compiled binary and
//! compares bytes. It does not link the exporter library.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FIXTURE_SHA: &str = "54cabda641bf1a2128e0246a78eb1a761895a0aa5a4226d136ad8aec399ad6eb";
const ORIGINAL_SHA: &str = "2a3ab40de0b44170f24ad916604478c76c2e3b67bb59b280105d56e9893f7750";
const EXECUTED_SHA: &str = "30de27abc8df667f41d05241c326f3f4cd09eefd5961caa81d431d3a2bc4e762";

struct Log {
    failed: bool,
    text: String,
}

fn main() {
    let mut args = env::args().skip(1);
    let bin = need(&mut args, "bin");
    let fixture = need(&mut args, "fixture");
    let gold = need(&mut args, "gold");
    let formula = need(&mut args, "formula");
    let sha_tool = need(&mut args, "sha256");
    let out = need(&mut args, "out");
    let out_dir = PathBuf::from(&out);
    fs::create_dir_all(&out_dir).expect("out dir");
    let mut log = Log {
        failed: false,
        text: String::new(),
    };
    run_all(
        Path::new(&bin),
        Path::new(&fixture),
        Path::new(&gold),
        Path::new(&formula),
        Path::new(&sha_tool),
        &out_dir,
        &mut log,
    );
    let result = out_dir.join("oracle-result.txt");
    fs::write(&result, &log.text).expect("result");
    if log.failed {
        std::process::exit(1);
    }
}

fn need(args: &mut impl Iterator<Item = String>, name: &str) -> String {
    args.next().unwrap_or_else(|| {
        eprintln!("missing {name}");
        std::process::exit(2);
    })
}

fn check(log: &mut Log, name: &str, ok: bool, detail: &str) {
    if ok {
        log.text.push_str("ok ");
        log.text.push_str(name);
        log.text.push('\n');
    } else {
        log.failed = true;
        log.text.push_str("fail ");
        log.text.push_str(name);
        log.text.push(' ');
        log.text.push_str(detail);
        log.text.push('\n');
    }
}

fn has(bytes: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || bytes.windows(needle.len()).any(|window| window == needle)
}

fn run_all(
    bin: &Path,
    fixture: &Path,
    gold_path: &Path,
    formula: &Path,
    sha_tool: &Path,
    out: &Path,
    log: &mut Log,
) {
    let source = fs::read(fixture).unwrap_or_default();
    let gold = fs::read(gold_path).unwrap_or_default();
    check(log, "fixture_len", source.len() == 1027, &source.len().to_string());
    check(log, "gold_len", gold.len() == 171, &gold.len().to_string());
    check(
        log,
        "fixture_has_no_decimal_point",
        !source.contains(&b'.'),
        "compiled receipt contains '.'",
    );
    check(
        log,
        "fixture_has_no_decimal_kind",
        !has(&source, br#""kind":"decimal""#) && !has(&source, br#""kind":"numeric""#),
        "compiled receipt has a decimal or numeric kind",
    );
    check(
        log,
        "fixture_has_int8_extremes",
        has(&source, br#""-9223372036854775808""#) && has(&source, br#""9223372036854775807""#),
        "int8 extremes absent",
    );
    check(
        log,
        "gold_has_no_bom",
        !gold.starts_with(&[0xEF, 0xBB, 0xBF]),
        "hand gold starts with UTF-8 BOM",
    );
    check(
        log,
        "gold_keeps_typed_int8_min",
        has(&gold, b"true,-9223372036854775808,"),
        "typed int8 minimum is not canonical in the hand gold",
    );
    let digest = sha256_file(sha_tool, fixture);
    check(
        log,
        "fixture_sha256",
        digest.as_deref() == Some(FIXTURE_SHA),
        digest.as_deref().unwrap_or("sha256 failed"),
    );

    let clerk = out.join("clerk");
    let _ = fs::remove_dir_all(&clerk);
    fs::create_dir_all(&clerk).unwrap();
    let ran = export(bin, fixture, &clerk, CLERK_COLUMNS, &clerk.join("run"));
    check(log, "clerk_exit", ran.code == Some(0), &fmt_code(&ran));
    check(log, "clerk_stdio_empty", ran.stdout.is_empty() && ran.stderr.is_empty(), "stdio was not empty");
    let csv = fs::read(clerk.join("export.csv")).unwrap_or_default();
    let side = fs::read(clerk.join("export.sidecar.json")).unwrap_or_default();
    let copied = fs::read(clerk.join("export.source.jsonl")).unwrap_or_default();
    fs::write(out.join("clerk.csv"), &csv).ok();
    fs::write(out.join("clerk.sidecar.json"), &side).ok();
    check(log, "clerk_csv_eq_hand_gold", csv == gold, "csv differs from reviewer hand gold");
    check(log, "clerk_source_eq_fixture", copied == source, "source copy differs");
    check(log, "clerk_csv_has_no_apostrophe", !csv.contains(&b'\''), "apostrophe in clerk csv");
    check(log, "clerk_csv_no_bom", !csv.starts_with(&[0xEF, 0xBB, 0xBF]), "csv bom");
    let side_text = String::from_utf8_lossy(&side);
    expect_side(log, &side_text, "clerk");
    let csv_sha = sha256_file(sha_tool, &clerk.join("export.csv"));
    if let Some(value) = csv_sha.as_deref() {
        check(
            log,
            "clerk_sidecar_csv_sha",
            side_text.contains(&format!("\"csv_sha256\":\"{value}\"")),
            "sidecar csv hash mismatch",
        );
        check(
            log,
            "clerk_sidecar_source_sha",
            side_text.contains(&format!("\"source_sha256\":\"{FIXTURE_SHA}\"")),
            "sidecar source hash mismatch",
        );
    } else {
        check(log, "clerk_sidecar_csv_sha", false, "sha256 failed");
    }
    check(
        log,
        "clerk_null_cells",
        side_text.contains("\"null_cells\":[[1,2],[1,5]]")
            && side_text.contains("\"empty_text_cells\":[]")
            && side_text.contains("\"null_count\":2")
            && side_text.contains("\"empty_text_count\":0"),
        "null sidecar mismatch",
    );
    check(
        log,
        "clerk_typed_int_sidecar",
        side_text.contains("\"text\":\"-9223372036854775808\"")
            && side_text.contains("\"text\":\"9223372036854775807\"")
            && side_text.contains("\"text\":\"0\""),
        "typed int sidecar text mismatch",
    );

    let again = export(bin, fixture, &clerk, CLERK_COLUMNS, &clerk.join("second"));
    check(log, "overwrite_exit", again.code == Some(4), &fmt_code(&again));
    check(
        log,
        "overwrite_stderr",
        again.stderr == b"query-result-export: path\n" && again.stdout.is_empty(),
        &String::from_utf8_lossy(&again.stderr),
    );
    let kept = fs::read(clerk.join("export.csv")).unwrap_or_default();
    check(log, "overwrite_keeps_csv", kept == gold, "csv changed after refusal");

    let occupied = out.join("occupied");
    let _ = fs::remove_dir_all(&occupied);
    fs::create_dir_all(&occupied).unwrap();
    fs::write(occupied.join("export.csv"), b"keep").unwrap();
    let refused = export(bin, fixture, &occupied, CLERK_COLUMNS, &occupied.join("run"));
    check(log, "existing_name_exit", refused.code == Some(4), &fmt_code(&refused));
    check(
        log,
        "existing_name_kept",
        fs::read(occupied.join("export.csv")).ok().as_deref() == Some(b"keep".as_slice())
            && !occupied.join("export.sidecar.json").exists()
            && !occupied.join("export.source.jsonl").exists(),
        "existing csv was not left untouched",
    );

    let partial_source = out.join("partial-source.jsonl");
    let partial_bytes = replace_once(&source, br#""truncated":false"#, br#""truncated":true"#);
    check(log, "partial_source_edited", partial_bytes.is_some(), "truncated flag absent");
    if let Some(bytes) = partial_bytes {
        fs::write(&partial_source, &bytes).unwrap();
        let partial_dir = out.join("partial");
        let _ = fs::remove_dir_all(&partial_dir);
        fs::create_dir_all(&partial_dir).unwrap();
        let partial = export(bin, &partial_source, &partial_dir, CLERK_COLUMNS, &partial_dir.join("run"));
        check(log, "partial_exit", partial.code == Some(1), &fmt_code(&partial));
        check(
            log,
            "partial_stderr",
            partial.stderr == b"query-result-export: partial\n" && partial.stdout.is_empty(),
            &String::from_utf8_lossy(&partial.stderr),
        );
        let partial_csv = fs::read(partial_dir.join("export.csv")).unwrap_or_default();
        let partial_side = fs::read(partial_dir.join("export.sidecar.json")).unwrap_or_default();
        fs::write(out.join("partial.sidecar.json"), &partial_side).ok();
        let partial_text = String::from_utf8_lossy(&partial_side);
        check(log, "partial_csv_eq_gold", partial_csv == gold, "partial csv changed");
        check(
            log,
            "partial_labels",
            partial_text.contains("\"complete\":false")
                && partial_text.contains("\"partial\":true")
                && partial_text.contains("\"truncated\":true"),
            "partial sidecar labels",
        );
    }

    let formula_dir = out.join("formula");
    let _ = fs::remove_dir_all(&formula_dir);
    fs::create_dir_all(&formula_dir).unwrap();
    let formula_run = export(bin, formula, &formula_dir, FORMULA_COLUMNS, &formula_dir.join("run"));
    check(log, "formula_exit", formula_run.code == Some(0), &fmt_code(&formula_run));
    let formula_csv = fs::read(formula_dir.join("export.csv")).unwrap_or_default();
    let formula_side = fs::read(formula_dir.join("export.sidecar.json")).unwrap_or_default();
    fs::write(out.join("formula.csv"), &formula_csv).ok();
    fs::write(out.join("formula.sidecar.json"), &formula_side).ok();
    check(
        log,
        "typed_negative_unprefixed",
        has(&formula_csv, b"0,-1,'=1+1"),
        "typed -1 was prefixed or text equals was not",
    );
    check(
        log,
        "text_negative_prefixed",
        has(&formula_csv, b"'-2+3"),
        "text -2+3 was not prefixed",
    );
    check(
        log,
        "typed_int8_min_unprefixed",
        has(&formula_csv, b"-9223372036854775808,9223372036854775807,0,-1,"),
        "typed extremes were rewritten",
    );
    check(log, "formula_space", has(&formula_csv, b"' =1"), "space");
    check(log, "formula_tab", has(&formula_csv, b"'\t=1"), "tab");
    check(log, "formula_cr", has(&formula_csv, b"\"'\r=1\""), "cr");
    check(log, "formula_lf", has(&formula_csv, b"\"'\n=1\""), "lf");
    check(log, "formula_soh", has(&formula_csv, &[b'\'', 0x01, b'=', b'1']), "soh");
    check(
        log,
        "formula_nbsp",
        has(&formula_csv, &[b'\'', 0xC2, 0xA0, b'=', b'1']),
        "nbsp",
    );
    check(
        log,
        "formula_ideo_space",
        has(&formula_csv, &[b'\'', 0xE3, 0x80, 0x80, b'=', b'1']),
        "ideo",
    );
    check(log, "formula_quote", has(&formula_csv, b"\"say \"\"hi\"\", next\""), "quote");
    check(log, "formula_newline_field", has(&formula_csv, b"\"line1\nline2\""), "newline");
    let formula_text = String::from_utf8_lossy(&formula_side);
    check(
        log,
        "formula_null_vs_empty",
        formula_text.contains("\"null_cells\":[[0,19]]")
            && formula_text.contains("\"empty_text_cells\":[[0,18]]"),
        "null versus empty text",
    );
    check(
        log,
        "formula_prefixed_count",
        formula_text.contains("\"formula_prefixed\":11"),
        "prefixed count",
    );
    log.text.push_str("note formula_source=committed hand grammar, not a compiled query-pg receipt\n");

    let feff_path = out.join("feff-source.jsonl");
    let feff_body = hand_row("text", "\"\\uFEFF=1+1\"", 1, 7);
    fs::write(&feff_path, &feff_body).unwrap();
    fs::write(
        out.join("feff-note.txt"),
        "hand grammar for the U+FEFF boundary. Not a compiled query-pg receipt.\n",
    )
    .ok();
    let feff_dir = out.join("feff");
    let _ = fs::remove_dir_all(&feff_dir);
    fs::create_dir_all(&feff_dir).unwrap();
    let feff = export(bin, &feff_path, &feff_dir, "[\"note\"]", &feff_dir.join("run"));
    check(log, "feff_exit", feff.code == Some(0), &fmt_code(&feff));
    let feff_csv = fs::read(feff_dir.join("export.csv")).unwrap_or_default();
    fs::write(out.join("feff.csv"), &feff_csv).ok();
    let prefixed: &[u8] = b"note\r\n'\xEF\xBB\xBF=1+1\r\n";
    let bare: &[u8] = b"note\r\n\xEF\xBB\xBF=1+1\r\n";
    let class = if feff_csv.as_slice() == prefixed {
        "prefixed"
    } else if feff_csv.as_slice() == bare {
        "unprefixed"
    } else {
        "other"
    };
    fs::write(out.join("feff-class.txt"), format!("{class}\n")).ok();
    log.text.push_str("note feff_class=");
    log.text.push_str(class);
    log.text.push('\n');
    check(log, "feff_class_known", class != "other", class);

    refuse_kind(log, bin, out, "decimal", "\"1.50\"", 4);
    refuse_kind(log, bin, out, "numeric", "\"1.50\"", 4);
}

fn expect_side(log: &mut Log, side: &str, prefix: &str) {
    let original = format!("\"original_sha256\":\"{ORIGINAL_SHA}\"");
    let executed = format!("\"executed_sha256\":\"{EXECUTED_SHA}\"");
    let pairs = [
        ("complete", "\"complete\":true"),
        ("partial_false", "\"partial\":false"),
        ("xlsx", "\"xlsx\":\"unsupported\""),
        ("http_auth", "\"http_auth\":false"),
        ("authenticated", "\"authenticated\":false"),
        ("citation", "\"citation\":{\"in_source\":false,\"query_id\":null}"),
        ("original", original.as_str()),
        ("executed", executed.as_str()),
        ("rows", "\"returned_rows\":3"),
        ("bytes", "\"result_bytes\":98"),
        ("truncated", "\"truncated\":false"),
    ];
    for (name, needle) in pairs {
        check(log, &format!("{prefix}_{name}"), side.contains(needle), needle);
    }
}

fn refuse_kind(log: &mut Log, bin: &Path, out: &Path, kind: &str, value_json: &str, bytes: u64) {
    let path = out.join(format!("{kind}-source.jsonl"));
    fs::write(&path, hand_row(kind, value_json, 1, bytes)).unwrap();
    let dir = out.join(kind);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let ran = export(bin, &path, &dir, "[\"amount\"]", &dir.join("run"));
    check(log, &format!("{kind}_refused_exit"), ran.code == Some(5), &fmt_code(&ran));
    check(
        log,
        &format!("{kind}_refused_stderr"),
        ran.stderr == b"query-result-export: source\n" && ran.stdout.is_empty(),
        &String::from_utf8_lossy(&ran.stderr),
    );
    check(
        log,
        &format!("{kind}_writes_nothing"),
        !dir.join("export.csv").exists()
            && !dir.join("export.sidecar.json").exists()
            && !dir.join("export.source.jsonl").exists(),
        "artifacts were written",
    );
    log.text.push_str("note ");
    log.text.push_str(kind);
    log.text.push_str("_source=hand grammar, not a compiled query-pg receipt\n");
}

fn hand_row(kind: &str, value_json: &str, rows: u32, bytes: u64) -> String {
    format!(
        "{{\"record\":\"row\",\"turn\":0,\"cells\":[{{\"kind\":\"{kind}\",\"value\":{value_json}}}]}}\n{{\"record\":\"summary\",\"turn\":0,\"status\":\"succeeded\",\"truncated\":false,\"returned_rows\":{rows},\"result_bytes\":{bytes},\"rollback_confirmed\":true,\"socket_dropped\":false,\"end_audit_ok\":true,\"original_sha256\":\"{pad}\",\"executed_sha256\":\"{pad_b}\",\"stop\":null,\"sqlstate\":null}}\n",
        pad = "a".repeat(64),
        pad_b = "b".repeat(64),
    )
}

struct Ran {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn export(bin: &Path, source: &Path, dir: &Path, columns: &str, request: &Path) -> Ran {
    let body = format!(
        "{{\"version\":\"oo.query-result-export/v1\",\"source_file\":\"{}\",\"output_dir\":\"{}\",\"columns\":{columns}}}",
        source.display(),
        dir.display()
    );
    fs::write(request, &body).unwrap();
    let output = Command::new(bin)
        .arg("--request")
        .arg(request)
        .output()
        .expect("spawn exporter");
    Ran {
        code: output.status.code(),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

fn sha256_file(tool: &Path, path: &Path) -> Option<String> {
    let output = Command::new(tool).arg("file").arg(path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let digest = text.split_whitespace().next()?.to_string();
    if digest.len() == 64 { Some(digest) } else { None }
}

fn replace_once(bytes: &[u8], from: &[u8], to: &[u8]) -> Option<Vec<u8>> {
    let index = bytes.windows(from.len()).position(|window| window == from)?;
    let mut out = Vec::with_capacity(bytes.len() - from.len() + to.len());
    out.extend_from_slice(&bytes[..index]);
    out.extend_from_slice(to);
    out.extend_from_slice(&bytes[index + from.len()..]);
    Some(out)
}

fn fmt_code(ran: &Ran) -> String {
    format!(
        "code={:?} stderr={}",
        ran.code,
        String::from_utf8_lossy(&ran.stderr)
    )
}

const CLERK_COLUMNS: &str =
    "[\"order_id\",\"customer_id\",\"memo\",\"posted\",\"amount\",\"clerk_note\"]";

const FORMULA_COLUMNS: &str = "[\
\"lo\",\"hi\",\"zero\",\"neg\",\"eq\",\"plus\",\"minus\",\"at\",\"sp\",\"tab\",\"cr\",\"lf\",\
\"soh\",\"nbsp\",\"ideo\",\"quote\",\"nl\",\"uni\",\"empty\",\"nil\",\"flag\",\"safe\",\"apos\",\"digits\"]";
