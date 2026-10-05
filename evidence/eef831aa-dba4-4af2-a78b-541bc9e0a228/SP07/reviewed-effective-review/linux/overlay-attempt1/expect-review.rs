//! Independent review of the reviewed-effective compiler.
//!
//! The catalog bytes are the locked shop capture so `pg-catalog-hash/v1`
//! stays `SHOP_SCHEMA`. Decisions, settings, the two roles, the curated echo,
//! and the tamper cases are constructed here. They are not the author fixture.
//! Hashing is FIPS 180-4 in this file. No Python.

use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::{Map, Value, json};

const SHOP_SCHEMA: &str = "bcb019d92270bf12092a3f47697b25719cc087a1ac4b7c56a10cf6dd00294c4c";
const COMMENT_BODY: &str = "a156-comment-body-do-not-carry";
const OWNER: &str = "owner_role_do_not_leak";
const NOTE: &str = "a228-note-do-not-echo";
const CAPTURED_AT: &str = "2026-10-04T12:00:00.000000Z";
const COMMAND: &str = "oo.builder.reviewed-effective-oos-command/v1";
const REVIEWER: &str = "mina.reviewer";
const REVIEWED_AT: &str = "2026-10-05";
const AUTO_ACCEPT: &str = "accept_everything";
const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256(message: &[u8]) -> [u8; 32] {
    let mut state = [
        0x6a09e667_u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (message.len() as u64).wrapping_mul(8);
    let mut padded = message.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in padded.chunks_exact(64) {
        let mut block = [0_u8; 64];
        block.copy_from_slice(chunk);
        compress(&mut state, &block);
    }
    let mut out = [0_u8; 32];
    for (index, word) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut words = [0_u32; 64];
    for index in 0..16 {
        let start = index * 4;
        words[index] = u32::from_be_bytes([
            block[start],
            block[start + 1],
            block[start + 2],
            block[start + 3],
        ]);
    }
    for index in 16..64 {
        let s0 = words[index - 15].rotate_right(7)
            ^ words[index - 15].rotate_right(18)
            ^ (words[index - 15] >> 3);
        let s1 = words[index - 2].rotate_right(17)
            ^ words[index - 2].rotate_right(19)
            ^ (words[index - 2] >> 10);
        words[index] = words[index - 16]
            .wrapping_add(s0)
            .wrapping_add(words[index - 7])
            .wrapping_add(s1);
    }
    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];
    for index in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choose = (e & f) ^ ((!e) & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(choose)
            .wrapping_add(K[index])
            .wrapping_add(words[index]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(majority);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn canonical(value: &Value) -> String {
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

fn canonical_sha256(value: &Value) -> String {
    hex(&sha256(canonical(value).as_bytes()))
}

fn self_check() {
    assert_eq!(hex(&sha256(b"")), EMPTY_SHA);
    assert_eq!(hex(&sha256(b"abc")), ABC_SHA);
    assert_eq!(
        canonical(&json!({"b": 1, "a": [true, null]})),
        r#"{"a":[true,null],"b":1}"#
    );
}

fn oracle(name: &str, body: &str) {
    let dir = std::env::var("OO_A228_ORACLE_DIR").expect("OO_A228_ORACLE_DIR");
    let path = std::path::Path::new(&dir).join(name);
    std::fs::write(&path, body).unwrap_or_else(|err| panic!("write {}: {err}", path.display()));
}

fn oo(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_oo-builder"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("oo-builder");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("wait oo-builder")
}

fn reviewed(stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_reviewed-effective-oos"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("reviewed-effective-oos");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("wait reviewed-effective-oos")
}

fn comment(text: &str) -> Value {
    json!({"byte_len": text.len(), "sha256": hex(&sha256(text.as_bytes()))})
}

fn column(
    attnum: i32,
    name: &str,
    type_oid: i64,
    type_name: &str,
    not_null: bool,
    comment: Option<Value>,
) -> Value {
    json!({
        "attnum": attnum,
        "name": name,
        "name_ident": name,
        "type_oid": type_oid,
        "type_schema": "pg_catalog",
        "type_name": type_name,
        "typtype": "b",
        "typmod": -1,
        "base_type_oid": Value::Null,
        "numeric_precision": Value::Null,
        "numeric_scale": Value::Null,
        "not_null": not_null,
        "collation_oid": 0,
        "collation_name": Value::Null,
        "identity": "",
        "generated": "",
        "default_expr": Value::Null,
        "comment": comment,
        "enum_labels": [],
        "domain_checks": [],
        "type_support": {"converted": false, "reason": "known_builtin_not_converted"},
        "current_privileges": {"select": true, "insert": false, "update": false}
    })
}

fn shop() -> Value {
    json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": "shop_src",
        "database": "shop",
        "captured_at": CAPTURED_AT,
        "schema_hash": SHOP_SCHEMA,
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": [
            {
                "schema": "public",
                "schema_ident": "public",
                "name": "customer",
                "name_ident": "customer",
                "relkind": "r",
                "replica_identity": "d",
                "rls_enabled": false,
                "rls_forced": false,
                "owner": OWNER,
                "comment": Value::Null,
                "columns": [
                    column(1, "tenant_id", 23, "int4", true, None),
                    column(2, "id", 23, "int4", true, None),
                    column(3, "email", 25, "text", false, Some(comment(COMMENT_BODY)))
                ],
                "primary_key": [{"attnum": 1, "name": "tenant_id"}, {"attnum": 2, "name": "id"}],
                "primary_key_nulls_not_distinct": false,
                "unique_keys": [],
                "foreign_keys": [],
                "policies": [],
                "privileges": [
                    {"grantee": OWNER, "privilege": "INSERT", "grantable": true},
                    {"grantee": "reader", "privilege": "SELECT", "grantable": false}
                ]
            },
            {
                "schema": "public",
                "schema_ident": "public",
                "name": "purchase",
                "name_ident": "purchase",
                "relkind": "r",
                "replica_identity": "d",
                "rls_enabled": false,
                "rls_forced": false,
                "owner": OWNER,
                "comment": Value::Null,
                "columns": [
                    column(1, "tenant_id", 23, "int4", true, None),
                    column(2, "id", 23, "int4", true, None),
                    column(3, "customer_id", 23, "int4", true, None)
                ],
                "primary_key": [{"attnum": 1, "name": "tenant_id"}, {"attnum": 2, "name": "id"}],
                "primary_key_nulls_not_distinct": false,
                "unique_keys": [],
                "foreign_keys": [{
                    "name": "purchase_customer_fk",
                    "columns": [
                        {"attnum": 1, "name": "tenant_id", "ref_schema": "public", "ref_table": "customer", "ref_attnum": 1, "ref_name": "tenant_id"},
                        {"attnum": 3, "name": "customer_id", "ref_schema": "public", "ref_table": "customer", "ref_attnum": 2, "ref_name": "id"}
                    ]
                }],
                "policies": [],
                "privileges": [
                    {"grantee": OWNER, "privilege": "INSERT", "grantable": true},
                    {"grantee": "reader", "privilege": "SELECT", "grantable": false}
                ]
            }
        ],
        "type_conversion": "unsupported"
    })
}

fn settings() -> Value {
    json!({
        "project": "ledger",
        "ontology_version": "ont-a228",
        "catalog_version": "cat-a228",
        "policy": {"id": "pol_ledger", "version": "a228.1"},
        "knowledge": {"id": "know_ledger", "version": "a228.1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "ledger",
            "locale": "en-US",
            "include_schemas": ["public"],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": AUTO_ACCEPT},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        }
    })
}

fn review_body(catalog: &Value, curated: Option<Value>, reviews: Option<Value>) -> String {
    let mut body = json!({"catalog": catalog});
    if let Some(curated) = curated {
        body["curated"] = curated;
    }
    if let Some(reviews) = reviews {
        body["reviews"] = reviews;
    }
    serde_json::to_string(&body).expect("review body")
}

fn artifact_from(body: &str) -> Value {
    let output = oo(&["review-pg-draft"], body);
    assert_eq!(
        output.status.code(),
        Some(0),
        "review-pg-draft {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("review json")
}

fn fingerprint(artifact: &Value, dataset: &str, column: &str) -> String {
    let item_id = format!("column/{dataset}/{column}");
    artifact["draft"]["review_queue"]
        .as_array()
        .expect("queue")
        .iter()
        .find(|item| item["item_id"] == item_id && item["kind"] == "structural_column")
        .unwrap_or_else(|| panic!("missing {item_id}"))["fact_fingerprint"]
        .as_str()
        .expect("fingerprint")
        .to_string()
}

fn decision(
    artifact: &Value,
    dataset: &str,
    column: &str,
    pii: &str,
    semantic: &str,
) -> Value {
    json!({
        "dataset": dataset,
        "column": column,
        "schema_hash": artifact["draft"]["provenance"]["schema_hash"],
        "fact_fingerprint": fingerprint(artifact, dataset, column),
        "pii": pii,
        "semantic_type": semantic,
        "reviewer": {"by": REVIEWER, "at": REVIEWED_AT}
    })
}

fn bindings(artifact: &Value) -> Vec<Value> {
    vec![
        decision(artifact, "public.customer", "tenant_id", "restricted", "identifier"),
        decision(artifact, "public.customer", "id", "none", "identifier"),
        decision(artifact, "public.purchase", "tenant_id", "none", "identifier"),
        decision(artifact, "public.purchase", "id", "none", "identifier"),
        decision(artifact, "public.purchase", "customer_id", "personal", "foreign_key"),
    ]
}

fn command(artifact: &Value, decisions: Vec<Value>) -> Value {
    json!({
        "protocol": COMMAND,
        "artifact": artifact,
        "settings": settings(),
        "decisions": decisions,
    })
}

fn command_text(artifact: &Value, decisions: Vec<Value>) -> String {
    serde_json::to_string(&command(artifact, decisions)).expect("command")
}

fn column_of<'a>(effective: &'a Value, dataset: &str, name: &str) -> &'a Value {
    effective["datasets"]
        .as_array()
        .expect("datasets")
        .iter()
        .find(|item| item["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing dataset {dataset}"))["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|column| column["name"] == name)
        .unwrap_or_else(|| panic!("missing column {dataset}.{name}"))
}

fn hidden_reason(value: &Value, dataset: &str, column: &str) -> String {
    value["hidden"]
        .as_array()
        .expect("hidden")
        .iter()
        .find(|item| item["dataset"] == dataset && item["column"] == column)
        .unwrap_or_else(|| panic!("missing hidden {dataset}.{column}"))["reason"]
        .as_str()
        .expect("reason")
        .to_string()
}

fn assert_quiet(output: &Output, code: &str) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "{code} {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty(), "{code} wrote stdout");
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains(code), "{err}");
    assert!(!err.contains(OWNER), "{err}");
    assert!(!err.contains(COMMENT_BODY), "{err}");
    assert!(!err.contains(NOTE), "{err}");
    assert!(!err.contains(REVIEWER), "{err}");
}

fn visible(dataset: &str, columns: &[&str]) -> Value {
    json!({
        "dataset": dataset,
        "row_security": "none",
        "global_stats": "denied",
        "columns": columns
    })
}

fn role_pack(effective: &Value, role: &str, datasets: Value) -> Output {
    let command = json!({
        "protocol": "oo.builder.role-pack-command/v1",
        "effective_oos": effective,
        "visibility": {
            "protocol": "oo.builder.role-visibility/v1",
            "project": "ledger",
            "principal": "mina",
            "role": role,
            "ontology_version": "ont-a228",
            "catalog_version": "cat-a228",
            "policy": {"id": "pol_ledger", "version": "a228.1"},
            "knowledge": {"id": "know_ledger", "version": "a228.1"},
            "issued_for_compiler": "oo-builder/role-pack/v1",
            "datasets": datasets,
            "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
        }
    });
    oo(&["role-pack"], &serde_json::to_string(&command).expect("pack command"))
}

fn pack_text(pack: &Value) -> String {
    pack["artifact"]["files"]
        .as_array()
        .expect("files")
        .iter()
        .map(|file| file["text"].as_str().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_no_fixture_leak(text: &str, label: &str) {
    assert!(!text.contains(OWNER), "{label} leaked owner");
    assert!(!text.contains(COMMENT_BODY), "{label} leaked comment");
    assert!(!text.contains(NOTE), "{label} leaked note");
}

fn sealed_shop() -> Value {
    let bare = artifact_from(&review_body(&shop(), None, None));
    assert_eq!(bare["protocol"], "oo.builder.captured-pg-draft-command/v1");
    assert_eq!(bare["wrapper"]["grants"], "not_authorized");
    assert_eq!(bare["wrapper"]["publication"], "unavailable");
    assert_eq!(bare["wrapper"]["certification"], "none");
    assert_eq!(bare["wrapper"]["llm"], "not_used");
    assert_eq!(bare["wrapper"]["source_connection"], "not_used");
    assert_eq!(bare["draft"]["provenance"]["schema_hash"], SHOP_SCHEMA);
    assert_eq!(bare["draft"]["provenance"]["publication"], "unavailable");
    assert_eq!(bare["draft"]["provenance"]["pg_catalog_integration"], "not_accepted");
    assert!(column_of(&bare["draft"]["effective"], "public.customer", "email")["pii"].is_null());
    assert!(column_of(&bare["draft"]["effective"], "public.customer", "id")["classification"].is_null());
    let mut reviews = Vec::new();
    for (dataset, column) in [
        ("public.customer", "tenant_id"),
        ("public.customer", "id"),
        ("public.purchase", "tenant_id"),
        ("public.purchase", "id"),
        ("public.purchase", "customer_id"),
    ] {
        reviews.push(json!({
            "item_id": format!("column/{dataset}/{column}"),
            "action": "accept",
            "schema_hash": SHOP_SCHEMA,
            "fact_fingerprint": fingerprint(&bare, dataset, column),
        }));
    }
    let relationship = bare["draft"]["review_queue"]
        .as_array()
        .expect("queue")
        .iter()
        .find(|item| {
            item["kind"] == "candidate_relationship"
                && item["item_id"]
                    .as_str()
                    .unwrap_or("")
                    .contains("purchase_customer_fk")
        })
        .expect("fk queue item");
    reviews.push(json!({
        "item_id": relationship["item_id"],
        "action": "accept",
        "schema_hash": SHOP_SCHEMA,
        "fact_fingerprint": relationship["fact_fingerprint"],
    }));
    let artifact = artifact_from(&review_body(&shop(), None, Some(json!(reviews))));
    assert_eq!(artifact["draft"]["provenance"]["schema_hash"], SHOP_SCHEMA);
    assert!(column_of(&artifact["draft"]["effective"], "public.customer", "email")["pii"].is_null());
    artifact
}

fn assert_not_invented(effective: &Value) {
    assert_eq!(effective["ontology"]["classification"]["auto_accept"], AUTO_ACCEPT);
    for dataset in effective["datasets"].as_array().expect("datasets") {
        for column in dataset["columns"].as_array().expect("columns") {
            assert_ne!(column["pii"], AUTO_ACCEPT);
            assert_ne!(column["semantic_type"], AUTO_ACCEPT);
            if let Some(classification) = column.get("classification") {
                assert_ne!(classification["state"], AUTO_ACCEPT);
            }
        }
    }
}

#[test]
fn two_roles_consume_explicit_bindings_and_keep_email_hidden() {
    self_check();
    let artifact = sealed_shop();
    let decisions = bindings(&artifact);
    let body = command(&artifact, decisions.clone());
    let expected_decisions = canonical_sha256(&body["decisions"]);
    let expected_sealed = canonical_sha256(&body["artifact"]);
    let text = serde_json::to_string(&body).expect("command");
    let first = reviewed(&text);
    let second = reviewed(&text);
    assert_eq!(first.status.code(), Some(0), "{}", String::from_utf8_lossy(&first.stderr));
    assert_eq!(first.stdout, second.stdout, "reviewed-effective stdout is not repeatable");
    assert!(first.stdout.last() != Some(&b'\n'));
    let stdout = String::from_utf8(first.stdout.clone()).expect("utf-8");
    assert_no_fixture_leak(&stdout, "effective");
    let value: Value = serde_json::from_str(&stdout).expect("effective json");
    assert_eq!(value["protocol"], COMMAND);
    assert_eq!(value["compiler"], "oo-builder/reviewed-effective-oos/v1");
    assert_eq!(value["effective_oos"]["protocol"], "oo.builder.effective-oos/v1");
    assert_eq!(value["effective_oos"]["project"], "ledger");
    assert!(value["effective_oos"].get("decisions").is_none());
    assert!(value["effective_oos"].get("samples").is_none());
    assert!(value["effective_oos"].get("credentials").is_none());
    assert!(value["effective_oos"].get("privileges").is_none());
    assert!(value["omitted_relationships"].as_array().expect("omitted").is_empty());
    assert_eq!(value["provenance"]["schema_hash"], SHOP_SCHEMA);
    assert_eq!(value["provenance"]["decisions_sha256"], expected_decisions);
    assert_eq!(value["provenance"]["sealed_source_sha256"], expected_sealed);
    assert_ne!(value["provenance"]["generated_sha256"], value["provenance"]["curated_sha256"]);
    for key in [
        "authentication",
        "authorization",
        "grants",
        "publication",
        "semantic_publication",
    ] {
        assert_eq!(value["provenance"][key], "not_claimed", "{key}");
    }
    assert_eq!(hidden_reason(&value, "public.customer", "email"), "classification.absent");
    assert_eq!(value["hidden"].as_array().expect("hidden").len(), 1);
    let email = column_of(&value["effective_oos"], "public.customer", "email");
    assert!(email["pii"].is_null());
    assert!(email["semantic_type"].is_null());
    assert!(email["classification"].is_null());
    let tenant = column_of(&value["effective_oos"], "public.customer", "tenant_id");
    assert_eq!(tenant["pii"], "restricted");
    assert_eq!(tenant["semantic_type"], "identifier");
    assert_eq!(tenant["classification"]["state"], "confirmed");
    assert_eq!(tenant["classification"]["by"], REVIEWER);
    assert_eq!(tenant["classification"]["at"], REVIEWED_AT);
    assert!(tenant["review"].is_null());
    let id = column_of(&value["effective_oos"], "public.customer", "id");
    assert_eq!(id["pii"], "none");
    assert_eq!(id["semantic_type"], "identifier");
    assert!(id["review"].is_null());
    let customer_id = column_of(&value["effective_oos"], "public.purchase", "customer_id");
    assert_eq!(customer_id["pii"], "personal");
    assert_eq!(customer_id["semantic_type"], "foreign_key");
    assert_eq!(customer_id["classification"]["by"], REVIEWER);
    let relationship = &value["effective_oos"]["relationships"][0];
    assert_eq!(relationship["name"], "purchase_customer_fk");
    assert_eq!(relationship["provenance"], "declared_fk");
    assert!(relationship["review"].is_null());
    assert_not_invented(&value["effective_oos"]);

    let effective = &value["effective_oos"];
    let pay = role_pack(
        effective,
        "oo_r_pay",
        json!([
            visible("public.customer", &["tenant_id", "id"]),
            visible("public.purchase", &["tenant_id", "id", "customer_id"])
        ]),
    );
    assert_eq!(pay.status.code(), Some(0), "{}", String::from_utf8_lossy(&pay.stderr));
    let pay_stdout = String::from_utf8(pay.stdout.clone()).expect("pay utf-8");
    assert_no_fixture_leak(&pay_stdout, "oo_r_pay");
    let pay_value: Value = serde_json::from_str(&pay_stdout).expect("pay json");
    let pay_text = pack_text(&pay_value);
    assert!(pay_text.contains("purchase_customer_fk"));
    assert!(pay_text.contains("declared_fk (source-declared, not a grant)"));
    assert!(!pay_text.contains("email"));
    assert_eq!(pay_value["manifest"]["authentication"], "not_claimed");
    assert_eq!(pay_value["manifest"]["publication"], "not_claimed");
    assert_eq!(pay_value["manifest"]["row_security_reproven"], false);

    let desk = role_pack(
        effective,
        "oo_r_desk",
        json!([
            visible("public.customer", &["tenant_id", "id"]),
            visible("public.purchase", &["tenant_id", "id"])
        ]),
    );
    assert_eq!(desk.status.code(), Some(0), "{}", String::from_utf8_lossy(&desk.stderr));
    let desk_stdout = String::from_utf8(desk.stdout.clone()).expect("desk utf-8");
    assert_no_fixture_leak(&desk_stdout, "oo_r_desk");
    let desk_value: Value = serde_json::from_str(&desk_stdout).expect("desk json");
    let desk_text = pack_text(&desk_value);
    assert!(!desk_text.contains("purchase_customer_fk"));
    assert!(!desk_text.contains("customer_id"));
    assert!(!desk_text.contains("email"));
    assert_eq!(desk_value["manifest"]["authentication"], "not_claimed");
    assert_eq!(desk_value["manifest"]["row_security_reproven"], false);

    let hidden = role_pack(effective, "oo_r_pay", json!([visible("public.customer", &["email"])]));
    assert_eq!(hidden.status.code(), Some(1));
    assert!(hidden.stdout.is_empty());
    let hidden_err = String::from_utf8_lossy(&hidden.stderr);
    assert!(hidden_err.contains("classification.unknown_pii"), "{hidden_err}");
    assert_no_fixture_leak(&hidden_err, "hidden email");

    oracle(
        "two-roles.json",
        &serde_json::to_string_pretty(&json!({
            "schema_hash": SHOP_SCHEMA,
            "decisions_sha256": expected_decisions,
            "sealed_source_sha256": expected_sealed,
            "generated_sha256": value["provenance"]["generated_sha256"],
            "curated_sha256": value["provenance"]["curated_sha256"],
            "draft_hash": value["provenance"]["draft_hash"],
            "wrapper_hash": value["provenance"]["wrapper_hash"],
            "hidden": value["hidden"],
            "tenant_pii": "restricted",
            "customer_id_pii": "personal",
            "customer_id_semantic": "foreign_key",
            "reviewer": REVIEWER,
            "reviewed_at": REVIEWED_AT,
            "auto_accept": AUTO_ACCEPT,
            "pay_has_fk": true,
            "desk_has_fk": false,
            "email_pack": "classification.unknown_pii"
        }))
        .expect("oracle"),
    );
    oracle("effective.stdout.json", &stdout);
    oracle("oo_r_pay.stdout.json", &pay_stdout);
    oracle("oo_r_desk.stdout.json", &desk_stdout);
}

#[test]
fn empty_decisions_leave_auto_accept_unapplied() {
    self_check();
    let artifact = sealed_shop();
    let output = reviewed(&command_text(&artifact, Vec::new()));
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let value: Value = serde_json::from_slice(&output.stdout).expect("empty decisions");
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    assert_no_fixture_leak(&stdout, "empty");
    assert_eq!(value["hidden"].as_array().expect("hidden").len(), 6);
    let reasons: Vec<&str> = value["hidden"]
        .as_array()
        .expect("hidden")
        .iter()
        .map(|item| item["reason"].as_str().expect("reason"))
        .collect();
    assert!(reasons.iter().all(|reason| *reason == "classification.absent"));
    assert_eq!(value["hidden"][0]["column"], "email");
    assert_eq!(value["hidden"][1]["column"], "id");
    assert_eq!(value["hidden"][2]["column"], "tenant_id");
    assert!(column_of(&value["effective_oos"], "public.customer", "id")["pii"].is_null());
    assert!(column_of(&value["effective_oos"], "public.customer", "id")["classification"].is_null());
    assert_not_invented(&value["effective_oos"]);
    let refused = role_pack(
        &value["effective_oos"],
        "oo_r_pay",
        json!([visible("public.customer", &["id"])]),
    );
    assert!(refused.stdout.is_empty());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("classification.unknown_pii"));
    oracle(
        "auto-accept.json",
        &serde_json::to_string_pretty(&json!({
            "auto_accept": AUTO_ACCEPT,
            "id_pii": null,
            "hidden_len": 6,
            "role": "classification.unknown_pii"
        }))
        .expect("oracle"),
    );
}

#[test]
fn stale_rename_reject_and_tamper_stay_unclassified() {
    self_check();
    let artifact = sealed_shop();
    let mut stale_schema = bindings(&artifact);
    for item in &mut stale_schema {
        if item["dataset"] == "public.customer" && item["column"] == "id" {
            item["schema_hash"] = json!("ab".repeat(32));
            item["fact_fingerprint"] = json!("00".repeat(32));
        }
    }
    let output = reviewed(&command_text(&artifact, stale_schema));
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let value: Value = serde_json::from_slice(&output.stdout).expect("stale schema");
    assert_eq!(hidden_reason(&value, "public.customer", "id"), "decision.stale_schema");
    assert!(column_of(&value["effective_oos"], "public.customer", "id")["pii"].is_null());
    assert_eq!(column_of(&value["effective_oos"], "public.customer", "tenant_id")["pii"], "restricted");
    let stale_pack = role_pack(
        &value["effective_oos"],
        "oo_r_pay",
        json!([visible("public.customer", &["id"])]),
    );
    assert!(stale_pack.stdout.is_empty());
    assert!(String::from_utf8_lossy(&stale_pack.stderr).contains("classification.unknown_pii"));

    let mut stale_fact = bindings(&artifact);
    for item in &mut stale_fact {
        if item["column"] == "customer_id" {
            item["fact_fingerprint"] = json!("11".repeat(32));
        }
    }
    let output = reviewed(&command_text(&artifact, stale_fact));
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let value: Value = serde_json::from_slice(&output.stdout).expect("stale fact");
    assert_eq!(hidden_reason(&value, "public.purchase", "customer_id"), "decision.stale_fact");
    assert!(column_of(&value["effective_oos"], "public.purchase", "customer_id")["pii"].is_null());

    let mut unknown = bindings(&artifact);
    unknown.push(decision(&artifact, "public.customer", "contact", "none", "identifier"));
    assert_quiet(&reviewed(&command_text(&artifact, unknown)), "/decision.unknown");

    let mut duplicated = bindings(&artifact);
    duplicated.push(decision(&artifact, "public.customer", "tenant_id", "none", "identifier"));
    assert_quiet(&reviewed(&command_text(&artifact, duplicated)), "/decision.duplicate");

    let mut invalid_pii = bindings(&artifact);
    invalid_pii[0]["pii"] = json!("secret");
    assert_quiet(&reviewed(&command_text(&artifact, invalid_pii)), "/decision.invalid");

    let mut invalid_semantic = bindings(&artifact);
    invalid_semantic[0]["semantic_type"] = json!("passport");
    assert_quiet(&reviewed(&command_text(&artifact, invalid_semantic)), "/decision.invalid");

    let mut upper = bindings(&artifact);
    upper[0]["schema_hash"] = json!(SHOP_SCHEMA.to_ascii_uppercase());
    assert_quiet(&reviewed(&command_text(&artifact, upper)), "/decision.invalid");

    let mut noted = command(&artifact, bindings(&artifact));
    noted["decisions"][0]["note"] = json!(NOTE);
    assert_quiet(&reviewed(&serde_json::to_string(&noted).expect("noted")), "/field.unknown");

    let mut renamed = shop();
    for table in renamed["tables"].as_array_mut().expect("tables") {
        if table["name"] == "customer" {
            for column in table["columns"].as_array_mut().expect("columns") {
                if column["name"] == "email" {
                    column["name"] = json!("contact");
                    column["name_ident"] = json!("contact");
                }
            }
        }
    }
    let renamed_out = oo(&["review-pg-draft"], &review_body(&renamed, None, None));
    assert_eq!(renamed_out.status.code(), Some(1));
    assert!(renamed_out.stdout.is_empty());
    let renamed_err = String::from_utf8_lossy(&renamed_out.stderr);
    assert!(renamed_err.contains("hash.schema"), "{renamed_err}");
    assert_no_fixture_leak(&renamed_err, "rename");

    let bare = artifact_from(&review_body(&shop(), None, None));
    let reject = json!([{
        "item_id": "column/public.customer/email",
        "action": "reject",
        "schema_hash": SHOP_SCHEMA,
        "fact_fingerprint": fingerprint(&bare, "public.customer", "email")
    }]);
    let rejected = artifact_from(&review_body(&shop(), None, Some(reject)));
    assert!(
        rejected["draft"]["effective"]["datasets"]
            .as_array()
            .expect("datasets")
            .iter()
            .filter(|dataset| dataset["dataset"] == "public.customer")
            .flat_map(|dataset| dataset["columns"].as_array().expect("columns"))
            .all(|column| column["name"] != "email")
    );
    let mut named = bindings(&rejected);
    named.push(decision(&rejected, "public.customer", "email", "none", "identifier"));
    assert_quiet(&reviewed(&command_text(&rejected, named)), "/decision.unknown");
    let kept = reviewed(&command_text(&rejected, bindings(&rejected)));
    assert_eq!(kept.status.code(), Some(0), "{}", String::from_utf8_lossy(&kept.stderr));
    let kept_value: Value = serde_json::from_slice(&kept.stdout).expect("rejected effective");
    assert!(
        kept_value["effective_oos"]["datasets"]
            .as_array()
            .expect("datasets")
            .iter()
            .flat_map(|dataset| dataset["columns"].as_array().expect("columns"))
            .all(|column| column["name"] != "email")
    );
    let missing = role_pack(
        &kept_value["effective_oos"],
        "oo_r_desk",
        json!([visible("public.customer", &["email"])]),
    );
    assert_eq!(missing.status.code(), Some(1));
    assert!(missing.stdout.is_empty());
    let missing_err = String::from_utf8_lossy(&missing.stderr);
    assert!(missing_err.contains("visibility.unknown_column"), "{missing_err}");
    assert_no_fixture_leak(&missing_err, "rejected email");

    let mut tampered = artifact.clone();
    tampered["draft"]["effective"]["datasets"]
        .as_array_mut()
        .expect("datasets")
        .iter_mut()
        .find(|dataset| dataset["dataset"] == "public.customer")
        .expect("customer")["columns"]
        .as_array_mut()
        .expect("columns")
        .iter_mut()
        .find(|column| column["name"] == "email")
        .expect("email")["pii"] = json!("personal");
    assert_quiet(&reviewed(&command_text(&tampered, Vec::new())), "/seal.rejected");

    let mut grants = artifact.clone();
    grants["wrapper"]["grants"] = json!("authorized");
    assert_quiet(&reviewed(&command_text(&grants, Vec::new())), "/seal.rejected");

    oracle(
        "refusals.txt",
        "decision.stale_schema\ndecision.stale_fact\ndecision.unknown\ndecision.duplicate\ndecision.invalid\nfield.unknown\nhash.schema\nvisibility.unknown_column\nseal.rejected\n",
    );
}

#[test]
fn curated_pii_is_not_copied_and_money_needs_a_unit() {
    self_check();
    let artifact = sealed_shop();
    let mut money = vec![decision(&artifact, "public.customer", "email", "none", "money")];
    money.extend(bindings(&artifact));
    assert_quiet(&reviewed(&command_text(&artifact, money)), "/oos.invalid");

    let bare = artifact_from(&review_body(&shop(), None, None));
    let curated = json!({
        "datasets": [{
            "dataset": "public.customer",
            "columns": [{
                "name": "email",
                "unit": "JPY",
                "pii": "sensitive",
                "semantic_type": "email"
            }]
        }],
        "relationships": []
    });
    let reviews = json!([{
        "item_id": "column/public.customer/email",
        "action": "override",
        "schema_hash": SHOP_SCHEMA,
        "fact_fingerprint": fingerprint(&bare, "public.customer", "email")
    }]);
    let overridden = artifact_from(&review_body(&shop(), Some(curated), Some(reviews)));
    assert_eq!(overridden["draft"]["provenance"]["schema_hash"], SHOP_SCHEMA);
    let mut decisions = bindings(&overridden);
    decisions.push(decision(&overridden, "public.customer", "email", "none", "money"));
    let body = command(&overridden, decisions);
    let plain_curated = canonical_sha256(&artifact["draft"]["curated"]);
    let echoed_curated = canonical_sha256(&overridden["draft"]["curated"]);
    assert_ne!(plain_curated, echoed_curated);
    let output = reviewed(&serde_json::to_string(&body).expect("curated command"));
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    assert!(!stdout.contains("sensitive"), "curated pii was copied");
    assert_no_fixture_leak(&stdout, "curated");
    let value: Value = serde_json::from_str(&stdout).expect("curated json");
    let email = column_of(&value["effective_oos"], "public.customer", "email");
    assert_eq!(email["pii"], "none");
    assert_eq!(email["semantic_type"], "money");
    assert_eq!(email["unit"], "JPY");
    assert_eq!(email["classification"]["by"], REVIEWER);
    assert_ne!(value["provenance"]["generated_sha256"], value["provenance"]["curated_sha256"]);
    assert_eq!(value["provenance"]["curated_sha256"], echoed_curated);
    assert_ne!(value["provenance"]["generated_sha256"], echoed_curated);
    oracle(
        "curated.json",
        &serde_json::to_string_pretty(&json!({
            "unit": "JPY",
            "decision_pii": "none",
            "curated_pii_copied": false,
            "plain_curated_sha256": plain_curated,
            "echoed_curated_sha256": echoed_curated,
            "generated_sha256": value["provenance"]["generated_sha256"]
        }))
        .expect("oracle"),
    );
}

#[test]
fn usage_and_scanner_do_not_echo_the_body() {
    self_check();
    let output = Command::new(env!("CARGO_BIN_EXE_reviewed-effective-oos"))
        .arg("help")
        .output()
        .expect("spawn");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage"));

    let artifact = sealed_shop();
    let mut extra = command(&artifact, Vec::new());
    extra["extra"] = json!(NOTE);
    assert_quiet(&reviewed(&serde_json::to_string(&extra).expect("extra")), "/field.unknown");

    let bad_protocol = command_text(&artifact, Vec::new()).replacen(COMMAND, "oo.builder.other/v1", 1);
    assert_quiet(&reviewed(&bad_protocol), "/protocol.mismatch");

    let mut raw = command(&artifact, Vec::new());
    raw["settings"]["ontology"]["compile"]["sample_values"]["default"] = json!("raw");
    assert_quiet(&reviewed(&serde_json::to_string(&raw).expect("raw")), "/settings.invalid");

    let duplicated = format!(
        "{{\"protocol\":\"{COMMAND}\",\"protocol\":\"{COMMAND}\",\"artifact\":{{}},\"settings\":{{}},\"decisions\":[],\"note\":\"{NOTE}\"}}"
    );
    assert_quiet(&reviewed(&duplicated), "/json.duplicate_key");
}
