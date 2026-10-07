//! Independent hand-gold check for the numeric query review.
//! Std only. No product edits. No Python.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

const AMOUNT: &str = "123456789012345678";
const WIDE: &str = "12345678901234567890123456789012345678";

fn main() -> ExitCode {
    let mut failures: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    check_sources(&mut failures, &mut notes);
    check_tests(&mut failures);
    check_steps(&mut failures);
    check_stdout(&mut failures, &mut notes);
    check_audits(&mut failures);
    let mut body = String::new();
    for note in &notes {
        body.push_str(note);
        body.push('\n');
    }
    if failures.is_empty() {
        body.push_str("gold_match=1\n");
    } else {
        body.push_str("gold_match=0\n");
        for failure in &failures {
            body.push_str("fail=");
            body.push_str(failure);
            body.push('\n');
        }
    }
    let path = "/evidence/linux/proof/gold.txt";
    if fs::write(path, &body).is_err() {
        eprintln!("gold write failed");
        return ExitCode::from(1);
    }
    if failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn read(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn must_contain(failures: &mut Vec<String>, label: &str, text: &str, needle: &str) {
    if !text.contains(needle) {
        failures.push(format!("{label} missing {needle}"));
    }
}

fn slice_fn<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let start = text.find(name)?;
    let rest = &text[start + name.len()..];
    let end = rest.find("\nfn ").or_else(|| rest.find("\nasync fn ")).unwrap_or(rest.len());
    Some(&text[start..start + name.len() + end])
}

fn check_sources(failures: &mut Vec<String>, notes: &mut Vec<String>) {
    let catalog = read("/src/crates/oo-gateway/src/policy_catalog.rs").unwrap_or_default();
    let policy = read("/src/crates/oo-gateway/src/policy.rs").unwrap_or_default();
    let mapping = read("/src/crates/oo-gateway/src/mapping_reader_access.rs").unwrap_or_default();
    let query = read("/src/crates/oo-gateway/src/bin/query-pg.rs").unwrap_or_default();
    let executor = read("/src/crates/oo-gateway/src/query_executor.rs").unwrap_or_default();
    let reviewed = read("/src/crates/oo-gateway/src/reviewed_policy.rs").unwrap_or_default();
    if catalog.is_empty() || policy.is_empty() || mapping.is_empty() || query.is_empty() || executor.is_empty() || reviewed.is_empty() {
        failures.push("source unreadable".to_string());
        return;
    }
    let allowlist = "pub const SUPPORTED_TYPES: &[&str] = &[\"bool\", \"int2\", \"int4\", \"int8\", \"numeric\", \"text\"];";
    must_contain(failures, "allowlist", &catalog, allowlist);
    if allowlist.contains("float") {
        failures.push("allowlist contains float".to_string());
    }
    let signatures = slice_fn(&catalog, "pub const COMPARISON_SIGNATURES").unwrap_or("");
    if signatures.contains("numeric") {
        failures.push("comparison signatures include numeric".to_string());
    } else {
        notes.push("numeric_comparison=absent".to_string());
    }
    if !catalog.contains("pub const AGGREGATE_STAR_NAME: &str = \"count\";") {
        failures.push("aggregate star changed".to_string());
    }
    if catalog.contains("NUMERIC_LITERAL") || signatures.contains("(\"numeric\"") {
        failures.push("numeric literal signature present".to_string());
    } else {
        notes.push("numeric_literal=absent".to_string());
    }
    let grant = slice_fn(&catalog, "pub fn grantable_base_type").unwrap_or("");
    must_contain(failures, "grant", grant, "NUMERIC_MAX_PRECISION");
    must_contain(failures, "grant", grant, "name != \"numeric\"");
    let supported = slice_fn(&catalog, "fn supported_base").unwrap_or("");
    must_contain(failures, "supported_base", supported, "grantable_base_type");
    must_contain(failures, "supported_base", supported, "*oid == attribute.type_oid");
    must_contain(failures, "supported_base", supported, "type_kind != \"b\"");
    notes.push("catalog_oid_equality=supported_base".to_string());
    let install = slice_fn(&policy, "async fn ensure_grantable_type").unwrap_or("");
    must_contain(failures, "install", install, "grantable_base_type");
    must_contain(failures, "install", install, "atttypmod");
    must_contain(failures, "install", install, "kind == \"b\"");
    // The select joins pg_type on atttypid to read typname. It does not compare
    // that oid with the loaded pg_catalog base type.
    if install.contains("type_oid") || install.contains("types.get") {
        failures.push("install compares a loaded type oid".to_string());
    } else {
        notes.push("install_oid_equality=absent".to_string());
    }
    let reader = slice_fn(&mapping, "async fn ensure_supported_type").unwrap_or("");
    must_contain(failures, "mapping", reader, "grantable_base_type");
    must_contain(failures, "mapping", reader, "atttypmod");
    must_contain(failures, "mapping", reader, "kind == \"b\"");
    if reader.contains("SUPPORTED_TYPES.contains") {
        failures.push("mapping reader still checks the name only".to_string());
    }
    if reader.contains("type_oid") || reader.contains("types.get") {
        notes.push("mapping_reader_oid_equality=present".to_string());
        failures.push("mapping reader compares a loaded type oid".to_string());
    } else {
        notes.push("mapping_reader_oid_equality=absent".to_string());
    }
    must_contain(failures, "row_filter", &query, "Value::Null if key == \"row_filter\" => Ok(())");
    must_contain(failures, "row_filter", &query, "Value::Null => Err(Fail::Request)");
    must_contain(failures, "decoder", &executor, "const OID_NUMERIC: u32 = 1700;");
    must_contain(failures, "decoder", &executor, "CANONICAL_MAX_COEFFICIENT_DIGITS");
    must_contain(failures, "decoder", &executor, "\"numeric_nan\"");
    must_contain(failures, "decoder", &executor, "\"numeric_infinity\"");
    must_contain(failures, "decoder", &executor, "\"numeric_neg_infinity\"");
    must_contain(failures, "admission", &reviewed, "\"numeric\" => semantic == SemanticType::Quantity");
    must_contain(failures, "admission", &reviewed, "typtype != \"b\"");
    let admitted = slice_fn(&reviewed, "fn base_type").unwrap_or("");
    if admitted.contains("float4") || admitted.contains("float8") {
        failures.push("base_type admits a float".to_string());
    }
    notes.push("type_base=typtype_b".to_string());
    notes.push("domains=refused_by_typtype".to_string());
    let _ = Path::new("/src");
}

fn log_ok(failures: &mut Vec<String>, path: &str, needle: &str) {
    match read(path) {
        Some(text) if text.contains(needle) => {}
        Some(_) => failures.push(format!("{path} missing {needle}")),
        None => failures.push(format!("{path} missing")),
    }
}

fn check_tests(failures: &mut Vec<String>) {
    log_ok(failures, "/evidence/linux/proof/test-lib.log", "bounded_numeric_admits_precision_1_through_38 ... ok");
    log_ok(failures, "/evidence/linux/proof/test-lib.log", "numeric_quantity_is_the_only_new_admission ... ok");
    log_ok(failures, "/evidence/linux/proof/test-lib.log", "numeric_wire_matches_the_reviewed_vectors ... ok");
    log_ok(failures, "/evidence/linux/proof/test-lib.log", "numeric_hand_literals_keep_scale_and_reject_specials ... ok");
    log_ok(failures, "/evidence/linux/proof/test-lib.log", "numeric_typmod_checks_scale_and_coefficient_length ... ok");
    log_ok(failures, "/evidence/linux/proof/test-query-pg.log", "decimal_json_is_exact_and_existing_kinds_stay ... ok");
    log_ok(failures, "/evidence/linux/proof/test-row-filter.log", "absent_row_filter_null_is_optional ... ok");
    log_ok(failures, "/evidence/linux/proof/test-live.log", "test a327_live::live_numeric_proof ... ok");
}

fn exit_is(failures: &mut Vec<String>, name: &str, code: &str) {
    let path = format!("/evidence/linux/proof/steps/{name}.exit");
    match read(&path) {
        Some(text) if text.trim() == code => {}
        Some(text) => failures.push(format!("{name} exit {text} wanted {code}")),
        None => failures.push(format!("{name} exit missing")),
    }
}

fn stderr_is(failures: &mut Vec<String>, name: &str, expected: &str) {
    let path = format!("/evidence/linux/proof/steps/{name}.stderr");
    match fs::read(&path) {
        Ok(bytes) if bytes == expected.as_bytes() => {}
        Ok(bytes) => failures.push(format!("{name} stderr len {}", bytes.len())),
        Err(_) => failures.push(format!("{name} stderr missing")),
    }
}

fn check_steps(failures: &mut Vec<String>) {
    for name in [
        "catalog-pg",
        "captured-pg-draft",
        "review-pg-draft",
        "reviewed-effective-oos",
        "role-pack",
        "role-pack-bundle",
        "init-pg",
        "import-pg",
        "reviewed-policy",
        "install-column-acl",
        "provision-mapping-reader",
        "analyze-pg",
        "amounts",
    ] {
        exit_is(failures, name, "0");
    }
    exit_is(failures, "import-pg-not-ready", "1");
    exit_is(failures, "provision-loose", "7");
    exit_is(failures, "provision-over", "7");
    exit_is(failures, "before-analyze", "5");
    exit_is(failures, "hidden", "6");
    stderr_is(failures, "provision-loose", "provision-mapping-reader: policy type\n");
    stderr_is(failures, "provision-over", "provision-mapping-reader: policy type\n");
    stderr_is(failures, "before-analyze", "query-pg: policy\n");
    stderr_is(failures, "hidden", "query-pg: guard\n");
    stderr_is(failures, "amounts", "");
    if Path::new("/evidence/linux/proof/steps/before-analyze.stdout").exists() {
        match fs::read("/evidence/linux/proof/steps/before-analyze.stdout") {
            Ok(bytes) if bytes.is_empty() => {}
            Ok(_) => failures.push("before-analyze stdout was not empty".to_string()),
            Err(_) => failures.push("before-analyze stdout missing".to_string()),
        }
    }
    if Path::new("/evidence/linux/proof/audits/before-analyze.jsonl").exists() {
        failures.push("before-analyze wrote an audit".to_string());
    }
    if Path::new("/evidence/linux/proof/audits/hidden.jsonl").exists() {
        failures.push("hidden wrote an audit".to_string());
    }
}

fn dec(negative: bool, coefficient: &str) -> String {
    format!(
        "{{\"kind\":\"decimal\",\"negative\":{negative},\"coefficient\":\"{coefficient}\",\"scale\":4}}",
        negative = if negative { "true" } else { "false" }
    )
}

fn row(id: &str, left: &str, right: &str) -> String {
    format!("{{\"record\":\"row\",\"turn\":0,\"cells\":[{{\"kind\":\"int\",\"value\":\"{id}\"}},{left},{right}]}}")
}

fn check_stdout(failures: &mut Vec<String>, notes: &mut Vec<String>) {
    let text = match read("/evidence/linux/proof/steps/amounts.stdout") {
        Some(text) => text,
        None => {
            failures.push("amounts stdout missing".to_string());
            return;
        }
    };
    if text.contains("SELECT ") || text.contains("numeric(") {
        failures.push("amounts stdout contains sql text".to_string());
    }
    let mut lines: Vec<&str> = text.split('\n').filter(|line| !line.is_empty()).collect();
    if lines.len() != 7 {
        failures.push(format!("amounts lines {}", lines.len()));
        return;
    }
    let expected = [
        row("1", &dec(false, AMOUNT), &dec(false, WIDE)),
        row("2", &dec(true, AMOUNT), &dec(true, WIDE)),
        row("3", &dec(false, "1"), &dec(false, "1")),
        row("4", &dec(true, "1"), &dec(true, "1")),
        row("5", &dec(false, "0"), &dec(false, "0")),
        row("6", "{\"kind\":\"null\"}", "{\"kind\":\"null\"}"),
    ];
    for (index, want) in expected.iter().enumerate() {
        if lines[index] != want {
            failures.push(format!("row {} mismatch", index + 1));
        }
    }
    let summary = lines.pop().unwrap_or("");
    for needle in [
        "\"record\":\"summary\"",
        "\"status\":\"succeeded\"",
        "\"truncated\":false",
        "\"returned_rows\":6",
        "\"result_bytes\":162",
        "\"rollback_confirmed\":true",
        "\"stop\":null",
        "\"sqlstate\":null",
        "\"original_sha256\":\"466f22e579afe5252e78ae6cffca1da0a9c4630ba8cb63440a0fc9b7a9c6957a\"",
    ] {
        if !summary.contains(needle) {
            failures.push(format!("summary missing {needle}"));
        }
    }
    if let Some(start) = summary.find("\"executed_sha256\":\"") {
        let rest = &summary[start + "\"executed_sha256\":\"".len()..];
        let hash = rest.split('"').next().unwrap_or("");
        if hash.len() == 64 && hash.chars().all(|byte| byte.is_ascii_hexdigit()) {
            notes.push(format!("executed_sha256={hash}"));
        } else {
            failures.push("executed sha256 missing".to_string());
        }
    } else {
        failures.push("executed sha256 missing".to_string());
    }
    notes.push(format!("summary={summary}"));
    match read("/evidence/linux/proof/outcome.txt") {
        Some(text) if text == "numeric_query_recorded\n" => {}
        Some(_) => failures.push("outcome text mismatch".to_string()),
        None => failures.push("outcome text missing".to_string()),
    }
    match read("/evidence/linux/proof/outcome.json") {
        Some(text) => {
            must_contain(failures, "outcome", &text, "\"amounts_rows\": 6");
            must_contain(failures, "outcome", &text, "\"ssl\": \"true\"");
            must_contain(failures, "outcome", &text, "\"loose_exit\": 7");
            must_contain(failures, "outcome", &text, "\"over_exit\": 7");
            must_contain(failures, "outcome", &text, "\"hidden_exit\": 6");
            must_contain(failures, "outcome", &text, "\"before_analyze_exit\": 5");
            must_contain(failures, "outcome", &text, "\"sql_in_query_output\": false");
            must_contain(failures, "outcome", &text, "\"parent_acceptance\": false");
        }
        None => failures.push("outcome json missing".to_string()),
    }
    match read("/evidence/linux/proof/steps/reviewed-effective-oos.stdout") {
        Some(text) => {
            must_contain(failures, "effective", &text, "\"semantic_type\":\"quantity\"");
            must_contain(failures, "effective", &text, "\"data_type\":\"numeric\"");
            must_contain(failures, "effective", &text, "\"name\":\"amount\"");
            must_contain(failures, "effective", &text, "\"name\":\"wide\"");
        }
        None => failures.push("effective stdout missing".to_string()),
    }
}

fn check_audits(failures: &mut Vec<String>) {
    match fs::read("/evidence/linux/proof/audits/amounts.jsonl") {
        Ok(bytes) if bytes.is_empty() => failures.push("amounts audit empty".to_string()),
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            if text.contains("SELECT ") || text.contains("password") {
                failures.push("amounts audit exposes sql or a password field".to_string());
            }
        }
        Err(_) => failures.push("amounts audit missing".to_string()),
    }
}
