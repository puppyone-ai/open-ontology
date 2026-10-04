//! `role-pack` passes two objects to `compile_role_pack` and prints that pack.
//!
//! The visibility object in these tests is fixture data. It is not authentication
//! and it is not a grant.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use oo_builder::compile_role_pack;
use oo_core::limits::DOCUMENT_MAX_BYTES;

const COMMAND_PROTOCOL: &str = "oo.builder.role-pack-command/v1";
const MARKER: &str = "a177-do-not-echo";
const OWNER: &str = "owner-a177-do-not-copy";
const HIDDEN_TERM: &str = "berth-hidden-a177";
const SAMPLE: &str = "pii-sample-do-not-echo-a177";
const STALE: &str = "stale-a177-secret";

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oo-builder"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn run_in(dir: Option<&std::path::Path>, args: &[&str], stdin: &[u8]) -> std::process::Output {
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

fn run(args: &[&str], stdin: &[u8]) -> std::process::Output {
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
    let canonical = compile_role_pack(&canonical_json(effective), &canonical_json(visibility))
        .expect("canonical compile");
    let unsorted = compile_role_pack(
        &serde_json::to_string(effective).expect("effective"),
        &serde_json::to_string(visibility).expect("visibility"),
    )
    .expect("unsorted compile");
    assert_eq!(canonical.artifact, unsorted.artifact);
    assert_eq!(canonical.pack_digest, unsorted.pack_digest);
    assert_eq!(canonical.cache_key, unsorted.cache_key);
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
    // reading_kg is distinct from the visible crate column mass_kg. A shared
    // bare name on an ungranted dataset makes the accepted scanner omit the metric.
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

fn file_texts(doc: &Value) -> String {
    doc["artifact"]["files"]
        .as_array()
        .expect("files")
        .iter()
        .map(|file| file["text"].as_str().expect("text"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_oracle(doc: &Value, pack: &oo_builder::RolePack) {
    assert_eq!(doc["protocol"], COMMAND_PROTOCOL);
    assert_eq!(doc["pack_digest"], pack.pack_digest);
    assert_eq!(doc["manifest_sha256"], pack.manifest_sha256);
    assert_eq!(doc["cache_key"], pack.cache_key);
    assert_eq!(doc["cache_key_sha256"], pack.cache_key_sha256);
    assert!(doc.get("principal").is_none());
    let artifact = canonical_json(&doc["artifact"]);
    assert_eq!(artifact, pack.artifact);
    let manifest_text = pack.file("manifest.json").expect("manifest file");
    assert_eq!(canonical_json(&doc["manifest"]), manifest_text);
    assert_eq!(doc["manifest"]["authentication"], "not_claimed");
    assert_eq!(doc["manifest"]["authorization"], "not_claimed");
    assert_eq!(doc["manifest"]["publication"], "not_claimed");
    assert_eq!(doc["manifest"]["mount"], "not_performed");
    assert_eq!(doc["manifest"]["samples"], "omitted");
    assert_eq!(doc["manifest"]["row_security_reproven"], false);
    assert_eq!(
        sha256_hex(manifest_text.as_bytes()),
        pack.manifest_sha256
    );
    assert_eq!(
        sha256_hex(pack.cache_key.as_bytes()),
        pack.cache_key_sha256
    );

    let mut rows = Vec::new();
    for file in doc["artifact"]["files"].as_array().expect("artifact files") {
        let path = file["path"].as_str().expect("path").to_string();
        let text = file["text"].as_str().expect("text");
        assert_eq!(pack.file(&path).expect("pack file"), text);
        let digest = sha256_hex(text.as_bytes());
        assert_eq!(file["sha256"], digest);
        assert_eq!(file["bytes"], text.len() as u64);
        rows.push((path, text.len(), digest));
    }
    rows.sort();
    let records = Value::Array(
        rows.into_iter()
            .map(|(path, bytes, digest)| {
                json!({"bytes": bytes, "path": path, "sha256": digest})
            })
            .collect(),
    );
    let pack_digest = sha256_hex(canonical_json(&records).as_bytes());
    assert_eq!(pack_digest, pack.pack_digest);
    assert_eq!(doc["pack_digest"], pack_digest);
}

fn dump(name: &str, bytes: &[u8]) {
    let Some(root) = std::env::var_os("OO_A177_OUT") else {
        return;
    };
    let dir = std::path::PathBuf::from(root);
    std::fs::create_dir_all(&dir).expect("evidence out");
    std::fs::write(dir.join(name), bytes).expect("write evidence");
}

#[test]
fn two_roles_match_the_library_pack_and_drop_hidden_metadata() {
    let (effective, clerk_vis, audit_vis) = wharf();
    let clerk_pack = library_pack(&effective, &clerk_vis);
    let audit_pack = library_pack(&effective, &audit_vis);
    assert_ne!(clerk_pack.pack_digest, audit_pack.pack_digest);
    assert_ne!(clerk_pack.artifact, audit_pack.artifact);
    assert!(clerk_pack.cache_key.contains("principal=ada177"));
    assert!(audit_pack.cache_key.contains("principal=aud177"));
    assert!(!clerk_pack.artifact.contains("ada177"));
    assert!(!audit_pack.artifact.contains("aud177"));

    let dir = std::env::temp_dir().join(format!("oo-a177-role-pack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).expect("temp dir");
    let clerk_out = run_in(
        Some(&dir),
        &["role-pack"],
        &command_stdin(&effective, &clerk_vis),
    );
    let audit_out = run_in(
        Some(&dir),
        &["role-pack"],
        &command_stdin(&effective, &audit_vis),
    );
    let left = std::fs::read_dir(&dir).expect("read temp").count();
    let _ = std::fs::remove_dir_all(&dir);
    dump("clerk.stdout.json", &clerk_out.stdout);
    dump("audit.stdout.json", &audit_out.stdout);
    dump(
        "outcome.json",
        format!(
            "{{\"audit_pack_digest\":\"{}\",\"clerk_pack_digest\":\"{}\",\"digests_differ\":true}}\n",
            audit_pack.pack_digest, clerk_pack.pack_digest
        )
        .as_bytes(),
    );

    assert_eq!(left, 0, "role-pack created a file");
    for output in [&clerk_out, &audit_out] {
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        assert!(!output.stdout.ends_with(b"\n"));
    }
    let clerk_stdout = String::from_utf8(clerk_out.stdout).expect("clerk utf8");
    let audit_stdout = String::from_utf8(audit_out.stdout).expect("audit utf8");
    let clerk_doc: Value = serde_json::from_str(&clerk_stdout).expect("clerk json");
    let audit_doc: Value = serde_json::from_str(&audit_stdout).expect("audit json");
    assert_oracle(&clerk_doc, &clerk_pack);
    assert_oracle(&audit_doc, &audit_pack);
    assert!(clerk_stdout.contains(&clerk_pack.artifact));
    assert!(clerk_stdout.contains(clerk_pack.file("manifest.json").expect("manifest")));

    let clerk_body = file_texts(&clerk_doc);
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
        assert!(!clerk_stdout.contains(hidden), "clerk leaked {hidden}");
    }

    let audit_body = file_texts(&audit_doc);
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
        assert!(!audit_stdout.contains(hidden), "audit leaked {hidden}");
    }
}

#[test]
fn stale_policy_and_sample_field_are_refused_without_echo() {
    let (effective, clerk_vis, _) = wharf();
    let mut stale = clerk_vis.clone();
    stale["policy"]["version"] = json!(STALE);
    let stale_out = run(&["role-pack"], &command_stdin(&effective, &stale));
    dump("stale.stderr", &stale_out.stderr);
    dump(
        "stale.exit",
        format!("{}\n", stale_out.status.code().unwrap_or(255)).as_bytes(),
    );
    assert_static(&stale_out, "binding.policy", &[STALE, OWNER, SAMPLE]);

    let mut sampled = clerk_vis.clone();
    sampled["samples"] = json!([SAMPLE]);
    let sample_out = run(&["role-pack"], &command_stdin(&effective, &sampled));
    dump("sample.stderr", &sample_out.stderr);
    assert_static(
        &sample_out,
        "unsupported.field",
        &[SAMPLE, STALE, "pii-sample"],
    );
}

#[test]
fn invalid_json_version_duplicate_unknown_and_oversize_are_static() {
    let bad = run(
        &["role-pack"],
        format!(r#"{{"protocol":"{COMMAND_PROTOCOL}","note":"{MARKER}""#).as_bytes(),
    );
    assert_static(&bad, "json.syntax", &[MARKER]);

    let version = run(
        &["role-pack"],
        br#"{"protocol":"oo.builder.role-pack-command/v0","effective_oos":{},"visibility":{}}"#,
    );
    assert_static(
        &version,
        "protocol.mismatch",
        &["role-pack-command/v0", MARKER],
    );

    let duplicate = run(
        &["role-pack"],
        format!(
            r#"{{"protocol":"{COMMAND_PROTOCOL}","protocol":"{COMMAND_PROTOCOL}","effective_oos":{{}},"visibility":{{}}}}"#
        )
        .as_bytes(),
    );
    assert_static(&duplicate, "json.duplicate_key", &[MARKER]);

    let unknown = run(
        &["role-pack"],
        format!(
            r#"{{"protocol":"{COMMAND_PROTOCOL}","effective_oos":{{}},"visibility":{{}},"{MARKER}":1}}"#
        )
        .as_bytes(),
    );
    assert_static(&unknown, "field.unknown", &[MARKER]);

    let (mut effective, visibility, _) = wharf();
    effective[MARKER] = json!(MARKER);
    let nested = run(&["role-pack"], &command_stdin(&effective, &visibility));
    assert_static(&nested, "schema.rejected", &[MARKER, OWNER]);

    let shape = run(
        &["role-pack"],
        format!(r#"{{"protocol":"{COMMAND_PROTOCOL}","effective_oos":[],"visibility":{{}}}}"#)
            .as_bytes(),
    );
    assert_static(&shape, "effective_oos is not an object", &[MARKER]);

    let utf8 = run(&["role-pack"], &[0xff, 0xfe]);
    assert_static(&utf8, "stdin is not UTF-8", &[MARKER]);

    let huge = vec![b' '; DOCUMENT_MAX_BYTES + 1];
    let over = run(&["role-pack"], &huge);
    let spaces = " ".repeat(64);
    assert_static(&over, "document exceeds the byte budget", &[&spaces]);
    assert!(over.stdout.is_empty());

    let extra = bin().args(["role-pack", MARKER]).output().expect("extra");
    assert_eq!(extra.status.code(), Some(2));
    assert!(extra.stdout.is_empty());
    let extra_err = String::from_utf8(extra.stderr).expect("extra stderr");
    assert!(extra_err.contains("takes no arguments"));
    assert!(!extra_err.contains(MARKER));
}

#[test]
fn usage_lists_role_pack_and_the_existing_commands_still_exit_2() {
    let bare = bin().output().expect("usage");
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
        "public data",
        "not a live permission proof",
        "do not connect",
    ] {
        assert!(stderr.contains(needle), "missing {needle}");
    }
    assert!(!stderr.contains("process entry is not implemented"));

    for command in ["structure-draft", "captured-pg-draft", "review-pg-draft"] {
        let extra = bin().args([command, MARKER]).output().expect("extra");
        assert_eq!(extra.status.code(), Some(2), "{command}");
        assert!(extra.stdout.is_empty());
        let text = String::from_utf8(extra.stderr).expect("stderr");
        assert!(text.contains("takes no arguments"), "{command}: {text}");
        assert!(!text.contains(MARKER), "{command}");
    }

    let unknown = bin().arg("publish").output().expect("unknown");
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8(unknown.stderr).expect("stderr").contains("unknown command"));
}

fn assert_static(output: &std::process::Output, needle: &str, absent: &[&str]) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr utf8");
    assert!(stderr.contains(needle), "{stderr}");
    assert!(stderr.len() < 400, "stderr length {}", stderr.len());
    for marker in absent {
        assert!(!stderr.contains(marker), "echoed {marker} in {stderr}");
    }
}
