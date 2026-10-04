#![forbid(unsafe_code)]
//! Independent review harness for `oo-builder/role-pack/v1`.
//!
//! The visibility snapshot is an input document. A role string in that
//! document is not authentication and is not a grant. This file does not
//! change the compiler.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use oo_builder::{compile_role_pack, normalize_pack_path};

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
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
        Value::Object(map) => {
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
    }
}

fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn compile(effective: &Value, visibility: &Value) -> Result<oo_builder::RolePack, oo_builder::RolePackError> {
    compile_role_pack(
        &serde_json::to_string(effective).expect("effective json"),
        &serde_json::to_string(visibility).expect("visibility json"),
    )
}

fn must(effective: &Value, visibility: &Value) -> oo_builder::RolePack {
    match compile(effective, visibility) {
        Ok(pack) => pack,
        Err(error) => panic!("compile failed: {error}"),
    }
}

fn confirmed(name: &str) -> Value {
    json!({
        "name": name,
        "pii": "none",
        "classification": {"state": "confirmed"}
    })
}

fn clerk_documents() -> (Value, Value) {
    let mut weight = confirmed("weight_kg");
    weight["data_type"] = json!("numeric queue_note");
    weight["unit"] = json!("queue_note");
    weight["description"] = json!("称重后对照 \"yard\".\"slip\" 的客户备注");
    weight["review"] = json!({"state": "approved"});

    let mut handle = confirmed("handle");
    handle["pii"] = json!("personal");
    handle["semantic_type"] = json!("phone");
    handle["description"] = json!("classified handle");
    handle["review"] = json!({"state": "approved", "by": "class.clerk"});
    handle["classification"]["by"] = json!("class.clerk");

    let mut note_ascii = confirmed("note_ascii");
    note_ascii["description"] = json!("legacy queue_note alias");
    note_ascii["review"] = json!({"state": "approved"});

    let mut note_quoted = confirmed("note_quoted");
    note_quoted["description"] = json!("see \"queue_note\" token");
    note_quoted["review"] = json!({"state": "approved"});

    let mut status = confirmed("status");
    status["enum"] = json!("depot.shipment.status");

    let queue_note = json!({
        "name": "queue_note",
        "label": "客户备注",
        "pii": "sensitive",
        "classification": {"state": "confirmed", "by": "class.clerk"},
        "description": "a168-hidden-body",
        "review": {"state": "approved", "by": "review.clerk"}
    });
    let pending_mark = json!({
        "name": "pending_mark",
        "pii": "none",
        "classification": {"state": "pending", "by": "class.clerk"}
    });
    let bare_col = json!({"name": "bare_col"});

    let shipment = json!({
        "schema_version": "oos/v1",
        "dataset": "depot.shipment",
        "label": "Shipment",
        "tier": "file",
        "grain": "one row per shipment",
        "primary_key": ["shipment_id"],
        "row_count_estimate": 4242427,
        "columns": [
            confirmed("shipment_id"),
            weight,
            handle,
            note_ascii,
            note_quoted,
            status,
            queue_note,
            pending_mark,
            bare_col
        ]
    });
    let tally = json!({
        "schema_version": "oos/v1",
        "dataset": "depot.tally",
        "label": "Tally",
        "tier": "file",
        "grain": "one row per tally",
        "primary_key": ["tally_id"],
        "row_count_estimate": 17,
        "columns": [confirmed("tally_id"), confirmed("weight_kg")]
    });
    let slip = json!({
        "schema_version": "oos/v1",
        "dataset": "yard.slip",
        "label": "Slip",
        "tier": "file",
        "grain": "one row per slip",
        "primary_key": ["slip_id"],
        "row_count_estimate": 8888887,
        "columns": [confirmed("slip_id")]
    });

    let metric = |name: &str, description: &str, sql: &str, filtered: bool| {
        let mut value = json!({
            "metric": name,
            "label": name,
            "description": description,
            "owner": "clerk.one",
            "certified": true,
            "unit": "kg",
            "status": "active",
            "default_anchor": "month_start",
            "sql": sql,
            "tests": [{"params": {}, "expect": {"rows": 3}}],
            "control_total": {
                "as_of": "month_start",
                "source": "depot",
                "sql": "SELECT sum(weight_kg) AS control_sum FROM depot.shipment",
                "tolerance": {"abs": "1"}
            }
        });
        if filtered {
            value["enum_filters"] = json!([{
                "column": "depot.shipment.status",
                "enum": "depot.shipment.status",
                "classes": ["open"]
            }]);
        }
        value
    };

    let effective = json!({
        "protocol": "oo.builder.effective-oos/v1",
        "project": "alpha",
        "ontology_version": "ont-a168",
        "catalog_version": "cat-a168",
        "policy": {"id": "pol-alpha", "version": "7"},
        "knowledge": {"id": "know-alpha", "version": "3"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "depotmap",
            "locale": "zh-CN",
            "include_schemas": ["depot", "yard"],
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
        "datasets": [shipment, tally, slip],
        "relationships": [
            {
                "name": "shipment_tally",
                "from": {"dataset": "depot.shipment", "columns": ["weight_kg"]},
                "to": {"dataset": "depot.tally", "columns": ["weight_kg"]},
                "cardinality": "many_to_one",
                "join_hint": "left",
                "provenance": "human",
                "confidence": "0.272727",
                "review": {"state": "approved", "by": "review.clerk"},
                "evidence": {
                    "containment": "0.919191",
                    "name_match": "exact",
                    "type_compatible": true,
                    "seen_in_query_log": 515151
                }
            },
            {
                "name": "declared_link",
                "from": {"dataset": "depot.shipment", "columns": ["weight_kg"]},
                "to": {"dataset": "depot.tally", "columns": ["weight_kg"]},
                "cardinality": "many_to_one",
                "join_hint": "left",
                "provenance": "declared_fk"
            },
            {
                "name": "guessed_link",
                "from": {"dataset": "depot.shipment", "columns": ["weight_kg"]},
                "to": {"dataset": "depot.tally", "columns": ["weight_kg"]},
                "cardinality": "many_to_one",
                "join_hint": "left",
                "provenance": "inferred"
            }
        ],
        "metrics": [
            metric(
                "weight_total",
                "sum of visible weight",
                "SELECT sum(weight_kg) AS total FROM depot.shipment",
                true
            ),
            metric(
                "quoted_mix",
                "客户备注 remains visible in this sentence",
                "SELECT weight_kg AS \"客户备注\" FROM depot.shipment\nUNION ALL\nSELECT weight_kg FROM \"yard\".\"slip\"",
                false
            ),
            metric(
                "ascii_hidden",
                "names the hidden column",
                "SELECT queue_note FROM depot.shipment",
                false
            )
        ],
        "glossary": [
            {
                "term": "在册",
                "definition": "a visible shipment term",
                "maps_to": {"dataset": "depot.shipment"},
                "status": "approved"
            },
            {
                "term": "引用隐藏",
                "definition": "see \"yard\".\"slip\" and 客户备注",
                "maps_to": {"dataset": "depot.shipment"},
                "status": "approved"
            },
            {
                "term": "草稿",
                "definition": "draft wording",
                "maps_to": {"dataset": "depot.shipment"},
                "status": "draft"
            },
            {
                "term": "隐藏表",
                "definition": "hidden dataset term",
                "maps_to": {"dataset": "yard.slip"},
                "status": "approved"
            }
        ],
        "examples": [
            example(
                "ex-weight",
                "how much weight is visible",
                "SELECT sum(weight_kg) AS total FROM depot.shipment",
                &["dataset:depot.shipment", "column:depot.shipment.weight_kg", "metric:weight_total"]
            ),
            example(
                "ex-quoted",
                "how the hidden slip is referenced",
                "SELECT weight_kg AS \"客户备注\" FROM depot.shipment\nUNION ALL\nSELECT weight_kg FROM \"yard\".\"slip\"",
                &["dataset:depot.shipment", "column:depot.shipment.weight_kg"]
            ),
            example(
                "ex-ascii",
                "show the hidden note",
                "SELECT queue_note FROM depot.shipment",
                &["dataset:depot.shipment"]
            ),
            example(
                "ex-token",
                "show the quoted spelling of the hidden note",
                "SELECT \"queue_note\" FROM depot.shipment",
                &["dataset:depot.shipment"]
            )
        ],
        "enums": [{
            "schema_version": "oos/v1",
            "enum": "depot.shipment.status",
            "classes": {"open": "open shipment", "unexplained": "meaning is not assigned"},
            "values": [
                {"code": "1", "label": "open", "class": "open", "status": "approved"},
                {
                    "code": "9",
                    "label": "unknown",
                    "class": "unexplained",
                    "status": "unexplained",
                    "evidence": {"rows": 183213}
                }
            ],
            "coverage": "0.131313",
            "unexplained_values": ["9"]
        }],
        "entities": [
            {
                "schema_version": "oos/v1",
                "entity": "shipment",
                "label": "Shipment",
                "primary_dataset": "depot.shipment",
                "key": "shipment_id",
                "display": "weight_kg",
                "members": [{
                    "dataset": "depot.shipment",
                    "key": ["shipment_id"],
                    "name": "weight_kg"
                }],
                "datasets": ["depot.shipment"]
            },
            {
                "schema_version": "oos/v1",
                "entity": "slip",
                "label": "Slip",
                "primary_dataset": "yard.slip",
                "key": "slip_id",
                "display": "slip_id",
                "members": [{
                    "dataset": "yard.slip",
                    "key": ["slip_id"]
                }],
                "datasets": ["yard.slip"]
            }
        ],
        "calendar": {
            "schema_version": "oos/v1",
            "business_tz": "Asia/Shanghai",
            "fiscal_year_start_month": 4,
            "week_start": "monday",
            "anchors": ["month_start", "today"],
            "phrases": {
                "queue_note": {"start": "month_start", "end": "today", "bounds": "[)"},
                "客户备注": {"start": "month_start", "end": "today", "bounds": "[)"},
                "本月": {"start": "month_start", "end": "today", "bounds": "[)"}
            }
        }
    });
    let visibility = json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": "alpha",
        "principal": "ada",
        "role": "oo_r_clerk",
        "ontology_version": "ont-a168",
        "catalog_version": "cat-a168",
        "policy": {"id": "pol-alpha", "version": "7"},
        "knowledge": {"id": "know-alpha", "version": "3"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [
            {
                "dataset": "depot.shipment",
                "row_security": "rls",
                "global_stats": "denied",
                "columns": ["shipment_id", "weight_kg", "handle", "note_ascii", "note_quoted", "status"]
            },
            {
                "dataset": "depot.tally",
                "row_security": "rls",
                "global_stats": "permitted",
                "global_stats_proof": {"ref": "proof-tally-1", "proven": true},
                "columns": ["tally_id", "weight_kg"]
            }
        ],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    });
    (effective, visibility)
}

fn example(id: &str, question: &str, sql: &str, uses: &[&str]) -> Value {
    json!({
        "id": id,
        "question": question,
        "sql": sql,
        "uses": uses,
        "verified_by": "review.clerk",
        "verified_at": "2026-04-01"
    })
}

fn with_principal(visibility: &Value, principal: &str) -> Value {
    let mut visibility = visibility.clone();
    visibility["principal"] = json!(principal);
    visibility
}

fn audit_visibility(visibility: &Value) -> Value {
    let mut visibility = visibility.clone();
    visibility["principal"] = json!("cy");
    visibility["role"] = json!("oo_r_audit");
    visibility["datasets"] = json!([{
        "dataset": "depot.tally",
        "row_security": "rls",
        "global_stats": "permitted",
        "global_stats_proof": {"ref": "proof-tally-1", "proven": true},
        "columns": ["tally_id", "weight_kg"]
    }]);
    visibility
}

fn beta_pair(effective: &Value, visibility: &Value) -> (Value, Value) {
    let mut effective = effective.clone();
    effective["project"] = json!("beta");
    effective["policy"] = json!({"id": "pol-beta", "version": "1"});
    effective["knowledge"] = json!({"id": "know-beta", "version": "1"});
    let mut visibility = visibility.clone();
    visibility["project"] = json!("beta");
    visibility["principal"] = json!("bea");
    visibility["role"] = json!("oo_r_clerk");
    visibility["policy"] = json!({"id": "pol-beta", "version": "1"});
    visibility["knowledge"] = json!({"id": "know-beta", "version": "1"});
    visibility["datasets"] = json!([{
        "dataset": "yard.slip",
        "row_security": "none",
        "global_stats": "denied",
        "columns": ["slip_id"]
    }]);
    (effective, visibility)
}

fn artifact_files(pack: &oo_builder::RolePack) -> Vec<Value> {
    let artifact: Value = serde_json::from_str(&pack.artifact).expect("artifact json");
    artifact["files"].as_array().expect("files").clone()
}

fn char_floor(text: &str, mut index: usize) -> usize {
    if index > text.len() {
        index = text.len();
    }
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn char_ceil(text: &str, mut index: usize) -> usize {
    if index > text.len() {
        index = text.len();
    }
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn prose_hits(pack: &oo_builder::RolePack, needle: &str) -> Vec<Value> {
    let mut found = Vec::new();
    for file in artifact_files(pack) {
        let path = file["path"].as_str().expect("path");
        let text = file["text"].as_str().expect("text");
        let mut start = 0;
        while let Some(index) = text[start..].find(needle) {
            let at = start + index;
            let from = char_floor(text, at.saturating_sub(48));
            let to = char_ceil(text, (at + needle.len() + 48).min(text.len()));
            found.push(json!({
                "path": path,
                "at": at,
                "snippet": text[from..to]
            }));
            start = at + needle.len();
            if found.len() == 6 {
                return found;
            }
        }
    }
    found
}

fn check_hashes(pack: &oo_builder::RolePack) -> Result<Value, String> {
    let files = artifact_files(pack);
    let manifest_text = pack.file("manifest.json").map_err(|error| error.to_string())?;
    let manifest: Value = serde_json::from_str(manifest_text).map_err(|error| error.to_string())?;
    let listed = manifest["files"].as_array().ok_or("manifest files")?;
    if listed.len() + 1 != files.len() {
        return Err(format!(
            "manifest lists {} content files and the artifact has {} files",
            listed.len(),
            files.len()
        ));
    }
    let mut records = Vec::new();
    let mut total = 0u64;
    for file in &files {
        let path = file["path"].as_str().ok_or("path")?.to_owned();
        let text = file["text"].as_str().ok_or("text")?;
        let bytes = u64::try_from(text.len()).map_err(|_| "byte length")?;
        total += bytes;
        let digest = sha256_hex(text.as_bytes());
        if file["sha256"].as_str() != Some(digest.as_str()) || file["bytes"].as_u64() != Some(bytes) {
            return Err(format!("artifact record does not match bytes of {path}"));
        }
        if path == "manifest.json" && pack.manifest_sha256 != digest {
            return Err("manifest_sha256 does not match manifest bytes".to_owned());
        }
        if path != "manifest.json" {
            let entry = listed
                .iter()
                .find(|item| item["path"].as_str() == Some(path.as_str()))
                .ok_or_else(|| format!("manifest omits {path}"))?;
            if entry["sha256"].as_str() != Some(digest.as_str()) || entry["bytes"].as_u64() != Some(bytes) {
                return Err(format!("manifest record does not match {path}"));
            }
        }
        let mut object = Map::new();
        object.insert("bytes".to_owned(), json!(bytes));
        object.insert("path".to_owned(), json!(path));
        object.insert("sha256".to_owned(), json!(digest));
        records.push(Value::Object(object));
    }
    records.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
    let digest = sha256_hex(canonical_json(&Value::Array(records)).as_bytes());
    if digest != pack.pack_digest {
        return Err(format!(
            "recomputed pack digest {digest} != library {}",
            pack.pack_digest
        ));
    }
    if total != pack.total_bytes || files.len() as u64 != pack.entry_count {
        return Err("total_bytes or entry_count does not match the files".to_owned());
    }
    let cache_digest = sha256_hex(pack.cache_key.as_bytes());
    if cache_digest != pack.cache_key_sha256 {
        return Err("cache_key_sha256 does not match the cache key line".to_owned());
    }
    Ok(json!({
        "pack_digest": digest,
        "manifest_sha256": pack.manifest_sha256,
        "cache_key_sha256": cache_digest,
        "entry_count": files.len(),
        "total_bytes": total
    }))
}

fn dump_pack(pack: &oo_builder::RolePack, dir: &Path) {
    let files_dir = dir.join("files");
    fs::create_dir_all(&files_dir).expect("out dir");
    for file in artifact_files(pack) {
        let path = file["path"].as_str().expect("path");
        let text = file["text"].as_str().expect("text");
        let destination = files_dir.join(path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).expect("parent");
        }
        fs::write(destination, text).expect("write file");
    }
    fs::write(dir.join("pack_digest.txt"), &pack.pack_digest).expect("digest");
    fs::write(dir.join("manifest_sha256.txt"), &pack.manifest_sha256).expect("manifest");
    fs::write(dir.join("cache_key.txt"), &pack.cache_key).expect("cache");
    fs::write(dir.join("cache_key_sha256.txt"), &pack.cache_key_sha256).expect("cache hash");
    fs::write(dir.join("artifact.json"), &pack.artifact).expect("artifact");
}

fn out_dir(name: &str) -> Option<PathBuf> {
    let root = std::env::var("OO_A168_OUT").ok()?;
    let dir = PathBuf::from(root).join(name);
    fs::create_dir_all(&dir).expect("review out");
    Some(dir)
}

fn require(pack: &oo_builder::RolePack, needle: &str, missing: &mut Vec<String>) {
    if prose_hits(pack, needle).is_empty() {
        missing.push(needle.to_owned());
    }
}

fn forbid(pack: &oo_builder::RolePack, needle: &str, hits: &mut Map<String, Value>) {
    let found = prose_hits(pack, needle);
    if !found.is_empty() {
        hits.insert(needle.to_owned(), Value::Array(found));
    }
}

#[test]
fn fresh_packs_separate_roles_and_recompute_hashes() {
    let (effective, visibility) = clerk_documents();
    let clerk = must(&effective, &visibility);
    let ada2 = must(&effective, &with_principal(&visibility, "ada2"));
    let audit = must(&effective, &audit_visibility(&visibility));
    let (beta_effective, beta_visibility) = beta_pair(&effective, &visibility);
    let beta = must(&beta_effective, &beta_visibility);
    let again = must(&effective, &visibility);

    let mut problems = Vec::new();
    if clerk.artifact != ada2.artifact || clerk.pack_digest != ada2.pack_digest {
        problems.push("principal changed artifact bytes".to_owned());
    }
    if clerk.artifact != again.artifact {
        problems.push("repeated compile changed artifact bytes".to_owned());
    }
    if clerk.cache_key == ada2.cache_key || clerk.pack_digest == audit.pack_digest {
        problems.push("role or principal cache did not separate".to_owned());
    }
    if clerk.pack_digest == beta.pack_digest || clerk.cache_key == beta.cache_key {
        problems.push("same role name was reused across projects".to_owned());
    }
    let clerk_key = format!(
        "oo.builder.role-pack-cache/v1|project=alpha|principal=ada|role=oo_r_clerk|policy=pol-alpha@7|knowledge=know-alpha@3|ontology=ont-a168|catalog=cat-a168|compiler=oo-builder/role-pack/v1|pack={}",
        clerk.pack_digest
    );
    let ada2_key = clerk_key.replace("principal=ada|", "principal=ada2|");
    let beta_key = format!(
        "oo.builder.role-pack-cache/v1|project=beta|principal=bea|role=oo_r_clerk|policy=pol-beta@1|knowledge=know-beta@1|ontology=ont-a168|catalog=cat-a168|compiler=oo-builder/role-pack/v1|pack={}",
        beta.pack_digest
    );
    if clerk.cache_key != clerk_key || ada2.cache_key != ada2_key || beta.cache_key != beta_key {
        problems.push("cache key does not bind project, principal, role, policy, and digest".to_owned());
    }
    if audit.cache_key.contains("role=oo_r_clerk|") || !audit.cache_key.contains("role=oo_r_audit|") {
        problems.push("audit cache key lost its role".to_owned());
    }
    let artifact: Value = serde_json::from_str(&clerk.artifact).expect("artifact");
    if artifact.get("principal").is_some() {
        problems.push("artifact carries a principal".to_owned());
    }
    match check_hashes(&clerk) {
        Ok(value) => {
            if let Some(dir) = out_dir("clerk") {
                fs::write(dir.join("hashes.json"), serde_json::to_string_pretty(&value).unwrap()).unwrap();
            }
        }
        Err(error) => problems.push(error),
    }
    for (name, pack) in [("audit", &audit), ("beta", &beta), ("ada2", &ada2)] {
        if let Err(error) = check_hashes(pack) {
            problems.push(format!("{name}: {error}"));
        }
    }

    let mut clerk_missing = Vec::new();
    for needle in [
        "project: `alpha`",
        "role: `oo_r_clerk`",
        "does not authenticate",
        "Samples are omitted because pii is personal.",
        "Estimated row count is omitted.",
        "estimated rows: 17",
        "did not re-prove",
        "proof-tally-1",
        "sum of visible weight",
        "Unconfirmed codes",
        "a visible shipment term",
        "ex-weight",
        "source-declared, not a grant",
        "本月",
        "classified handle",
    ] {
        require(&clerk, needle, &mut clerk_missing);
    }
    let mut clerk_hits = Map::new();
    for needle in [
        "queue_note",
        "客户备注",
        "a168-hidden-body",
        "\"yard\".\"slip\"",
        "slip_id",
        "pending_mark",
        "bare_col",
        "a168-sample-value",
        "clerk.one",
        "review.clerk",
        "class.clerk",
        "4242427",
        "8888887",
        "0.919191",
        "0.272727",
        "515151",
        "0.131313",
        "183213",
        "guessed_link",
        "draft wording",
        "hidden dataset term",
        "ex-ascii",
        "ex-token",
        "ex-quoted",
        "quoted_mix",
        "ascii_hidden",
        "legacy queue_note alias",
        "引用隐藏",
        "domains/",
    ] {
        forbid(&clerk, needle, &mut clerk_hits);
    }
    let readme = clerk.file("README.md").unwrap_or("");
    if readme.contains("ada") {
        problems.push("readme contains the principal".to_owned());
    }
    let manifest: Value = serde_json::from_str(clerk.file("manifest.json").expect("manifest")).expect("manifest json");
    for (key, expect) in [
        ("authentication", "not_claimed"),
        ("authorization", "not_claimed"),
        ("publication", "not_claimed"),
        ("mount", "not_performed"),
        ("samples", "omitted"),
        ("domains", "unsupported"),
        ("file_splitting", "unsupported"),
        ("search", "unsupported"),
    ] {
        if manifest[key].as_str() != Some(expect) {
            problems.push(format!("manifest {key} is not {expect}"));
        }
    }
    if manifest["row_security_reproven"] != json!(false) {
        problems.push("row security was marked re-proven".to_owned());
    }
    if manifest["tokens"]["status"] != json!("unsupported") || manifest["tokens"]["estimator"] != json!("none") {
        problems.push("token status is not an unsupported estimator".to_owned());
    }
    if manifest["tokens"]["readme_token_budget"] != json!(2000)
        || manifest["tokens"]["table_file_token_budget"] != json!(3000)
    {
        problems.push("token budgets were not kept as labels".to_owned());
    }
    if manifest.get("principal").is_some() {
        problems.push("manifest carries a principal".to_owned());
    }
    let clerk_files = artifact_files(&clerk);
    let paths: Vec<&str> = clerk_files
        .iter()
        .filter_map(|file| file["path"].as_str())
        .collect();
    if paths.iter().any(|path| path.starts_with("domains/")) || !paths.contains(&"examples/verified.sql") {
        problems.push("pack layout claims a domain chapter or drops the single example file".to_owned());
    }
    if clerk.file("entities/slip.md").is_ok() || clerk.file("tables/yard.slip.md").is_ok() {
        problems.push("hidden dataset or entity has a file".to_owned());
    }

    let mut audit_hits = Map::new();
    for needle in ["queue_note", "客户备注", "shipment_id", "slip_id", "\"yard\".\"slip\"", "在册"] {
        forbid(&audit, needle, &mut audit_hits);
    }
    let mut audit_missing = Vec::new();
    require(&audit, "estimated rows: 17", &mut audit_missing);
    require(&audit, "role: `oo_r_audit`", &mut audit_missing);

    let mut beta_hits = Map::new();
    for needle in ["queue_note", "客户备注", "shipment_id", "a168-hidden-body", "4242427"] {
        forbid(&beta, needle, &mut beta_hits);
    }
    let mut beta_missing = Vec::new();
    require(&beta, "yard.slip", &mut beta_missing);
    require(&beta, "slip_id", &mut beta_missing);
    require(&beta, "project: `beta`", &mut beta_missing);
    require(&beta, "role: `oo_r_clerk`", &mut beta_missing);

    if !clerk_missing.is_empty() {
        problems.push(format!("clerk missing {clerk_missing:?}"));
    }
    if !clerk_hits.is_empty() {
        problems.push(format!("clerk leaked {}", serde_json::to_string(&clerk_hits).unwrap()));
    }
    if !audit_missing.is_empty() || !audit_hits.is_empty() {
        problems.push(format!(
            "audit missing {audit_missing:?} leaked {}",
            serde_json::to_string(&audit_hits).unwrap()
        ));
    }
    if !beta_missing.is_empty() || !beta_hits.is_empty() {
        problems.push(format!(
            "beta missing {beta_missing:?} leaked {}",
            serde_json::to_string(&beta_hits).unwrap()
        ));
    }

    let outcome = json!({
        "problems": problems,
        "clerk_hits": clerk_hits,
        "audit_hits": audit_hits,
        "beta_hits": beta_hits,
        "clerk_digest": clerk.pack_digest,
        "ada2_digest": ada2.pack_digest,
        "audit_digest": audit.pack_digest,
        "beta_digest": beta.pack_digest,
        "clerk_cache": clerk.cache_key,
        "ada2_cache": ada2.cache_key,
        "audit_cache": audit.cache_key,
        "beta_cache": beta.cache_key
    });
    if let Some(dir) = out_dir("summary") {
        dump_pack(&clerk, &out_dir("clerk").expect("clerk dir"));
        dump_pack(&audit, &out_dir("audit").expect("audit dir"));
        dump_pack(&beta, &out_dir("beta").expect("beta dir"));
        dump_pack(&ada2, &out_dir("ada2").expect("ada2 dir"));
        fs::write(dir.join("outcome.json"), serde_json::to_string_pretty(&outcome).unwrap()).unwrap();
    }
    assert!(problems.is_empty(), "{outcome}");
}

#[test]
fn refusals_cover_identity_budget_path_classification_and_samples() {
    let (effective, visibility) = clerk_documents();
    let mut stale = visibility.clone();
    stale["policy"]["version"] = json!("8");
    assert_code(&effective, &stale, "binding.policy");

    let mut bad_role = visibility.clone();
    bad_role["role"] = json!("clerk");
    assert_code(&effective, &bad_role, "identity.invalid");

    let mut entries = visibility.clone();
    entries["limits"]["max_entries"] = json!(1);
    assert_code(&effective, &entries, "budget.entries");

    let mut zero = visibility.clone();
    zero["limits"]["max_entries"] = json!(0);
    assert_code(&effective, &zero, "limit.invalid");

    let mut narrow = visibility.clone();
    narrow["limits"]["max_file_bytes"] = json!(64);
    narrow["limits"]["max_total_bytes"] = json!(64);
    assert_code(&effective, &narrow, "budget.file");

    let mut unknown = visibility.clone();
    unknown["datasets"][0]["columns"].as_array_mut().unwrap().push(json!("bare_col"));
    assert_code(&effective, &unknown, "classification.unknown_pii");

    let mut pending = visibility.clone();
    pending["datasets"][0]["columns"].as_array_mut().unwrap().push(json!("pending_mark"));
    assert_code(&effective, &pending, "classification.incomplete");

    let mut samples = visibility.clone();
    samples["samples"] = json!(["a168-sample-value"]);
    assert_code(&effective, &samples, "unsupported.field");

    let mut row_samples = visibility.clone();
    row_samples["datasets"][0]["samples"] = json!(["a168-sample-value"]);
    assert_code(&effective, &row_samples, "unsupported.samples");

    let mut most = visibility.clone();
    most["datasets"][0]["most_common_vals"] = json!(["a168-sample-value"]);
    assert_code(&effective, &most, "unsupported.samples");

    let mut raw = effective.clone();
    raw["ontology"]["compile"]["sample_values"]["default"] = json!("raw");
    assert_code(&raw, &visibility, "sample.raw_refused");

    let mut per_role = effective.clone();
    per_role["ontology"]["compile"]["per_role"] = json!(false);
    assert_code(&per_role, &visibility, "compile.per_role");

    let mut unproven = visibility.clone();
    unproven["datasets"][1]["global_stats_proof"]["proven"] = json!(false);
    assert_code(&effective, &unproven, "evidence.unproven");

    let mut missing_proof = visibility.clone();
    missing_proof["datasets"][1].as_object_mut().unwrap().remove("global_stats_proof");
    assert_code(&effective, &missing_proof, "evidence.unproven");

    let mut non_rls = visibility.clone();
    non_rls["datasets"][1]["row_security"] = json!("none");
    assert_code(&effective, &non_rls, "evidence.unproven");

    let path = normalize_pack_path("../queue_note").expect_err("traversal");
    assert_eq!(path.path, "/");
    let rendered = path.to_string();
    assert!(!rendered.contains("queue_note"));
    assert!(!rendered.contains(".."));
    let absolute = normalize_pack_path("/tmp/queue_note").expect_err("absolute");
    assert_eq!(absolute.path, "/");
    assert!(!absolute.to_string().contains("queue_note"));
    let normalized = normalize_pack_path("tables/e\u{0301}.md").expect("nfc");
    assert_eq!(normalized, "tables/é.md");
}

#[test]
fn non_ascii_column_name_is_rejected_before_visibility() {
    let (mut effective, visibility) = clerk_documents();
    effective["datasets"][0]["columns"][0]["name"] = json!("客户");
    assert_code(&effective, &visibility, "schema.rejected");
}

fn assert_code(effective: &Value, visibility: &Value, marker: &str) {
    match compile(effective, visibility) {
        Ok(_) => panic!("expected refusal containing {marker}"),
        Err(error) => {
            assert!(
                error.code.contains(marker),
                "code {} does not contain {marker}; message {}",
                error.code,
                error.message
            );
            assert!(
                !error.to_string().contains("a168-sample-value"),
                "refusal echoed a sample marker: {error}"
            );
        }
    }
}
