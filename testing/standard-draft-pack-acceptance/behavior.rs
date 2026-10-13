use super::write_sealed_pack;

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn fresh_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../evidence/eef831aa-dba4-4af2-a78b-541bc9e0a411/SP11/minimal-product-loop/linux/fresh-copy",
    )
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("json")).expect("json")
}

fn binding() -> Value {
    json!({
        "import_hash": "f7c21ea98c20b88b8a7986da37f77772f9d8874f2efc055b48afdb0fef31aed1",
        "snapshot_id": "0000000D-0000000C-1",
        "source_id": "a407bulk",
        "import_generation": 2,
        "manifest_sha256": "77adb990b8aac4ec42ab2e328e7b38cf5cef5c103efc6b13dd7168d1306debc7",
        "target_config_sha256": "379615f4f792d2afa6ecceb851355edc309f1473162f5f6ea7ccdf835c42363d"
    })
}

fn tables() -> Value {
    json!([
        {"schema": "public", "name": "batch_a", "rows": 2000},
        {"schema": "public", "name": "batch_b", "rows": 8000},
        {"schema": "public", "name": "batch_c", "rows": 90000}
    ])
}

fn real_input() -> Value {
    let dir = fresh_dir();
    json!({
        "protocol": "oo.minimal-product-loop.pack-input/v1",
        "catalog": read_json(&dir.join("catalog.json")),
        "profile": read_json(&dir.join("profile.json")),
        "generation": read_json(&dir.join("generation.json")),
        "binding": binding(),
        "tables": tables()
    })
}

fn work_dir(env_name: &str, label: &str) -> (PathBuf, bool) {
    if let Ok(path) = std::env::var(env_name) {
        let path = PathBuf::from(path);
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("dir");
        return (path, true);
    }
    let path = std::env::temp_dir().join(format!("oo-a418-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("dir");
    (path, false)
}

fn finish(path: &Path, keep: bool) {
    if !keep {
        let _ = fs::remove_dir_all(path);
    }
}

fn write_receipt(env_name: &str, receipt: &Value) {
    if let Ok(path) = std::env::var(env_name) {
        let mut text = serde_json::to_string_pretty(receipt).expect("receipt");
        text.push('\n');
        fs::write(path, text).expect("receipt");
    }
}

fn rehash(generation: &mut Value) {
    for call in generation["calls"].as_array_mut().expect("calls") {
        let hash = pg_builder::canonical_sha256(&call["candidates"]);
        call["candidate_sha256"] = json!(hash);
    }
    let mut fields = Vec::new();
    let mut tables = Vec::new();
    for call in generation["calls"].as_array().expect("calls") {
        fields.extend(call["candidates"]["field"].as_array().expect("field").iter().cloned());
        tables.push(call["candidates"]["table"][0].clone());
    }
    let candidates = json!({"field": fields, "table": tables});
    let hash = pg_builder::canonical_sha256(&candidates);
    generation["candidates"] = candidates;
    generation["candidate_sha256"] = json!(hash);
}

fn rewrite_table_text(call: &mut Value, key: &str, text: &str) {
    call["candidates"]["table"][0][key] = json!(text);
    call["table_outcome"]["candidates"][0][key] = json!(text);
    let mut command: Value = serde_json::from_str(call["complete_command"].as_str().expect("command")).expect("command");
    let mut body: Value = serde_json::from_str(command["captured_response"]["body"].as_str().expect("body")).expect("body");
    body["candidates"][0][key] = json!(text);
    let encoded = serde_json::to_string(&body).expect("body");
    command["captured_response"]["byte_len"] = json!(encoded.len());
    command["captured_response"]["body"] = json!(encoded);
    call["complete_command"] = json!(serde_json::to_string(&command).expect("command"));
}

#[test]
fn sealed_pack_writes_native_shaped_unreviewed_files() {
    let (dir, keep) = work_dir("OO_PACK_OUT", "real");
    let input = real_input();
    let receipt = write_sealed_pack(&dir, &input).expect("sealed");
    write_receipt("OO_PACK_RECEIPT", &receipt);
    assert_eq!(receipt["protocol"], json!("oo.minimal-product-loop.pack-receipt/v1"));
    assert_eq!(receipt["publication"], json!("not_published"));
    assert_eq!(receipt["review"], json!("unreviewed"));
    assert_eq!(receipt["human_approval"], json!(false));
    assert_eq!(receipt["git_publication"], json!("not_published"));
    assert_eq!(receipt["model_id"], json!("grok-4.7"));
    assert_eq!(receipt["schema_hash"], json!("e49d1ee4baf3867404f9ce7a584d0bab4e3f9717768ed17b9893221b4ef08ee6"));
    assert_eq!(receipt["generation_candidate_sha256"], json!("7a08b477d1443db4f08adc4e2a81bfc6775098da1debbd232a86d7c30f5bf11e"));
    assert_eq!(receipt["binding"]["import_hash"], binding()["import_hash"]);
    assert_eq!(receipt["binding"]["manifest_sha256"], binding()["manifest_sha256"]);
    assert_ne!(receipt["files"]["manifest"]["sha256"], receipt["binding"]["manifest_sha256"]);
    assert_eq!(receipt["foreign_keys"]["local_copy_count"], json!(0));
    assert_eq!(receipt["foreign_keys"]["original_source_metadata"], json!("unknown"));
    let warnings = receipt["warnings"].as_array().expect("warnings");
    assert!(warnings.iter().any(|item| item.as_str() == Some("physical_not_supplied:public.batch_a")));
    assert!(!warnings.iter().any(|item| item.as_str().unwrap_or("").starts_with("profile_reltuples_differs:")));
    let ontology = fs::read_to_string(dir.join("ontology/ontology.yaml")).expect("ontology");
    assert!(ontology.starts_with("spec_version: oos/v1\n"));
    assert!(!ontology.starts_with('{'));
    assert!(ontology.contains("name: a407bulk\n"));
    assert!(ontology.contains("include_schemas: [public]\n"));
    assert!(ontology.contains("auto_accept: not_authorized\n"));
    assert!(ontology.contains("include_unreviewed: []\n"));
    assert!(!ontology.contains("publication:"));
    assert!(!ontology.contains("review_state:"));
    assert!(!ontology.contains("llm_generation:"));
    let batch_a = fs::read_to_string(dir.join("ontology/generated/datasets/public/batch_a.yaml")).expect("a");
    assert!(batch_a.starts_with("schema_version: oos/v1\n"));
    assert!(batch_a.contains("label: \"Batch A\"\n"));
    assert!(batch_a.contains("provenance: generated\n"));
    assert!(batch_a.contains("row_count_estimate: 2000\n"));
    assert!(!batch_a.contains("review:"));
    assert!(!batch_a.contains("pii:"));
    assert!(!batch_a.contains("semantic_type:"));
    let batch_b = fs::read_to_string(dir.join("ontology/generated/datasets/public/batch_b.yaml")).expect("b");
    assert!(batch_b.contains("name: \"at_time\"\n"));
    assert!(batch_b.contains("name: \"blob\"\n"));
    assert!(batch_b.contains("data_type: time\n"));
    assert!(batch_b.contains("data_type: bytea\n"));
    let readme = fs::read_to_string(dir.join("README.md")).expect("readme");
    assert!(readme.starts_with("# DRAFT — unreviewed — not published\n"));
    assert!(readme.contains("original_source_metadata: unknown"));
    assert!(!readme.contains("declared_foreign_key"));
    assert!(readme.contains("ontology/generated/datasets/public/batch_c.yaml"));
    let review = fs::read_to_string(dir.join("ontology/reviews/README.md")).expect("review");
    assert_eq!(review, "No authenticated review. No Git publication. LLM descriptions are unreviewed drafts.\n");
    let candidate = fs::read_to_string(dir.join("ontology/generated/candidates/public/batch_a.json")).expect("candidate");
    assert!(candidate.contains("\"review\": \"unreviewed\""));
    assert!(candidate.contains("\"label\": \"Batch A\""));
    assert!(candidate.contains("\"effect\": \"review_only\""));
    assert!(!candidate.contains("auto_accepted"));
    let keys = fs::read_to_string(dir.join("ontology/generated/evidence/foreign-keys.json")).expect("keys");
    assert!(keys.contains("\"original_source_metadata\": \"unknown\""));
    assert!(keys.contains("\"local_copy_count\": 0"));
    let manifest_hash = pg_builder::sha256_hex(&fs::read(dir.join("ontology/ontology.yaml")).expect("bytes"));
    assert_eq!(receipt["files"]["manifest"]["sha256"], json!(manifest_hash));
    assert_eq!(receipt["files"]["manifest"]["path"], json!("ontology/ontology.yaml"));
    finish(&dir, keep);
}

#[test]
fn unicode_and_quoted_labels_come_from_the_validated_candidate() {
    let (dir, keep) = work_dir("OO_PACK_UNICODE", "unicode");
    let mut input = real_input();
    let label = "金额 \"客户\"";
    {
        let call = &mut input["generation"]["calls"][0];
        rewrite_table_text(call, "label", label);
        call["candidates"]["field"][2]["label"] = json!(label);
    }
    rehash(&mut input["generation"]);
    let receipt = write_sealed_pack(&dir, &input).expect("unicode");
    write_receipt("OO_PACK_UNICODE_RECEIPT", &receipt);
    let batch_a = fs::read_to_string(dir.join("ontology/generated/datasets/public/batch_a.yaml")).expect("a");
    assert!(batch_a.contains("金额"));
    assert!(batch_a.contains("\\\"客户\\\""));
    assert!(!batch_a.contains("Batch A"));
    let candidate = fs::read_to_string(dir.join("ontology/generated/candidates/public/batch_a.json")).expect("candidate");
    assert!(candidate.contains("金额"));
    assert!(candidate.contains("客户"));
    assert_ne!(
        receipt["generation_candidate_sha256"],
        json!("7a08b477d1443db4f08adc4e2a81bfc6775098da1debbd232a86d7c30f5bf11e")
    );
    finish(&dir, keep);
}

#[test]
fn missing_binding_identifiers_and_collisions_are_rejected() {
    let (dir, _) = work_dir("", "reject");
    let mut missing = real_input();
    missing["binding"].as_object_mut().expect("binding").remove("import_hash");
    assert_eq!(write_sealed_pack(&dir, &missing), Err("pack.input"));
    assert!(!dir.join("README.md").exists());
    let mut extra = real_input();
    extra["password"] = json!("no");
    assert_eq!(write_sealed_pack(&dir, &extra), Err("pack.input"));
    let mut duplicate = real_input();
    duplicate["tables"] = json!([
        {"schema": "public", "name": "batch_a", "rows": 2000},
        {"schema": "public", "name": "batch_a", "rows": 2000}
    ]);
    assert_eq!(write_sealed_pack(&dir, &duplicate), Err("pack.collision"));
    let mut collided = real_input();
    collided["tables"] = json!([
        {"schema": "public", "name": "batch_a", "rows": 2000},
        {"schema": "public", "name": "Batch_A", "rows": 2000}
    ]);
    assert_eq!(write_sealed_pack(&dir, &collided), Err("pack.collision"));
    let mut cafe = real_input();
    cafe["tables"][0]["name"] = json!("café");
    assert_eq!(write_sealed_pack(&dir, &cafe), Err("pack.identifier"));
    let mut spaced = real_input();
    spaced["tables"][0]["name"] = json!("batch a");
    assert_eq!(write_sealed_pack(&dir, &spaced), Err("pack.identifier"));
    let mut sampled = real_input();
    sampled["profile"]["sample_policy"] = json!("rows");
    assert_eq!(write_sealed_pack(&dir, &sampled), Err("pack.input"));
    let mut draft = real_input();
    draft["generation"]["outcome"] = json!("structural_draft");
    assert_eq!(write_sealed_pack(&dir, &draft), Err("pack.input"));
    finish(&dir, false);
}

#[test]
fn tampered_generation_text_is_not_replaced_with_a_handwritten_label() {
    let (dir, _) = work_dir("", "tamper");
    let mut hashed = real_input();
    hashed["generation"]["candidate_sha256"] = json!("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
    assert_eq!(write_sealed_pack(&dir, &hashed), Err("pack.generation"));
    let mut published = real_input();
    published["generation"]["statements"]["publication"] = json!("available");
    assert_eq!(write_sealed_pack(&dir, &published), Err("pack.generation"));
    let mut selected = real_input();
    rewrite_table_text(&mut selected["generation"]["calls"][0], "description", "SELECT hidden");
    rehash(&mut selected["generation"]);
    assert_eq!(write_sealed_pack(&dir, &selected), Err("pack.generation"));
    assert!(!dir.join("ontology/generated/datasets/public/batch_a.yaml").exists());
    let mut disagreed = real_input();
    disagreed["tables"][1]["columns"] = json!([{"name": "blob", "admitted": true}]);
    assert_eq!(write_sealed_pack(&dir, &disagreed), Err("pack.input"));
    let mut half = real_input();
    half["tables"][0]["physical_schema"] = json!("_oo");
    assert_eq!(write_sealed_pack(&dir, &half), Err("pack.input"));
    finish(&dir, false);
}

#[test]
fn row_mismatch_is_a_warning_and_curated_bytes_stay() {
    let (dir, _) = work_dir("", "curated");
    fs::create_dir_all(dir.join("ontology/curated/notes")).expect("curated");
    fs::create_dir_all(dir.join("ontology/reviews")).expect("reviews");
    fs::create_dir_all(dir.join("ontology/generated/datasets/public")).expect("generated");
    let curated = b"keep\xff\n";
    fs::write(dir.join("ontology/curated/notes/keep.bin"), curated).expect("keep");
    fs::write(dir.join("ontology/reviews/decisions.yaml"), b"operator-note\n").expect("decisions");
    fs::write(dir.join("ontology/generated/datasets/public/stale.yaml"), b"keep-stale\n").expect("stale");
    let mut input = real_input();
    input["tables"][0]["rows"] = json!(2001);
    input["tables"][0]["physical_schema"] = json!("_oo");
    input["tables"][0]["physical_table"] = json!("pt_a");
    let receipt = write_sealed_pack(&dir, &input).expect("sealed");
    assert_eq!(fs::read(dir.join("ontology/curated/notes/keep.bin")).expect("keep"), curated);
    assert_eq!(fs::read(dir.join("ontology/reviews/decisions.yaml")).expect("decisions"), b"operator-note\n");
    assert_eq!(fs::read(dir.join("ontology/generated/datasets/public/stale.yaml")).expect("stale"), b"keep-stale\n");
    let warnings = receipt["warnings"].as_array().expect("warnings");
    assert!(warnings.iter().any(|item| item.as_str() == Some("profile_reltuples_differs:public.batch_a")));
    assert!(warnings.iter().any(|item| item.as_str() == Some("unexpected_generated_dataset:ontology/generated/datasets/public/stale.yaml")));
    assert!(!warnings.iter().any(|item| item.as_str() == Some("physical_not_supplied:public.batch_a")));
    assert!(warnings.iter().any(|item| item.as_str() == Some("physical_not_supplied:public.batch_b")));
    let batch_a = fs::read_to_string(dir.join("ontology/generated/datasets/public/batch_a.yaml")).expect("a");
    assert!(batch_a.contains("row_count_estimate: 2000\n"));
    let markdown = fs::read_to_string(dir.join("tables/public.batch_a.md")).expect("md");
    assert!(markdown.contains("sealed_rows: 2001\n"));
    assert!(markdown.contains("physical: _oo.pt_a\n"));
    assert!(markdown.contains("original_source_key_metadata: unknown"));
    finish(&dir, false);
}

#[test]
fn a_curated_symlink_is_refused() {
    let (dir, _) = work_dir("", "symlink");
    fs::create_dir_all(dir.join("ontology/curated")).expect("curated");
    std::os::unix::fs::symlink("keep.bin", dir.join("ontology/curated/link")).expect("link");
    assert_eq!(write_sealed_pack(&dir, &real_input()), Err("pack.curated"));
    assert!(!dir.join("ontology/ontology.yaml").exists());
    finish(&dir, false);
}
