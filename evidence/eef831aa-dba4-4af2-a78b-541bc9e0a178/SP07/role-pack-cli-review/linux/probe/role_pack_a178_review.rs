//! Independent review of `oo-builder role-pack` on the af3308c3 archive.
//!
//! The binary is the subject. `compile_role_pack` is the accepted library.
//! Digests use `sha2` 0.10.9 directly. This file does not call the library
//! hash helper and does not re-run the a176 closure campaign.

#![forbid(unsafe_code)]

use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use oo_builder::compile_role_pack;
use oo_core::limits::{DOCUMENT_MAX_BYTES, JSON_MAX_DEPTH, JSON_MAX_NODES};

const COMMAND_PROTOCOL: &str = "oo.builder.role-pack-command/v1";
const MARKER: &str = "a178-do-not-echo";
const OWNER: &str = "owner-a178-do-not-copy";
const SAMPLE: &str = "pii-sample-do-not-echo-a178";
const STALE: &str = "stale-a178-secret";
const HIDDEN_TERM: &str = "slip-hidden-a178";
const COMPILER_MARK: &str = "compiler-a178-do-not-echo";

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oo-builder"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn run_in(dir: Option<&std::path::Path>, args: &[&str], stdin: &[u8]) -> Output {
    let mut command = bin();
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    let mut child = command.spawn().expect("spawn oo-builder");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin)
        .expect("write stdin");
    child.wait_with_output().expect("wait")
}

fn run(args: &[&str], stdin: &[u8]) -> Output {
    run_in(None, args, stdin)
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

fn out_root() -> Option<std::path::PathBuf> {
    std::env::var_os("OO_A178_OUT").map(std::path::PathBuf::from)
}

fn dump(name: &str, bytes: &[u8]) {
    let Some(root) = out_root() else {
        return;
    };
    std::fs::create_dir_all(&root).expect("evidence out");
    std::fs::write(root.join(name), bytes).expect("write evidence");
}

fn none_col(name: &str) -> Value {
    json!({
        "name": name,
        "pii": "none",
        "classification": {"state": "confirmed"}
    })
}

fn command_stdin(effective: &Value, visibility: &Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "visibility": visibility,
        "effective_oos": effective,
        "protocol": COMMAND_PROTOCOL
    }))
    .expect("command json")
}

fn library_pack(effective: &Value, visibility: &Value) -> oo_builder::RolePack {
    let canonical = match compile_role_pack(&canonical_json(effective), &canonical_json(visibility))
    {
        Ok(pack) => pack,
        Err(err) => {
            dump(
                "library.err",
                format!("{} {}", err.code, err.message).as_bytes(),
            );
            panic!("{} {}", err.code, err.message);
        }
    };
    let unsorted = compile_role_pack(
        &serde_json::to_string(effective).expect("effective"),
        &serde_json::to_string(visibility).expect("visibility"),
    )
    .expect("unsorted library compile");
    assert_eq!(canonical.artifact, unsorted.artifact);
    assert_eq!(canonical.pack_digest, unsorted.pack_digest);
    assert_eq!(canonical.cache_key, unsorted.cache_key);
    canonical
}

fn yard() -> (Value, Value, Value, Value) {
    let mut bale_id = none_col("bale_id");
    bale_id["label"] = json!("包号");
    bale_id["data_type"] = json!("bigint");
    bale_id["description"] = json!("本季到港重量");
    bale_id["review"] = json!({"state": "approved"});

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

    let bale = json!({
        "schema_version": "oos/v1",
        "dataset": "yard.bale",
        "label": "货包",
        "tier": "file",
        "grain": "one row per bale",
        "primary_key": ["bale_id"],
        "row_count_estimate": 717171,
        "columns": [bale_id, mass, vault, badge]
    });
    let scale = json!({
        "schema_version": "oos/v1",
        "dataset": "yard.scale",
        "label": "秤台",
        "tier": "file",
        "grain": "one row per scale",
        "primary_key": ["scale_id"],
        "row_count_estimate": 41,
        "columns": [none_col("scale_id"), none_col("reading_kg")]
    });
    let slip = json!({
        "schema_version": "oos/v1",
        "dataset": "pier.slip",
        "label": "泊位单",
        "tier": "file",
        "grain": "one row per slip",
        "primary_key": ["slip_id"],
        "row_count_estimate": 292929,
        "columns": [none_col("slip_id")]
    });
    let effective = json!({
        "protocol": "oo.builder.effective-oos/v1",
        "project": "yard",
        "ontology_version": "ontology-v1",
        "catalog_version": "catalog-v1",
        "policy": {"id": "pol-yard", "version": "3"},
        "knowledge": {"id": "know-yard", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "yard",
            "locale": "zh-CN",
            "include_schemas": ["yard", "pier"],
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
        "datasets": [bale, scale, slip],
        "relationships": [{
            "name": "bale_scale",
            "from": {"dataset": "yard.bale", "columns": ["mass_kg"]},
            "to": {"dataset": "yard.scale", "columns": ["reading_kg"]},
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
                "sql": "SELECT mass_kg AS total FROM yard.bale",
                "tests": [{"params": {}, "expect": {"rows": 1}}],
                "control_total": {
                    "as_of": "month_start",
                    "source": "yard",
                    "sql": "SELECT mass_kg AS control_sum FROM yard.bale",
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
                "sql": "SELECT vault_bin AS hidden_total FROM yard.bale",
                "tests": [{"params": {}, "expect": {"rows": 1}}],
                "control_total": {
                    "as_of": "month_start",
                    "source": "yard",
                    "sql": "SELECT vault_bin FROM yard.bale",
                    "tolerance": {"abs": "1"}
                }
            }
        ],
        "glossary": [
            {
                "term": "净重口径",
                "definition": "visible mass note",
                "maps_to": {"dataset": "yard.bale"},
                "status": "approved"
            },
            {
                "term": "泊位",
                "definition": HIDDEN_TERM,
                "maps_to": {"dataset": "pier.slip"},
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
        "ada178",
        "oo_r_clerk",
        json!([{
            "dataset": "yard.bale",
            "row_security": "rls",
            "global_stats": "denied",
            "columns": ["bale_id", "mass_kg", "badge"]
        }]),
    );
    let ada2 = visibility(
        "ada178b",
        "oo_r_clerk",
        json!([{
            "dataset": "yard.bale",
            "row_security": "rls",
            "global_stats": "denied",
            "columns": ["bale_id", "mass_kg", "badge"]
        }]),
    );
    let audit = visibility(
        "aud178",
        "oo_r_audit",
        json!([{
            "dataset": "yard.scale",
            "row_security": "none",
            "global_stats": "permitted",
            "columns": ["scale_id", "reading_kg"]
        }]),
    );
    (effective, clerk, ada2, audit)
}

fn visibility(principal: &str, role: &str, datasets: Value) -> Value {
    json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": "yard",
        "principal": principal,
        "role": role,
        "ontology_version": "ontology-v1",
        "catalog_version": "catalog-v1",
        "policy": {"id": "pol-yard", "version": "3"},
        "knowledge": {"id": "know-yard", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": datasets,
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    })
}

fn file_texts(doc: &Value) -> String {
    doc["artifact"]["files"]
        .as_array()
        .expect("files")
        .iter()
        .map(|file| file["text"].as_str().expect("text"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn write_role(role: &str, stdout: &str, doc: &Value) {
    let Some(root) = out_root() else {
        return;
    };
    let dir = root.join(role);
    std::fs::create_dir_all(&dir).expect("role dir");
    std::fs::write(dir.join("stdout.json"), stdout.as_bytes()).expect("stdout");
    if let Some(text) = doc["pack_digest"].as_str() {
        std::fs::write(dir.join("pack_digest.txt"), text).expect("digest");
    }
    if let Some(text) = doc["cache_key"].as_str() {
        std::fs::write(dir.join("cache_key.txt"), text).expect("cache");
    }
    if let Some(text) = doc["manifest_sha256"].as_str() {
        std::fs::write(dir.join("manifest_sha256.txt"), text).expect("manifest hash");
    }
    let files = dir.join("files");
    for file in doc["artifact"]["files"].as_array().expect("files") {
        let path = file["path"].as_str().expect("path");
        if path.starts_with('/') || path.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
            continue;
        }
        let dest = files.join(path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).expect("file parent");
        }
        std::fs::write(dest, file["text"].as_str().expect("text")).expect("file text");
    }
}

fn independent_hash(doc: &Value, pack: &oo_builder::RolePack) -> Vec<String> {
    let mut violations = Vec::new();
    let manifest_text = match pack.file("manifest.json") {
        Ok(text) => text,
        Err(err) => {
            violations.push(format!("manifest file {}", err.code));
            return violations;
        }
    };
    if sha256_hex(manifest_text.as_bytes()) != pack.manifest_sha256 {
        violations.push("manifest_sha256".to_owned());
    }
    if sha256_hex(pack.cache_key.as_bytes()) != pack.cache_key_sha256 {
        violations.push("cache_key_sha256".to_owned());
    }
    if canonical_json(&doc["manifest"]) != manifest_text {
        violations.push("stdout manifest bytes".to_owned());
    }
    if canonical_json(&doc["artifact"]) != pack.artifact {
        violations.push("stdout artifact bytes".to_owned());
    }
    let mut rows = Vec::new();
    let files = doc["artifact"]["files"].as_array().cloned().unwrap_or_default();
    for file in &files {
        let path = file["path"].as_str().unwrap_or("");
        let text = file["text"].as_str().unwrap_or("");
        let stored = pack.file(path).unwrap_or_default();
        if stored != text {
            violations.push(format!("pack file {path}"));
        }
        let digest = sha256_hex(text.as_bytes());
        if file["sha256"].as_str() != Some(digest.as_str()) {
            violations.push(format!("file sha256 {path}"));
        }
        if file["bytes"].as_u64() != Some(text.len() as u64) {
            violations.push(format!("file bytes {path}"));
        }
        rows.push((path.to_owned(), text.len(), digest));
    }
    rows.sort();
    let records = Value::Array(
        rows.into_iter()
            .map(|(path, bytes, digest)| json!({"bytes": bytes, "path": path, "sha256": digest}))
            .collect(),
    );
    let pack_digest = sha256_hex(canonical_json(&records).as_bytes());
    if pack_digest != pack.pack_digest {
        violations.push("pack_digest".to_owned());
    }
    if doc["pack_digest"].as_str() != Some(pack_digest.as_str()) {
        violations.push("stdout pack_digest".to_owned());
    }
    violations
}

fn assert_exact(output: &Output, exit_code: i32, line: &str, absent: &[&str]) {
    assert_eq!(
        output.status.code(),
        Some(exit_code),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr utf8");
    assert_eq!(stderr, format!("{line}\n"), "{stderr}");
    for marker in absent {
        assert!(!stderr.contains(marker), "echoed {marker}");
    }
}

#[test]
fn two_roles_match_library_bytes_and_independent_hashes() {
    let (effective, clerk_vis, ada2_vis, audit_vis) = yard();
    let clerk_pack = library_pack(&effective, &clerk_vis);
    let ada2_pack = library_pack(&effective, &ada2_vis);
    let audit_pack = library_pack(&effective, &audit_vis);

    let dir = std::env::temp_dir().join(format!("oo-a178-role-pack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).expect("temp dir");
    let clerk_out = run_in(Some(&dir), &["role-pack"], &command_stdin(&effective, &clerk_vis));
    let ada2_out = run_in(Some(&dir), &["role-pack"], &command_stdin(&effective, &ada2_vis));
    let audit_out = run_in(Some(&dir), &["role-pack"], &command_stdin(&effective, &audit_vis));
    let left = std::fs::read_dir(&dir).expect("read temp").count();
    let _ = std::fs::remove_dir_all(&dir);

    dump("clerk.stdout.json", &clerk_out.stdout);
    dump("ada2.stdout.json", &ada2_out.stdout);
    dump("audit.stdout.json", &audit_out.stdout);
    dump("clerk.stderr", &clerk_out.stderr);
    dump("audit.stderr", &audit_out.stderr);

    for output in [&clerk_out, &ada2_out, &audit_out] {
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        assert!(!output.stdout.ends_with(b"\n"));
    }
    assert_eq!(left, 0, "role-pack created a file");

    let clerk_stdout = String::from_utf8(clerk_out.stdout).expect("clerk utf8");
    let ada2_stdout = String::from_utf8(ada2_out.stdout).expect("ada2 utf8");
    let audit_stdout = String::from_utf8(audit_out.stdout).expect("audit utf8");
    let clerk_doc: Value = serde_json::from_str(&clerk_stdout).expect("clerk json");
    let ada2_doc: Value = serde_json::from_str(&ada2_stdout).expect("ada2 json");
    let audit_doc: Value = serde_json::from_str(&audit_stdout).expect("audit json");
    write_role("clerk", &clerk_stdout, &clerk_doc);
    write_role("ada2", &ada2_stdout, &ada2_doc);
    write_role("audit", &audit_stdout, &audit_doc);

    let mut hash_violations = independent_hash(&clerk_doc, &clerk_pack);
    hash_violations.extend(independent_hash(&ada2_doc, &ada2_pack));
    hash_violations.extend(independent_hash(&audit_doc, &audit_pack));

    let mut kept_misses = Vec::new();
    let clerk_body = file_texts(&clerk_doc);
    for legal in [
        "包号",
        "净重",
        "货包",
        "本季到港重量",
        "badge note stays",
        "Samples are omitted because pii is personal.",
        "SELECT mass_kg AS total FROM yard.bale",
        "净重口径",
        "本季",
        "one row per bale",
        "Estimated row count is omitted.",
        "did not re-prove row security",
        "Cross-row samples and most_common values are omitted.",
        "not_claimed",
        "not_performed",
    ] {
        if !clerk_body.contains(legal) {
            kept_misses.push(legal.to_owned());
        }
    }
    let mut hidden_hits = Vec::new();
    for hidden in [
        "vault_bin",
        "舱记",
        "秤台",
        "yard.scale",
        "pier.slip",
        "泊位单",
        "717171",
        "292929",
        "hidden_mass",
        OWNER,
        HIDDEN_TERM,
        "bale_scale",
        "reading_kg",
        "scale_id",
        "most_common_vals",
        SAMPLE,
        STALE,
        MARKER,
    ] {
        if clerk_stdout.contains(hidden) {
            hidden_hits.push(format!("clerk {hidden}"));
        }
    }
    let audit_body = file_texts(&audit_doc);
    for legal in [
        "yard.scale",
        "秤台",
        "estimated rows: 41",
        "Row security: none. This compiler did not query the database.",
        "not_claimed",
    ] {
        if !audit_body.contains(legal) {
            kept_misses.push(format!("audit {legal}"));
        }
    }
    for hidden in [
        "yard.bale",
        "包号",
        "vault_bin",
        "舱记",
        "badge",
        "工牌",
        "717171",
        "292929",
        "泊位单",
        "pier.slip",
        "kept_mass",
        "hidden_mass",
        OWNER,
        HIDDEN_TERM,
        "本季到港重量",
        "personal",
        SAMPLE,
        "bale_scale",
    ] {
        if audit_stdout.contains(hidden) {
            hidden_hits.push(format!("audit {hidden}"));
        }
    }

    let mut status_mismatches = Vec::new();
    for (name, doc) in [("clerk", &clerk_doc), ("audit", &audit_doc)] {
        for (key, status) in [
            ("authentication", "not_claimed"),
            ("authorization", "not_claimed"),
            ("publication", "not_claimed"),
            ("mount", "not_performed"),
            ("samples", "omitted"),
        ] {
            if doc["manifest"][key].as_str() != Some(status) {
                status_mismatches.push(format!("{name} {key}"));
            }
        }
        if doc["manifest"]["row_security_reproven"] != Value::Bool(false) {
            status_mismatches.push(format!("{name} row_security_reproven"));
        }
        if doc["protocol"] != COMMAND_PROTOCOL {
            status_mismatches.push(format!("{name} protocol"));
        }
        if doc.get("principal").is_some() || doc["artifact"].get("principal").is_some() {
            status_mismatches.push(format!("{name} principal field"));
        }
    }

    let shared_artifact = clerk_pack.artifact == ada2_pack.artifact
        && clerk_doc["artifact"] == ada2_doc["artifact"]
        && clerk_stdout.contains(&clerk_pack.artifact)
        && ada2_stdout.contains(&ada2_pack.artifact);
    let shared_digest = clerk_pack.pack_digest == ada2_pack.pack_digest
        && clerk_doc["pack_digest"] == ada2_doc["pack_digest"];
    let cache_differs = clerk_pack.cache_key != ada2_pack.cache_key
        && clerk_pack.cache_key.contains("principal=ada178")
        && ada2_pack.cache_key.contains("principal=ada178b")
        && !clerk_pack.artifact.contains("ada178")
        && !ada2_pack.artifact.contains("ada178b")
        && !audit_pack.artifact.contains("aud178");
    let roles_differ = clerk_pack.pack_digest != audit_pack.pack_digest;

    let outcome = json!({
        "audit_pack_digest": audit_pack.pack_digest,
        "cache_keys_differ": cache_differs,
        "clerk_pack_digest": clerk_pack.pack_digest,
        "digests_differ_by_role": roles_differ,
        "hash_violations": hash_violations,
        "hidden_hits": hidden_hits,
        "kept_misses": kept_misses,
        "principal_artifact_shared": shared_artifact && shared_digest,
        "status_mismatches": status_mismatches,
        "temp_files": left,
    });
    dump("outcome.json", format!("{outcome}\n").as_bytes());

    assert!(shared_artifact, "principals did not share artifact bytes");
    assert!(shared_digest, "principals did not share pack_digest");
    assert!(cache_differs, "cache keys did not bind the principal");
    assert!(roles_differ, "roles shared a pack digest");
    assert_eq!(canonical_json(&clerk_doc), clerk_stdout);
    assert_eq!(canonical_json(&audit_doc), audit_stdout);
    assert!(hash_violations.is_empty(), "{hash_violations:?}");
    assert!(kept_misses.is_empty(), "{kept_misses:?}");
    assert!(hidden_hits.is_empty(), "{hidden_hits:?}");
    assert!(status_mismatches.is_empty(), "{status_mismatches:?}");
    assert_eq!(clerk_doc["cache_key"], clerk_pack.cache_key);
    assert_eq!(clerk_doc["cache_key_sha256"], clerk_pack.cache_key_sha256);
    assert_eq!(audit_doc["manifest_sha256"], audit_pack.manifest_sha256);
}

#[test]
fn forwarding_stale_binding_and_samples_do_not_echo() {
    let (effective, clerk_vis, _, _) = yard();
    let mut stale = clerk_vis.clone();
    stale["policy"]["version"] = json!(STALE);
    let stale_out = run(&["role-pack"], &command_stdin(&effective, &stale));
    dump("stale.stderr", &stale_out.stderr);
    dump(
        "stale.exit",
        format!("{}\n", stale_out.status.code().unwrap_or(255)).as_bytes(),
    );
    assert_exact(
        &stale_out,
        1,
        "oo-builder: role-pack: oo.builder.role-pack/v1/binding.policy the snapshot policy version does not match the effective document",
        &[STALE, OWNER, SAMPLE, "/policy/version"],
    );

    let mut compiler = clerk_vis.clone();
    compiler["issued_for_compiler"] = json!(COMPILER_MARK);
    let compiler_out = run(&["role-pack"], &command_stdin(&effective, &compiler));
    dump("compiler.stderr", &compiler_out.stderr);
    assert_exact(
        &compiler_out,
        1,
        "oo-builder: role-pack: oo.builder.role-pack/v1/binding.compiler the visibility snapshot is not bound to this compiler version",
        &[COMPILER_MARK, "/issued_for_compiler"],
    );

    let mut version = effective.clone();
    version["protocol"] = json!("oo.builder.effective-oos/v0");
    let version_out = run(&["role-pack"], &command_stdin(&version, &clerk_vis));
    dump("effective-version.stderr", &version_out.stderr);
    assert_exact(
        &version_out,
        1,
        "oo-builder: role-pack: oo.builder.role-pack/v1/protocol.mismatch the document protocol is not the role-pack input this compiler accepts",
        &["effective-oos/v0"],
    );

    let mut sampled = clerk_vis.clone();
    sampled["samples"] = json!([SAMPLE]);
    let sample_out = run(&["role-pack"], &command_stdin(&effective, &sampled));
    dump("sample.stderr", &sample_out.stderr);
    assert_exact(
        &sample_out,
        1,
        "oo-builder: role-pack: oo.builder.role-pack/v1/unsupported.field that field is not a role-pack input and was not interpreted as authority",
        &[SAMPLE, "/samples"],
    );

    let mut nested = effective.clone();
    nested[MARKER] = json!(MARKER);
    let nested_out = run(&["role-pack"], &command_stdin(&nested, &clerk_vis));
    dump("nested.stderr", &nested_out.stderr);
    assert_exact(
        &nested_out,
        1,
        "oo-builder: role-pack: oo.builder.role-pack/v1/schema.rejected the effective document does not match the role-pack contract",
        &[MARKER, OWNER],
    );
}

#[test]
fn bounded_input_version_duplicate_unknown_and_oversize_are_static() {
    let syntax = run(
        &["role-pack"],
        format!(r#"{{"protocol":"{COMMAND_PROTOCOL}","note":"{MARKER}""#).as_bytes(),
    );
    dump("syntax.stderr", &syntax.stderr);
    assert_exact(
        &syntax,
        1,
        "oo-builder: role-pack: oo.builder.role-pack-command/v1/json.syntax role-pack command JSON is not valid",
        &[MARKER],
    );

    let version = run(
        &["role-pack"],
        br#"{"protocol":"oo.builder.role-pack-command/v0","effective_oos":{},"visibility":{}}"#,
    );
    dump("version.stderr", &version.stderr);
    assert_exact(
        &version,
        1,
        "oo-builder: role-pack: oo.builder.role-pack-command/v1/protocol.mismatch the command protocol is not the role-pack command this entry accepts",
        &["role-pack-command/v0", MARKER],
    );

    let duplicate = run(
        &["role-pack"],
        format!(
            r#"{{"protocol":"{COMMAND_PROTOCOL}","protocol":"{MARKER}","effective_oos":{{}},"visibility":{{}}}}"#
        )
        .as_bytes(),
    );
    dump("duplicate.stderr", &duplicate.stderr);
    assert_exact(
        &duplicate,
        1,
        "oo-builder: role-pack: oo.builder.role-pack-command/v1/json.duplicate_key duplicate JSON object key",
        &[MARKER],
    );

    let unknown = run(
        &["role-pack"],
        format!(
            r#"{{"protocol":"{COMMAND_PROTOCOL}","effective_oos":{{}},"visibility":{{}},"{MARKER}":1}}"#
        )
        .as_bytes(),
    );
    dump("unknown.stderr", &unknown.stderr);
    assert_exact(
        &unknown,
        1,
        "oo-builder: role-pack: oo.builder.role-pack-command/v1/field.unknown role-pack command has an unknown field",
        &[MARKER],
    );

    let missing = run(
        &["role-pack"],
        format!(r#"{{"protocol":"{COMMAND_PROTOCOL}","effective_oos":{{}}}}"#).as_bytes(),
    );
    assert_exact(
        &missing,
        1,
        "oo-builder: role-pack: oo.builder.role-pack-command/v1/field.missing role-pack command is missing a required field",
        &[MARKER],
    );

    let shape = run(
        &["role-pack"],
        format!(
            r#"{{"protocol":"{COMMAND_PROTOCOL}","effective_oos":["{MARKER}"],"visibility":{{}}}}"#
        )
        .as_bytes(),
    );
    assert_exact(
        &shape,
        1,
        "oo-builder: role-pack: oo.builder.role-pack-command/v1/schema.invalid effective_oos is not an object",
        &[MARKER],
    );

    let visibility = run(
        &["role-pack"],
        format!(
            r#"{{"protocol":"{COMMAND_PROTOCOL}","effective_oos":{{}},"visibility":"{MARKER}"}}"#
        )
        .as_bytes(),
    );
    assert_exact(
        &visibility,
        1,
        "oo-builder: role-pack: oo.builder.role-pack-command/v1/schema.invalid visibility is not an object",
        &[MARKER],
    );

    let trailing = run(
        &["role-pack"],
        format!(
            r#"{{"protocol":"{COMMAND_PROTOCOL}","effective_oos":{{}},"visibility":{{}}}} {MARKER}"#
        )
        .as_bytes(),
    );
    dump("trailing.stderr", &trailing.stderr);
    assert_exact(
        &trailing,
        1,
        "oo-builder: role-pack: oo.builder.role-pack-command/v1/json.syntax role-pack command JSON is not valid",
        &[MARKER, "trailing data"],
    );

    let mut deep = String::new();
    for _ in 0..40 {
        deep.push_str("{\"k\":");
    }
    deep.push_str(&format!("\"{MARKER}\""));
    for _ in 0..40 {
        deep.push('}');
    }
    let depth = run(&["role-pack"], deep.as_bytes());
    dump("depth.stderr", &depth.stderr);
    assert_exact(
        &depth,
        1,
        "oo-builder: role-pack: oo.builder.role-pack-command/v1/json.budget.depth JSON nesting exceeds the depth budget",
        &[MARKER],
    );
    assert!(JSON_MAX_DEPTH < 40);

    let mut nodes = String::from("[");
    nodes.push_str(&format!("\"{MARKER}\""));
    for _ in 0..JSON_MAX_NODES {
        nodes.push_str(",null");
    }
    nodes.push(']');
    let node_out = run(&["role-pack"], nodes.as_bytes());
    dump("nodes.stderr", &node_out.stderr);
    assert_exact(
        &node_out,
        1,
        "oo-builder: role-pack: oo.builder.role-pack-command/v1/json.budget.nodes JSON node count exceeds the budget",
        &[MARKER],
    );

    let mut huge = MARKER.repeat(8).into_bytes();
    huge.resize(DOCUMENT_MAX_BYTES + 1, b'x');
    let over = run(&["role-pack"], &huge);
    dump("oversize.stderr", &over.stderr);
    assert_exact(
        &over,
        1,
        "oo-builder: role-pack: oo.builder.role-pack-command/v1/json.budget.bytes document exceeds the byte budget",
        &[MARKER, "1048576", "1048577"],
    );

    let utf8 = run(&["role-pack"], &[0xff, 0xfe]);
    assert_exact(
        &utf8,
        1,
        "oo-builder: role-pack: oo.builder.catalog-evidence/v1/json.syntax stdin is not UTF-8",
        &[MARKER],
    );

    let extra = bin().args(["role-pack", MARKER]).output().expect("extra");
    dump("extra.stderr", &extra.stderr);
    dump(
        "extra.exit",
        format!("{}\n", extra.status.code().unwrap_or(255)).as_bytes(),
    );
    assert_exact(
        &extra,
        2,
        "oo-builder: role-pack reads one oo.builder.role-pack-command/v1 JSON document from stdin and takes no arguments",
        &[MARKER],
    );
}

#[test]
fn no_args_and_the_other_three_commands_keep_their_exits() {
    let bare = bin().output().expect("usage");
    dump("usage.stderr", &bare.stderr);
    dump(
        "usage.exit",
        format!("{}\n", bare.status.code().unwrap_or(255)).as_bytes(),
    );
    assert_eq!(bare.status.code(), Some(2));
    assert!(bare.stdout.is_empty());
    let stderr = String::from_utf8(bare.stderr).expect("usage stderr");
    for needle in [
        "usage: oo-builder <command>",
        "structure-draft",
        "captured-pg-draft",
        "review-pg-draft",
        "role-pack",
        "one oo.source.pgsql/v1 catalog",
        "compile_role_pack",
        "caller-supplied public data",
        "not a live permission proof",
        "does not write a file, mount a pack, authenticate, grant, or publish",
        "do not connect",
    ] {
        assert!(stderr.contains(needle), "missing {needle}");
    }
    assert!(!stderr.contains("process entry is not implemented"));
    assert!(!stderr.contains(MARKER));

    let extras = [
        (
            "structure-draft",
            "oo-builder: structure-draft reads JSON from stdin and takes no arguments",
        ),
        (
            "captured-pg-draft",
            "oo-builder: captured-pg-draft reads one oo.source.pgsql/v1 JSON document from stdin and takes no arguments",
        ),
        (
            "review-pg-draft",
            "oo-builder: review-pg-draft reads one JSON document from stdin and takes no arguments",
        ),
    ];
    for (command, line) in extras {
        let extra = bin().args([command, MARKER]).output().expect("extra");
        dump(&format!("{command}.extra.stderr"), &extra.stderr);
        assert_exact(&extra, 2, line, &[MARKER]);
        let bad = run(&[command], format!(r#"{{"note":"{MARKER}"}}"#).as_bytes());
        dump(&format!("{command}.bad.stderr"), &bad.stderr);
        dump(
            &format!("{command}.bad.exit"),
            format!("{}\n", bad.status.code().unwrap_or(255)).as_bytes(),
        );
        assert_eq!(bad.status.code(), Some(1), "{command}");
        assert!(bad.stdout.is_empty(), "{command}");
        let text = String::from_utf8(bad.stderr).expect("stderr");
        assert!(!text.contains(MARKER), "{command} echoed the body: {text}");
        assert!(text.starts_with("oo-builder:"), "{command}: {text}");
        if command != "structure-draft" {
            assert!(text.contains(command), "{command}: {text}");
        }
        assert!(!text.contains("role-pack"), "{command} was routed to role-pack: {text}");
    }

    let unknown = bin().arg(MARKER).output().expect("unknown");
    dump("unknown-command.stderr", &unknown.stderr);
    assert_exact(&unknown, 2, "oo-builder: unknown command", &[MARKER]);
}
