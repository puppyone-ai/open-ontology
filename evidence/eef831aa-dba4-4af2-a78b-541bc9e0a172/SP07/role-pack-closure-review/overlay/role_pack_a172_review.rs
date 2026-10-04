//! Independent closure oracle for the role pack compiler.
//!
//! The visibility snapshot is an input. This test does not authenticate, does
//! not grant, and does not execute SQL. Digests are SHA-256 from the `sha2`
//! crate in this file. They do not call the library hash helper.

#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use oo_builder::{RolePack, compile_role_pack, normalize_pack_path};

const PREFIX: &str = "oo.builder.role-pack/v1/";
const COMPILER: &str = "oo-builder/role-pack/v1";
const CACHE_PROTOCOL: &str = "oo.builder.role-pack-cache/v1";

const NFD_LABEL: &str = "e\u{0301}货签";
const NFC_LABEL: &str = "é货签";
const QUOTE_LABEL: &str = "签\"记";
const APOSTROPHE_LABEL: &str = "签'记";
const KEPT_SQL: &str = "SeLeCt \"mass_kg\" AS \"净\"\"重\" FROM \"dock\" . \"parcel\"\nUNION ALL\nSELECT mass_kg FROM dock.scale";

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(&mut out, value);
    out
}

fn write_canonical(out: &mut String, value: &Value) {
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
                write_canonical(out, item);
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
                write_canonical(out, &map[*key]);
            }
            out.push('}');
        }
    }
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
                let code = u32::from(ch);
                out.push_str(&format!("\\u{code:04x}"));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn compile(effective: &Value, visibility: &Value) -> Result<RolePack, oo_builder::RolePackError> {
    compile_role_pack(
        &serde_json::to_string(effective).expect("effective json"),
        &serde_json::to_string(visibility).expect("visibility json"),
    )
}

fn must(effective: &Value, visibility: &Value) -> RolePack {
    match compile(effective, visibility) {
        Ok(pack) => pack,
        Err(error) => panic!("compile failed: {error}"),
    }
}

fn none_col(name: &str) -> Value {
    json!({
        "name": name,
        "pii": "none",
        "classification": {"state": "confirmed"}
    })
}

fn example(id: &str, question: &str, sql: &str, uses: &[&str]) -> Value {
    json!({
        "id": id,
        "question": question,
        "sql": sql,
        "uses": uses,
        "verified_by": "review.a172",
        "verified_at": "2026-04-02"
    })
}

fn base_metric(name: &str, label: &str, description: &str, unit: &str, sql: &str) -> Value {
    json!({
        "metric": name,
        "label": label,
        "description": description,
        "owner": "owner.a172",
        "certified": true,
        "unit": unit,
        "status": "active",
        "default_anchor": "month_start",
        "sql": sql,
        "tests": [{"params": {}, "expect": {"rows": 3}}],
        "control_total": {
            "as_of": "month_start",
            "source": "dock",
            "sql": "SELECT count(*) AS n FROM dock.scale",
            "tolerance": {"abs": "1"}
        }
    })
}

fn fresh_documents() -> (Value, Value) {
    let mut parcel_id = none_col("parcel_id");
    parcel_id["label"] = json!("包裹号");
    parcel_id["data_type"] = json!("bigint");
    parcel_id["description"] = json!("本季到港重量");
    parcel_id["review"] = json!({"state": "approved", "by": "review.a172"});

    let mut mass = none_col("mass_kg");
    mass["data_type"] = json!("numeric hold_bin");
    mass["unit"] = json!("HOLD_BIN");
    mass["description"] = json!("对照 \"pier\".\"berth\" 的é货签");
    mass["review"] = json!({"state": "approved", "by": "review.a172"});

    let mut badge = none_col("badge");
    badge["pii"] = json!("personal");
    badge["semantic_type"] = json!("phone");
    badge["description"] = json!("工牌不进样本");
    badge["review"] = json!({"state": "approved", "by": "review.a172"});
    badge["classification"]["by"] = json!("class.a172");

    let mut status = none_col("status");
    status["enum"] = json!("dock.parcel.status");

    let mut mark_ok = none_col("mark_ok");
    mark_ok["label"] = json!(NFC_LABEL);
    mark_ok["description"] = json!("可见标记说明");
    mark_ok["review"] = json!({"state": "approved"});

    let hold_bin = json!({
        "name": "hold_bin",
        "label": "暂存格",
        "pii": "sensitive",
        "classification": {"state": "confirmed", "by": "class.a172"},
        "description": "a172-hidden-body",
        "review": {"state": "approved", "by": "review.a172"}
    });
    let mark_nfd = json!({
        "name": "mark_nfd",
        "label": NFD_LABEL,
        "pii": "none",
        "classification": {"state": "confirmed"}
    });
    let quote_col = json!({
        "name": "quote_col",
        "label": QUOTE_LABEL,
        "pii": "none",
        "classification": {"state": "confirmed"}
    });
    let tick_col = json!({
        "name": "tick_col",
        "label": APOSTROPHE_LABEL,
        "pii": "none",
        "classification": {"state": "confirmed"}
    });
    let quarter = none_col("quarter_start");
    let mut bare = json!({"name": "bare_col"});
    bare["classification"] = json!({"state": "confirmed"});
    let mut pending = none_col("pending_mark");
    pending["classification"] = json!({"state": "pending"});

    let mut scale_mass = none_col("mass_kg");
    scale_mass["data_type"] = json!("numeric");
    scale_mass["unit"] = json!("kg");

    let parcel = json!({
        "schema_version": "oos/v1",
        "dataset": "dock.parcel",
        "label": "包裹",
        "source_name": "码头副本",
        "tier": "file",
        "grain": "one row per parcel",
        "primary_key": ["parcel_id"],
        "row_count_estimate": 5151515,
        "columns": [
            parcel_id, mass, badge, status, mark_ok, hold_bin, mark_nfd, quote_col,
            tick_col, quarter, bare, pending
        ]
    });
    let scale = json!({
        "schema_version": "oos/v1",
        "dataset": "dock.scale",
        "label": "Scale",
        "tier": "file",
        "grain": "one row per scale",
        "primary_key": ["scale_id"],
        "row_count_estimate": 19,
        "columns": [none_col("scale_id"), scale_mass]
    });
    let berth = json!({
        "schema_version": "oos/v1",
        "dataset": "pier.berth",
        "label": "泊位单",
        "tier": "file",
        "grain": "one row per berth",
        "primary_key": ["berth_id"],
        "row_count_estimate": 7777777,
        "columns": [none_col("berth_id")]
    });

    let mut kept = base_metric("kept_mass", "可见总量", "本季到港重量", "kg", KEPT_SQL);
    kept["enum_filters"] = json!([{
        "column": "dock.parcel.status",
        "enum": "dock.parcel.status",
        "classes": ["open"]
    }]);
    kept["control_total"] = json!({
        "as_of": "month_start",
        "source": "dock",
        "sql": "SELECT sum(mass_kg) AS ctrl_hold FROM dock.parcel",
        "tolerance": {"abs": "1"}
    });
    let mut ctrl_leak = base_metric(
        "ctrl_leak",
        "控制泄漏",
        "本季到港重量",
        "kg",
        "SELECT sum(mass_kg) AS total FROM dock.parcel",
    );
    ctrl_leak["control_total"] = json!({
        "as_of": "month_start",
        "source": "dock",
        "sql": "SELECT hold_bin FROM dock.parcel",
        "tolerance": {"abs": "1"}
    });
    let mut syn_leak = base_metric(
        "syn_leak",
        "同义词泄漏",
        "本季到港重量",
        "kg",
        "SELECT sum(mass_kg) AS total FROM dock.parcel",
    );
    syn_leak["synonyms"] = json!([NFC_LABEL]);
    let mut nfd_leak = base_metric(
        "nfd_leak",
        "分解泄漏",
        NFD_LABEL,
        "kg",
        "SELECT sum(mass_kg) AS total FROM dock.parcel",
    );
    nfd_leak["description"] = json!(NFD_LABEL);

    let effective = json!({
        "protocol": "oo.builder.effective-oos/v1",
        "project": "alpha",
        "ontology_version": "ont-a172",
        "catalog_version": "cat-a172",
        "policy": {"id": "pol-alpha", "version": "7"},
        "knowledge": {"id": "know-alpha", "version": "3"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "dockmap",
            "locale": "zh-CN",
            "include_schemas": ["dock", "pier"],
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
        "datasets": [parcel, scale, berth],
        "relationships": [
            {
                "name": "parcel_scale",
                "from": {"dataset": "dock.parcel", "columns": ["mass_kg"]},
                "to": {"dataset": "dock.scale", "columns": ["mass_kg"]},
                "cardinality": "many_to_one",
                "join_hint": "left",
                "provenance": "human",
                "confidence": "0.191919",
                "review": {"state": "approved", "by": "review.a172"},
                "evidence": {
                    "containment": "0.818181",
                    "name_match": "exact",
                    "type_compatible": true,
                    "seen_in_query_log": 616161
                }
            },
            {
                "name": "declared_link",
                "from": {"dataset": "dock.parcel", "columns": ["mass_kg"]},
                "to": {"dataset": "dock.scale", "columns": ["mass_kg"]},
                "cardinality": "many_to_one",
                "join_hint": "left",
                "provenance": "declared_fk"
            },
            {
                "name": "guessed_link",
                "from": {"dataset": "dock.parcel", "columns": ["mass_kg"]},
                "to": {"dataset": "dock.scale", "columns": ["mass_kg"]},
                "cardinality": "many_to_one",
                "join_hint": "left",
                "provenance": "inferred"
            }
        ],
        "metrics": [
            kept,
            base_metric("cmp_metric", "比较", "本季到港重量", "kg", "SELECT mass_kg FROM dock.parcel WHERE mass_kg <> 0 AND mass_kg != 2 AND mass_kg IS NOT NULL"),
            base_metric("count_metric", "计数", "可见秤计数", "kg", "SELECT count(*) AS n FROM dock.scale"),
            base_metric("string_metric", "字符串", "可见引号字符串", "kg", "SELECT 'it''s visible' AS label_ok FROM dock.parcel"),
            base_metric("mix_quoted", "引用隐藏", "本季到港重量", "kg", "SELECT mass_kg FROM \"pier\".\"berth\""),
            base_metric("mix_case", "大小写", "本季到港重量", "kg", "SELECT mass_kg FROM \"Pier\".\"Berth\""),
            base_metric("mix_ws", "空白", "本季到港重量", "kg", "SELECT mass_kg FROM \"pier\" . \"berth\""),
            base_metric("mix_alias", "双引号别名", "本季到港重量", "kg", "SELECT mass_kg AS \"签\"\"记\" FROM dock.parcel"),
            base_metric("mix_nfc", "标签字符串", "本季到港重量", "kg", "SELECT 'é货签' AS mass_kg FROM dock.parcel"),
            base_metric("mix_token", "隐藏列", "本季到港重量", "kg", "SELECT hold_bin FROM dock.parcel"),
            base_metric("mix_path", "三段名", "本季到港重量", "kg", "SELECT mass_kg FROM dock.parcel WHERE dock.parcel.hold_bin IS NULL"),
            base_metric("sq_leak", "单引号标签", "本季到港重量", "kg", "SELECT '签''记' AS mass_kg FROM dock.parcel"),
            base_metric("prose_metric", "散文", "本季到港重量", "kg", "mass is just prose"),
            base_metric("join_metric", "连接", "本季到港重量", "kg", "SELECT mass_kg FROM dock.parcel JOIN dock.scale ON mass_kg = mass_kg"),
            base_metric("or_metric", "析取", "本季到港重量", "kg", "SELECT mass_kg FROM dock.parcel WHERE mass_kg = 1 OR mass_kg = 2"),
            base_metric("comment_metric", "注释", "本季到港重量", "kg", "SELECT mass_kg FROM dock.parcel -- hold_bin"),
            base_metric("semi_metric", "分号", "本季到港重量", "kg", "SELECT mass_kg FROM dock.parcel;"),
            base_metric("star_metric", "星号", "本季到港重量", "kg", "SELECT * FROM dock.parcel"),
            base_metric("with_metric", "公用表", "本季到港重量", "kg", "WITH s AS (SELECT mass_kg FROM dock.parcel) SELECT mass_kg FROM s"),
            base_metric("cast_metric", "转换", "本季到港重量", "kg", "SELECT mass_kg::text FROM dock.parcel"),
            base_metric("sub_metric", "子查询", "本季到港重量", "kg", "SELECT (SELECT mass_kg FROM dock.scale) FROM dock.parcel"),
            base_metric("distinct_cols", "去重列", "本季到港重量", "kg", "SELECT DISTINCT mass_kg FROM dock.parcel"),
            base_metric("union_bare", "裸并", "本季到港重量", "kg", "SELECT mass_kg FROM dock.parcel UNION SELECT mass_kg FROM dock.scale"),
            base_metric("unclosed_metric", "未闭合", "本季到港重量", "kg", "SELECT \"mass_kg FROM dock.parcel"),
            base_metric("unit_leak", "单位泄漏", "本季到港重量", "hold_bin", "SELECT sum(mass_kg) AS total FROM dock.parcel"),
            base_metric("distinct_bare", "裸去重", "本季到港重量", "kg", "SELECT DISTINCT FROM dock.parcel"),
            ctrl_leak,
            syn_leak,
            nfd_leak
        ],
        "glossary": [
            {
                "term": "在港",
                "definition": "本季到港重量，只计可见列",
                "maps_to": {"dataset": "dock.parcel"},
                "status": "approved"
            },
            {
                "term": "引用泊位",
                "definition": "see \"pier\".\"berth\" and é货签",
                "maps_to": {"dataset": "dock.parcel"},
                "status": "approved"
            },
            {
                "term": "草稿",
                "definition": "draft wording",
                "maps_to": {"dataset": "dock.parcel"},
                "status": "draft"
            },
            {
                "term": "泊位术语",
                "definition": "hidden dataset term",
                "maps_to": {"dataset": "pier.berth"},
                "status": "approved"
            }
        ],
        "examples": [
            example("ex-kept", "本季到港重量是多少", "SELECT sum(mass_kg) AS total FROM dock.parcel", &["dataset:dock.parcel", "column:dock.parcel.mass_kg", "metric:kept_mass"]),
            example("ex-kept-quote", "可见的引号查询", "SELECT mass_kg AS \"净\"\"重\" FROM \"dock\".\"parcel\"", &["dataset:dock.parcel", "column:dock.parcel.mass_kg"]),
            example("ex-scale", "how many scale rows", "SELECT count(*) AS n FROM dock.scale", &["dataset:dock.scale", "column:dock.scale.mass_kg"]),
            example("ex-hidden-quote", "hidden berth", "SELECT mass_kg FROM \"pier\".\"berth\"", &["dataset:dock.parcel"]),
            example("ex-nfc", "hidden label alias", "SELECT mass_kg AS \"é货签\" FROM dock.parcel", &["dataset:dock.parcel"]),
            example("ex-join", "joined visible tables", "SELECT mass_kg FROM dock.parcel JOIN dock.scale ON mass_kg = mass_kg", &["dataset:dock.parcel", "dataset:dock.scale"]),
            example("ex-prose", "not a query", "mass is just prose", &["dataset:dock.parcel"]),
            example("ex-double", "doubled hidden alias", "SELECT mass_kg AS \"签\"\"记\" FROM dock.parcel", &["dataset:dock.parcel"])
        ],
        "enums": [{
            "schema_version": "oos/v1",
            "enum": "dock.parcel.status",
            "classes": {"open": "open parcel", "unexplained": "meaning is not assigned"},
            "values": [
                {"code": "1", "label": "open", "class": "open", "status": "approved"},
                {"code": "9", "label": "unknown", "class": "unexplained", "status": "unexplained", "evidence": {"rows": 183213}},
                {"code": NFC_LABEL, "label": "hidden code", "class": "open", "status": "approved"}
            ],
            "coverage": "0.272727",
            "unexplained_values": ["9"]
        }],
        "entities": [
            {
                "schema_version": "oos/v1",
                "entity": "parcel",
                "label": "包裹",
                "primary_dataset": "dock.parcel",
                "key": "parcel_id",
                "display": "mass_kg",
                "members": [{"dataset": "dock.parcel", "key": ["parcel_id"], "name": "mass_kg"}],
                "datasets": ["dock.parcel"]
            },
            {
                "schema_version": "oos/v1",
                "entity": "berth",
                "label": "泊位单",
                "primary_dataset": "pier.berth",
                "key": "berth_id",
                "display": "berth_id",
                "members": [{"dataset": "pier.berth", "key": ["berth_id"]}],
                "datasets": ["pier.berth"]
            }
        ],
        "calendar": {
            "schema_version": "oos/v1",
            "business_tz": "Asia/Shanghai",
            "fiscal_year_start_month": 4,
            "week_start": "monday",
            "anchors": ["month_start", "today", "quarter_start"],
            "phrases": {
                "hold_bin": {"start": "month_start", "end": "today", "bounds": "[)"},
                "é货签": {"start": "month_start", "end": "today", "bounds": "[)"},
                "本季": {"start": "month_start", "end": "today", "bounds": "[)"},
                "锚点泄漏": {"start": "quarter_start", "end": "today", "bounds": "[)"}
            }
        }
    });
    let visibility = visibility_for("ada", "oo_r_clerk", vec![
        json!({
            "dataset": "dock.parcel",
            "row_security": "rls",
            "global_stats": "denied",
            "columns": ["parcel_id", "mass_kg", "badge", "status", "mark_ok"]
        }),
        json!({
            "dataset": "dock.scale",
            "row_security": "rls",
            "global_stats": "permitted",
            "global_stats_proof": {"ref": "proof-scale-a172", "proven": true},
            "columns": ["scale_id", "mass_kg"]
        }),
    ]);
    (effective, visibility)
}

fn visibility_for(principal: &str, role: &str, datasets: Vec<Value>) -> Value {
    json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": "alpha",
        "principal": principal,
        "role": role,
        "ontology_version": "ont-a172",
        "catalog_version": "cat-a172",
        "policy": {"id": "pol-alpha", "version": "7"},
        "knowledge": {"id": "know-alpha", "version": "3"},
        "issued_for_compiler": COMPILER,
        "datasets": datasets,
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    })
}

fn audit_visibility() -> Value {
    visibility_for(
        "cy",
        "oo_r_audit",
        vec![json!({
            "dataset": "dock.scale",
            "row_security": "rls",
            "global_stats": "permitted",
            "global_stats_proof": {"ref": "proof-scale-a172", "proven": true},
            "columns": ["scale_id", "mass_kg"]
        })],
    )
}

fn pier_visibility() -> Value {
    visibility_for(
        "bea",
        "oo_r_pier",
        vec![json!({
            "dataset": "pier.berth",
            "row_security": "rls",
            "global_stats": "denied",
            "columns": ["berth_id"]
        })],
    )
}

fn out_root() -> PathBuf {
    std::env::var_os("OO_A172_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("oo-a172-out"))
}

fn dump_pack(role: &str, pack: &RolePack) {
    let dir = out_root().join(role);
    fs::create_dir_all(dir.join("files")).expect("dump dir");
    fs::write(dir.join("artifact.json"), &pack.artifact).expect("artifact");
    fs::write(dir.join("cache_key.txt"), &pack.cache_key).expect("cache key");
    fs::write(dir.join("pack_digest.txt"), &pack.pack_digest).expect("digest");
    fs::write(dir.join("manifest_sha256.txt"), &pack.manifest_sha256).expect("manifest hash");
    let artifact: Value = serde_json::from_str(&pack.artifact).expect("artifact json");
    let files = artifact["files"].as_array().expect("files");
    for file in files {
        let path = file["path"].as_str().expect("path");
        let text = file["text"].as_str().expect("text");
        let target = dir.join("files").join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).expect("file parent");
        }
        fs::write(target, text).expect("file text");
    }
}

fn file_map(pack: &RolePack) -> Vec<(String, String)> {
    let artifact: Value = serde_json::from_str(&pack.artifact).expect("artifact json");
    artifact["files"]
        .as_array()
        .expect("files")
        .iter()
        .map(|file| {
            (
                file["path"].as_str().expect("path").to_owned(),
                file["text"].as_str().expect("text").to_owned(),
            )
        })
        .collect()
}

fn snippet(text: &str, needle: &str) -> String {
    let Some(start) = text.find(needle) else {
        return String::new();
    };
    let mut from = start.saturating_sub(48);
    while from < start && !text.is_char_boundary(from) {
        from += 1;
    }
    let mut to = (start + needle.len() + 48).min(text.len());
    while to > start && !text.is_char_boundary(to) {
        to -= 1;
    }
    text[from..to].replace('\n', "\\n")
}

fn require_present(violations: &mut Vec<String>, files: &[(String, String)], needle: &str) {
    if !files.iter().any(|(_, text)| text.contains(needle)) {
        violations.push(format!("missing legal text `{needle}`"));
    }
}

fn require_absent(violations: &mut Vec<String>, files: &[(String, String)], needle: &str) {
    for (path, text) in files {
        if text.contains(needle) {
            violations.push(format!(
                "`{needle}` in {path}: {}",
                snippet(text, needle)
            ));
        }
    }
}

fn require_file(violations: &mut Vec<String>, files: &[(String, String)], path: &str, present: bool) {
    let found = files.iter().any(|(item, _)| item == path);
    if found != present {
        violations.push(format!("file {path} present={found} expect={present}"));
    }
}

fn heading_present(files: &[(String, String)], heading: &str) -> bool {
    files.iter().any(|(_, text)| text.contains(heading))
}

fn check_hashes(
    violations: &mut Vec<String>,
    label: &str,
    principal: &str,
    role: &str,
    pack: &RolePack,
) {
    let artifact: Value = match serde_json::from_str(&pack.artifact) {
        Ok(value) => value,
        Err(error) => {
            violations.push(format!("{label} artifact is not json: {error}"));
            return;
        }
    };
    if pack.artifact != canonical_json(&artifact) {
        violations.push(format!("{label} artifact bytes are not independent canonical json"));
    }
    if artifact.get("principal").is_some() || pack.artifact.contains("\"principal\"") {
        violations.push(format!("{label} artifact carries principal"));
    }
    let Some(files) = artifact["files"].as_array() else {
        violations.push(format!("{label} artifact has no files"));
        return;
    };
    let mut records = Vec::new();
    let mut total = 0_u64;
    for file in files {
        let Some(text) = file["text"].as_str() else {
            violations.push(format!("{label} file text missing"));
            continue;
        };
        let bytes = text.len() as u64;
        total += bytes;
        let digest = sha256_hex(text.as_bytes());
        if file["sha256"] != digest {
            violations.push(format!(
                "{label} file {} sha256 {} != {digest}",
                file["path"], file["sha256"]
            ));
        }
        if file["bytes"] != bytes {
            violations.push(format!("{label} file {} byte length mismatch", file["path"]));
        }
        records.push(json!({"bytes": bytes, "path": file["path"], "sha256": digest}));
    }
    if pack.total_bytes != total {
        violations.push(format!(
            "{label} total_bytes {total_bytes} != {total}",
            total_bytes = pack.total_bytes
        ));
    }
    if pack.entry_count != files.len() as u64 {
        violations.push(format!("{label} entry_count mismatch"));
    }
    let digest = sha256_hex(canonical_json(&Value::Array(records)).as_bytes());
    if pack.pack_digest != digest {
        violations.push(format!(
            "{label} pack_digest {pack_digest} != {digest}",
            pack_digest = pack.pack_digest
        ));
    }
    let cache_key = format!(
        "{CACHE_PROTOCOL}|project=alpha|principal={principal}|role={role}|policy=pol-alpha@7|knowledge=know-alpha@3|ontology=ont-a172|catalog=cat-a172|compiler={COMPILER}|pack={digest}"
    );
    if pack.cache_key != cache_key {
        violations.push(format!(
            "{label} cache key mismatch\n got {got}\n want {cache_key}",
            got = pack.cache_key
        ));
    }
    let cache_hash = sha256_hex(pack.cache_key.as_bytes());
    if pack.cache_key_sha256 != cache_hash {
        violations.push(format!("{label} cache_key_sha256 mismatch"));
    }
    let manifest = pack.file("manifest.json").unwrap_or("");
    if pack.manifest_sha256 != sha256_hex(manifest.as_bytes()) {
        violations.push(format!("{label} manifest_sha256 mismatch"));
    }
    if let Ok(manifest_value) = serde_json::from_str::<Value>(manifest) {
        let expect = [
            ("authentication", "not_claimed"),
            ("authorization", "not_claimed"),
            ("publication", "not_claimed"),
            ("mount", "not_performed"),
            ("samples", "omitted"),
            ("domains", "unsupported"),
            ("file_splitting", "unsupported"),
            ("search", "unsupported"),
        ];
        for (key, value) in expect {
            if manifest_value[key] != value {
                violations.push(format!("{label} manifest {key} is {}", manifest_value[key]));
            }
        }
        if manifest_value["row_security_reproven"] != false {
            violations.push(format!("{label} row_security_reproven is not false"));
        }
        if manifest_value["tokens"]["status"] != "unsupported"
            || manifest_value["tokens"]["estimator"] != "none"
        {
            violations.push(format!("{label} token status is not unsupported/none"));
        }
        if manifest_value["compiler"] != COMPILER {
            violations.push(format!("{label} manifest compiler mismatch"));
        }
        if manifest_value.get("principal").is_some() {
            violations.push(format!("{label} manifest carries principal"));
        }
        if let Some(listed) = manifest_value["files"].as_array()
            && listed.iter().any(|item| item["path"] == "manifest.json")
        {
            violations.push(format!("{label} manifest lists itself"));
        }
    } else {
        violations.push(format!("{label} manifest is not json"));
    }
}

fn code_suffix(effective: &Value, visibility: &Value) -> String {
    match compile(effective, visibility) {
        Ok(_) => "ok".to_owned(),
        Err(error) => error.code,
    }
}

fn expect_code(violations: &mut Vec<String>, name: &str, got: &str, suffix: &str) {
    let want = format!("{PREFIX}{suffix}");
    if got != want {
        violations.push(format!("refusal {name}: got {got} want {want}"));
    }
}

fn check_refusals(violations: &mut Vec<String>, effective: &Value, visibility: &Value) {
    let mut stale = visibility.clone();
    stale["policy"]["version"] = json!("9");
    expect_code(violations, "stale-policy", &code_suffix(effective, &stale), "binding.policy");

    let mut role = visibility.clone();
    role["role"] = json!("clerk");
    expect_code(violations, "bad-role", &code_suffix(effective, &role), "identity.invalid");

    let mut per_role = effective.clone();
    per_role["ontology"]["compile"]["per_role"] = json!(false);
    expect_code(violations, "per-role", &code_suffix(&per_role, visibility), "compile.per_role");

    let mut raw = effective.clone();
    raw["ontology"]["compile"]["sample_values"]["default"] = json!("raw");
    expect_code(violations, "raw-sample", &code_suffix(&raw, visibility), "sample.raw_refused");

    let mut decisions = effective.clone();
    decisions["decisions"] = json!([]);
    expect_code(
        violations,
        "decisions",
        &code_suffix(&decisions, visibility),
        "unsupported.field",
    );

    let mut samples = visibility.clone();
    samples["datasets"][0]["sample_values"] = json!(["a172-sample"]);
    expect_code(
        violations,
        "sample-values",
        &code_suffix(effective, &samples),
        "unsupported.samples",
    );

    let mut common = visibility.clone();
    common["datasets"][0]["most_common_vals"] = json!(["a172-sample"]);
    expect_code(
        violations,
        "most-common",
        &code_suffix(effective, &common),
        "unsupported.samples",
    );

    let mut entries = visibility.clone();
    entries["limits"]["max_entries"] = json!(0);
    expect_code(violations, "entries", &code_suffix(effective, &entries), "limit.invalid");

    let mut false_proof = visibility.clone();
    false_proof["datasets"][1]["global_stats_proof"]["proven"] = json!(false);
    expect_code(
        violations,
        "proof-false",
        &code_suffix(effective, &false_proof),
        "evidence.unproven",
    );

    let mut missing_proof = visibility.clone();
    missing_proof["datasets"][1]
        .as_object_mut()
        .expect("scale")
        .remove("global_stats_proof");
    expect_code(
        violations,
        "proof-missing",
        &code_suffix(effective, &missing_proof),
        "evidence.unproven",
    );

    let mut extra_proof = visibility.clone();
    extra_proof["datasets"][0]["global_stats_proof"] = json!({"ref": "proof-parcel", "proven": true});
    expect_code(
        violations,
        "proof-on-denied",
        &code_suffix(effective, &extra_proof),
        "evidence.unproven",
    );

    let mut unknown = visibility.clone();
    unknown["datasets"][0]["columns"]
        .as_array_mut()
        .expect("columns")
        .push(json!("bare_col"));
    expect_code(
        violations,
        "unknown-pii",
        &code_suffix(effective, &unknown),
        "classification.unknown_pii",
    );

    let mut pending = visibility.clone();
    pending["datasets"][0]["columns"]
        .as_array_mut()
        .expect("columns")
        .push(json!("pending_mark"));
    expect_code(
        violations,
        "incomplete",
        &code_suffix(effective, &pending),
        "classification.incomplete",
    );

    let crossed = normalize_pack_path("../hold_bin").expect_err("traversal");
    if crossed.code != format!("{PREFIX}path.invalid") || crossed.path != "/" {
        violations.push(format!(
            "path error {code} at {path}",
            code = crossed.code,
            path = crossed.path
        ));
    }
    let rendered = crossed.to_string();
    if rendered.contains("hold_bin") || rendered.contains("..") {
        violations.push(format!("path error echoes input: {rendered}"));
    }
    let nfc_path = normalize_pack_path("e\u{0301}").expect("nfc path");
    if nfc_path != "é" {
        violations.push(format!("nfc path normalized to {nfc_path}"));
    }
    let duplicate = "{\"protocol\":\"oo.builder.effective-oos/v1\",\"protocol\":\"again\",\"leak\":\"password=a172-sentinel\"}";
    let error = compile_role_pack(duplicate, "{}").expect_err("duplicate");
    let display = error.to_string();
    if error.code != "oo.builder.effective-oos/v1/json.duplicate_key" || display.contains("a172-sentinel") || display.contains("password")
    {
        violations.push(format!("duplicate-key display leaked: {display}"));
    }
}

fn hidden_needles() -> Vec<String> {
    vec![
        "hold_bin".to_owned(),
        "HOLD_BIN".to_owned(),
        "quarter_start".to_owned(),
        "暂存格".to_owned(),
        NFC_LABEL.to_owned(),
        NFD_LABEL.to_owned(),
        QUOTE_LABEL.to_owned(),
        "签\"\"记".to_owned(),
        APOSTROPHE_LABEL.to_owned(),
        "签''记".to_owned(),
        "泊位单".to_owned(),
        "pier".to_owned(),
        "berth".to_owned(),
        "a172-hidden-body".to_owned(),
        "5151515".to_owned(),
        "7777777".to_owned(),
        "review.a172".to_owned(),
        "class.a172".to_owned(),
        "owner.a172".to_owned(),
        "0.191919".to_owned(),
        "0.818181".to_owned(),
        "616161".to_owned(),
        "0.272727".to_owned(),
        "183213".to_owned(),
        "ctrl_hold".to_owned(),
        "guessed_link".to_owned(),
        "锚点泄漏".to_owned(),
        "numeric hold_bin".to_owned(),
        "a172-sample".to_owned(),
    ]
}

fn check_clerk(violations: &mut Vec<String>, pack: &RolePack) -> Vec<Value> {
    let files = file_map(pack);
    for needle in hidden_needles() {
        require_absent(violations, &files, &needle);
    }
    let legal = [
        "tables/dock.parcel.md",
        "bigint",
        "包裹号",
        "本季到港重量",
        "可见标记说明",
        "mark_ok",
        "mass_kg",
        "unit kg",
        "estimated rows: 19",
        "proof-scale-a172",
        "did not re-prove",
        "Samples are omitted because pii is personal",
        "Estimated row count is omitted",
        "source-declared",
        "declared_link",
        "parcel_scale",
        "## kept_mass",
        "## cmp_metric",
        "## count_metric",
        "## string_metric",
        "净\"\"重",
        "it''s visible",
        "SeLeCt",
        "\"dock\" . \"parcel\"",
        "UNION ALL",
        "-- ex-kept\n",
        "-- ex-kept-quote\n",
        "-- ex-scale\n",
        "## 在港",
        "本季到港重量，只计可见列",
        "本季:",
        "`month_start`",
        "`today`",
        "Asia/Shanghai",
        "does not authenticate",
        "does not grant",
        "Meaning is unconfirmed",
        "oo_r_clerk",
    ];
    for needle in legal {
        require_present(violations, &files, needle);
    }
    require_file(violations, &files, "tables/dock.parcel.md", true);
    require_file(violations, &files, "tables/dock.scale.md", true);
    require_file(violations, &files, "tables/pier.berth.md", false);
    require_file(violations, &files, "entities/parcel.md", true);
    require_file(violations, &files, "entities/berth.md", false);
    require_file(violations, &files, "examples/verified.sql", true);
    require_file(violations, &files, "calendar.md", true);
    require_file(violations, &files, "domains/dock.md", false);
    let omitted = [
        "## mix_quoted",
        "## mix_case",
        "## mix_ws",
        "## mix_alias",
        "## mix_nfc",
        "## mix_token",
        "## mix_path",
        "## sq_leak",
        "## prose_metric",
        "## join_metric",
        "## or_metric",
        "## comment_metric",
        "## semi_metric",
        "## star_metric",
        "## with_metric",
        "## cast_metric",
        "## sub_metric",
        "## distinct_cols",
        "## union_bare",
        "## unclosed_metric",
        "## unit_leak",
        "## ctrl_leak",
        "## syn_leak",
        "## nfd_leak",
        "-- ex-hidden-quote",
        "-- ex-nfc",
        "-- ex-join",
        "-- ex-prose",
        "-- ex-double",
        "## 引用泊位",
        "## 草稿",
        "## 泊位术语",
    ];
    let mut subset = Vec::new();
    for heading in omitted {
        let kept = heading_present(&files, heading);
        subset.push(json!({"name": heading, "kept": kept, "expect_kept": false}));
        if kept {
            violations.push(format!("hidden or unsupported object kept: {heading}"));
        }
    }
    for heading in ["## kept_mass", "## cmp_metric", "## count_metric", "## string_metric"] {
        let kept = heading_present(&files, heading);
        subset.push(json!({"name": heading, "kept": kept, "expect_kept": true}));
        if !kept {
            violations.push(format!("legal object omitted: {heading}"));
        }
    }
    let distinct_bare = heading_present(&files, "## distinct_bare");
    subset.push(json!({
        "name": "## distinct_bare",
        "kept": distinct_bare,
        "expect_kept": false,
        "note": "observation only; ordinary SELECT DISTINCT col is ## distinct_cols"
    }));
    subset
}

fn check_other_role(violations: &mut Vec<String>, role: &str, pack: &RolePack, absent: &[&str], present: &[&str]) {
    let files = file_map(pack);
    for needle in hidden_needles() {
        if role == "pier" && matches!(needle.as_str(), "pier" | "berth" | "泊位单") {
            continue;
        }
        require_absent(violations, &files, &needle);
    }
    for needle in absent {
        require_absent(violations, &files, needle);
    }
    for needle in present {
        require_present(violations, &files, needle);
    }
    if files.iter().any(|(path, _)| path.starts_with("domains/")) {
        violations.push(format!("{role} produced a domains file"));
    }
}

fn collision_observation(effective_shape: &Value) -> Value {
    let mut effective = effective_shape.clone();
    effective["relationships"] = json!([{
        "name": "hold_bin",
        "from": {"dataset": "dock.parcel", "columns": ["parcel_id"]},
        "to": {"dataset": "dock.scale", "columns": ["scale_id"]},
        "cardinality": "many_to_one",
        "join_hint": "left",
        "provenance": "declared_fk"
    }]);
    effective["entities"] = json!([{
        "schema_version": "oos/v1",
        "entity": "hold_bin",
        "label": "可见实体",
        "primary_dataset": "dock.parcel",
        "key": "parcel_id",
        "display": "parcel_id",
        "members": [{"dataset": "dock.parcel", "key": ["parcel_id"]}],
        "datasets": ["dock.parcel"]
    }]);
    effective["metrics"] = json!([]);
    effective["glossary"] = json!([]);
    effective["examples"] = json!([]);
    effective["enums"] = json!([]);
    let visibility = visibility_for(
        "ada",
        "oo_r_clerk",
        vec![
            json!({
                "dataset": "dock.parcel",
                "row_security": "none",
                "global_stats": "denied",
                "columns": ["parcel_id", "mass_kg", "badge", "status", "mark_ok"]
            }),
            json!({
                "dataset": "dock.scale",
                "row_security": "none",
                "global_stats": "denied",
                "columns": ["scale_id", "mass_kg"]
            }),
        ],
    );
    match compile(&effective, &visibility) {
        Ok(pack) => {
            dump_pack("collision", &pack);
            let files = file_map(&pack);
            let relationship = heading_present(&files, "## hold_bin");
            let entity = files.iter().any(|(path, _)| path == "entities/hold_bin.md");
            json!({
                "compiled": true,
                "relationship_name_copied": relationship,
                "entity_name_copied": entity
            })
        }
        Err(error) => json!({"compiled": false, "error": error.to_string()}),
    }
}

#[test]
fn fresh_closure_cases_and_independent_oracle() {
    assert_ne!(NFD_LABEL, NFC_LABEL);
    assert_eq!(NFD_LABEL.nfc().collect::<String>(), NFC_LABEL);

    let (effective, clerk_vis) = fresh_documents();
    let clerk = must(&effective, &clerk_vis);
    dump_pack("clerk", &clerk);
    let mut ada2_vis = clerk_vis.clone();
    ada2_vis["principal"] = json!("ada2");
    let ada2 = must(&effective, &ada2_vis);
    dump_pack("ada2", &ada2);
    let audit = must(&effective, &audit_visibility());
    dump_pack("audit", &audit);
    let pier = must(&effective, &pier_visibility());
    dump_pack("pier", &pier);

    let mut violations = Vec::new();
    if clerk.artifact != ada2.artifact || clerk.pack_digest != ada2.pack_digest {
        violations.push("ada and ada2 artifacts differ".to_owned());
    }
    if clerk.cache_key == ada2.cache_key {
        violations.push("ada and ada2 cache keys match".to_owned());
    }
    if !clerk.cache_key.contains("|principal=ada|") || !ada2.cache_key.contains("|principal=ada2|") {
        violations.push("cache keys do not bind the principal".to_owned());
    }
    if clerk.pack_digest == audit.pack_digest || clerk.pack_digest == pier.pack_digest {
        violations.push("role artifacts were not separated".to_owned());
    }

    check_hashes(&mut violations, "clerk", "ada", "oo_r_clerk", &clerk);
    check_hashes(&mut violations, "ada2", "ada2", "oo_r_clerk", &ada2);
    check_hashes(&mut violations, "audit", "cy", "oo_r_audit", &audit);
    check_hashes(&mut violations, "pier", "bea", "oo_r_pier", &pier);
    let subset = check_clerk(&mut violations, &clerk);
    check_other_role(
        &mut violations,
        "audit",
        &audit,
        &["dock.parcel", "包裹", "kept_mass", "本季到港重量"],
        &[
            "oo_r_audit",
            "本季:",
            "`month_start`",
            "unit kg",
            "estimated rows: 19",
            "## count_metric",
            "does not authenticate",
        ],
    );
    let audit_files = file_map(&audit);
    require_file(&mut violations, &audit_files, "tables/dock.scale.md", true);
    require_file(&mut violations, &audit_files, "tables/dock.parcel.md", false);
    require_file(&mut violations, &audit_files, "tables/pier.berth.md", false);
    check_other_role(
        &mut violations,
        "pier",
        &pier,
        &["dock.parcel", "dock.scale", "mass_kg", "包裹", "kept_mass", "5151515"],
        &[
            "oo_r_pier",
            "泊位单",
            "berth_id",
            "本季:",
            "`month_start`",
            "does not grant",
            "Estimated row count is omitted",
        ],
    );
    let pier_files = file_map(&pier);
    require_file(&mut violations, &pier_files, "tables/pier.berth.md", true);
    require_file(&mut violations, &pier_files, "entities/berth.md", true);
    require_file(&mut violations, &pier_files, "tables/dock.parcel.md", false);
    require_file(&mut violations, &pier_files, "tables/dock.scale.md", false);
    check_refusals(&mut violations, &effective, &clerk_vis);
    let collision = collision_observation(&effective);
    let distinct_bare = subset
        .iter()
        .find(|item| item["name"] == "## distinct_bare")
        .cloned()
        .unwrap_or(json!(null));

    let outcome = json!({
        "closure_violations": violations,
        "subset": subset,
        "distinct_bare": distinct_bare,
        "collision": collision,
        "nfc_label_equals_nfd": NFD_LABEL.nfc().collect::<String>() == NFC_LABEL,
        "clerk_pack_digest": clerk.pack_digest,
        "audit_pack_digest": audit.pack_digest,
        "pier_pack_digest": pier.pack_digest,
        "ada2_pack_digest": ada2.pack_digest,
        "clerk_files": file_map(&clerk).into_iter().map(|(path, _)| path).collect::<Vec<_>>()
    });
    let root = out_root();
    fs::create_dir_all(&root).expect("out root");
    fs::write(root.join("outcome.json"), serde_json::to_string_pretty(&outcome).expect("outcome")).expect("write outcome");
    assert!(
        outcome["closure_violations"].as_array().expect("list").is_empty(),
        "closure violations:\n{}",
        outcome["closure_violations"]
    );
}
