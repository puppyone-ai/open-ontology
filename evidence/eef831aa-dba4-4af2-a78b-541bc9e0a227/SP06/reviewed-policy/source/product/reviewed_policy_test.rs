//! Reviewed column policies from a real `reviewed-effective-oos` command.
//!
//! `OO_A227_OO_BUILDER` and `OO_A227_REVIEWED_EFFECTIVE` are the compiled
//! binaries. This file does not open a database and does not install a grant.

use std::collections::BTreeMap;
use std::env;
use std::io::Write;
use std::process::{Command, Stdio};

use oo_gateway::policy::PolicyError;
use oo_gateway::reviewed_policy::{
    ReviewedAllow, ReviewedBinding, ReviewedColumnPolicies, ReviewedIdentity, ReviewedPolicyError,
    ReviewedSourceSeal, compile_reviewed_policies,
};
use oo_gateway::{ClassStatus, ColumnAclPolicy, ColumnSpec, PiiClass};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const OWNER: &str = "owner_role_do_not_leak";
const CAPTURED_AT: &str = "2026-10-04T12:00:00.000000Z";
const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ONE_COLUMN_PREIMAGE: &str = r#"{"tables":[{"schema":"public","name":"customer","relkind":"r","replica_identity":"d","rls":false,"force_rls":false,"columns":[{"attnum":1,"name":"id","type_oid":23,"typmod":-1,"not_null":true,"collation_oid":0,"typtype":"b","base_type_oid":null,"identity":"","generated":"","default_sha256":null,"enum_labels":[],"domain_checks":[]}],"primary_key":[{"attnum":1,"name":"id"}],"primary_key_nulls_not_distinct":false,"unique_keys":[],"foreign_keys":[],"policies":[]}]}"#;
const ONE_COLUMN_HASH: &str = "60f7939195cc539644d11132ae4a604c3b025167f1fe657d121167feb8463499";

struct Col {
    attnum: i32,
    name: &'static str,
    oid: i64,
    type_name: &'static str,
}

struct TableSpec {
    name: &'static str,
    rls: bool,
    columns: Vec<Col>,
    pk: Vec<(i32, &'static str)>,
}

struct Chain {
    artifact_json: String,
    command_json: String,
    decisions_json: String,
    schema_hash: String,
    artifact_sha256: String,
}

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

fn canon(value: &Value) -> String {
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

fn schema_preimage(tables: &[TableSpec]) -> String {
    let mut ordered: Vec<&TableSpec> = tables.iter().collect();
    ordered.sort_by(|left, right| left.name.cmp(right.name));
    let mut buf = String::from("{\"tables\":[");
    for (index, table) in ordered.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_pair(&mut buf, "schema", "public");
        buf.push(',');
        write_pair(&mut buf, "name", table.name);
        buf.push(',');
        write_pair(&mut buf, "relkind", "r");
        buf.push(',');
        write_pair(&mut buf, "replica_identity", "d");
        buf.push(',');
        write_bool(&mut buf, "rls", table.rls);
        buf.push(',');
        write_bool(&mut buf, "force_rls", false);
        buf.push_str(",\"columns\":[");
        for (col_index, column) in table.columns.iter().enumerate() {
            if col_index > 0 {
                buf.push(',');
            }
            buf.push('{');
            write_i32(&mut buf, "attnum", column.attnum);
            buf.push(',');
            write_pair(&mut buf, "name", column.name);
            buf.push(',');
            write_i64(&mut buf, "type_oid", column.oid);
            buf.push_str(",\"typmod\":-1,\"not_null\":true,\"collation_oid\":0,\"typtype\":\"b\",\"base_type_oid\":null,\"identity\":\"\",\"generated\":\"\",\"default_sha256\":null,\"enum_labels\":[],\"domain_checks\":[]}");
        }
        buf.push_str("],\"primary_key\":[");
        for (key_index, (attnum, name)) in table.pk.iter().enumerate() {
            if key_index > 0 {
                buf.push(',');
            }
            buf.push_str("{\"attnum\":");
            buf.push_str(&attnum.to_string());
            buf.push_str(",\"name\":");
            push_string(&mut buf, name);
            buf.push('}');
        }
        buf.push_str("],\"primary_key_nulls_not_distinct\":false,\"unique_keys\":[],\"foreign_keys\":[],\"policies\":[]}");
    }
    buf.push_str("]}");
    buf
}

fn write_pair(buf: &mut String, name: &str, value: &str) {
    push_string(buf, name);
    buf.push(':');
    push_string(buf, value);
}

fn write_bool(buf: &mut String, name: &str, value: bool) {
    push_string(buf, name);
    buf.push(':');
    buf.push_str(if value { "true" } else { "false" });
}

fn write_i32(buf: &mut String, name: &str, value: i32) {
    push_string(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn write_i64(buf: &mut String, name: &str, value: i64) {
    push_string(buf, name);
    buf.push(':');
    buf.push_str(&value.to_string());
}

fn column_json(column: &Col) -> Value {
    json!({
        "attnum": column.attnum,
        "name": column.name,
        "name_ident": column.name,
        "type_oid": column.oid,
        "type_schema": "pg_catalog",
        "type_name": column.type_name,
        "typtype": "b",
        "typmod": -1,
        "base_type_oid": Value::Null,
        "numeric_precision": Value::Null,
        "numeric_scale": Value::Null,
        "not_null": true,
        "collation_oid": 0,
        "collation_name": Value::Null,
        "identity": "",
        "generated": "",
        "default_expr": Value::Null,
        "comment": Value::Null,
        "enum_labels": [],
        "domain_checks": [],
        "type_support": {"converted": false, "reason": "known_builtin_not_converted"},
        "current_privileges": {"select": true, "insert": false, "update": false}
    })
}

fn catalog_json(tables: &[TableSpec]) -> Value {
    let hash = sha256_hex(schema_preimage(tables).as_bytes());
    json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": "shop_src",
        "database": "shop",
        "captured_at": CAPTURED_AT,
        "schema_hash": hash,
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": tables.iter().map(|table| json!({
            "schema": "public",
            "schema_ident": "public",
            "name": table.name,
            "name_ident": table.name,
            "relkind": "r",
            "replica_identity": "d",
            "rls_enabled": table.rls,
            "rls_forced": false,
            "owner": OWNER,
            "comment": Value::Null,
            "columns": table.columns.iter().map(column_json).collect::<Vec<_>>(),
            "primary_key": table.pk.iter().map(|(attnum, name)| json!({"attnum": attnum, "name": name})).collect::<Vec<_>>(),
            "primary_key_nulls_not_distinct": false,
            "unique_keys": [],
            "foreign_keys": [],
            "policies": [],
            "privileges": [{"grantee": OWNER, "privilege": "SELECT", "grantable": false}]
        })).collect::<Vec<_>>(),
        "type_conversion": "unsupported"
    })
}

fn shop_tables(email_name: &'static str, customer_rls: bool) -> Vec<TableSpec> {
    vec![
        TableSpec {
            name: "customer",
            rls: customer_rls,
            columns: vec![
                Col { attnum: 1, name: "id", oid: 23, type_name: "int4" },
                Col { attnum: 2, name: email_name, oid: 25, type_name: "text" },
                Col { attnum: 3, name: "active", oid: 16, type_name: "bool" },
            ],
            pk: vec![(1, "id")],
        },
        TableSpec {
            name: "purchase",
            rls: false,
            columns: vec![
                Col { attnum: 1, name: "id", oid: 23, type_name: "int4" },
                Col { attnum: 2, name: "customer_id", oid: 23, type_name: "int4" },
                Col { attnum: 3, name: "note", oid: 25, type_name: "text" },
            ],
            pk: vec![(1, "id")],
        },
    ]
}

fn settings() -> Value {
    json!({
        "project": "shop",
        "ontology_version": "ont-1",
        "catalog_version": "cat-1",
        "policy": {"id": "pol-shop", "version": "1"},
        "knowledge": {"id": "know-shop", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "shop",
            "locale": "en-US",
            "include_schemas": ["public"],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        }
    })
}

fn run_bin(env_name: &str, args: &[&str], stdin_text: &str) -> String {
    let program = env::var(env_name).unwrap_or_else(|_| panic!("missing {env_name}"));
    let mut child = Command::new(&program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {program}: {err}"));
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin_text.as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8(output.stdout).expect("stdout utf-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf-8");
    assert!(
        output.status.success(),
        "{program} {args:?} exited {:?}\n{stderr}\n{stdout}",
        output.status.code()
    );
    stdout
}

fn fingerprints_of(artifact: &Value) -> BTreeMap<(String, String), String> {
    let mut out = BTreeMap::new();
    let queue = artifact["draft"]["review_queue"]
        .as_array()
        .expect("review queue");
    for item in queue {
        if item["kind"] != "structural_column" {
            continue;
        }
        let item_id = item["item_id"].as_str().expect("item id");
        let Some(rest) = item_id.strip_prefix("column/") else {
            continue;
        };
        let Some((dataset, column)) = rest.rsplit_once('/') else {
            continue;
        };
        let fingerprint = item["fact_fingerprint"]
            .as_str()
            .expect("fingerprint")
            .to_string();
        out.insert((dataset.to_string(), column.to_string()), fingerprint);
    }
    out
}

fn decision(
    fingerprints: &BTreeMap<(String, String), String>,
    schema_hash: &str,
    dataset: &str,
    column: &str,
    pii: &str,
    semantic: &str,
) -> Value {
    let fingerprint = fingerprints
        .get(&(dataset.to_string(), column.to_string()))
        .unwrap_or_else(|| panic!("missing fingerprint {dataset}.{column}"))
        .clone();
    json!({
        "dataset": dataset,
        "column": column,
        "schema_hash": schema_hash,
        "fact_fingerprint": fingerprint,
        "pii": pii,
        "semantic_type": semantic,
        "reviewer": {"by": "ada.owner", "at": "2026-10-06"}
    })
}

fn shop_decisions(
    fingerprints: &BTreeMap<(String, String), String>,
    schema_hash: &str,
    email_name: &str,
) -> Vec<Value> {
    vec![
        decision(fingerprints, schema_hash, "public.customer", "id", "none", "identifier"),
        decision(fingerprints, schema_hash, "public.customer", email_name, "personal", "email"),
        decision(fingerprints, schema_hash, "public.customer", "active", "none", "boolean_flag"),
        decision(fingerprints, schema_hash, "public.purchase", "id", "none", "identifier"),
        decision(fingerprints, schema_hash, "public.purchase", "customer_id", "none", "foreign_key"),
        decision(fingerprints, schema_hash, "public.purchase", "note", "sensitive", "free_text"),
    ]
}

fn compile_tables(tables: &[TableSpec], decisions: Vec<Value>) -> Chain {
    let catalog = catalog_json(tables);
    let artifact_json = run_bin(
        "OO_A227_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).unwrap(),
    );
    assert!(!artifact_json.ends_with('\n'));
    let artifact: Value = serde_json::from_str(&artifact_json).expect("artifact json");
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .expect("schema hash")
        .to_string();
    assert_eq!(schema_hash, catalog["schema_hash"].as_str().unwrap());
    let artifact_canon = canon(&artifact);
    let decisions_json = serde_json::to_string(&decisions).unwrap();
    let command_in = json!({
        "protocol": "oo.builder.reviewed-effective-oos-command/v1",
        "artifact": artifact,
        "settings": settings(),
        "decisions": decisions,
    });
    let command_json = run_bin(
        "OO_A227_REVIEWED_EFFECTIVE",
        &[],
        &serde_json::to_string(&command_in).unwrap(),
    );
    assert!(!command_json.ends_with('\n'));
    assert!(!command_json.contains(OWNER));
    let command: Value = serde_json::from_str(&command_json).expect("command json");
    assert_eq!(canon(&command), command_json);
    let artifact_sha256 = command["provenance"]["sealed_source_sha256"]
        .as_str()
        .expect("sealed hash")
        .to_string();
    assert_eq!(artifact_sha256, sha256_hex(artifact_canon.as_bytes()));
    Chain {
        artifact_json,
        command_json,
        decisions_json,
        schema_hash,
        artifact_sha256,
    }
}

fn compile_shop(email_name: &'static str, customer_rls: bool) -> Chain {
    let tables = shop_tables(email_name, customer_rls);
    let catalog = catalog_json(&tables);
    let artifact_json = run_bin(
        "OO_A227_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).unwrap(),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).unwrap();
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let prints = fingerprints_of(&artifact);
    let decisions = shop_decisions(&prints, &schema_hash, email_name);
    compile_tables(&tables, decisions)
}

fn binding(chain: &Chain, role: &str, login: &str, allow: Vec<ReviewedAllow>) -> ReviewedBinding {
    ReviewedBinding {
        identity: ReviewedIdentity {
            project: "shop".to_string(),
            ontology_version: "ont-1".to_string(),
            catalog_version: "cat-1".to_string(),
            policy_id: "pol-shop".to_string(),
            policy_version: "1".to_string(),
            knowledge_id: "know-shop".to_string(),
            knowledge_version: "1".to_string(),
        },
        seal: ReviewedSourceSeal {
            source_id: "shop_src".to_string(),
            schema: "public".to_string(),
            schema_hash: chain.schema_hash.clone(),
            artifact_sha256: chain.artifact_sha256.clone(),
        },
        config_digest: DIGEST.to_string(),
        generation: 4,
        role: role.to_string(),
        login: login.to_string(),
        allow,
    }
}

fn allow(dataset: &str, column: &str, explicit_allow: bool) -> ReviewedAllow {
    ReviewedAllow {
        dataset: dataset.to_string(),
        column: column.to_string(),
        explicit_allow,
    }
}

fn clerk_allow() -> Vec<ReviewedAllow> {
    vec![
        allow("public.customer", "id", false),
        allow("public.customer", "email", true),
        allow("public.customer", "active", false),
        allow("public.purchase", "id", false),
        allow("public.purchase", "customer_id", false),
        allow("public.purchase", "note", true),
    ]
}

fn peek_allow() -> Vec<ReviewedAllow> {
    vec![
        allow("public.customer", "id", false),
        allow("public.customer", "active", false),
        allow("public.purchase", "id", false),
        allow("public.purchase", "customer_id", false),
    ]
}

fn compile_with(
    chain: &Chain,
    role: &str,
    login: &str,
    allow_list: Vec<ReviewedAllow>,
) -> Result<ReviewedColumnPolicies, ReviewedPolicyError> {
    let bound = binding(chain, role, login, allow_list);
    compile_reviewed_policies(
        &chain.command_json,
        &chain.artifact_json,
        &chain.decisions_json,
        &bound,
    )
}

fn reason(err: ReviewedPolicyError) -> &'static str {
    match err {
        ReviewedPolicyError::Decision { reason } => reason,
        other => panic!("expected a decision refusal, got {other}"),
    }
}

fn column<'a>(policy: &'a ColumnAclPolicy, name: &str) -> &'a ColumnSpec {
    policy
        .table
        .columns
        .iter()
        .find(|column| column.name == name)
        .unwrap_or_else(|| panic!("missing column {name}"))
}

fn shared(policies: &[ColumnAclPolicy], role: &str, login: &str) {
    assert!(!policies.is_empty());
    for policy in policies {
        assert_eq!(policy.project, "shop");
        assert_eq!(policy.config_digest, DIGEST);
        assert_eq!(policy.generation, 4);
        assert_eq!(policy.role, role);
        assert_eq!(policy.login, login);
        assert!(policy.row_filter.is_none());
        assert!(!format!("{policy:?}").contains(OWNER));
    }
}

fn recanon_command(chain: &Chain, edit: impl FnOnce(&mut Value)) -> String {
    let mut command: Value = serde_json::from_str(&chain.command_json).unwrap();
    edit(&mut command);
    let text = canon(&command);
    assert_eq!(canon(&serde_json::from_str::<Value>(&text).unwrap()), text);
    text
}

#[test]
fn schema_preimage_matches_the_captured_catalog_vector() {
    let one = [TableSpec {
        name: "customer",
        rls: false,
        columns: vec![Col { attnum: 1, name: "id", oid: 23, type_name: "int4" }],
        pk: vec![(1, "id")],
    }];
    assert_eq!(schema_preimage(&one), ONE_COLUMN_PREIMAGE);
    assert_eq!(sha256_hex(ONE_COLUMN_PREIMAGE.as_bytes()), ONE_COLUMN_HASH);
}

#[test]
fn two_tables_and_two_roles_compile_from_the_reviewed_command() {
    let chain = compile_shop("email", false);
    let clerk = compile_with(&chain, "clerk", "clerk_login", clerk_allow()).expect("clerk");
    assert_eq!(clerk.policies.len(), 2);
    assert_eq!(clerk.compiled.len(), 2);
    assert_eq!(clerk.compiled.role(), "oo_r_clerk");
    assert!(!clerk.compiled.is_empty());
    shared(&clerk.policies, "clerk", "clerk_login");
    assert_eq!(clerk.policies[0].table.namespace, "public");
    assert_eq!(clerk.policies[0].table.name, "customer");
    assert_eq!(clerk.policies[1].table.name, "purchase");
    let id = column(&clerk.policies[0], "id");
    assert_eq!(id.status, ClassStatus::Confirmed);
    assert_eq!(id.semantic_type, "identifier");
    assert_eq!(id.pii, PiiClass::None);
    let email = column(&clerk.policies[0], "email");
    assert_eq!(email.semantic_type, "email");
    assert_eq!(email.pii, PiiClass::Declared { explicit_allow: true });
    assert_eq!(column(&clerk.policies[0], "active").semantic_type, "boolean_flag");
    assert_eq!(column(&clerk.policies[0], "active").pii, PiiClass::None);
    assert_eq!(column(&clerk.policies[1], "customer_id").semantic_type, "foreign_key");
    assert_eq!(column(&clerk.policies[1], "note").semantic_type, "free_text");
    assert_eq!(
        column(&clerk.policies[1], "note").pii,
        PiiClass::Declared { explicit_allow: true }
    );
    let peek = compile_with(&chain, "peek", "peek_login", peek_allow()).expect("peek");
    assert_eq!(peek.compiled.role(), "oo_r_peek");
    assert_eq!(peek.compiled.len(), 2);
    shared(&peek.policies, "peek", "peek_login");
    assert!(peek.policies[0].table.columns.iter().all(|column| column.name != "email"));
    assert!(peek.policies[1].table.columns.iter().all(|column| column.name != "note"));
    assert_eq!(peek.policies[0].table.columns.len(), 2);
    assert_eq!(peek.policies[1].table.columns.len(), 2);
}

#[test]
fn stale_fact_refuses_the_whole_allowlist() {
    let tables = shop_tables("email", false);
    let catalog = catalog_json(&tables);
    let artifact_json = run_bin(
        "OO_A227_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).unwrap(),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).unwrap();
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let prints = fingerprints_of(&artifact);
    let mut decisions = shop_decisions(&prints, &schema_hash, "email");
    decisions[1]["fact_fingerprint"] = json!("ab".repeat(32));
    let chain = compile_tables(&tables, decisions);
    let hidden = serde_json::from_str::<Value>(&chain.command_json).unwrap();
    assert!(hidden["hidden"].as_array().unwrap().iter().any(|item| {
        item["column"] == "email" && item["reason"] == "decision.stale_fact"
    }));
    let err = compile_with(
        &chain,
        "clerk",
        "clerk_login",
        vec![
            allow("public.customer", "id", false),
            allow("public.customer", "email", true),
        ],
    )
    .expect_err("stale email refuses id too");
    assert_eq!(reason(err), "stale_fact");
    let id_only = compile_with(
        &chain,
        "clerk",
        "clerk_login",
        vec![allow("public.customer", "id", false)],
    )
    .expect("unrequested stale column stays out of the set");
    assert_eq!(id_only.policies.len(), 1);
    assert_eq!(id_only.policies[0].table.columns.len(), 1);
}

#[test]
fn confirmed_output_with_a_different_fingerprint_is_stale() {
    let chain = compile_shop("email", false);
    let mut decisions: Value = serde_json::from_str(&chain.decisions_json).unwrap();
    decisions
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["column"] == "email")
        .unwrap()["fact_fingerprint"] = json!("cd".repeat(32));
    let command_json = recanon_command(&chain, |command| {
        command["provenance"]["decisions_sha256"] = json!(sha256_hex(canon(&decisions).as_bytes()));
    });
    let bound = binding(&chain, "clerk", "clerk_login", clerk_allow());
    let err = compile_reviewed_policies(
        &command_json,
        &chain.artifact_json,
        &serde_json::to_string(&decisions).unwrap(),
        &bound,
    )
    .expect_err("confirmed output does not override the fingerprint");
    assert_eq!(reason(err), "stale_fact");
}

#[test]
fn stale_schema_refuses_the_column() {
    let tables = shop_tables("email", false);
    let catalog = catalog_json(&tables);
    let artifact_json = run_bin(
        "OO_A227_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).unwrap(),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).unwrap();
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let prints = fingerprints_of(&artifact);
    let mut decisions = shop_decisions(&prints, &schema_hash, "email");
    decisions[1]["schema_hash"] = json!("ef".repeat(32));
    let chain = compile_tables(&tables, decisions);
    let err = compile_with(
        &chain,
        "clerk",
        "clerk_login",
        vec![allow("public.customer", "email", true)],
    )
    .expect_err("stale schema");
    assert_eq!(reason(err), "stale_schema");
}

#[test]
fn renamed_and_unknown_columns_are_unknown() {
    let renamed = compile_shop("contact", false);
    let err = compile_with(
        &renamed,
        "clerk",
        "clerk_login",
        vec![allow("public.customer", "email", true)],
    )
    .expect_err("old name");
    assert_eq!(reason(err), "unknown");
    let chain = compile_shop("email", false);
    let err = compile_with(
        &chain,
        "clerk",
        "clerk_login",
        vec![allow("public.customer", "hidden_col", false)],
    )
    .expect_err("absent name");
    assert_eq!(reason(err), "unknown");
}

#[test]
fn missing_decision_refuses_even_when_auto_accept_is_set() {
    let tables = shop_tables("email", false);
    let catalog = catalog_json(&tables);
    let artifact_json = run_bin(
        "OO_A227_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).unwrap(),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).unwrap();
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let prints = fingerprints_of(&artifact);
    let mut decisions = shop_decisions(&prints, &schema_hash, "email");
    decisions.retain(|item| item["column"] != "id");
    let chain = compile_tables(&tables, decisions);
    let command: Value = serde_json::from_str(&chain.command_json).unwrap();
    assert_eq!(
        command["effective_oos"]["ontology"]["classification"]["auto_accept"],
        "rule_high_confidence"
    );
    assert!(command["hidden"].as_array().unwrap().iter().any(|item| {
        item["column"] == "id" && item["reason"] == "classification.absent"
    }));
    let err = compile_with(
        &chain,
        "clerk",
        "clerk_login",
        vec![
            allow("public.customer", "id", false),
            allow("public.customer", "email", true),
        ],
    )
    .expect_err("missing id refuses email");
    assert_eq!(reason(err), "missing");
}

#[test]
fn rejected_column_refuses_the_set() {
    let tables = shop_tables("email", false);
    let catalog = catalog_json(&tables);
    let first = run_bin(
        "OO_A227_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).unwrap(),
    );
    let first_value: Value = serde_json::from_str(&first).unwrap();
    let schema_hash = first_value["draft"]["provenance"]["schema_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let prints = fingerprints_of(&first_value);
    let email_fp = prints
        .get(&("public.customer".to_string(), "email".to_string()))
        .unwrap()
        .clone();
    let review = json!({
        "catalog": catalog,
        "curated": {
            "datasets": [{
                "dataset": "public.customer",
                "columns": [{"name": "email", "status": "rejected", "label": "Email"}]
            }],
            "relationships": []
        },
        "reviews": [{
            "item_id": "column/public.customer/email",
            "action": "reject",
            "schema_hash": schema_hash,
            "fact_fingerprint": email_fp
        }]
    });
    let artifact_json = run_bin(
        "OO_A227_OO_BUILDER",
        &["review-pg-draft"],
        &serde_json::to_string(&review).unwrap(),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).unwrap();
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let prints = fingerprints_of(&artifact);
    assert_eq!(
        artifact["draft"]["review_queue"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["item_id"] == "column/public.customer/email" && item["kind"] == "structural_column")
            .unwrap()["state"],
        "rejected"
    );
    let mut decisions = shop_decisions(&prints, &schema_hash, "email");
    decisions.retain(|item| item["column"] != "email");
    let command_in = json!({
        "protocol": "oo.builder.reviewed-effective-oos-command/v1",
        "artifact": artifact,
        "settings": settings(),
        "decisions": decisions,
    });
    let command_json = run_bin(
        "OO_A227_REVIEWED_EFFECTIVE",
        &[],
        &serde_json::to_string(&command_in).unwrap(),
    );
    let command: Value = serde_json::from_str(&command_json).unwrap();
    let chain = Chain {
        artifact_json,
        command_json,
        decisions_json: serde_json::to_string(&decisions).unwrap(),
        schema_hash,
        artifact_sha256: command["provenance"]["sealed_source_sha256"]
            .as_str()
            .unwrap()
            .to_string(),
    };
    let err = compile_with(
        &chain,
        "clerk",
        "clerk_login",
        vec![
            allow("public.customer", "id", false),
            allow("public.customer", "email", true),
        ],
    )
    .expect_err("rejected email");
    assert_eq!(reason(err), "rejected");
}

#[test]
fn declared_pii_needs_the_explicit_allow_token() {
    let chain = compile_shop("email", false);
    let err = compile_with(
        &chain,
        "clerk",
        "clerk_login",
        vec![
            allow("public.customer", "id", false),
            allow("public.customer", "email", false),
        ],
    )
    .expect_err("personal without the token");
    assert!(matches!(err, ReviewedPolicyError::PiiAllow));
    assert!(err.code().ends_with("/pii_allow"));
    let err = compile_with(
        &chain,
        "clerk",
        "clerk_login",
        vec![allow("public.customer", "id", true)],
    )
    .expect_err("none with the token");
    assert!(matches!(err, ReviewedPolicyError::PiiAllow));
}

#[test]
fn duplicate_allow_and_duplicate_decision_refuse() {
    let chain = compile_shop("email", false);
    let err = compile_with(
        &chain,
        "clerk",
        "clerk_login",
        vec![
            allow("public.customer", "email", true),
            allow("public.customer", "email", true),
        ],
    )
    .expect_err("duplicate allow");
    assert!(matches!(err, ReviewedPolicyError::Duplicate));
    let mut decisions: Value = serde_json::from_str(&chain.decisions_json).unwrap();
    let email = decisions
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["column"] == "email")
        .unwrap()
        .clone();
    decisions.as_array_mut().unwrap().push(email);
    let bound = binding(&chain, "clerk", "clerk_login", clerk_allow());
    let err = compile_reviewed_policies(
        &chain.command_json,
        &chain.artifact_json,
        &serde_json::to_string(&decisions).unwrap(),
        &bound,
    )
    .expect_err("duplicate decision");
    assert!(matches!(err, ReviewedPolicyError::Duplicate));
}

#[test]
fn identity_must_match_the_effective_document() {
    let chain = compile_shop("email", false);
    let mut bound = binding(&chain, "clerk", "clerk_login", clerk_allow());
    bound.identity.project = "other".to_string();
    let err = compile_reviewed_policies(
        &chain.command_json,
        &chain.artifact_json,
        &chain.decisions_json,
        &bound,
    )
    .expect_err("project");
    assert!(matches!(err, ReviewedPolicyError::Identity));
    assert!(err
        .to_string()
        .contains("reviewed document identity does not match the administrative binding"));
}

#[test]
fn seal_tamper_refuses() {
    let chain = compile_shop("email", false);
    let flipped = recanon_command(&chain, |command| {
        let hash = command["provenance"]["sealed_source_sha256"]
            .as_str()
            .unwrap()
            .to_string();
        let mut chars: Vec<u8> = hash.into_bytes();
        chars[63] = if chars[63] == b'0' { b'1' } else { b'0' };
        command["provenance"]["sealed_source_sha256"] =
            json!(String::from_utf8(chars).unwrap());
    });
    let bound = binding(&chain, "clerk", "clerk_login", clerk_allow());
    let err = compile_reviewed_policies(
        &flipped,
        &chain.artifact_json,
        &chain.decisions_json,
        &bound,
    )
    .expect_err("provenance hash");
    assert!(matches!(err, ReviewedPolicyError::Seal));

    let mut artifact: Value = serde_json::from_str(&chain.artifact_json).unwrap();
    artifact["evidence_envelope"]["catalog"]["tables"][0]["owner"] = json!("owner_role_changed");
    let err = compile_reviewed_policies(
        &chain.command_json,
        &canon(&artifact),
        &chain.decisions_json,
        &bound,
    )
    .expect_err("artifact byte");
    assert!(matches!(err, ReviewedPolicyError::Seal));

    artifact = serde_json::from_str(&chain.artifact_json).unwrap();
    artifact["wrapper"]["grants"] = json!("authorized");
    let err = compile_reviewed_policies(
        &chain.command_json,
        &canon(&artifact),
        &chain.decisions_json,
        &bound,
    )
    .expect_err("wrapper grants");
    assert!(matches!(err, ReviewedPolicyError::Seal));
}

#[test]
fn pretty_output_and_protocol_only_json_refuse() {
    let chain = compile_shop("email", false);
    let bound = binding(&chain, "clerk", "clerk_login", clerk_allow());
    let pretty = format!("{}\n", chain.command_json);
    let err = compile_reviewed_policies(&pretty, &chain.artifact_json, &chain.decisions_json, &bound)
        .expect_err("newline");
    assert!(matches!(err, ReviewedPolicyError::Protocol));
    let only = r#"{"protocol":"oo.builder.reviewed-effective-oos-command/v1"}"#;
    let err = compile_reviewed_policies(only, &chain.artifact_json, &chain.decisions_json, &bound)
        .expect_err("protocol string");
    assert!(matches!(err, ReviewedPolicyError::Shape));
}

#[test]
fn credentials_row_filter_and_raw_sample_are_forbidden() {
    let chain = compile_shop("email", false);
    let bound = binding(&chain, "clerk", "clerk_login", clerk_allow());
    for key in ["credentials", "row_filter", "samples", "password", "secret", "privileges"] {
        let command_json = recanon_command(&chain, |command| {
            command
                .as_object_mut()
                .unwrap()
                .insert(key.to_string(), Value::Null);
        });
        let err = compile_reviewed_policies(
            &command_json,
            &chain.artifact_json,
            &chain.decisions_json,
            &bound,
        )
        .expect_err(key);
        assert!(matches!(err, ReviewedPolicyError::Forbidden), "{key} -> {err}");
    }
    let raw = recanon_command(&chain, |command| {
        command["effective_oos"]["ontology"]["compile"]["sample_values"]["default"] = json!("raw");
    });
    let err = compile_reviewed_policies(&raw, &chain.artifact_json, &chain.decisions_json, &bound)
        .expect_err("raw sample");
    assert!(matches!(err, ReviewedPolicyError::Forbidden));
}

#[test]
fn unsupported_semantic_types_and_float4_refuse() {
    let tables = shop_tables("email", false);
    let catalog = catalog_json(&tables);
    let artifact_json = run_bin(
        "OO_A227_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).unwrap(),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).unwrap();
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let prints = fingerprints_of(&artifact);
    for semantic in ["json", "event_time"] {
        let mut decisions = shop_decisions(&prints, &schema_hash, "email");
        decisions
            .iter_mut()
            .find(|item| item["column"] == "note")
            .unwrap()["semantic_type"] = json!(semantic);
        let chain = compile_tables(&tables, decisions);
        let err = compile_with(
            &chain,
            "clerk",
            "clerk_login",
            vec![allow("public.purchase", "note", true)],
        )
        .expect_err(semantic);
        assert!(matches!(err, ReviewedPolicyError::Semantic), "{semantic} -> {err}");
    }
    let float_table = [TableSpec {
        name: "measure",
        rls: false,
        columns: vec![Col { attnum: 1, name: "amount", oid: 700, type_name: "float4" }],
        pk: vec![(1, "amount")],
    }];
    let catalog = catalog_json(&float_table);
    let artifact_json = run_bin(
        "OO_A227_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).unwrap(),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).unwrap();
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let prints = fingerprints_of(&artifact);
    let decisions = vec![decision(&prints, &schema_hash, "public.measure", "amount", "none", "identifier")];
    let chain = compile_tables(&float_table, decisions);
    let err = compile_with(
        &chain,
        "clerk",
        "clerk_login",
        vec![allow("public.measure", "amount", false)],
    )
    .expect_err("float4");
    assert!(matches!(err, ReviewedPolicyError::Semantic));
}

#[test]
fn rls_on_an_allowlisted_table_refuses() {
    let chain = compile_shop("email", true);
    let err = compile_with(
        &chain,
        "clerk",
        "clerk_login",
        vec![allow("public.customer", "id", false)],
    )
    .expect_err("rls");
    assert!(matches!(err, ReviewedPolicyError::Rls));
    assert!(err.code().ends_with("/rls"));
}

#[test]
fn empty_allowlist_is_the_existing_empty_policy_set() {
    let chain = compile_shop("email", false);
    let err = compile_with(&chain, "clerk", "clerk_login", Vec::new()).expect_err("empty");
    assert!(matches!(err, ReviewedPolicyError::Policy(PolicyError::EmptyPolicySet)));
}

#[test]
fn scanner_and_byte_budget_refuse_before_a_grant() {
    let chain = compile_shop("email", false);
    let bound = binding(&chain, "clerk", "clerk_login", clerk_allow());
    let dup = r#"{"protocol":"oo.builder.reviewed-effective-oos-command/v1","protocol":"x"}"#;
    let err = compile_reviewed_policies(dup, &chain.artifact_json, &chain.decisions_json, &bound)
        .expect_err("duplicate key");
    assert!(matches!(err, ReviewedPolicyError::Scan));
    let huge = "x".repeat(1_048_577);
    let err = compile_reviewed_policies(&huge, &chain.artifact_json, &chain.decisions_json, &bound)
        .expect_err("budget");
    assert!(matches!(err, ReviewedPolicyError::Budget));
}

#[test]
fn hidden_after_a_successful_bind_is_a_seal_failure() {
    let chain = compile_shop("email", false);
    let command_json = recanon_command(&chain, |command| {
        command["hidden"].as_array_mut().unwrap().push(json!({
            "column": "id",
            "dataset": "public.customer",
            "reason": "classification.absent"
        }));
    });
    let bound = binding(
        &chain,
        "clerk",
        "clerk_login",
        vec![allow("public.customer", "id", false)],
    );
    let err = compile_reviewed_policies(
        &command_json,
        &chain.artifact_json,
        &chain.decisions_json,
        &bound,
    )
    .expect_err("hidden id");
    assert!(matches!(err, ReviewedPolicyError::Seal));
}

#[test]
fn output_classification_must_match_the_decision() {
    let chain = compile_shop("email", false);
    let command_json = recanon_command(&chain, |command| {
        let columns = command["effective_oos"]["datasets"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|dataset| dataset["dataset"] == "public.customer")
            .unwrap()["columns"]
            .as_array_mut()
            .unwrap();
        let id = columns.iter_mut().find(|column| column["name"] == "id").unwrap();
        id["pii"] = json!("personal");
    });
    let bound = binding(
        &chain,
        "clerk",
        "clerk_login",
        vec![allow("public.customer", "id", false)],
    );
    let err = compile_reviewed_policies(
        &command_json,
        &chain.artifact_json,
        &chain.decisions_json,
        &bound,
    )
    .expect_err("pii mismatch");
    assert_eq!(reason(err), "mismatch");
}

#[test]
fn adapter_source_does_not_open_a_database() {
    let source = include_str!("../src/reviewed_policy.rs");
    assert!(!source.contains("tokio_postgres"));
    assert!(!source.contains("install_column"));
    assert!(!source.contains("Client"));
}
