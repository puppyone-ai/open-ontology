//! `role-pack-bundle` stores one `compile_role_pack_command` result.
//!
//! The visibility object in these tests is fixture data. It is not authentication
//! and it is not a grant. Hash checks use `sha2` and `oo_core::canonical::hash_bytes`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use oo_builder::{compile_role_pack, compile_role_pack_command};
use oo_core::limits::DOCUMENT_MAX_BYTES;

const COMMAND_PROTOCOL: &str = "oo.builder.role-pack-command/v1";
const BUNDLE_PROTOCOL: &str = "oo.builder.role-pack-bundle/v1";
const MARKER: &str = "a214-do-not-echo";
const OWNER: &str = "owner-a177-do-not-copy";
const HIDDEN_TERM: &str = "berth-hidden-a177";
const SAMPLE: &str = "pii-sample-do-not-echo-a177";
const STATUSES: &str = "{\"authentication\":\"not_claimed\",\"authorization\":\"not_claimed\",\"domains\":\"unsupported\",\"file_splitting\":\"unsupported\",\"mount\":\"not_performed\",\"publication\":\"not_claimed\",\"row_security_reproven\":false,\"samples\":\"omitted\",\"search\":\"unsupported\",\"tokens_estimator\":\"none\",\"tokens_status\":\"unsupported\"}";

fn sha256_hex(bytes: &[u8]) -> String {
    let independent = Sha256::digest(bytes);
    let core = oo_core::canonical::hash_bytes(bytes);
    assert!(independent.iter().copied().eq(core));
    independent.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn compiled_command(bytes: &[u8]) -> String {
    let text = std::str::from_utf8(bytes).expect("request utf8");
    match compile_role_pack_command(text) {
        Ok(document) => document,
        Err(err) => panic!("{} {}", err.code, err.message),
    }
}

fn scratch(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("oo-a214-{label}-{}-{n}", std::process::id()));
    fs::create_dir(&dir).expect("temp dir");
    dir
}

fn bundle_cmd(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_role-pack-bundle"))
        .env_clear()
        .args(args)
        .output()
        .expect("spawn role-pack-bundle")
}

fn run_bundle(request: &Path, output: &Path) -> Output {
    bundle_cmd(&[
        "--request",
        request.to_str().expect("request path"),
        "--output",
        output.to_str().expect("output path"),
    ])
}

fn staging_of(dest: &Path) -> PathBuf {
    let name = dest.file_name().expect("file name").to_str().expect("utf8 name");
    dest.parent()
        .expect("parent")
        .join(format!("{name}.role-pack-bundle-staging"))
}

fn names(dir: &Path) -> Vec<String> {
    let mut found = fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    found.sort();
    found
}

fn assert_line(output: &Output, code: i32, line: &str, absent: &[&str]) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr utf8");
    assert_eq!(stderr, format!("{line}\n"));
    assert!(stderr.len() < 400, "stderr length {}", stderr.len());
    for marker in absent {
        assert!(!stderr.contains(marker), "echoed {marker} in {stderr}");
    }
}

fn bundle_line(code: &str, message: &str) -> String {
    format!("role-pack-bundle: {code} {message}")
}

fn own(code: &str, message: &str) -> String {
    bundle_line(&format!("{BUNDLE_PROTOCOL}/{code}"), message)
}

fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => push_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => write_object(out, map),
    }
}

fn write_object(out: &mut String, map: &Map<String, Value>) {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    out.push('{');
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_string(out, key);
        out.push(':');
        write_value(out, &map[*key]);
    }
    out.push('}');
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(ch)));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn none_col(name: &str) -> Value {
    json!({
        "name": name,
        "pii": "none",
        "classification": {"state": "confirmed"}
    })
}

fn command_bytes(effective: &Value, visibility: &Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "visibility": visibility,
        "effective_oos": effective,
        "protocol": COMMAND_PROTOCOL
    }))
    .expect("command json")
}

fn library_pack(effective: &Value, visibility: &Value) -> oo_builder::RolePack {
    let canonical = compile_role_pack(&canonical_json(effective), &canonical_json(visibility))
        .expect("canonical compile");
    let unsorted = compile_role_pack(
        &serde_json::to_string(effective).expect("effective"),
        &serde_json::to_string(visibility).expect("visibility"),
    )
    .expect("unsorted compile");
    assert_eq!(canonical.artifact, unsorted.artifact);
    assert_eq!(canonical.pack_digest, unsorted.pack_digest);
    canonical
}

fn wharf() -> (Value, Value, Value) {
    let mut crate_id = none_col("crate_id");
    crate_id["label"] = json!("箱号");
    crate_id["data_type"] = json!("bigint");
    crate_id["description"] = json!("本季到港重量");
    crate_id["review"] = json!({"state": "approved"});

    let mut mass = none_col("mass_kg");
    mass["label"] = json!("净重");
    mass["unit"] = json!("kg");

    let mut vault = none_col("vault_bin");
    vault["label"] = json!("舱记");
    vault["pii"] = json!("sensitive");

    let mut badge = none_col("badge");
    badge["label"] = json!("工牌");
    badge["pii"] = json!("personal");
    badge["description"] = json!("badge note stays");
    badge["review"] = json!({"state": "approved"});

    let crate_ds = json!({
        "schema_version": "oos/v1",
        "dataset": "wharf.crate",
        "label": "货箱",
        "tier": "file",
        "grain": "one row per crate",
        "primary_key": ["crate_id"],
        "row_count_estimate": 616161,
        "columns": [crate_id, mass, vault, badge]
    });
    let scale = json!({
        "schema_version": "oos/v1",
        "dataset": "wharf.scale",
        "label": "秤台",
        "tier": "file",
        "grain": "one row per scale",
        "primary_key": ["scale_id"],
        "row_count_estimate": 23,
        "columns": [none_col("scale_id"), none_col("reading_kg")]
    });
    let berth = json!({
        "schema_version": "oos/v1",
        "dataset": "pier.berth",
        "label": "泊位单",
        "tier": "file",
        "grain": "one row per berth",
        "primary_key": ["berth_id"],
        "row_count_estimate": 191919,
        "columns": [none_col("berth_id")]
    });
    let effective = json!({
        "protocol": "oo.builder.effective-oos/v1",
        "project": "wharf",
        "ontology_version": "ontology-v1",
        "catalog_version": "catalog-v1",
        "policy": {"id": "pol-wharf", "version": "3"},
        "knowledge": {"id": "know-wharf", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "acme",
            "locale": "zh-CN",
            "include_schemas": ["wharf", "pier"],
            "default_tier_preference": ["file"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        },
        "datasets": [crate_ds, scale, berth],
        "relationships": [{
            "name": "crate_scale",
            "from": {"dataset": "wharf.crate", "columns": ["mass_kg"]},
            "to": {"dataset": "wharf.scale", "columns": ["reading_kg"]},
            "cardinality": "many_to_one",
            "join_hint": "left",
            "provenance": "declared_fk"
        }],
        "metrics": [
            {
                "metric": "kept_mass",
                "label": "净重合计",
                "description": "visible mass",
                "owner": OWNER,
                "certified": true,
                "status": "active",
                "default_anchor": "month_start",
                "sql": "SELECT mass_kg AS total FROM wharf.crate",
                "tests": [{"params": {}, "expect": {"rows": 1}}],
                "control_total": {
                    "as_of": "month_start",
                    "source": "wharf",
                    "sql": "SELECT mass_kg AS control_sum FROM wharf.crate",
                    "tolerance": {"abs": "1"}
                }
            },
            {
                "metric": "hidden_mass",
                "label": "舱记合计",
                "owner": OWNER,
                "certified": true,
                "status": "active",
                "default_anchor": "month_start",
                "sql": "SELECT vault_bin AS hidden_total FROM wharf.crate",
                "tests": [{"params": {}, "expect": {"rows": 1}}],
                "control_total": {
                    "as_of": "month_start",
                    "source": "wharf",
                    "sql": "SELECT vault_bin FROM wharf.crate",
                    "tolerance": {"abs": "1"}
                }
            }
        ],
        "glossary": [
            {
                "term": "净重口径",
                "definition": "visible mass note",
                "maps_to": {"dataset": "wharf.crate"},
                "status": "approved"
            },
            {
                "term": "泊位",
                "definition": HIDDEN_TERM,
                "maps_to": {"dataset": "pier.berth"},
                "status": "approved"
            }
        ],
        "calendar": {
            "schema_version": "oos/v1",
            "business_tz": "Asia/Shanghai",
            "fiscal_year_start_month": 4,
            "week_start": "monday",
            "anchors": ["month_start", "today"],
            "phrases": {
                "本季": {"start": "month_start", "end": "today", "bounds": "[)"},
                "舱记季": {"start": "month_start", "end": "today", "bounds": "[)"}
            }
        }
    });
    let clerk = visibility(
        "ada177",
        "oo_r_clerk",
        json!([{
            "dataset": "wharf.crate",
            "row_security": "rls",
            "global_stats": "denied",
            "columns": ["crate_id", "mass_kg", "badge"]
        }]),
    );
    let audit = visibility(
        "aud177",
        "oo_r_audit",
        json!([{
            "dataset": "wharf.scale",
            "row_security": "none",
            "global_stats": "permitted",
            "columns": ["scale_id", "reading_kg"]
        }]),
    );
    (effective, clerk, audit)
}

fn visibility(principal: &str, role: &str, datasets: Value) -> Value {
    json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": "wharf",
        "principal": principal,
        "role": role,
        "ontology_version": "ontology-v1",
        "catalog_version": "catalog-v1",
        "policy": {"id": "pol-wharf", "version": "3"},
        "knowledge": {"id": "know-wharf", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": datasets,
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    })
}

fn file_texts(pack: &Value) -> String {
    pack["files"]
        .as_array()
        .expect("files")
        .iter()
        .map(|file| file["text"].as_str().expect("text"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn json_string(value: &Value) -> String {
    serde_json::to_string(value).expect("json string")
}

fn expect_snapshot(manifest: &Value) -> String {
    format!(
        "{{\"catalog_version\":{},\"knowledge_id\":{},\"knowledge_version\":{},\"ontology_version\":{},\"policy_id\":{},\"policy_version\":{},\"project\":{},\"role\":{}}}",
        json_string(&manifest["catalog_version"]),
        json_string(&manifest["knowledge"]["id"]),
        json_string(&manifest["knowledge"]["version"]),
        json_string(&manifest["ontology_version"]),
        json_string(&manifest["policy"]["id"]),
        json_string(&manifest["policy"]["version"]),
        json_string(&manifest["project"]),
        json_string(&manifest["role"]),
    )
}

fn assert_bundle(bytes: &[u8], pack: &oo_builder::RolePack, role: &str) {
    let text = std::str::from_utf8(bytes).expect("bundle utf8");
    assert!(!text.ends_with('\n'));
    let manifest = pack.file("manifest.json").expect("manifest");
    let manifest_sha = sha256_hex(manifest.as_bytes());
    let pack_sha = sha256_hex(pack.artifact.as_bytes());
    assert_eq!(manifest_sha, pack.manifest_sha256);
    assert_ne!(pack_sha, pack.pack_digest);
    assert!(text.contains(&format!("\"manifest\":{manifest},\"manifest_length\":{},\"manifest_sha256\":\"{manifest_sha}\"", manifest.len())));
    assert!(text.contains(&format!(
        "\"pack\":{},\"pack_digest\":\"{}\",\"pack_length\":{},\"pack_protocol\":\"oo.builder.role-pack/v1\",\"pack_sha256\":\"{pack_sha}\"",
        pack.artifact,
        pack.pack_digest,
        pack.artifact.len()
    )));
    let manifest_value: Value = serde_json::from_str(manifest).expect("manifest json");
    assert!(text.contains(&format!("\"snapshot\":{}", expect_snapshot(&manifest_value))));
    assert!(text.contains(&format!("\"statuses\":{STATUSES}")));
    let doc: Value = serde_json::from_str(text).expect("bundle json");
    assert_eq!(doc["protocol"], BUNDLE_PROTOCOL);
    assert_eq!(doc["source_protocol"], COMMAND_PROTOCOL);
    assert_eq!(doc["pack_protocol"], "oo.builder.role-pack/v1");
    assert_eq!(doc["compiler"], "oo-builder/role-pack/v1");
    assert_eq!(doc["compiler"], manifest_value["compiler"]);
    assert_eq!(doc["pack_digest"], pack.pack_digest);
    assert_eq!(doc["pack_sha256"], pack_sha);
    assert_eq!(doc["manifest_sha256"], manifest_sha);
    assert_eq!(doc["pack_length"], pack.artifact.len() as u64);
    assert_eq!(doc["manifest_length"], manifest.len() as u64);
    assert_eq!(doc["snapshot"]["role"], role);
    assert_eq!(doc["snapshot"]["project"], manifest_value["project"]);
    assert_eq!(doc["snapshot"]["policy_id"], manifest_value["policy"]["id"]);
    assert_eq!(doc["snapshot"]["policy_version"], manifest_value["policy"]["version"]);
    assert_eq!(doc["snapshot"]["knowledge_id"], manifest_value["knowledge"]["id"]);
    assert_eq!(doc["snapshot"]["knowledge_version"], manifest_value["knowledge"]["version"]);
    assert_eq!(doc["statuses"]["authentication"], "not_claimed");
    assert_eq!(doc["statuses"]["authorization"], "not_claimed");
    assert_eq!(doc["statuses"]["publication"], "not_claimed");
    assert_eq!(doc["statuses"]["mount"], "not_performed");
    assert_eq!(doc["statuses"]["row_security_reproven"], false);
    assert_eq!(doc["statuses"]["tokens_status"], manifest_value["tokens"]["status"]);
    assert_eq!(doc["statuses"]["tokens_estimator"], manifest_value["tokens"]["estimator"]);
    assert!(doc.get("cache_key").is_none());
    assert!(doc.get("cache_key_sha256").is_none());
    assert!(doc.get("principal").is_none());
    assert!(doc["statuses"].get("readme_tokens").is_none());
    assert_eq!(doc["statuses"].as_object().expect("statuses").len(), 11);
    let keys: Vec<&str> = doc.as_object().expect("object").keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "compiler",
            "manifest",
            "manifest_length",
            "manifest_sha256",
            "pack",
            "pack_digest",
            "pack_length",
            "pack_protocol",
            "pack_sha256",
            "protocol",
            "snapshot",
            "source_protocol",
            "statuses",
        ]
    );
}

#[test]
fn independent_sha256_matches_core_hash_bytes() {
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
fn two_roles_splice_compiler_bytes_and_independent_hashes() {
    let (effective, clerk_vis, audit_vis) = wharf();
    let clerk_pack = library_pack(&effective, &clerk_vis);
    let audit_pack = library_pack(&effective, &audit_vis);
    assert_ne!(clerk_pack.artifact, audit_pack.artifact);
    assert_ne!(clerk_pack.pack_digest, audit_pack.pack_digest);
    assert!(clerk_pack.cache_key.contains("principal=ada177"));
    assert!(audit_pack.cache_key.contains("principal=aud177"));
    assert!(!clerk_pack.artifact.contains("ada177"));
    assert!(!audit_pack.artifact.contains("aud177"));
    assert!(clerk_pack.artifact.contains("has no fixed tokenizer"));

    let clerk_request = command_bytes(&effective, &clerk_vis);
    let audit_request = command_bytes(&effective, &audit_vis);
    let clerk_command = compiled_command(&clerk_request);
    let audit_command = compiled_command(&audit_request);
    assert!(clerk_command.contains(&format!("\"artifact\":{}", clerk_pack.artifact)));
    assert!(audit_command.contains(&format!("\"artifact\":{}", audit_pack.artifact)));
    assert!(clerk_command.contains("cache_key"));
    assert!(clerk_command.contains("ada177"));

    let dir = scratch("roles");
    let clerk_input = dir.join("clerk-request.json");
    let audit_input = dir.join("audit-request.json");
    fs::write(&clerk_input, &clerk_request).expect("clerk request");
    fs::write(&audit_input, &audit_request).expect("audit request");
    let clerk_output = dir.join("clerk.json");
    let audit_output = dir.join("audit.json");
    let clerk_run = run_bundle(&clerk_input, &clerk_output);
    let audit_run = run_bundle(&audit_input, &audit_output);
    for output in [&clerk_run, &audit_run] {
        assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
    let clerk_bytes = fs::read(&clerk_output).expect("clerk bundle");
    let audit_bytes = fs::read(&audit_output).expect("audit bundle");
    assert_bundle(&clerk_bytes, &clerk_pack, "oo_r_clerk");
    assert_bundle(&audit_bytes, &audit_pack, "oo_r_audit");
    let clerk_text = String::from_utf8(clerk_bytes).expect("clerk text");
    let audit_text = String::from_utf8(audit_bytes).expect("audit text");
    assert_ne!(
        serde_json::from_str::<Value>(&clerk_text).expect("clerk")["pack_sha256"],
        serde_json::from_str::<Value>(&audit_text).expect("audit")["pack_sha256"]
    );
    assert!(!clerk_text.contains("ada177"));
    assert!(!clerk_text.contains("aud177"));
    assert!(!clerk_text.contains("cache_key"));
    assert!(!clerk_text.contains("principal"));
    assert!(!audit_text.contains("ada177"));
    assert!(!audit_text.contains("aud177"));
    assert!(!audit_text.contains("cache_key"));
    assert!(!audit_text.contains("principal"));
    assert!(!staging_of(&clerk_output).exists());
    assert!(!staging_of(&audit_output).exists());

    let clerk_doc: Value = serde_json::from_str(&clerk_text).expect("clerk json");
    let audit_doc: Value = serde_json::from_str(&audit_text).expect("audit json");
    let clerk_body = file_texts(&clerk_doc["pack"]);
    for legal in [
        "箱号",
        "净重",
        "本季到港重量",
        "badge note stays",
        "Samples are omitted because pii is personal.",
        "SELECT mass_kg AS total FROM wharf.crate",
        "净重口径",
        "本季",
        "one row per crate",
        "Estimated row count is omitted.",
        "did not re-prove row security",
        "not_claimed",
    ] {
        assert!(clerk_body.contains(legal), "missing {legal}");
    }
    for hidden in [
        "vault_bin",
        "舱记",
        "秤台",
        "wharf.scale",
        "pier.berth",
        "泊位单",
        "616161",
        "191919",
        "hidden_mass",
        OWNER,
        HIDDEN_TERM,
        "crate_scale",
        "reading_kg",
        "scale_id",
        "most_common_vals",
        SAMPLE,
    ] {
        assert!(!clerk_text.contains(hidden), "clerk leaked {hidden}");
    }
    let audit_body = file_texts(&audit_doc["pack"]);
    assert!(audit_body.contains("wharf.scale"));
    assert!(audit_body.contains("秤台"));
    assert!(audit_body.contains("estimated rows: 23"));
    assert!(audit_body.contains("not_claimed"));
    for hidden in [
        "wharf.crate",
        "箱号",
        "vault_bin",
        "舱记",
        "badge",
        "工牌",
        "616161",
        "191919",
        "泊位单",
        "pier.berth",
        "kept_mass",
        "hidden_mass",
        OWNER,
        HIDDEN_TERM,
        "本季到港重量",
        "personal",
        SAMPLE,
    ] {
        assert!(!audit_text.contains(hidden), "audit leaked {hidden}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn missing_kind_and_parent_failures_are_static() {
    let (effective, clerk_vis, _) = wharf();
    let dir = scratch("missing");
    let request = dir.join("request.json");
    fs::write(&request, command_bytes(&effective, &clerk_vis)).expect("request");
    let absent = dir.join(format!("{MARKER}-missing.json"));
    let missing = run_bundle(&absent, &dir.join("out.json"));
    assert_line(&missing, 1, &own("io.missing", "the request file is missing"), &[MARKER]);
    assert_eq!(names(&dir), vec!["request.json".to_string()]);

    let folder = dir.join(format!("{MARKER}-dir"));
    fs::create_dir(&folder).expect("dir request");
    let kind = run_bundle(&folder, &dir.join("out.json"));
    assert_line(&kind, 1, &own("io.kind", "the request path is not a regular file"), &[MARKER]);

    let target = dir.join("target.json");
    fs::write(&target, b"keep").expect("target");
    let link = dir.join(format!("{MARKER}-link.json"));
    std::os::unix::fs::symlink(&target, &link).expect("symlink");
    let linked = run_bundle(&link, &dir.join("out.json"));
    assert_line(&linked, 1, &own("io.kind", "the request path is not a regular file"), &[MARKER]);
    assert_eq!(fs::read(&target).expect("target bytes"), b"keep");

    let parent = dir.join("no-parent").join("out.json");
    let no_parent = run_bundle(&request, &parent);
    assert_line(&no_parent, 1, &own("io.parent", "the output parent is not a directory"), &[MARKER]);
    assert!(!dir.join("no-parent").exists());

    let parent_file = dir.join("parent-file");
    fs::write(&parent_file, b"not-dir").expect("parent file");
    let file_parent = run_bundle(&request, &parent_file.join("out.json"));
    assert_line(&file_parent, 1, &own("io.parent", "the output parent is not a directory"), &[]);
    assert_eq!(fs::read(&parent_file).expect("parent bytes"), b"not-dir");

    let real_parent = dir.join("real-parent");
    fs::create_dir(&real_parent).expect("real parent");
    let link_parent = dir.join("link-parent");
    std::os::unix::fs::symlink(&real_parent, &link_parent).expect("parent symlink");
    let followed = run_bundle(&request, &link_parent.join("out.json"));
    assert_line(&followed, 1, &own("io.parent", "the output parent is not a directory"), &[]);
    assert!(names(&real_parent).is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn corrupt_utf8_and_oversize_do_not_echo_or_create_output() {
    let dir = scratch("corrupt");
    let bad = dir.join("bad.json");
    fs::write(&bad, format!(r#"{{"protocol":"{COMMAND_PROTOCOL}","note":"{MARKER}""#)).expect("bad");
    let output = dir.join("out.json");
    let syntax = run_bundle(&bad, &output);
    assert_line(
        &syntax,
        1,
        &bundle_line(
            "oo.builder.role-pack-command/v1/json.syntax",
            "role-pack command JSON is not valid",
        ),
        &[MARKER],
    );
    assert!(!output.exists());
    assert!(!staging_of(&output).exists());

    let utf8 = dir.join("utf8.json");
    fs::write(&utf8, [0xff, 0xfe]).expect("utf8");
    let bytes = run_bundle(&utf8, &output);
    assert_line(&bytes, 1, &own("json.syntax", "the request file is not UTF-8"), &[MARKER]);

    let mut huge = vec![b' '; DOCUMENT_MAX_BYTES + 1];
    huge[..MARKER.len()].copy_from_slice(MARKER.as_bytes());
    let over_path = dir.join("over.json");
    fs::write(&over_path, &huge).expect("over");
    let over = run_bundle(&over_path, &output);
    assert_line(&over, 1, &own("json.budget.bytes", "document exceeds the byte budget"), &[MARKER]);
    assert!(!over.stderr.windows(64).any(|window| window.iter().all(|byte| *byte == b' ')));
    assert_eq!(names(&dir), vec!["bad.json".to_string(), "over.json".to_string(), "utf8.json".to_string()]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn existing_destination_keeps_the_sentinel() {
    let (effective, clerk_vis, _) = wharf();
    let dir = scratch("overwrite");
    let request = dir.join("request.json");
    fs::write(&request, command_bytes(&effective, &clerk_vis)).expect("request");
    let dest = dir.join("out.json");
    fs::write(&dest, b"SENTINEL-a214").expect("sentinel");
    let output = run_bundle(&request, &dest);
    assert_line(&output, 1, &own("io.destination", "the output path already exists"), &[MARKER, "SENTINEL-a214"]);
    assert_eq!(fs::read(&dest).expect("dest"), b"SENTINEL-a214");
    assert!(!staging_of(&dest).exists());

    let real = dir.join("real.json");
    fs::write(&real, b"KEEP-REAL").expect("real");
    let link = dir.join("link.json");
    std::os::unix::fs::symlink(&real, &link).expect("dest symlink");
    let linked = run_bundle(&request, &link);
    assert_line(&linked, 1, &own("io.destination", "the output path already exists"), &[]);
    assert_eq!(fs::read(&real).expect("real bytes"), b"KEEP-REAL");
    assert!(fs::symlink_metadata(&link).expect("link meta").file_type().is_symlink());
    assert!(!staging_of(&link).exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn destination_collision_removes_only_the_owned_staging_file() {
    let (effective, clerk_vis, _) = wharf();
    let dir = scratch("cleanup");
    let request = dir.join("request.json");
    fs::write(&request, command_bytes(&effective, &clerk_vis)).expect("request");
    let dest = dir.join("out.json");
    fs::write(&dest, b"SENTINEL-a214").expect("sentinel");
    fs::write(dir.join("foreign.txt"), b"FOREIGN-a214").expect("foreign");
    fs::create_dir(dir.join("neighbor")).expect("neighbor");
    fs::write(dir.join("neighbor").join("child.txt"), b"CHILD-a214").expect("child");
    std::thread::sleep(Duration::from_secs(2));
    let before = fs::metadata(&dir).expect("mtime").modified().expect("modified");
    let output = run_bundle(&request, &dest);
    let after = fs::metadata(&dir).expect("mtime").modified().expect("modified");
    assert_line(&output, 1, &own("io.destination", "the output path already exists"), &[MARKER]);
    assert!(after > before, "directory mtime did not move: {before:?} -> {after:?}");
    assert_eq!(fs::read(&dest).expect("dest"), b"SENTINEL-a214");
    assert_eq!(fs::read(dir.join("foreign.txt")).expect("foreign"), b"FOREIGN-a214");
    assert_eq!(fs::read(dir.join("neighbor").join("child.txt")).expect("child"), b"CHILD-a214");
    assert!(!staging_of(&dest).exists());
    assert!(dir.join("neighbor").is_dir());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn preexisting_staging_and_long_name_are_left_untouched() {
    let (effective, clerk_vis, _) = wharf();
    let dir = scratch("collision");
    let request = dir.join("request.json");
    fs::write(&request, command_bytes(&effective, &clerk_vis)).expect("request");
    let dest = dir.join("out.json");
    let staging = staging_of(&dest);
    fs::write(&staging, format!("KEEP-{MARKER}")).expect("staging");
    let foreign = dir.join("foreign.txt");
    fs::write(&foreign, b"FOREIGN").expect("foreign");
    let output = run_bundle(&request, &dest);
    assert_line(&output, 1, &own("io.collision", "the staging path already exists"), &[MARKER]);
    assert_eq!(fs::read_to_string(&staging).expect("staging bytes"), format!("KEEP-{MARKER}"));
    assert!(!dest.exists());
    assert_eq!(fs::read(&foreign).expect("foreign"), b"FOREIGN");

    let staging_dir = dir.join("held.json.role-pack-bundle-staging");
    fs::create_dir(&staging_dir).expect("staging dir");
    fs::write(staging_dir.join("child.txt"), b"KEEP-DIR").expect("child");
    let held = run_bundle(&request, &dir.join("held.json"));
    assert_line(&held, 1, &own("io.collision", "the staging path already exists"), &[]);
    assert_eq!(fs::read(staging_dir.join("child.txt")).expect("child"), b"KEEP-DIR");
    assert!(!dir.join("held.json").exists());

    let long_name = "n".repeat(231);
    let long_out = dir.join(&long_name);
    let long = bundle_cmd(&[
        "--request",
        request.to_str().expect("request"),
        "--output",
        long_out.to_str().expect("long"),
    ]);
    assert_line(&long, 1, &own("io.name", "the output file name does not fit the staging suffix"), &[]);
    assert!(!long_out.exists());

    let slash = format!("{}/", dest.display());
    let slashed = bundle_cmd(&[
        "--request",
        request.to_str().expect("request"),
        "--output",
        &slash,
    ]);
    assert_line(&slashed, 1, &own("io.name", "the output file name does not fit the staging suffix"), &[]);
    assert_eq!(fs::read_to_string(&staging).expect("staging still"), format!("KEEP-{MARKER}"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn help_and_usage_do_not_touch_files() {
    let dir = scratch("usage");
    let help = Command::new(env!("CARGO_BIN_EXE_role-pack-bundle"))
        .env_clear()
        .current_dir(&dir)
        .arg("--help")
        .output()
        .expect("help");
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    let stdout = String::from_utf8(help.stdout).expect("help utf8");
    for phrase in [
        "compile_role_pack_command",
        "caller-supplied public data",
        "does not authenticate, grant, mount, tokenize, trust a cache, or publish",
        "trusted path, not a jail",
        "create_new",
        "rename",
        "staging file created by this attempt is removed",
    ] {
        assert!(stdout.contains(phrase), "missing {phrase}");
    }
    let short = bundle_cmd(&["-h"]);
    assert_eq!(short.status.code(), Some(0));
    assert!(short.stderr.is_empty());
    assert_eq!(String::from_utf8(short.stdout).expect("short"), stdout);
    assert!(names(&dir).is_empty());

    let usage = own("usage", "arguments are --request, --output, or --help");
    let cases = [
        vec!["--request".to_string()],
        vec![format!("--request={MARKER}")],
        vec!["--help".to_string(), "--request".to_string(), MARKER.to_string()],
        vec![
            "--request".to_string(),
            "request.json".to_string(),
            "--output".to_string(),
            "out.json".to_string(),
            MARKER.to_string(),
        ],
        vec![
            "--request".to_string(),
            "a.json".to_string(),
            "--request".to_string(),
            "b.json".to_string(),
            "--output".to_string(),
            "out.json".to_string(),
        ],
    ];
    for args in cases {
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = bundle_cmd(&borrowed);
        assert_line(&output, 2, &usage, &[MARKER]);
    }
    assert!(names(&dir).is_empty());
    let _ = fs::remove_dir_all(&dir);
}
