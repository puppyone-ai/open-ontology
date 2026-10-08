//! Independent COPY and gate oracle for the a349 100k review.
//! Formulas are the frozen contract. This file does not link pg-e2e.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const PARTY_ROWS: u32 = 2_000;
const ACCOUNT_ROWS: u32 = 8_000;
const ENTRY_ROWS: u32 = 90_000;
const MARKER: &str = "甲注评";

fn main() -> ExitCode {
    let mut log = String::new();
    let root = env::args().nth(1).unwrap_or_else(|| "/evidence".to_owned());
    let root = PathBuf::from(root);
    let outcome = run(&root, &mut log);
    log.push_str("budget_certificate=false\n");
    log.push_str("scope=trusted_operator_100k_only\n");
    match &outcome {
        Ok(()) => log.push_str("oracle_ok=true\n"),
        Err(err) => {
            log.push_str("oracle_ok=false\n");
            log.push_str(&format!("oracle_error={err}\n"));
            eprintln!("{err}");
        }
    }
    let result = root.join("oracle/result.txt");
    let _ = fs::create_dir_all(root.join("oracle"));
    let _ = fs::write(&result, &log);
    let _ = io::stdout().write_all(log.as_bytes());
    if outcome.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn run(root: &Path, log: &mut String) -> Result<(), String> {
    check_copy(&root.join("live/fixture/party.copy"), PARTY_ROWS, party_line)?;
    check_copy(&root.join("live/fixture/account.copy"), ACCOUNT_ROWS, account_line)?;
    check_copy(&root.join("live/fixture/entry.copy"), ENTRY_ROWS, entry_line)?;
    check_copy(&root.join("live/actual/party.copy"), PARTY_ROWS, party_line)?;
    check_copy(&root.join("live/actual/account.copy"), ACCOUNT_ROWS, account_line)?;
    check_copy(&root.join("live/actual/entry.copy"), ENTRY_ROWS, entry_line)?;
    log.push_str("copy_party=2000\ncopy_account=8000\ncopy_entry=90000\n");
    log.push_str(&format!("party0={}\n", party_line(0).trim_end()));
    log.push_str(&format!("party1={}\n", party_line(1).trim_end()));
    log.push_str(&format!("account0={}\n", account_line(0).trim_end()));
    for index in [0_u32, 1, 2, 3, 4, 5, 6, 29] {
        log.push_str(&format!("entry{index}={}\n", entry_line(index).trim_end()));
    }

    require_row(
        &root.join("live/run/steps/query-wide-count.stdout"),
        r#"{"record":"row","turn":0,"cells":[{"kind":"int","value":"90000"}]}"#,
    )?;
    require_row(
        &root.join("live/run/steps/query-wide-null.stdout"),
        r#"{"record":"row","turn":0,"cells":[{"kind":"text","value":"p00000"},{"kind":"null"},{"kind":"bool","value":true}]}"#,
    )?;
    require_row(
        &root.join("live/run/steps/query-wide-qty.stdout"),
        r#"{"record":"row","turn":0,"cells":[{"kind":"int","value":"0"},{"kind":"int","value":"-9223372036854775808"}]}"#,
    )?;
    require_contains(
        &root.join("live/run/steps/query-wide-qty.stdout"),
        r#"{"kind":"int","value":"1"},{"kind":"int","value":"9223372036854775807"}"#,
    )?;
    require_contains(
        &root.join("live/run/steps/query-wide-qty.stdout"),
        r#"{"kind":"int","value":"2"},{"kind":"int","value":"0"}"#,
    )?;
    require_row(
        &root.join("live/run/steps/query-wide-fk.stdout"),
        r#"{"record":"row","turn":0,"cells":[{"kind":"int","value":"0"},{"kind":"text","value":"p00000"}]}"#,
    )?;
    require_row(
        &root.join("live/run/steps/query-wide-phone.stdout"),
        r#"{"record":"row","turn":0,"cells":[{"kind":"text","value":"tel-00001"}]}"#,
    )?;
    require_row(
        &root.join("live/run/steps/query-narrow-party.stdout"),
        r#"{"record":"row","turn":0,"cells":[{"kind":"text","value":"p00001"},{"kind":"text","value":"n00001"}]}"#,
    )?;
    require_row(
        &root.join("live/work/probe.stdout"),
        r#"{"record":"row","turn":0,"cells":[{"kind":"decimal","negative":false,"coefficient":"12345678901234","scale":4}]}"#,
    )?;
    log.push_str("query_cells=count_null_qty_fk_phone_narrow_decimal\n");

    require_exit(&root.join("live/run/steps/import-not-ready.exit"), "1")?;
    require_contains(
        &root.join("live/run/steps/import-not-ready.stderr"),
        "oo-pipeline: target schema is not ready",
    )?;
    require_exit(&root.join("live/run/steps/query-wide-before.exit"), "5")?;
    require_eq_trim(&root.join("live/run/steps/query-wide-before.stderr"), "query-pg: policy")?;
    for name in [
        "query-wide-hidden",
        "query-wide-insert",
        "query-wide-pos",
        "query-narrow-hidden",
        "query-narrow-phone",
        "query-narrow-amount",
    ] {
        require_exit(&root.join(format!("live/run/steps/{name}.exit")), "6")?;
        require_eq_trim(&root.join(format!("live/run/steps/{name}.stderr")), "query-pg: guard")?;
        require_empty(&root.join(format!("live/run/steps/{name}.stdout")))?;
    }
    for name in [
        "import-pg",
        "init-pg",
        "catalog-pg",
        "captured-pg-draft",
        "review-pg-draft",
        "reviewed-policy-wide",
        "reviewed-policy-narrow",
        "role-pack-wide",
        "role-pack-narrow",
        "role-pack-bundle-wide",
        "role-pack-bundle-narrow",
        "reviewed-effective-oos",
        "install-column-acl-wide",
        "install-column-acl-narrow",
        "provision-mapping-reader-wide",
        "provision-mapping-reader-narrow",
        "analyze-pg",
        "query-wide-count",
        "query-wide-null",
        "query-wide-qty",
        "query-wide-fk",
        "query-wide-phone",
        "query-narrow-party",
    ] {
        require_exit(&root.join(format!("live/run/steps/{name}.exit")), "0")?;
    }

    let fresh = fs::read(&root.join("live/work/fresh.stdout")).map_err(|err| err.to_string())?;
    let restarted = fs::read(&root.join("live/work/fresh-restart.stdout")).map_err(|err| err.to_string())?;
    if fresh != restarted {
        return Err("fresh and restart query stdout bytes differ".to_owned());
    }
    let fresh_text = String::from_utf8(fresh).map_err(|_| "fresh stdout is not utf-8".to_owned())?;
    if !fresh_text.starts_with(r#"{"record":"row","turn":0,"cells":[{"kind":"int","value":"2000"}]}"#) {
        return Err("fresh party count is not int 2000".to_owned());
    }
    log.push_str("restart_stdout_bytes_equal=true\nrestart_party_count=2000\n");
    same_file(
        &root.join("live/measurements/pack-wide.before"),
        &root.join("live/measurements/pack-wide.after"),
    )?;
    same_file(
        &root.join("live/measurements/pack-narrow.before"),
        &root.join("live/measurements/pack-narrow.after"),
    )?;
    log.push_str("pack_digests_stable=true\n");

    require_contains(&root.join("live/side/reader.err"), "42501")?;
    require_contains(&root.join("live/side/sleep.err"), "57014")?;
    require_contains(&root.join("live/side/writer.out"), "INSERT 0 1")?;
    require_contains(&root.join("live/side/writer.out"), "ROLLBACK")?;
    require_exit(&root.join("live/side/writer.exit"), "0")?;
    log.push_str("reader_sqlstate=42501\ncancel_sqlstate=57014\nwriter_rolled_back=true\n");

    for name in ["catalog.json", "captured-draft.json", "inspect-manifest.json", "outcome.json", "apply-outcome.json"] {
        let path = root.join("live/run").join(name);
        let text = read_string(&path)?;
        if text.contains(MARKER) {
            return Err(format!("{name} contains the party comment marker"));
        }
    }
    log.push_str("marker_absent=true\n");

    let inspect = read_string(&root.join("live/work/inspect.json"))?;
    let apply = read_string(&root.join("live/work/apply.json"))?;
    for (label, text) in [("inspect.json", &inspect), ("apply.json", &apply)] {
        if !text.contains(r#""caller":"trusted_local_admin""#) {
            return Err(format!("{label} caller is not trusted_local_admin"));
        }
    }
    if !inspect.contains(r#""host":"oo-a317-pg""#) {
        return Err("inspect config host is not the frozen alias".to_owned());
    }
    log.push_str("caller=trusted_local_admin\n");

    let mut fence = None;
    for name in ["outcome.json", "inspect-manifest.json", "apply-outcome.json"] {
        let text = read_string(&root.join("live/run").join(name))?;
        let cut = json_int(&text, "source_cut")?;
        let generation = json_int(&text, "mapping_run_generation")?;
        let target = json_int(&text, "target_fence")?;
        if cut != 317 || generation != 317 {
            return Err(format!("{name} source_cut={cut} mapping_run_generation={generation}"));
        }
        if target == 317 {
            return Err(format!("{name} target_fence collided with source_cut"));
        }
        if let Some(previous) = fence {
            if previous != target {
                return Err("target_fence differs across outcome files".to_owned());
            }
        }
        fence = Some(target);
        require_gates(&text, name)?;
    }
    log.push_str(&format!("source_cut=317\ntarget_fence={}\n", fence.unwrap_or(-1)));
    let outcome = read_string(&root.join("live/run/outcome.json"))?;
    let applied = read_string(&root.join("live/run/apply-outcome.json"))?;
    if !outcome.contains(r#""status":"awaiting_review""#) {
        return Err("inspect status is not awaiting_review".to_owned());
    }
    if !applied.contains(r#""status":"ready_for_trusted_query""#) {
        return Err("apply status is not ready_for_trusted_query".to_owned());
    }
    if !applied.contains(r#""classification.absent""#) {
        return Err("hidden national_id reason is not classification.absent".to_owned());
    }
    if !applied.contains(r#""effect":"installed","slug":"wide""#)
        || !applied.contains(r#""effect":"replay","slug":"narrow""#)
    {
        return Err("provision effects are not installed wide and replay narrow".to_owned());
    }
    log.push_str("parent_gates=false\nm1_i01=not_accepted\nm1_i02=not_accepted\n");

    let analyze = read_string(&root.join("live/run/steps/analyze-pg.stdout"))?;
    if !analyze.contains(r#""published":false"#) || !analyze.contains(r#""estimate_is_exact":false"#) {
        return Err("analyze published or exact estimate is not false".to_owned());
    }
    for rows in [2000, 8000, 90000] {
        if !analyze.contains(&format!(r#""estimated_rows":{rows}"#)) {
            return Err(format!("analyze is missing estimated_rows {rows}"));
        }
    }

    let import = read_string(&root.join("live/measurements/import.txt"))?;
    for line in [
        "party_rows=2000",
        "account_rows=8000",
        "entry_rows=90000",
        "budget_connect_ms=30000",
        "budget_statement_ms=60000",
        "budget_lock_ms=30000",
        "budget_max_rows=10000",
        "budget_max_bytes=8388608",
    ] {
        if !import.lines().any(|item| item == line) {
            return Err(format!("import measurement missing {line}"));
        }
    }
    if import.contains("certificate") {
        return Err("import measurement text claims a certificate".to_owned());
    }
    log.push_str("budget_max_rows=10000\nentry_rows=90000\nbudget_is_measurement=true\n");
    Ok(())
}

fn require_gates(text: &str, name: &str) -> Result<(), String> {
    let needles = [
        r#""parent_acceptance":false"#,
        r#""development_unit_approval":false"#,
        r#""component_approval":false"#,
        r#""m1_i01":"not_accepted""#,
        r#""m1_i02":"not_accepted""#,
        r#""semantic_publication":"missing""#,
        r#""authentication":"not_claimed""#,
        r#""authorization":"not_claimed""#,
        r#""natural_language":"missing""#,
        r#""harness_mount":"not_performed""#,
        r#""public_auth":false"#,
        r#""wal":"missing""#,
        r#""analyze_published":false"#,
        r#""role_login_binding":"configuration_not_auth""#,
    ];
    for needle in needles {
        if !text.contains(needle) {
            return Err(format!("{name} missing {needle}"));
        }
    }
    Ok(())
}

fn json_int(text: &str, key: &str) -> Result<i64, String> {
    let needle = format!("\"{key}\":");
    let Some(pos) = text.find(&needle) else {
        return Err(format!("missing {key}"));
    };
    let rest = text[pos + needle.len()..].trim_start();
    let digits: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '-')
        .collect();
    digits.parse::<i64>().map_err(|_| format!("bad {key}"))
}

fn check_copy(path: &Path, rows: u32, line_at: fn(u32) -> String) -> Result<(), String> {
    let raw = fs::read(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    let mut offset = 0usize;
    for index in 0..rows {
        let line = line_at(index);
        let end = offset + line.len();
        if end > raw.len() || &raw[offset..end] != line.as_bytes() {
            return Err(format!("{} mismatch at row {index} byte {offset}", path.display()));
        }
        offset = end;
    }
    if offset != raw.len() {
        return Err(format!("{} has {} trailing bytes", path.display(), raw.len() - offset));
    }
    Ok(())
}

fn party_line(index: u32) -> String {
    let key = format!("p{index:05}");
    let display = if index % 17 == 0 {
        "\\N".to_owned()
    } else {
        format!("n{index:05}")
    };
    let active = if index % 2 == 0 { 't' } else { 'f' };
    format!("{key}\t{display}\t{active}\ttel-{index:05}\tnid-{index:05}\n")
}

fn account_line(index: u32) -> String {
    let party = format!("p{:05}", index % PARTY_ROWS);
    let opened = if index % 3 != 0 { 't' } else { 'f' };
    let label = if index % 19 == 0 {
        "\\N".to_owned()
    } else {
        format!("a{index}")
    };
    format!("{index}\t{party}\t{opened}\t{label}\n")
}

fn entry_line(index: u32) -> String {
    let account = index % ACCOUNT_ROWS;
    let qty = match index {
        0 => i64::MIN,
        1 => i64::MAX,
        2 => 0,
        _ => i64::from(index) - 45_000,
    };
    let amount = match index {
        3 => Some(12_345_678_901_234_i64),
        4 => Some(-12_345_678_901_234),
        5 => Some(1),
        6 => Some(-1),
        _ if index % 29 == 0 => None,
        _ => Some(i64::from(index % 10_000) - 5_000),
    };
    let amount_text = match amount {
        None => "\\N".to_owned(),
        Some(minor) => format_minor(minor),
    };
    let memo = if index % 13 == 0 {
        "\\N".to_owned()
    } else {
        format!("m{index}")
    };
    format!("{index}\t{account}\t{qty}\t{amount_text}\t{memo}\n")
}

fn format_minor(minor: i64) -> String {
    let negative = minor < 0;
    let absolute = minor.unsigned_abs();
    let whole = absolute / 10_000;
    let frac = absolute % 10_000;
    if negative {
        format!("-{whole}.{frac:04}")
    } else {
        format!("{whole}.{frac:04}")
    }
}

fn read_string(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    String::from_utf8(bytes).map_err(|_| format!("{} is not utf-8", path.display()))
}

fn require_row(path: &Path, first: &str) -> Result<(), String> {
    let text = read_string(path)?;
    let line = text.lines().next().unwrap_or("");
    if line != first {
        return Err(format!("{} first row mismatch", path.display()));
    }
    if !text.contains(r#""status":"succeeded""#) || !text.contains(r#""truncated":false"#) {
        return Err(format!("{} summary is not a full success", path.display()));
    }
    Ok(())
}

fn require_contains(path: &Path, needle: &str) -> Result<(), String> {
    let text = read_string(path)?;
    if text.contains(needle) {
        Ok(())
    } else {
        Err(format!("{} missing {needle}", path.display()))
    }
}

fn require_eq_trim(path: &Path, expected: &str) -> Result<(), String> {
    let text = read_string(path)?;
    if text.trim() == expected {
        Ok(())
    } else {
        Err(format!("{} stderr is not {expected}", path.display()))
    }
}

fn require_empty(path: &Path) -> Result<(), String> {
    let text = read_string(path)?;
    if text.is_empty() {
        Ok(())
    } else {
        Err(format!("{} stdout is not empty", path.display()))
    }
}

fn require_exit(path: &Path, expected: &str) -> Result<(), String> {
    let text = read_string(path)?;
    if text.trim() == expected {
        Ok(())
    } else {
        Err(format!("{} exit is {} not {expected}", path.display(), text.trim()))
    }
}

fn same_file(left: &Path, right: &Path) -> Result<(), String> {
    let a = fs::read(left).map_err(|err| format!("read {}: {err}", left.display()))?;
    let b = fs::read(right).map_err(|err| format!("read {}: {err}", right.display()))?;
    if a == b && !a.is_empty() {
        Ok(())
    } else {
        Err(format!("{} and {} differ", left.display(), right.display()))
    }
}
