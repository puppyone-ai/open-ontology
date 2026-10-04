//! Independent closure oracle for eff1959b.
//!
//! The visibility snapshot is an input. This test does not authenticate, does
//! not grant, and does not execute SQL. Digests use `sha2` 0.10.9 in this
//! file. They do not call the library hash helper. No Python.

#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use oo_builder::{RolePack, compile_role_pack, normalize_pack_path};

const PREFIX: &str = "oo.builder.role-pack/v1/";
const NFD_LABEL: &str = "e\u{0301}舷签";
const LEGAL_STRING: &str = "SELECT 'it''s visible' AS label_ok FROM wharf.crate";
const SQ_LEAK: &str = "SELECT '舱''记' AS mass_kg FROM wharf.crate";
const DQ_LEAK: &str = "SELECT mass_kg AS \"舱\"\"记\" FROM wharf.crate";
const QUOTED_KEPT: &str = "SELECT mass_kg AS \"净\"\"重\" FROM \"wharf\" . \"crate\"";
const QUOTED_EXAMPLE: &str = "SELECT mass_kg AS \"净重\" FROM \"wharf\".\"crate\"";
const WHERE_SQL: &str =
    "SELECT mass_kg FROM wharf.crate WHERE mass_kg >= 1 AND mass_kg IS NOT NULL";
const KEPT_MASS: &str = "SELECT sum(mass_kg) AS total_kg FROM wharf.crate";
const SCALE_COUNT: &str = "SELECT count(*) AS scale_n FROM wharf.scale";
const DISTINCT_COLS: &str = "SELECT DISTINCT mass_kg FROM wharf.crate";
const DISTINCT_BARE: &str = "SELECT DISTINCT FROM wharf.crate";
const DISTINCT_LOWER: &str = "select distinct mass_kg from wharf.crate";
const DISTINCT_QUOTED: &str = "SELECT mass_kg AS \"DISTINCT\" FROM wharf.crate";
const PROSE: &str = "本季到港只计可见列，净重以公斤计。";

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

fn code_of(effective: &Value, visibility: &Value) -> String {
    match compile(effective, visibility) {
        Ok(_) => "compiled".to_owned(),
        Err(error) => error.code,
    }
}

fn none_col(name: &str) -> Value {
    json!({
        "name": name,
        "pii": "none",
        "classification": {"state": "confirmed"}
    })
}

fn metric(
    name: &str,
    label: &str,
    description: &str,
    unit: &str,
    sql: &str,
    control_sql: &str,
) -> Value {
    json!({
        "metric": name,
        "label": label,
        "description": description,
        "owner": "owner.a176",
        "certified": true,
        "unit": unit,
        "status": "active",
        "default_anchor": "month_start",
        "sql": sql,
        "tests": [{"params": {}, "expect": {"rows": 1}}],
        "control_total": {
            "as_of": "month_start",
            "source": "wharf",
            "sql": control_sql,
            "tolerance": {"abs": "1"}
        }
    })
}

fn example(id: &str, question: &str, sql: &str, uses: &[&str]) -> Value {
    json!({
        "id": id,
        "question": question,
        "sql": sql,
        "uses": uses,
        "verified_by": "review.a176",
        "verified_at": "2026-08-19"
    })
}

fn documents() -> (Value, Value, String) {
    let nfc_label: String = NFD_LABEL.nfc().collect();
    let mut crate_id = none_col("crate_id");
    crate_id["label"] = json!("箱号");
    crate_id["data_type"] = json!("bigint");
    crate_id["description"] = json!("本季到港重量");
    crate_id["review"] = json!({"state": "approved", "by": "review.a176"});

    let mut mass = none_col("mass_kg");
    mass["label"] = json!("净重");
    mass["data_type"] = json!("numeric vault_bin");
    mass["unit"] = json!("VAULT_BIN");
    mass["semantic_type"] = json!("money");
    mass["description"] = json!(format!("对照 \"pier\".\"berth\" 与 {nfc_label}"));
    mass["review"] = json!({"state": "approved", "by": "review.a176"});

    let mut badge = none_col("badge");
    badge["pii"] = json!("personal");
    badge["classification"]["by"] = json!("class.a176");

    let mut mark_ok = none_col("mark_ok");
    mark_ok["label"] = json!(nfc_label.clone());
    mark_ok["description"] = json!("可见标记说明");
    mark_ok["review"] = json!({"state": "approved", "by": "review.a176"});

    let mut status = none_col("status");
    status["enum"] = json!("wharf.crate.status");

    let vault = json!({
        "name": "vault_bin",
        "label": "暂存舱",
        "pii": "none",
        "classification": {"state": "confirmed"}
    });
    let tick_sq = json!({
        "name": "tick_sq",
        "label": "舱'记",
        "pii": "none",
        "classification": {"state": "confirmed"}
    });
    let tick_dq = json!({
        "name": "tick_dq",
        "label": "舱\"记",
        "pii": "none",
        "classification": {"state": "confirmed"}
    });
    let mark_nfd = json!({
        "name": "mark_nfd",
        "label": NFD_LABEL,
        "pii": "none",
        "classification": {"state": "confirmed"}
    });
    let quarter = none_col("quarter_start");

    let crate_ds = json!({
        "schema_version": "oos/v1",
        "dataset": "wharf.crate",
        "label": "货箱",
        "tier": "file",
        "grain": "one row per crate",
        "primary_key": ["crate_id"],
        "row_count_estimate": 616161,
        "columns": [
            crate_id, mass, badge, mark_ok, status, vault, tick_sq, tick_dq, mark_nfd, quarter
        ]
    });
    let mut scale_mass = none_col("mass_kg");
    scale_mass["unit"] = json!("kg");
    let scale = json!({
        "schema_version": "oos/v1",
        "dataset": "wharf.scale",
        "label": "秤台",
        "tier": "file",
        "grain": "one row per scale",
        "primary_key": ["scale_id"],
        "row_count_estimate": 23,
        "columns": [none_col("scale_id"), scale_mass]
    });
    let pier = json!({
        "schema_version": "oos/v1",
        "dataset": "pier.berth",
        "label": "泊位单",
        "tier": "file",
        "grain": "one row per berth",
        "primary_key": ["berth_id"],
        "row_count_estimate": 828282,
        "columns": [none_col("berth_id")]
    });

    let mut kept_mass = metric(
        "kept_mass",
        "可见重量",
        "visible mass",
        "kg",
        KEPT_MASS,
        SCALE_COUNT,
    );
    kept_mass["enum_filters"] = json!([{
        "column": "wharf.crate.status",
        "enum": "wharf.crate.status",
        "classes": ["open"]
    }]);

    let crate_scale = json!({
        "name": "crate_scale",
        "from": {"dataset": "wharf.crate", "columns": ["mass_kg"]},
        "to": {"dataset": "wharf.scale", "columns": ["mass_kg"]},
        "cardinality": "many_to_one",
        "join_hint": "left",
        "provenance": "human",
        "confidence": "0.272727",
        "review": {"state": "approved", "by": "review.a176"},
        "evidence": {
            "containment": "0.818181",
            "name_match": "exact",
            "type_compatible": true,
            "seen_in_query_log": 434343
        }
    });
    let hidden_rel = json!({
        "name": "vault_bin",
        "from": {"dataset": "wharf.crate", "columns": ["mass_kg"]},
        "to": {"dataset": "wharf.scale", "columns": ["mass_kg"]},
        "cardinality": "many_to_one",
        "join_hint": "left",
        "provenance": "declared_fk"
    });

    let entity = |name: &str, dataset: &str, key: &str, display: &str| {
        json!({
            "schema_version": "oos/v1",
            "entity": name,
            "label": "Crate",
            "primary_dataset": dataset,
            "key": key,
            "display": display,
            "members": [{"dataset": dataset, "key": [key], "name": display}],
            "datasets": [dataset]
        })
    };

    let effective = json!({
        "protocol": "oo.builder.effective-oos/v1",
        "project": "alpha",
        "ontology_version": "ont-a176",
        "catalog_version": "cat-a176",
        "policy": {"id": "pol-alpha", "version": "7"},
        "knowledge": {"id": "know-alpha", "version": "3"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "wharfmap",
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
        "datasets": [crate_ds, scale, pier],
        "relationships": [crate_scale, hidden_rel],
        "metrics": [
            metric("string_metric", "可见字符串", "可见字符串，撇号仍按原文保留。", "kg", LEGAL_STRING, SCALE_COUNT),
            metric("sq_leak", "单引号标签", "escaped hidden label", "kg", SQ_LEAK, SCALE_COUNT),
            metric("quote_leak", "双引号标签", "escaped hidden alias", "kg", DQ_LEAK, SCALE_COUNT),
            metric("quoted_metric", "引号重量", PROSE, "kg", QUOTED_KEPT, SCALE_COUNT),
            metric("where_metric", "条件重量", "visible comparison", "kg", WHERE_SQL, SCALE_COUNT),
            metric("quoted_distinct", "引号保留字", "quoted identifier", "kg", DISTINCT_QUOTED, SCALE_COUNT),
            metric("scale_count", "秤台计数", "visible scale count", "kg", SCALE_COUNT, SCALE_COUNT),
            kept_mass,
            metric("control_leak", "对账泄漏", "legal display sql", "kg", LEGAL_STRING, SQ_LEAK),
            metric("unit_leak", "单位泄漏", "visible unit carrier", "vault_bin", "SELECT sum(mass_kg) AS unit_leak_alias FROM wharf.crate", SCALE_COUNT),
            metric("nfc_leak", "组合泄漏", &format!("备注里出现了{nfc_label}"), "kg", "SELECT mass_kg AS nfc_alias FROM wharf.crate", SCALE_COUNT),
            metric("quoted_table", "引用泊位", "quoted hidden dataset", "kg", "SELECT mass_kg FROM \"pier\".\"berth\"", SCALE_COUNT),
            metric("distinct_cols", "列去重", "distinct before a column", "kg", DISTINCT_COLS, SCALE_COUNT),
            metric("distinct_bare", "退化去重", "degenerate distinct", "kg", DISTINCT_BARE, SCALE_COUNT),
            metric("distinct_lower", "小写去重", "lowercase distinct", "kg", DISTINCT_LOWER, SCALE_COUNT),
            metric("open_quote", "未闭合", "see '舱''记", "kg", "SELECT mass_kg AS open_alias FROM wharf.crate", SCALE_COUNT)
        ],
        "glossary": [
            {"term": "在港", "definition": "it's a visible note", "maps_to": {"dataset": "wharf.crate"}, "status": "approved"},
            {"term": "净重口径", "definition": PROSE, "maps_to": {"dataset": "wharf.crate"}, "status": "approved"},
            {"term": "引用泊位", "definition": "对照 \"pier\".\"berth\" 的泊位单", "maps_to": {"dataset": "wharf.crate"}, "status": "approved"},
            {"term": "草稿", "definition": "draft wording", "maps_to": {"dataset": "wharf.crate"}, "status": "draft"},
            {"term": "泊位术语", "definition": "hidden dataset term", "maps_to": {"dataset": "pier.berth"}, "status": "approved"}
        ],
        "examples": [
            example("ex-kept", "可见关系还在，净重仍可引用。", "SELECT mass_kg AS rel_kept_alias FROM wharf.crate", &["rel:crate_scale", "dataset:wharf.crate", "column:wharf.crate.mass_kg"]),
            example("ex-rel-hidden", "hidden relationship question", "SELECT mass_kg AS rel_hidden_alias FROM wharf.crate", &["rel:vault_bin", "dataset:wharf.crate", "column:wharf.crate.mass_kg"]),
            example("ex-quote", "可见引号查询，净重仍在。", QUOTED_EXAMPLE, &["dataset:wharf.crate", "column:wharf.crate.mass_kg"]),
            example("ex-string", "可见字符串示例，撇号按原文保留。", LEGAL_STRING, &["dataset:wharf.crate", "column:wharf.crate.mass_kg"]),
            example("ex-hidden-quote", "how the hidden berth is referenced", "SELECT mass_kg FROM \"pier\".\"berth\"", &["dataset:wharf.crate", "column:wharf.crate.mass_kg"])
        ],
        "enums": [{
            "schema_version": "oos/v1",
            "enum": "wharf.crate.status",
            "classes": {"open": "open crate", "unexplained": "meaning is not assigned"},
            "values": [
                {"code": "ok", "label": "完好", "class": "open", "status": "approved"},
                {"code": "x", "label": "unknown", "class": "unexplained", "status": "unexplained", "evidence": {"rows": 191919}}
            ],
            "coverage": "0.414141",
            "unexplained_values": ["x"]
        }],
        "entities": [
            entity("crate", "wharf.crate", "crate_id", "mass_kg"),
            entity("vault_bin", "wharf.crate", "crate_id", "mass_kg"),
            entity("berth", "pier.berth", "berth_id", "berth_id")
        ],
        "calendar": {
            "schema_version": "oos/v1",
            "business_tz": "Asia/Shanghai",
            "fiscal_year_start_month": 4,
            "week_start": "monday",
            "anchors": ["month_start", "today", "quarter_start"],
            "phrases": {
                "vault_bin": {"start": "month_start", "end": "today", "bounds": "[)"},
                "暂存舱": {"start": "month_start", "end": "today", "bounds": "[)"},
                "本季": {"start": "month_start", "end": "today", "bounds": "[)"},
                "锚点泄漏": {"start": "quarter_start", "end": "today", "bounds": "[)"}
            }
        }
    });
    let mut phrases = effective["calendar"]["phrases"].clone();
    phrases[nfc_label.as_str()] = json!({"start": "month_start", "end": "today", "bounds": "[)"});
    let mut effective = effective;
    effective["calendar"]["phrases"] = phrases;

    let visibility = json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": "alpha",
        "principal": "ada",
        "role": "oo_r_clerk",
        "ontology_version": "ont-a176",
        "catalog_version": "cat-a176",
        "policy": {"id": "pol-alpha", "version": "7"},
        "knowledge": {"id": "know-alpha", "version": "3"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [
            {
                "dataset": "wharf.crate",
                "row_security": "rls",
                "global_stats": "denied",
                "columns": ["crate_id", "mass_kg", "badge", "mark_ok", "status"]
            },
            {
                "dataset": "wharf.scale",
                "row_security": "rls",
                "global_stats": "permitted",
                "global_stats_proof": {"ref": "proof-scale-a176", "proven": true},
                "columns": ["scale_id", "mass_kg"]
            }
        ],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    });
    (effective, visibility, nfc_label)
}

fn out_dir() -> PathBuf {
    PathBuf::from(std::env::var("OO_A176_OUT").unwrap_or_else(|_| "/tmp/a176-out".to_owned()))
}

fn dump(root: &std::path::Path, name: &str, pack: &RolePack) {
    let dir = root.join(name);
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
        let full = dir.join("files").join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("parent");
        }
        fs::write(full, text).expect("file");
    }
}

fn body(pack: &RolePack) -> String {
    let artifact: Value = serde_json::from_str(&pack.artifact).expect("artifact json");
    let mut out = String::new();
    for file in artifact["files"].as_array().expect("files") {
        out.push_str(file["path"].as_str().expect("path"));
        out.push('\n');
        out.push_str(file["text"].as_str().expect("text"));
        out.push('\n');
    }
    out
}

fn absent(text: &str, token: &str, violations: &mut Vec<String>) {
    if text.contains(token) {
        violations.push(format!("present `{token}`"));
    }
}

fn present(text: &str, token: &str, misses: &mut Vec<String>) {
    if !text.contains(token) {
        misses.push(format!("absent `{token}`"));
    }
}

fn table_row<'a>(table: &'a str, column: &str) -> &'a str {
    let needle = format!("| `{column}` |");
    table
        .lines()
        .find(|line| line.starts_with(&needle))
        .unwrap_or("")
}

fn cache_key(principal: &str, role: &str, digest: &str) -> String {
    format!(
        "oo.builder.role-pack-cache/v1|project=alpha|principal={principal}|role={role}|policy=pol-alpha@7|knowledge=know-alpha@3|ontology=ont-a176|catalog=cat-a176|compiler=oo-builder/role-pack/v1|pack={digest}"
    )
}

fn check_hash(pack: &RolePack, principal: &str, role: &str, violations: &mut Vec<String>) {
    let artifact: Value = serde_json::from_str(&pack.artifact).expect("artifact json");
    if pack.artifact != canonical_json(&artifact) {
        violations.push(format!("{role} artifact is not canonical"));
    }
    if pack.artifact.ends_with('\n') {
        violations.push(format!("{role} artifact has a trailing newline"));
    }
    if artifact.get("principal").is_some() || pack.artifact.contains("principal") {
        violations.push(format!("{role} artifact names a principal"));
    }
    let files = artifact["files"].as_array().expect("files");
    let mut records = Vec::new();
    let mut total = 0_u64;
    for file in files {
        let text = file["text"].as_str().expect("text");
        let bytes = text.len() as u64;
        total += bytes;
        let digest = sha256_hex(text.as_bytes());
        if file["sha256"] != digest {
            violations.push(format!("{role} file hash {}", file["path"]));
        }
        if file["bytes"] != bytes {
            violations.push(format!("{role} file bytes {}", file["path"]));
        }
        records.push(json!({"bytes": bytes, "path": file["path"], "sha256": digest}));
    }
    records.sort_by(|left, right| {
        left["path"]
            .as_str()
            .unwrap_or("")
            .cmp(right["path"].as_str().unwrap_or(""))
    });
    let digest = sha256_hex(canonical_json(&Value::Array(records)).as_bytes());
    if pack.pack_digest != digest || artifact["pack_digest"] != digest {
        violations.push(format!("{role} pack_digest"));
    }
    let key = cache_key(principal, role, &digest);
    if pack.cache_key != key {
        violations.push(format!("{role} cache key rebuilt"));
    }
    let key_hash = sha256_hex(key.as_bytes());
    if pack.cache_key_sha256 != key_hash {
        violations.push(format!("{role} cache_key_sha256"));
    }
    let manifest = pack.file("manifest.json").unwrap_or("");
    let manifest_hash = sha256_hex(manifest.as_bytes());
    if pack.manifest_sha256 != manifest_hash || artifact["manifest_sha256"] != manifest_hash {
        violations.push(format!("{role} manifest_sha256"));
    }
    if artifact["total_bytes"] != total || pack.total_bytes != total {
        violations.push(format!("{role} total_bytes"));
    }
    if artifact["entry_count"] != files.len() as u64 {
        violations.push(format!("{role} entry_count"));
    }
    let parsed: Value = serde_json::from_str(manifest).unwrap_or(Value::Null);
    for (field, expect) in [
        ("authentication", "not_claimed"),
        ("authorization", "not_claimed"),
        ("publication", "not_claimed"),
        ("mount", "not_performed"),
        ("samples", "omitted"),
        ("domains", "unsupported"),
        ("file_splitting", "unsupported"),
        ("search", "unsupported"),
    ] {
        if parsed[field] != expect {
            violations.push(format!("{role} manifest {field}"));
        }
    }
    if parsed["row_security_reproven"] != false {
        violations.push(format!("{role} row_security_reproven"));
    }
    if parsed["tokens"]["status"] != "unsupported" || parsed["tokens"]["estimator"] != "none" {
        violations.push(format!("{role} tokens"));
    }
    if parsed.get("principal").is_some() {
        violations.push(format!("{role} manifest principal"));
    }
    if parsed["role"] != role || parsed["project"] != "alpha" {
        violations.push(format!("{role} manifest identity"));
    }
    let paths: Vec<&str> = files
        .iter()
        .filter_map(|file| file["path"].as_str())
        .collect();
    if paths.iter().any(|path| path.starts_with("domains/")) {
        violations.push(format!("{role} domains file"));
    }
}

fn expect_code(got: &str, expect: &str, mismatches: &mut Vec<String>) {
    let full = format!("{PREFIX}{expect}");
    if got != full {
        mismatches.push(format!("got `{got}` expected `{full}`"));
    }
}

fn check_refusals(effective: &Value, visibility: &Value, mismatches: &mut Vec<String>) {
    let mut stale = visibility.clone();
    stale["policy"]["version"] = json!("8");
    expect_code(&code_of(effective, &stale), "binding.policy", mismatches);

    let mut role = visibility.clone();
    role["role"] = json!("clerk");
    expect_code(&code_of(effective, &role), "identity.invalid", mismatches);

    let mut per_role = effective.clone();
    per_role["ontology"]["compile"]["per_role"] = json!(false);
    expect_code(
        &code_of(&per_role, visibility),
        "compile.per_role",
        mismatches,
    );

    let mut raw = effective.clone();
    raw["ontology"]["compile"]["sample_values"]["default"] = json!("raw");
    expect_code(&code_of(&raw, visibility), "sample.raw_refused", mismatches);

    let mut missing_proof = visibility.clone();
    missing_proof["datasets"][1]
        .as_object_mut()
        .expect("scale")
        .remove("global_stats_proof");
    expect_code(
        &code_of(effective, &missing_proof),
        "evidence.unproven",
        mismatches,
    );

    let mut false_proof = visibility.clone();
    false_proof["datasets"][1]["global_stats_proof"]["proven"] = json!(false);
    expect_code(
        &code_of(effective, &false_proof),
        "evidence.unproven",
        mismatches,
    );

    let mut non_rls = visibility.clone();
    non_rls["datasets"][1]["row_security"] = json!("none");
    expect_code(
        &code_of(effective, &non_rls),
        "evidence.unproven",
        mismatches,
    );

    let mut unknown = effective.clone();
    unknown["datasets"][0]["columns"]
        .as_array_mut()
        .expect("columns")
        .push(json!({"name": "bare_pii", "classification": {"state": "confirmed"}}));
    let mut unknown_vis = visibility.clone();
    unknown_vis["datasets"][0]["columns"]
        .as_array_mut()
        .expect("columns")
        .push(json!("bare_pii"));
    expect_code(
        &code_of(&unknown, &unknown_vis),
        "classification.unknown_pii",
        mismatches,
    );

    let mut pending = effective.clone();
    pending["datasets"][0]["columns"]
        .as_array_mut()
        .expect("columns")
        .push(json!({
            "name": "pending_col",
            "pii": "none",
            "classification": {"state": "pending"}
        }));
    let mut pending_vis = visibility.clone();
    pending_vis["datasets"][0]["columns"]
        .as_array_mut()
        .expect("columns")
        .push(json!("pending_col"));
    expect_code(
        &code_of(&pending, &pending_vis),
        "classification.incomplete",
        mismatches,
    );

    let mut samples = visibility.clone();
    samples["datasets"][0]["sample_values"] = json!("masked");
    expect_code(
        &code_of(effective, &samples),
        "unsupported.samples",
        mismatches,
    );

    let crossed = normalize_pack_path("../vault_bin").expect_err("traversal");
    if crossed.code != format!("{PREFIX}path.invalid") || crossed.path != "/" {
        mismatches.push(format!("path code {} at {}", crossed.code, crossed.path));
    }
    if crossed.to_string().contains("vault_bin") {
        mismatches.push("path display echoed the rejected text".to_owned());
    }
}

#[test]
fn fresh_literal_name_and_distinct_closure() {
    let nfc_probe: String = NFD_LABEL.nfc().collect();
    assert_ne!(nfc_probe, NFD_LABEL, "fixture NFC and NFD must differ");

    let (effective, visibility, nfc_label) = documents();
    assert_eq!(nfc_label, nfc_probe);
    let clerk = must(&effective, &visibility);
    let mut ada2_vis = visibility.clone();
    ada2_vis["principal"] = json!("ada2");
    let ada2 = must(&effective, &ada2_vis);

    let mut audit_vis = visibility.clone();
    audit_vis["principal"] = json!("cy");
    audit_vis["role"] = json!("oo_r_audit");
    audit_vis["datasets"] = json!([{
        "dataset": "wharf.scale",
        "row_security": "rls",
        "global_stats": "permitted",
        "global_stats_proof": {"ref": "proof-scale-a176", "proven": true},
        "columns": ["scale_id", "mass_kg"]
    }]);
    let audit = must(&effective, &audit_vis);

    let mut pier_vis = visibility.clone();
    pier_vis["principal"] = json!("bea");
    pier_vis["role"] = json!("oo_r_pier");
    pier_vis["datasets"] = json!([{
        "dataset": "pier.berth",
        "row_security": "none",
        "global_stats": "denied",
        "columns": ["berth_id"]
    }]);
    let pier = must(&effective, &pier_vis);

    let root = out_dir();
    fs::create_dir_all(&root).expect("out");
    dump(&root, "clerk", &clerk);
    dump(&root, "ada2", &ada2);
    dump(&root, "audit", &audit);
    dump(&root, "pier", &pier);

    let mut violations = Vec::new();
    let mut misses = Vec::new();
    let mut hash_bad = Vec::new();
    let mut refusal_bad = Vec::new();

    check_hash(&clerk, "ada", "oo_r_clerk", &mut hash_bad);
    check_hash(&ada2, "ada2", "oo_r_clerk", &mut hash_bad);
    check_hash(&audit, "cy", "oo_r_audit", &mut hash_bad);
    check_hash(&pier, "bea", "oo_r_pier", &mut hash_bad);
    if clerk.artifact != ada2.artifact || clerk.pack_digest != ada2.pack_digest {
        hash_bad.push("ada and ada2 artifacts differ".to_owned());
    }
    if clerk.cache_key == ada2.cache_key || !ada2.cache_key.contains("principal=ada2") {
        hash_bad.push("ada2 cache key did not change".to_owned());
    }
    if audit.pack_digest == clerk.pack_digest || pier.pack_digest == clerk.pack_digest {
        hash_bad.push("role packs shared a digest".to_owned());
    }

    let clerk_body = body(&clerk);
    let nfd_label = NFD_LABEL;
    for token in [
        "vault_bin",
        "VAULT_BIN",
        "Vault_Bin",
        "暂存舱",
        "舱'记",
        "舱''记",
        "舱\"记",
        "舱\"\"记",
        nfc_label.as_str(),
        nfd_label,
        "quarter_start",
        "tick_sq",
        "tick_dq",
        "mark_nfd",
        "pier",
        "berth",
        "泊位单",
        "616161",
        "828282",
        "191919",
        "0.414141",
        "0.818181",
        "434343",
        "0.272727",
        "owner.a176",
        "review.a176",
        "class.a176",
        "sq_leak",
        "单引号标签",
        "quote_leak",
        "双引号标签",
        "quoted_table",
        "引用泊位",
        "control_leak",
        "对账泄漏",
        "unit_leak",
        "单位泄漏",
        "unit_leak_alias",
        "nfc_leak",
        "nfc_alias",
        "组合泄漏",
        "open_quote",
        "open_alias",
        "see '舱",
        "distinct_cols",
        "distinct_bare",
        "distinct_lower",
        "列去重",
        "退化去重",
        "小写去重",
        DISTINCT_COLS,
        DISTINCT_BARE,
        DISTINCT_LOWER,
        "ex-rel-hidden",
        "rel_hidden_alias",
        "hidden relationship question",
        "ex-hidden-quote",
        "declared_fk",
        "草稿",
        "泊位术语",
        "most_common_vals",
        "锚点泄漏",
    ] {
        absent(&clerk_body, token, &mut violations);
    }
    for token in [
        "## string_metric",
        LEGAL_STRING,
        "## kept_mass",
        KEPT_MASS,
        "## where_metric",
        WHERE_SQL,
        "## quoted_metric",
        QUOTED_KEPT,
        "## quoted_distinct",
        DISTINCT_QUOTED,
        "## scale_count",
        SCALE_COUNT,
        "it's a visible note",
        PROSE,
        "可见字符串，撇号仍按原文保留。",
        "## crate_scale",
        "ex-kept",
        "rel_kept_alias",
        "可见关系还在，净重仍可引用。",
        "ex-quote",
        QUOTED_EXAMPLE,
        "可见引号查询，净重仍在。",
        "ex-string",
        "可见字符串示例，撇号按原文保留。",
        "# crate",
        "本季: month_start .. today",
        "Asia/Shanghai",
        "箱号",
        "bigint",
        "本季到港重量",
        "可见标记说明",
        "Samples are omitted because pii is personal.",
        "Estimated row count is omitted.",
        "did not re-prove",
        "estimated rows: 23",
        "proof-scale-a176",
        "Unconfirmed codes: wharf.crate.status",
        "完好",
    ] {
        present(&clerk_body, token, &mut misses);
    }

    let crate_table = clerk.file("tables/wharf.crate.md").unwrap_or("");
    let mass_row = table_row(crate_table, "mass_kg");
    present(mass_row, "| 净重 |  | money | none |", &mut misses);
    absent(mass_row, "vault_bin", &mut violations);
    absent(mass_row, "pier", &mut violations);
    absent(mass_row, &nfc_label, &mut violations);
    absent(mass_row, "unit ", &mut violations);
    let mark_row = table_row(crate_table, "mark_ok");
    present(mark_row, "可见标记说明", &mut misses);
    absent(mark_row, &nfc_label, &mut violations);
    let id_row = table_row(crate_table, "crate_id");
    present(id_row, "箱号", &mut misses);
    present(id_row, "bigint", &mut misses);
    present(id_row, "本季到港重量", &mut misses);

    for path in [
        "entities/vault_bin.md",
        "tables/pier.berth.md",
        "entities/berth.md",
        "domains/wharf.md",
    ] {
        match clerk.file(path) {
            Ok(_) => violations.push(format!("{path} was written")),
            Err(error) if error.code == format!("{PREFIX}path.absent") => {}
            Err(error) => violations.push(format!("{path} -> {}", error.code)),
        }
    }
    if clerk.file("tables/wharf.crate.md").is_err() {
        misses.push("tables/wharf.crate.md".to_owned());
    }
    if clerk.file("tables/wharf.scale.md").is_err() {
        misses.push("tables/wharf.scale.md".to_owned());
    }
    if clerk.file("entities/crate.md").is_err() {
        misses.push("entities/crate.md".to_owned());
    }
    if clerk.file("enums/wharf.crate.status.md").is_err() {
        misses.push("enums/wharf.crate.status.md".to_owned());
    }

    let audit_body = body(&audit);
    for token in [
        "wharf.crate",
        "vault_bin",
        "暂存舱",
        "舱'记",
        "舱''记",
        "舱\"记",
        nfc_label.as_str(),
        "quarter_start",
        "616161",
        "pier",
        "泊位单",
        "owner.a176",
    ] {
        absent(&audit_body, token, &mut violations);
    }
    present(&audit_body, "## scale_count", &mut misses);
    present(&audit_body, "estimated rows: 23", &mut misses);
    present(&audit_body, "proof-scale-a176", &mut misses);
    if audit.file("tables/wharf.scale.md").is_err() {
        misses.push("audit scale table".to_owned());
    }
    if audit.file("tables/wharf.crate.md").is_ok() {
        violations.push("audit kept wharf.crate".to_owned());
    }

    let pier_body = body(&pier);
    for token in ["wharf.crate", "vault_bin", "616161", "mass_kg", "crate_id"] {
        absent(&pier_body, token, &mut violations);
    }
    if pier.file("tables/pier.berth.md").is_err() {
        misses.push("pier berth table".to_owned());
    }
    if pier.file("entities/berth.md").is_err() {
        misses.push("pier berth entity".to_owned());
    }
    if pier.file("tables/wharf.crate.md").is_ok() {
        violations.push("pier kept wharf.crate".to_owned());
    }
    present(&pier_body, "泊位单", &mut misses);

    check_refusals(&effective, &visibility, &mut refusal_bad);

    let outcome = json!({
        "closure_violations": violations,
        "kept_misses": misses,
        "hash_violations": hash_bad,
        "refusal_mismatches": refusal_bad,
        "clerk_digest": clerk.pack_digest,
        "ada2_digest": ada2.pack_digest,
        "audit_digest": audit.pack_digest,
        "pier_digest": pier.pack_digest,
        "ada_cache_key": clerk.cache_key,
        "ada2_cache_key": ada2.cache_key,
        "nfc_label": nfc_label,
        "nfd_label": NFD_LABEL
    });
    fs::write(root.join("outcome.json"), canonical_json(&outcome)).expect("outcome");

    assert!(
        violations.is_empty() && misses.is_empty() && hash_bad.is_empty() && refusal_bad.is_empty(),
        "violations={violations:?}\nmisses={misses:?}\nhash={hash_bad:?}\nrefusal={refusal_bad:?}"
    );
}
