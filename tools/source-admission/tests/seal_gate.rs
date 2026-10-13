//! Capture witness and native seal. Derivation uses the capturer and the Arrow
//! bytes. A caller hash or a caller boolean is not treated as admission.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use source_admission::{
    CapturedInput, DECISION_PROTOCOL, OutputPaths, SourceConfig, TargetReport, decide_captured,
    parse_strict, seal_captured,
};

const NZ_SCHEMA: &str = "c51a4cb79d5011a1eed68164a810e0ddf62d4ab01628033f05cfced04c6db361";
const NZ_ARROW: &str = "4f81786e75c3a09d21e422e356f24f8c87b93c9c0abcb5d8a004b7da199a9347";

fn bin() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_oo_source_admission") {
        return PathBuf::from(path);
    }
    let mut exe = std::env::current_exe().expect("current_exe");
    exe.pop();
    exe.pop();
    exe.push("oo-source-admission");
    exe
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "oo-dl01w1-02-seal-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn write_json(path: &Path, value: &Value) {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    fs::write(path, bytes).unwrap();
}

struct Laid {
    dir: PathBuf,
    report: Value,
    config: Value,
    witness: Vec<u8>,
    capturer: Vec<u8>,
    arrow: Vec<u8>,
    driver: Vec<u8>,
    schema_preimage: Vec<u8>,
    type_preimage: Vec<u8>,
    report_bytes: Vec<u8>,
    config_bytes: Vec<u8>,
    report_path: PathBuf,
    config_path: PathBuf,
}

fn nz_columns() -> Value {
    json!([
        {"table":"nz_obs","columns":[
            {"name":"id","logical_name":"id","arrow_type":"int64","nullable":false},
            {"name":"qty","logical_name":"qty","arrow_type":"decimal128","nullable":true,"precision":10,"scale":2}
        ]}
    ])
}

fn nz_types() -> Value {
    json!([
        {"table":"nz_obs","name":"id","arrow_type":"int64","precision":null,"scale":null},
        {"table":"nz_obs","name":"qty","arrow_type":"decimal128","precision":10,"scale":2}
    ])
}

fn nz_table() -> Value {
    json!({
        "name": "nz_obs",
        "arrow": "ns2-nz_obs.arrow",
        "route": "C5",
        "consistency": "S0",
        "delete_mode": "reconcile",
        "cursor_safety": "no_cursor",
        "quality": "DELETES_LAGGED",
        "rows": ["1\u{1f}\\N", "2\u{1f}\\N"]
    })
}

fn capturer(engine: &str, table: Value) -> Value {
    json!({
        "protocol": format!("oo.{engine}.capture-tick/v1"),
        "database": "oo_dl01w1_43",
        "label": "ns2",
        "product_reader_write_attempts": 0,
        "snapshot_token": false,
        "cross_table_consistency": "B",
        "admitted": true,
        "trusted": true,
        "tables": [table]
    })
}

fn lay(
    engine: &str,
    compat: &str,
    table: Value,
    columns: Value,
    types: Value,
    arrow_file: &str,
) -> Laid {
    let dir = scratch();
    let arrow_path = dir.join(arrow_file);
    let fixture_name = arrow_file.replace(".arrow", ".ipc");
    fs::copy(
        manifest().join("fixtures/native").join(fixture_name),
        &arrow_path,
    )
    .unwrap();
    let driver = b"oo-doris-reader-fixture\n".to_vec();
    let driver_path = dir.join("reader");
    fs::write(&driver_path, &driver).unwrap();
    let capturer_path = dir.join("tick.json");
    let schema_path = dir.join("schema.json");
    let type_path = dir.join("types.json");
    let witness_path = dir.join("witness.json");
    let report_path = dir.join("report.json");
    let config_path = dir.join("config.json");
    let capturer_value = capturer(engine, table.clone());
    write_json(&capturer_path, &capturer_value);
    write_json(&schema_path, &columns);
    write_json(&type_path, &types);
    let witness = json!({
        "protocol": "oo.source-admission.capture-witness/v1",
        "source_engine": engine,
        "source_id": "dorispreview",
        "source_generation": 2,
        "snapshot": {"kind": "capture-tick", "value": "ns2"},
        "run_id": "dorispreview-ns2-1791764112013",
        "logical_schema": "oo_dl01w1_43",
        "logical_table": table["name"].as_str().unwrap(),
        "capturer_path": capturer_path.to_str().unwrap(),
        "arrow_path": arrow_path.to_str().unwrap(),
        "driver_path": driver_path.to_str().unwrap(),
        "schema_preimage_path": schema_path.to_str().unwrap(),
        "type_preimage_path": type_path.to_str().unwrap()
    });
    write_json(&witness_path, &witness);
    let mut report: Value = serde_json::from_slice(
        &fs::read(manifest().join("fixtures/native/doris-preview-report.json")).unwrap(),
    )
    .unwrap();
    report["compat_mode"] = json!(compat);
    report["driver"]["digest"] = json!(sha256_hex(&driver));
    report["driver"]["version"] = json!("0.36.1");
    report["capture_methods"] = json!([]);
    report["capture_witness"] = json!(witness_path.to_str().unwrap());
    if compat == "oracle" {
        report["permissions"] = json!([
            {"name":"CREATE SESSION","status":"known","granted":true},
            {"name":"SELECT","status":"known","granted":true,"scope":{"database":"oo_dl01w1_43","table": table["name"]}}
        ]);
    } else if compat == "mongodb-native" {
        report["permissions"] = json!([{"name":"read","status":"known","granted":true}]);
    }
    let mut config: Value = serde_json::from_slice(
        &fs::read(manifest().join("fixtures/native/doris-preview-config.json")).unwrap(),
    )
    .unwrap();
    config["compat_mode"] = json!(compat);
    if compat == "oracle" || compat == "mongodb-native" {
        config["permissions"] = report["permissions"].clone();
    }
    write_json(&report_path, &report);
    write_json(&config_path, &config);
    Laid {
        dir,
        witness: fs::read(&witness_path).unwrap(),
        capturer: fs::read(&capturer_path).unwrap(),
        arrow: fs::read(&arrow_path).unwrap(),
        driver,
        schema_preimage: fs::read(&schema_path).unwrap(),
        type_preimage: fs::read(&type_path).unwrap(),
        report_bytes: fs::read(&report_path).unwrap(),
        config_bytes: fs::read(&config_path).unwrap(),
        report,
        config,
        report_path,
        config_path,
    }
}

fn nz() -> Laid {
    lay(
        "doris",
        "doris",
        nz_table(),
        nz_columns(),
        nz_types(),
        "ns2-nz_obs.arrow",
    )
}

fn input<'a>(laid: &'a Laid) -> CapturedInput<'a> {
    CapturedInput {
        witness: &laid.witness,
        capturer: &laid.capturer,
        arrow: &laid.arrow,
        driver: &laid.driver,
        schema_preimage: &laid.schema_preimage,
        type_preimage: &laid.type_preimage,
        report: &laid.report_bytes,
        config: &laid.config_bytes,
    }
}

fn parsed(laid: &Laid) -> (TargetReport, SourceConfig) {
    (
        parse_strict(&laid.report_bytes).unwrap(),
        parse_strict(&laid.config_bytes).unwrap(),
    )
}

fn reload(laid: &mut Laid) {
    let witness_path = PathBuf::from(laid.report["capture_witness"].as_str().unwrap());
    let witness: Value = serde_json::from_slice(&laid.witness).unwrap();
    write_json(&witness_path, &witness);
    write_json(
        &laid.dir.join("tick.json"),
        &serde_json::from_slice::<Value>(&laid.capturer).unwrap(),
    );
    write_json(
        &laid.dir.join("schema.json"),
        &serde_json::from_slice::<Value>(&laid.schema_preimage).unwrap(),
    );
    write_json(
        &laid.dir.join("types.json"),
        &serde_json::from_slice::<Value>(&laid.type_preimage).unwrap(),
    );
    write_json(&laid.report_path, &laid.report);
    write_json(&laid.config_path, &laid.config);
    laid.witness = fs::read(&witness_path).unwrap();
    laid.capturer = fs::read(laid.dir.join("tick.json")).unwrap();
    laid.schema_preimage = fs::read(laid.dir.join("schema.json")).unwrap();
    laid.type_preimage = fs::read(laid.dir.join("types.json")).unwrap();
    laid.report_bytes = fs::read(&laid.report_path).unwrap();
    laid.config_bytes = fs::read(&laid.config_path).unwrap();
}

fn replace_capturer(laid: &mut Laid, value: Value) {
    laid.capturer = serde_json::to_vec_pretty(&value).unwrap();
    laid.capturer.push(b'\n');
    reload(laid);
}

fn decision_of(laid: &Laid) -> (bool, Value) {
    let (report, config) = parsed(laid);
    let decision = decide_captured(&report, &config, &input(laid)).unwrap();
    let bytes = source_admission::decision_json(&decision).unwrap();
    let doc: Value = serde_json::from_slice(&bytes).unwrap();
    (decision.admitted, doc)
}

fn codes(doc: &Value) -> Vec<String> {
    doc["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["code"].as_str().unwrap().to_string())
        .collect()
}

fn assert_public(doc: &Value) {
    let text = serde_json::to_string(doc).unwrap();
    assert!(!text.contains("secret://"), "{text}");
    assert!(doc.pointer("/identity/secret_ref").is_none(), "{doc}");
}

fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(bin()).args(args).output().expect("run");
    (
        output.status.code().unwrap_or(127),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

#[test]
fn doris_nz_obs_derives_c5_without_secret_and_seals() {
    let laid = nz();
    let (report, config) = parsed(&laid);
    let out = laid.dir.join("out");
    let sealed = seal_captured(
        &report,
        &config,
        &input(&laid),
        &OutputPaths {
            report_path: laid.report_path.to_str().unwrap(),
            config_path: laid.config_path.to_str().unwrap(),
            catalog_path: out.join("catalog.json").to_str().unwrap(),
            receipt_path: out.join("admission-receipt.json").to_str().unwrap(),
        },
    )
    .unwrap();
    assert!(sealed.decision.admitted, "{:?}", sealed.decision.blocks);
    assert!(sealed.refusal.is_none());
    let doc: Value = serde_json::from_slice(&sealed.decision_bytes).unwrap();
    assert_eq!(doc["protocol"], json!(DECISION_PROTOCOL));
    assert_eq!(doc["admitted"], json!(true));
    assert_eq!(doc["matrix_mutated"], json!(false));
    assert_public(&doc);
    assert_eq!(doc["model"]["capture_methods"][0]["method"], json!("C5"));
    assert_eq!(
        doc["model"]["capture_methods"][0]["consistency"],
        json!("S0")
    );
    assert_eq!(
        doc["model"]["capture_methods"][0]["write_class"],
        json!("R0")
    );
    assert_eq!(
        doc["model"]["capture_methods"][0]["delete_detection"],
        json!("reconcile")
    );
    assert_eq!(doc["zero_write"]["proven"], json!(false));
    let bundle = sealed.bundle.expect("bundle");
    assert_eq!(bundle.arrow_schema_sha256, NZ_SCHEMA);
    assert_ne!(bundle.schema_digest, bundle.arrow_schema_sha256);
    assert_ne!(bundle.registry_sha256, bundle.preimage_sha256);
    let receipt: Value = serde_json::from_slice(&bundle.receipt).unwrap();
    assert_eq!(
        receipt["schema"],
        json!("oo.source-admission.capture-receipt/v1")
    );
    assert_eq!(receipt["source_bound"], json!(true));
    assert_eq!(receipt["source_engine"], json!("doris"));
    assert_eq!(receipt["source_generation"], json!(2));
    assert_eq!(receipt["run_id"], json!("dorispreview-ns2-1791764112013"));
    assert_eq!(receipt["registry_sha256"], json!(bundle.preimage_sha256));
    assert_eq!(receipt["arrow_schema_sha256"], json!(NZ_SCHEMA));
    assert_eq!(receipt["arrow_sha256"][0], json!(NZ_ARROW));
    assert_eq!(receipt["input_digest"], json!(NZ_ARROW));
    assert_eq!(receipt["quality"], json!("DELETES_LAGGED"));
    assert_eq!(receipt["cross_table_consistency"], json!("B"));
    assert_eq!(receipt["delete_observation"], json!("unknown"));
    assert_eq!(receipt["capture_method"], json!("C5"));
    assert_eq!(receipt["consistency"], json!("S0"));
    assert_eq!(receipt["delete_detection"], json!("reconcile"));
    assert_eq!(receipt["write_attempts"], json!(0));
    assert_eq!(receipt["snapshot_token"], json!(false));
    assert!(receipt.get("admitted").is_none());
    assert!(receipt.get("secret_ref").is_none());
    assert_public(&receipt);
    let registry: Value = serde_json::from_slice(&bundle.registry).unwrap();
    assert_public(&registry);
    let binding = &registry["bindings"][0];
    assert_eq!(binding["source_engine"], json!("doris"));
    assert_eq!(binding["driver_sha256"], binding["driver_digest"]);
    assert!(
        binding["driver_path"]
            .as_str()
            .is_some_and(|path| path.starts_with('/'))
    );
    assert_eq!(binding["objects"][0]["row_count"], json!(2));
    assert_eq!(binding["objects"][0]["arrow_sha256"], json!(NZ_ARROW));
    assert!(binding["fields"][0].get("precision").is_none());
    assert_eq!(binding["fields"][1]["precision"], json!(10));
    assert_eq!(binding["fields"][1]["scale"], json!(2));
    assert_eq!(binding["fields"][1]["arrow_type"], json!("decimal128"));
    assert_eq!(
        binding["admission_receipt_sha256"],
        json!(sha256_hex(&bundle.receipt))
    );
    let blank = "0".repeat(64);
    let registry_text = String::from_utf8(bundle.registry.clone()).unwrap();
    let receipt_sha = binding["admission_receipt_sha256"].as_str().unwrap();
    let preimage = registry_text.replacen(receipt_sha, &blank, 1);
    assert_eq!(sha256_hex(preimage.as_bytes()), bundle.preimage_sha256);
    let catalog: Value = serde_json::from_slice(&bundle.catalog).unwrap();
    assert_eq!(
        catalog["protocol"],
        json!("oo.pipeline.sealed-import-catalog/v1")
    );
    assert_eq!(catalog["schema_digest"], json!(bundle.schema_digest));
    assert_ne!(catalog["schema_digest"], catalog["input_digest"]);
    assert_public(&catalog);
    assert!(!out.exists());
}

#[test]
fn part_day_keeps_c4_and_does_not_promote_partition_quality() {
    let table = json!({
        "name": "part_day",
        "arrow": "ns2-part_day.arrow",
        "route": "C4",
        "consistency": "S0",
        "delete_mode": "partition_replace",
        "cursor_safety": "no_cursor",
        "quality": "partition_metadata_recorded",
        "rows": ["a", "b", "c", "d"]
    });
    let columns = json!([{
        "table": "part_day",
        "columns": [
            {"name":"day","logical_name":"day","arrow_type":"date32","nullable":false},
            {"name":"region","logical_name":"region","arrow_type":"utf8","nullable":false},
            {"name":"amount","logical_name":"amount","arrow_type":"decimal128","nullable":true,"precision":12,"scale":2}
        ]
    }]);
    let types = json!([
        {"table":"part_day","name":"day","arrow_type":"date32","precision":null,"scale":null},
        {"table":"part_day","name":"region","arrow_type":"utf8","precision":null,"scale":null},
        {"table":"part_day","name":"amount","arrow_type":"decimal128","precision":12,"scale":2}
    ]);
    let laid = lay(
        "doris",
        "doris",
        table,
        columns,
        types,
        "ns2-part_day.arrow",
    );
    let (admitted, doc) = decision_of(&laid);
    assert!(admitted, "{doc}");
    assert_eq!(doc["model"]["capture_methods"][0]["method"], json!("C4"));
    assert_eq!(
        doc["model"]["capture_methods"][0]["consistency"],
        json!("S0")
    );
    assert_eq!(
        doc["model"]["capture_methods"][0]["delete_detection"],
        json!("partition_replace")
    );
}

#[test]
fn the_same_fields_derive_the_same_method_for_three_engines() {
    let mut seen = Vec::new();
    for (engine, compat) in [
        ("doris", "doris"),
        ("oracle", "oracle"),
        ("mongodb", "mongodb-native"),
    ] {
        let laid = lay(
            engine,
            compat,
            nz_table(),
            nz_columns(),
            nz_types(),
            "ns2-nz_obs.arrow",
        );
        let (admitted, doc) = decision_of(&laid);
        assert!(admitted, "{engine} {doc}");
        let method = doc["model"]["capture_methods"][0].clone();
        seen.push((
            method["method"].as_str().unwrap().to_string(),
            method["consistency"].as_str().unwrap().to_string(),
            method["delete_detection"].as_str().unwrap().to_string(),
        ));
    }
    assert_eq!(
        seen,
        vec![
            ("C5".to_string(), "S0".to_string(), "reconcile".to_string()),
            ("C5".to_string(), "S0".to_string(), "reconcile".to_string()),
            ("C5".to_string(), "S0".to_string(), "reconcile".to_string()),
        ]
    );
}

#[test]
fn postgresql_decision_matches_and_seal_emits_bound_registry() {
    let laid = lay(
        "postgresql",
        "postgres-native",
        nz_table(),
        nz_columns(),
        nz_types(),
        "ns2-nz_obs.arrow",
    );
    let (report, config) = parsed(&laid);
    let out = laid.dir.join("no-seal");
    let sealed = seal_captured(
        &report,
        &config,
        &input(&laid),
        &OutputPaths {
            report_path: laid.report_path.to_str().unwrap(),
            config_path: laid.config_path.to_str().unwrap(),
            catalog_path: out.join("catalog.json").to_str().unwrap(),
            receipt_path: out.join("admission-receipt.json").to_str().unwrap(),
        },
    )
    .unwrap();
    assert!(sealed.decision.admitted, "{:?}", sealed.decision.blocks);
    assert_eq!(sealed.refusal, None);
    assert!(sealed.bundle.is_some());
    assert!(!out.exists());
}

#[test]
fn bytes_budget_and_r1_are_admitted_and_refused_for_seal() {
    let mut laid = nz();
    laid.report["cost_model"]["unit"] = json!("Bytes");
    laid.config["budget"]["unit"] = json!("Bytes");
    reload(&mut laid);
    let (report, config) = parsed(&laid);
    let out = laid.dir.join("budget");
    let sealed = seal_captured(
        &report,
        &config,
        &input(&laid),
        &OutputPaths {
            report_path: laid.report_path.to_str().unwrap(),
            config_path: laid.config_path.to_str().unwrap(),
            catalog_path: out.join("catalog.json").to_str().unwrap(),
            receipt_path: out.join("admission-receipt.json").to_str().unwrap(),
        },
    )
    .unwrap();
    assert!(sealed.decision.admitted);
    assert_eq!(sealed.refusal, Some("budget unit"));
    assert!(sealed.bundle.is_none());

    let mut laid = nz();
    laid.report["capabilities"][0]["write_class"] = json!("R1");
    reload(&mut laid);
    let (report, config) = parsed(&laid);
    let sealed = seal_captured(
        &report,
        &config,
        &input(&laid),
        &OutputPaths {
            report_path: laid.report_path.to_str().unwrap(),
            config_path: laid.config_path.to_str().unwrap(),
            catalog_path: laid.dir.join("r1").join("catalog.json").to_str().unwrap(),
            receipt_path: laid
                .dir
                .join("r1")
                .join("admission-receipt.json")
                .to_str()
                .unwrap(),
        },
    )
    .unwrap();
    assert!(sealed.decision.admitted, "{:?}", sealed.decision.blocks);
    assert_eq!(sealed.refusal, Some("not r0"));
    assert!(sealed.bundle.is_none());
}

#[test]
fn necessary_failures_do_not_invent_a_method_or_a_grade() {
    let mut alias = nz();
    let mut tick = serde_json::from_slice::<Value>(&alias.capturer).unwrap();
    tick["tables"][0].as_object_mut().unwrap().remove("route");
    tick["tables"][0]["capture_method"] = json!("c5_diff");
    replace_capturer(&mut alias, tick);
    let (admitted, doc) = decision_of(&alias);
    assert!(!admitted);
    assert!(
        codes(&doc).contains(&"CAPTURE_UNKNOWN".to_string()),
        "{doc}"
    );
    assert!(
        doc["model"]["capture_methods"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let mut replay = nz();
    let mut tick = serde_json::from_slice::<Value>(&replay.capturer).unwrap();
    tick["label"] = json!("a1");
    replace_capturer(&mut replay, tick);
    let (admitted, doc) = decision_of(&replay);
    assert!(!admitted);
    assert!(
        codes(&doc).contains(&"SNAPSHOT_MISMATCH".to_string()),
        "{doc}"
    );

    let mut grade = nz();
    let mut tick = serde_json::from_slice::<Value>(&grade.capturer).unwrap();
    tick["tables"][0]["consistency"] = json!("T");
    replace_capturer(&mut grade, tick);
    let (admitted, doc) = decision_of(&grade);
    assert!(!admitted);
    assert!(
        codes(&doc).contains(&"GRADE_CONTRADICTION".to_string()),
        "{doc}"
    );
    assert_eq!(
        doc["model"]["capture_methods"][0]["consistency"],
        json!("T")
    );

    let mut rows = nz();
    let mut tick = serde_json::from_slice::<Value>(&rows.capturer).unwrap();
    tick["tables"][0]["rows"] = json!(["only"]);
    replace_capturer(&mut rows, tick);
    let (admitted, doc) = decision_of(&rows);
    assert!(!admitted);
    assert!(codes(&doc).contains(&"ROW_COUNT".to_string()), "{doc}");

    let mut missing = nz();
    let mut tick = serde_json::from_slice::<Value>(&missing.capturer).unwrap();
    tick.as_object_mut()
        .unwrap()
        .remove("product_reader_write_attempts");
    replace_capturer(&mut missing, tick);
    let (admitted, doc) = decision_of(&missing);
    assert!(!admitted);
    assert!(
        codes(&doc).contains(&"WRITE_UNOBSERVED".to_string()),
        "{doc}"
    );
    assert_eq!(doc["model"]["capture_methods"][0]["method"], json!("C5"));
    assert_eq!(
        doc["model"]["capture_methods"][0]["consistency"],
        json!("S0")
    );

    let mut writes = nz();
    let mut tick = serde_json::from_slice::<Value>(&writes.capturer).unwrap();
    tick["product_reader_write_attempts"] = json!(1);
    replace_capturer(&mut writes, tick);
    let (admitted, doc) = decision_of(&writes);
    assert!(!admitted);
    assert!(codes(&doc).contains(&"WRITE_OBSERVED".to_string()), "{doc}");

    let mut secret = nz();
    let mut tick = serde_json::from_slice::<Value>(&secret.capturer).unwrap();
    tick["note"] = json!("secret://hidden");
    replace_capturer(&mut secret, tick);
    let (admitted, doc) = decision_of(&secret);
    assert!(!admitted);
    assert!(codes(&doc).contains(&"SECRET_LEAK".to_string()));
    assert_public(&doc);

    let mut lie = nz();
    lie.report["capture_methods"] = json!([{
        "method": "C4",
        "write_class": "R0",
        "consistency": "S0",
        "token_order": "None",
        "retention_window": null,
        "cursor": null,
        "feed": null,
        "cdc": null
    }]);
    reload(&mut lie);
    let (admitted, doc) = decision_of(&lie);
    assert!(!admitted);
    assert!(
        codes(&doc).contains(&"CAPTURE_UNTRUSTED".to_string()),
        "{doc}"
    );
    assert_eq!(doc["model"]["capture_methods"][0]["method"], json!("C5"));

    let mut pii = nz();
    let mut schema = serde_json::from_slice::<Value>(&pii.schema_preimage).unwrap();
    schema[0]["columns"][0]["pii"] = json!("unknown");
    pii.schema_preimage = serde_json::to_vec_pretty(&schema).unwrap();
    pii.schema_preimage.push(b'\n');
    reload(&mut pii);
    let (admitted, doc) = decision_of(&pii);
    assert!(!admitted);
    assert!(codes(&doc).contains(&"PII_UNAPPROVED".to_string()), "{doc}");
}

#[test]
fn insert_permission_blocks_and_cli_creates_no_directory() {
    let mut laid = nz();
    laid.report["permissions"] = json!([
        {"name":"SELECT","status":"known","granted":true},
        {"name":"INSERT","status":"known","granted":true}
    ]);
    laid.config["permissions"] = laid.report["permissions"].clone();
    reload(&mut laid);
    let out = laid.dir.join("sealed");
    let (code, stdout, stderr) = run(&[
        "seal-native",
        "--report",
        laid.report_path.to_str().unwrap(),
        "--config",
        laid.config_path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{stderr} {stdout}");
    assert!(!stdout.contains("secret://"), "{stdout}");
    assert!(!stdout.contains("\"secret_ref\""), "{stdout}");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(doc["admitted"], json!(false));
    assert!(codes(&doc).contains(&"PERMISSION_NOT_READONLY".to_string()));
    assert!(!out.exists());
}

#[test]
fn cli_seals_doris_and_rejects_a_witness_trust_bit() {
    let laid = nz();
    let out = laid.dir.join("sealed");
    let (code, stdout, stderr) = run(&[
        "seal-native",
        "--report",
        laid.report_path.to_str().unwrap(),
        "--config",
        laid.config_path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{stderr} {stdout}");
    assert!(out.join("decision.json").is_file());
    assert!(out.join("admission-receipt.json").is_file());
    assert!(out.join("registry.json").is_file());
    assert!(out.join("catalog.json").is_file());
    let decision = fs::read_to_string(out.join("decision.json")).unwrap();
    assert!(!decision.contains("secret://"));
    assert!(!decision.contains("\"secret_ref\""));
    let (again, _, err) = run(&[
        "seal-native",
        "--report",
        laid.report_path.to_str().unwrap(),
        "--config",
        laid.config_path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(again, 1, "{err}");
    assert!(out.join("decision.json").is_file());

    let bad = nz();
    let mut witness: Value = serde_json::from_slice(&bad.witness).unwrap();
    witness["trusted"] = json!(true);
    let witness_path = PathBuf::from(bad.report["capture_witness"].as_str().unwrap());
    write_json(&witness_path, &witness);
    let missing = bad.dir.join("not-created");
    let (code, stdout, stderr) = run(&[
        "seal-native",
        "--report",
        bad.report_path.to_str().unwrap(),
        "--config",
        bad.config_path.to_str().unwrap(),
        "--out",
        missing.to_str().unwrap(),
    ]);
    assert_eq!(code, 1, "{stderr} {stdout}");
    assert!(!missing.exists());
    assert!(!stdout.contains("secret://"));
}

#[test]
fn validate_with_witness_omits_secret_and_legacy_validate_keeps_it() {
    let laid = nz();
    let (code, stdout, stderr) = run(&[
        "validate",
        "--report",
        laid.report_path.to_str().unwrap(),
        "--config",
        laid.config_path.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{stderr} {stdout}");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    assert_public(&doc);
    assert_eq!(doc["model"]["capture_methods"][0]["method"], json!("C5"));

    let (code, stdout, stderr) = run(&[
        "validate",
        "--report",
        manifest()
            .join("fixtures/native/doris-preview-report.json")
            .to_str()
            .unwrap(),
        "--config",
        manifest()
            .join("fixtures/native/doris-preview-config.json")
            .to_str()
            .unwrap(),
    ]);
    assert_eq!(code, 0, "{stderr} {stdout}");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        doc["identity"]["secret_ref"],
        json!("secret://oo-dl01w1-43/reader")
    );
    assert!(
        doc["model"]["capture_methods"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

struct ProducerCase<'a> {
    engine: &'a str,
    compat: &'a str,
    source_id: &'a str,
    generation: i64,
    snapshot_kind: &'a str,
    snapshot_value: &'a str,
    schema: &'a str,
    table: &'a str,
    capturer_file: &'a str,
    permissions: Value,
}

fn producer_laid(case: ProducerCase<'_>) -> Laid {
    let ProducerCase {
        engine,
        compat,
        source_id,
        generation,
        snapshot_kind,
        snapshot_value,
        schema,
        table,
        capturer_file,
        permissions,
    } = case;
    let dir = scratch();
    let arrow_path = dir.join("batch.arrow");
    fs::copy(
        manifest().join("fixtures/native/ns2-nz_obs.ipc"),
        &arrow_path,
    )
    .unwrap();
    let driver = b"producer-driver-fixture\n".to_vec();
    let driver_path = dir.join("reader");
    fs::write(&driver_path, &driver).unwrap();
    let capturer_path = dir.join("tick.json");
    let schema_path = dir.join("schema.json");
    let type_path = dir.join("types.json");
    let witness_path = dir.join("witness.json");
    let report_path = dir.join("report.json");
    let config_path = dir.join("config.json");
    let capturer = fs::read(manifest().join("fixtures/producers").join(capturer_file)).unwrap();
    fs::write(&capturer_path, &capturer).unwrap();
    fs::write(&schema_path, b"[]\n").unwrap();
    fs::write(&type_path, b"[]\n").unwrap();
    let witness = json!({
        "protocol": "oo.source-admission.capture-witness/v1",
        "source_engine": engine,
        "source_id": source_id,
        "source_generation": generation,
        "snapshot": {"kind": snapshot_kind, "value": snapshot_value},
        "run_id": format!("{source_id}-run"),
        "logical_schema": schema,
        "logical_table": table,
        "capturer_path": capturer_path.to_str().unwrap(),
        "arrow_path": arrow_path.to_str().unwrap(),
        "driver_path": driver_path.to_str().unwrap(),
        "schema_preimage_path": schema_path.to_str().unwrap(),
        "type_preimage_path": type_path.to_str().unwrap()
    });
    write_json(&witness_path, &witness);
    let mut report: Value = serde_json::from_slice(
        &fs::read(manifest().join("fixtures/native/doris-preview-report.json")).unwrap(),
    )
    .unwrap();
    report["compat_mode"] = json!(compat);
    report["source_id"] = json!(source_id);
    report["driver"]["kind"] = json!("native");
    report["driver"]["name"] = json!("fixture");
    report["driver"]["version"] = json!("0");
    report["driver"]["digest"] = json!(sha256_hex(&driver));
    report["capture_methods"] = json!([]);
    report["capture_witness"] = json!(witness_path.to_str().unwrap());
    report["permissions"] = permissions.clone();
    let mut config: Value = serde_json::from_slice(
        &fs::read(manifest().join("fixtures/native/doris-preview-config.json")).unwrap(),
    )
    .unwrap();
    config["compat_mode"] = json!(compat);
    config["source_id"] = json!(source_id);
    config["permissions"] = permissions;
    write_json(&report_path, &report);
    write_json(&config_path, &config);
    Laid {
        dir,
        witness: fs::read(&witness_path).unwrap(),
        capturer,
        arrow: fs::read(&arrow_path).unwrap(),
        driver,
        schema_preimage: fs::read(&schema_path).unwrap(),
        type_preimage: fs::read(&type_path).unwrap(),
        report_bytes: fs::read(&report_path).unwrap(),
        config_bytes: fs::read(&config_path).unwrap(),
        report,
        config,
        report_path,
        config_path,
    }
}

fn seal_of(laid: &Laid) -> source_admission::SealResult {
    let (report, config) = parsed(laid);
    let out = laid.dir.join("sealed-out");
    seal_captured(
        &report,
        &config,
        &input(laid),
        &OutputPaths {
            report_path: laid.report_path.to_str().unwrap(),
            config_path: laid.config_path.to_str().unwrap(),
            catalog_path: out.join("catalog.json").to_str().unwrap(),
            receipt_path: out.join("admission-receipt.json").to_str().unwrap(),
        },
    )
    .unwrap()
}

fn select_only() -> Value {
    json!([{"name":"SELECT","status":"known","granted":true}])
}

fn oracle_permissions() -> Value {
    json!([
        {"name":"CREATE SESSION","status":"known","granted":true},
        {"name":"SELECT","status":"known","granted":true,"scope":{"database":"OO04_FIX","table":"T_BATCH"}}
    ])
}

fn oracle_catalog() -> Laid {
    producer_laid(ProducerCase {
        engine: "oracle",
        compat: "oracle",
        source_id: "oo04_t_batch",
        generation: 8,
        snapshot_kind: "token",
        snapshot_value: "oo-dl01w1-04-scn-6464157",
        schema: "OO04_FIX",
        table: "T_BATCH",
        capturer_file: "oracle-c5-producer-catalog.json",
        permissions: oracle_permissions(),
    })
}

#[test]
fn oracle_catalog_derives_c5_s0_and_does_not_admit() {
    let laid = oracle_catalog();
    let (admitted, doc) = decision_of(&laid);
    assert!(!admitted, "{doc}");
    assert!(
        !codes(&doc).contains(&"PROTOCOL_UNSUPPORTED".to_string()),
        "{doc}"
    );
    assert!(
        codes(&doc).contains(&"WRITE_UNOBSERVED".to_string()),
        "{doc}"
    );
    assert!(codes(&doc).contains(&"ARROW_UNBOUND".to_string()), "{doc}");
    assert!(codes(&doc).contains(&"ROW_COUNT".to_string()), "{doc}");
    assert_eq!(doc["model"]["capture_methods"][0]["method"], json!("C5"));
    assert_eq!(
        doc["model"]["capture_methods"][0]["consistency"],
        json!("S0")
    );
    assert_eq!(
        doc["model"]["capture_methods"][0]["delete_detection"],
        json!("reconcile")
    );
    let write_facts: Vec<_> = doc["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["code"] == "WRITE_UNOBSERVED")
        .map(|item| item["fact"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        write_facts,
        vec!["product_reader_write_attempts".to_string()]
    );
    assert_public(&doc);
    let sealed = seal_of(&laid);
    assert!(sealed.bundle.is_none());
    assert!(sealed.refusal.is_none());
}

#[test]
fn oracle_profile_requested_s1_is_not_a_grade() {
    let laid = producer_laid(ProducerCase {
        engine: "oracle",
        compat: "oracle",
        source_id: "oo04_t_batch",
        generation: 8,
        snapshot_kind: "token",
        snapshot_value: "oo-dl01w1-04-scn-6464157",
        schema: "OO04_FIX",
        table: "T_BATCH",
        capturer_file: "oracle-c5-producer-profile.json",
        permissions: oracle_permissions(),
    });
    let (admitted, doc) = decision_of(&laid);
    assert!(!admitted, "{doc}");
    assert!(
        codes(&doc).contains(&"PROTOCOL_UNSUPPORTED".to_string()),
        "{doc}"
    );
    assert!(
        doc["model"]["capture_methods"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!doc["model"].to_string().contains("S1"), "{doc}");
}

#[test]
fn caller_c4_does_not_replace_the_oracle_catalog_method() {
    let mut laid = oracle_catalog();
    laid.report["capture_methods"] = json!([{
        "method": "C4",
        "write_class": "R0",
        "consistency": "T",
        "token_order": "None",
        "retention_window": null,
        "cursor": null,
        "feed": null,
        "cdc": null
    }]);
    reload(&mut laid);
    let (admitted, doc) = decision_of(&laid);
    assert!(!admitted, "{doc}");
    assert!(
        codes(&doc).contains(&"CAPTURE_UNTRUSTED".to_string()),
        "{doc}"
    );
    assert_eq!(doc["model"]["capture_methods"][0]["method"], json!("C5"));
    assert_eq!(
        doc["model"]["capture_methods"][0]["consistency"],
        json!("S0")
    );
}

#[test]
fn deletes_lagged_array_contradicts_grade_t_and_c5_diff_is_unknown() {
    let mut grade = oracle_catalog();
    let mut tick = serde_json::from_slice::<Value>(&grade.capturer).unwrap();
    tick["tables"][0]["consistency_grade"] = json!("T");
    replace_capturer(&mut grade, tick);
    let (admitted, doc) = decision_of(&grade);
    assert!(!admitted, "{doc}");
    assert!(
        codes(&doc).contains(&"GRADE_CONTRADICTION".to_string()),
        "{doc}"
    );
    assert_eq!(
        doc["model"]["capture_methods"][0]["consistency"],
        json!("T")
    );

    let mut alias = oracle_catalog();
    let mut tick = serde_json::from_slice::<Value>(&alias.capturer).unwrap();
    tick["tables"][0]["method"] = json!("c5_diff");
    replace_capturer(&mut alias, tick);
    let (admitted, doc) = decision_of(&alias);
    assert!(!admitted, "{doc}");
    assert!(
        codes(&doc).contains(&"CAPTURE_UNKNOWN".to_string()),
        "{doc}"
    );
    assert!(
        doc["model"]["capture_methods"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn doris_witness_does_not_carry_the_oracle_catalog() {
    let laid = producer_laid(ProducerCase {
        engine: "doris",
        compat: "doris",
        source_id: "dorispreview",
        generation: 2,
        snapshot_kind: "capture-tick",
        snapshot_value: "ns2",
        schema: "oo_dl01w1_43",
        table: "T_BATCH",
        capturer_file: "oracle-c5-producer-catalog.json",
        permissions: select_only(),
    });
    let (admitted, doc) = decision_of(&laid);
    assert!(!admitted, "{doc}");
    assert!(
        codes(&doc).contains(&"PROTOCOL_UNSUPPORTED".to_string()),
        "{doc}"
    );
    assert!(
        codes(&doc).contains(&"ENGINE_MISMATCH".to_string()),
        "{doc}"
    );
    assert!(codes(&doc).contains(&"CROSS_SOURCE".to_string()), "{doc}");
    let sealed = seal_of(&laid);
    assert!(sealed.bundle.is_none());
    assert!(!sealed.decision.admitted);
}

#[test]
fn mariadb_catalog_is_not_a_doris_seal() {
    let laid = producer_laid(ProducerCase {
        engine: "mariadb",
        compat: "mariadb-native",
        source_id: "oo_c5_k_repr",
        generation: 2,
        snapshot_kind: "sha256",
        snapshot_value: "6901360967ed3587175c80e92209c8d4e51e44d2091eba9eb7fe52acbc66a9cf",
        schema: "oo_c5",
        table: "k_repr",
        capturer_file: "mariadb-c5-capture-catalog.json",
        permissions: select_only(),
    });
    let (admitted, doc) = decision_of(&laid);
    assert!(!admitted, "{doc}");
    assert!(
        !codes(&doc).contains(&"PROTOCOL_UNSUPPORTED".to_string()),
        "{doc}"
    );
    assert!(
        !codes(&doc).contains(&"ENGINE_MISMATCH".to_string()),
        "{doc}"
    );
    assert!(
        codes(&doc).contains(&"CAPTURE_UNKNOWN".to_string()),
        "{doc}"
    );
    assert!(
        doc["model"]["capture_methods"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(source_admission::seal_engine("mariadb"));
    let sealed = seal_of(&laid);
    assert!(sealed.bundle.is_none());

    let mut borrowed = lay(
        "mariadb",
        "mariadb-native",
        nz_table(),
        nz_columns(),
        nz_types(),
        "ns2-nz_obs.arrow",
    );
    let mut tick = serde_json::from_slice::<Value>(&borrowed.capturer).unwrap();
    tick["protocol"] = json!("oo.doris.capture-tick/v1");
    replace_capturer(&mut borrowed, tick);
    let (admitted, doc) = decision_of(&borrowed);
    assert!(!admitted, "{doc}");
    assert!(
        codes(&doc).contains(&"PROTOCOL_UNSUPPORTED".to_string()),
        "{doc}"
    );
    let sealed = seal_of(&borrowed);
    assert!(sealed.bundle.is_none());
    assert!(source_admission::seal_engine("mysql"));
    assert!(source_admission::seal_engine("postgresql"));
}

#[test]
fn postgres_check_maps_c1_logical_without_inventing_a_grade() {
    let laid = producer_laid(ProducerCase {
        engine: "postgresql",
        compat: "postgres-native",
        source_id: "pg_ready",
        generation: 1,
        snapshot_kind: "capture-check",
        snapshot_value: "absent",
        schema: "absent",
        table: "absent",
        capturer_file: "postgres-ready-capture-check.json",
        permissions: select_only(),
    });
    let (admitted, doc) = decision_of(&laid);
    assert!(!admitted, "{doc}");
    assert!(
        !codes(&doc).contains(&"PROTOCOL_UNSUPPORTED".to_string()),
        "{doc}"
    );
    assert!(
        codes(&doc).contains(&"CONSISTENCY_UNKNOWN".to_string()),
        "{doc}"
    );
    assert!(codes(&doc).contains(&"OBJECT_UNBOUND".to_string()), "{doc}");
    assert!(
        codes(&doc).contains(&"SNAPSHOT_MISMATCH".to_string()),
        "{doc}"
    );
    assert!(
        codes(&doc).contains(&"WRITE_UNOBSERVED".to_string()),
        "{doc}"
    );
    assert!(
        doc["model"]["capture_methods"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let sealed = seal_of(&laid);
    assert_eq!(sealed.refusal, None);
    assert!(sealed.bundle.is_none());

    let mut cdc = laid;
    let mut tick = serde_json::from_slice::<Value>(&cdc.capturer).unwrap();
    tick["method"] = json!("c1_cdc");
    tick["route"] = json!("c1_cdc");
    replace_capturer(&mut cdc, tick);
    let (admitted, doc) = decision_of(&cdc);
    assert!(!admitted, "{doc}");
    assert!(
        codes(&doc).contains(&"CAPTURE_UNKNOWN".to_string()),
        "{doc}"
    );
    assert!(
        doc["model"]["capture_methods"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn mysql_audit_fixture_observes_zero_writes_and_does_not_admit() {
    let laid = producer_laid(ProducerCase {
        engine: "mysql",
        compat: "mysql-native",
        source_id: "yard",
        generation: 1,
        snapshot_kind: "audit",
        snapshot_value: "absent",
        schema: "yard",
        table: "absent",
        capturer_file: "mysql-customer-audit-receipt.json",
        permissions: select_only(),
    });
    let (admitted, doc) = decision_of(&laid);
    assert!(!admitted, "{doc}");
    assert!(
        !codes(&doc).contains(&"PROTOCOL_UNSUPPORTED".to_string()),
        "{doc}"
    );
    assert!(
        !codes(&doc).contains(&"WRITE_UNOBSERVED".to_string()),
        "{doc}"
    );
    assert!(
        codes(&doc).contains(&"CAPTURE_UNKNOWN".to_string()),
        "{doc}"
    );
    assert!(
        doc["model"]["capture_methods"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let sealed = seal_of(&laid);
    assert_eq!(sealed.refusal, None);
    assert!(sealed.bundle.is_none());

    let mut unread = laid;
    let mut tick = serde_json::from_slice::<Value>(&unread.capturer).unwrap();
    tick["product_read"] = json!("unreadable");
    replace_capturer(&mut unread, tick);
    let (admitted, doc) = decision_of(&unread);
    assert!(!admitted);
    assert!(
        codes(&doc).contains(&"WRITE_UNOBSERVED".to_string()),
        "{doc}"
    );
}

#[test]
fn unequal_table_keys_do_not_bind() {
    let mut laid = nz();
    let mut tick = serde_json::from_slice::<Value>(&laid.capturer).unwrap();
    tick["tables"][0]["table"] = json!("other");
    replace_capturer(&mut laid, tick);
    let (admitted, doc) = decision_of(&laid);
    assert!(!admitted, "{doc}");
    assert!(codes(&doc).contains(&"OBJECT_UNBOUND".to_string()), "{doc}");
}

#[test]
fn registration_recipe_records_the_published_allowlist_without_a_trust_bit() {
    assert!(source_admission::consumer_published(
        "a205eb5797f69d2c95e4e1cc533fce5642be5424df7669fb8a49fef4026eeaf2"
    ));
    assert!(source_admission::consumer_published(
        "ae7dc727181ae101bab84e328d39c38c3fdd6613255f79062404cbc9c8069577"
    ));
    assert!(source_admission::consumer_published(
        "e465241fc1292b6eb5a13cf12efbffe360a9fbf1cbb03c02affb0241a4e58e1f"
    ));
    assert!(!source_admission::consumer_published(
        "4673c8b6e58526c46cac9697b13fd015601992179ac6c38f0e21cf3566acabfe"
    ));
    const DORIS: &str = "d5dc8555ad76ecc9d4550ac5d2abd6b00bb2fee6ca5a17c7c206d294b206010c";
    assert_eq!(
        source_admission::quota_for(DORIS, 115_155_008),
        "approved_driver_resource"
    );
    assert_eq!(
        source_admission::quota_for(DORIS, 115_155_007),
        "exceeds_open_server_registration"
    );
    assert_eq!(
        source_admission::quota_for("ab", 115_155_008),
        "exceeds_open_server_registration"
    );
    assert_eq!(
        source_admission::quota_for("ab", 32 * 1024 * 1024),
        "within_open_server_registration"
    );
    let recipe = source_admission::registration_recipe(b"validator", b"driver");
    let doc: Value = serde_json::from_slice(&source_admission::recipe_json(&recipe)).unwrap();
    assert_eq!(doc["trusted"], json!(false));
    assert_eq!(doc["allowlist_updated"], json!(false));
    assert_eq!(doc["approved_by_consumer"], json!(false));
    assert_eq!(doc["lease_issued"], json!(false));
    assert_eq!(doc["grade_source"], json!("capturer"));
    assert_eq!(doc["self_written_grade_accepted"], json!(false));
    assert_eq!(doc["receipt_registry_sha256"], json!("preimage"));
    assert_eq!(doc["oo_native_binding_registry_sha256"], json!("full_file"));
    assert!(!doc.to_string().contains("secret://"));

    let dir = scratch();
    let validator = dir.join("validator");
    let driver = dir.join("driver");
    fs::write(&validator, b"validator").unwrap();
    fs::write(&driver, b"driver").unwrap();
    let (code, stdout, stderr) = run(&[
        "registration-recipe",
        "--validator",
        validator.to_str().unwrap(),
        "--driver",
        driver.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{stderr} {stdout}");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(doc["approved_by_consumer"], json!(false));
    assert_eq!(
        doc["driver_quota"],
        json!("within_open_server_registration")
    );
    assert_eq!(doc["trusted"], json!(false));
}

fn opensearch_permissions() -> Value {
    json!([
        {"name":"indices:data/read/search","status":"known","granted":true,"scope":{"index":"oo-dl01w1-44-ledger","observed":true}},
        {"name":"indices:data/read/mget","status":"known","granted":false,"scope":{"index":"oo-dl01w1-44-ledger","observed":false}}
    ])
}

fn opensearch_laid() -> Laid {
    let mut laid = producer_laid(ProducerCase {
        engine: "opensearch",
        compat: "opensearch-native",
        source_id: "oo_dl01w1_44",
        generation: 4,
        snapshot_kind: "c5_cycle",
        snapshot_value: "fast-8",
        schema: "ledger",
        table: "ledger_rows",
        capturer_file: "mysql-customer-audit-receipt.json",
        permissions: opensearch_permissions(),
    });
    let root = manifest().join("fixtures/opensearch");
    laid.arrow = fs::read(root.join("batch.arrow")).unwrap();
    laid.schema_preimage = fs::read(root.join("schema-preimage.json")).unwrap();
    laid.type_preimage = fs::read(root.join("type-preimage.json")).unwrap();
    laid.capturer = fs::read(root.join("fast-8.tick.json")).unwrap();
    fs::write(laid.dir.join("batch.arrow"), &laid.arrow).unwrap();
    reload(&mut laid);
    laid
}

#[test]
fn opensearch_index_maps_to_ledger_rows_and_named_arrow_types() {
    let laid = opensearch_laid();
    let (admitted, doc) = decision_of(&laid);
    assert!(admitted, "{doc}");
    assert!(
        !codes(&doc).contains(&"SCHEMA_UNSUPPORTED".to_string()),
        "{doc}"
    );
    assert!(
        !codes(&doc).contains(&"OBJECT_UNAPPROVED".to_string()),
        "{doc}"
    );
    assert_eq!(doc["model"]["capture_methods"][0]["method"], json!("C5"));
    assert_eq!(
        doc["model"]["capture_methods"][0]["consistency"],
        json!("B")
    );
    assert_public(&doc);
    let text = String::from_utf8(laid.type_preimage.clone()).unwrap();
    assert!(
        text.contains("\"approval\":\"not_approved\"")
            || text.contains("\"approval\": \"not_approved\"")
    );
    assert!(!text.contains("where_point"));

    let sealed = seal_of(&laid);
    let bundle = sealed.bundle.expect("opensearch seal");
    assert!(sealed.refusal.is_none());
    let registry: Value = serde_json::from_slice(&bundle.registry).unwrap();
    let fields = registry["bindings"][0]["fields"].as_array().unwrap();
    let kind = |name: &str| {
        fields
            .iter()
            .find(|field| field["name"] == name)
            .unwrap_or_else(|| panic!("missing {name}"))
    };
    assert_eq!(kind("double_value")["arrow_type"], json!("float64"));
    assert!(kind("double_value").get("timezone").is_none());
    assert_eq!(
        kind("event_ms")["arrow_type"],
        json!("timestamp_micros_utc")
    );
    assert_eq!(kind("event_ms")["timezone"], json!("UTC"));
    assert_eq!(kind("tags")["arrow_type"], json!("list_binary"));
    assert_eq!(kind("c5_extra")["arrow_type"], json!("utf8"));
    assert!(kind("c5_extra").get("approved").is_none());
    assert!(fields.iter().all(|field| field["name"] != "where_point"));
    assert_eq!(
        registry["bindings"][0]["objects"][0]["logical_table"],
        json!("ledger_rows")
    );

    let mut other = opensearch_laid();
    let mut tick = serde_json::from_slice::<Value>(&other.capturer).unwrap();
    tick["reader_audit"][0]["path"] = json!("/oo-other-index/_mapping");
    replace_capturer(&mut other, tick);
    let (admitted, doc) = decision_of(&other);
    assert!(!admitted, "{doc}");
    assert!(
        codes(&doc).contains(&"OBJECT_UNAPPROVED".to_string()),
        "{doc}"
    );

    let mut bare = opensearch_laid();
    let mut tick = serde_json::from_slice::<Value>(&bare.capturer).unwrap();
    tick.as_object_mut().unwrap().remove("reader_audit");
    replace_capturer(&mut bare, tick);
    let (admitted, doc) = decision_of(&bare);
    assert!(!admitted, "{doc}");
    assert!(
        codes(&doc).contains(&"OBJECT_UNAPPROVED".to_string()),
        "{doc}"
    );

    let mut leaked = opensearch_laid();
    let mut tick = serde_json::from_slice::<Value>(&leaked.capturer).unwrap();
    tick["unclassified"] = json!([{"name":"double_value","source_type":"double"}]);
    replace_capturer(&mut leaked, tick);
    let (admitted, doc) = decision_of(&leaked);
    assert!(!admitted, "{doc}");
    assert!(
        codes(&doc).contains(&"SCHEMA_UNSUPPORTED".to_string()),
        "{doc}"
    );
}

#[test]
fn actual_arrow_rows_over_configured_budget_are_not_admitted() {
    let mut laid = nz();
    laid.config["budget"]["hard_limit"] = json!(1);
    reload(&mut laid);
    let (admitted, doc) = decision_of(&laid);
    assert!(!admitted, "{doc}");
    assert!(codes(&doc).contains(&"BUDGET_EXCEEDED".to_string()), "{doc}");
    assert!(seal_of(&laid).bundle.is_none());
}
