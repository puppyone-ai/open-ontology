//! Independent review of `compile_reviewed_policies` at baf70c5c.
//!
//! The fixture is a ledger catalog. It is not the author shop fixture.
//! `OO_A233_OO_BUILDER` and `OO_A233_REVIEWED_EFFECTIVE` are the binaries
//! built from the fixed archive. This file does not open a database, grant,
//! publish, or authenticate a reviewer.

use std::collections::BTreeMap;
use std::env;
use std::io::Write;
use std::process::{Command, Stdio};

use oo_gateway::reviewed_policy::{
    compile_reviewed_policies, ReviewedAllow, ReviewedBinding, ReviewedColumnPolicies,
    ReviewedIdentity, ReviewedPolicyError, ReviewedSourceSeal,
};
use oo_gateway::{ClassStatus, ColumnAclPolicy, ColumnSpec, PiiClass};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const OWNER: &str = "ledger_owner_do_not_copy";
const CAPTURED_AT: &str = "2026-10-05T08:30:00.000000Z";
const DIGEST: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
const REVIEWER: &str = "nina.records";
const REVIEWED_ON: &str = "2026-10-05";

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

fn raw_value(json: &str) -> &str {
    let bytes = json.as_bytes();
    assert!(bytes[0] == b'{' || bytes[0] == b'[');
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escape = false;
    for (index, byte) in bytes.iter().enumerate() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if *byte == b'\\' {
                escape = true;
                continue;
            }
            if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return &json[..=index];
                }
            }
            _ => {}
        }
    }
    panic!("unclosed json value");
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for char in value.chars() {
        match char {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(char),
        }
    }
    out.push('"');
}

fn write_pair(buf: &mut String, name: &str, value: &str) {
    push_string(buf, name);
    buf.push(':');
    push_string(buf, value);
}

fn schema_preimage(tables: &[TableSpec]) -> String {
    let mut ordered: Vec<&TableSpec> = tables.iter().collect();
    ordered.sort_by_key(|table| table.name);
    let mut buf = String::from("{\"tables\":[");
    for (index, table) in ordered.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push('{');
        write_pair(&mut buf, "schema", "public");
        buf.push(',');
        write_pair(&mut buf, "name", table.name);
        buf.push_str(",\"relkind\":\"r\",\"replica_identity\":\"d\",\"rls\":");
        buf.push_str(if table.rls { "true" } else { "false" });
        buf.push_str(",\"force_rls\":false,\"columns\":[");
        for (col_index, column) in table.columns.iter().enumerate() {
            if col_index > 0 {
                buf.push(',');
            }
            buf.push_str("{\"attnum\":");
            buf.push_str(&column.attnum.to_string());
            buf.push(',');
            write_pair(&mut buf, "name", column.name);
            buf.push_str(",\"type_oid\":");
            buf.push_str(&column.oid.to_string());
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
        "source_id": "ledger_src",
        "database": "ledger",
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

fn ledger_tables() -> Vec<TableSpec> {
    vec![
        TableSpec {
            name: "account",
            rls: false,
            columns: vec![
                Col { attnum: 1, name: "id", oid: 20, type_name: "int8" },
                Col { attnum: 2, name: "email", oid: 25, type_name: "text" },
                Col { attnum: 3, name: "open", oid: 16, type_name: "bool" },
            ],
            pk: vec![(1, "id")],
        },
        TableSpec {
            name: "entry",
            rls: false,
            columns: vec![
                Col { attnum: 1, name: "id", oid: 23, type_name: "int4" },
                Col { attnum: 2, name: "account_id", oid: 23, type_name: "int4" },
                Col { attnum: 3, name: "memo", oid: 25, type_name: "text" },
            ],
            pk: vec![(1, "id")],
        },
    ]
}

fn settings() -> Value {
    json!({
        "project": "ledger",
        "ontology_version": "ont-ledger",
        "catalog_version": "cat-ledger",
        "policy": {"id": "pol-ledger", "version": "1"},
        "knowledge": {"id": "know-ledger", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "ledger",
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
        .env_clear()
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
    let queue = artifact["draft"]["review_queue"].as_array().expect("review queue");
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
        let fingerprint = item["fact_fingerprint"].as_str().expect("fingerprint").to_string();
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
        "reviewer": {"by": REVIEWER, "at": REVIEWED_ON}
    })
}

fn ledger_decisions(
    fingerprints: &BTreeMap<(String, String), String>,
    schema_hash: &str,
) -> Vec<Value> {
    vec![
        decision(fingerprints, schema_hash, "public.account", "id", "none", "identifier"),
        decision(fingerprints, schema_hash, "public.account", "email", "personal", "email"),
        decision(fingerprints, schema_hash, "public.account", "open", "none", "boolean_flag"),
        decision(fingerprints, schema_hash, "public.entry", "id", "none", "identifier"),
        decision(fingerprints, schema_hash, "public.entry", "account_id", "none", "foreign_key"),
        decision(fingerprints, schema_hash, "public.entry", "memo", "sensitive", "free_text"),
    ]
}

fn compile_tables(tables: &[TableSpec], decisions: Vec<Value>) -> Chain {
    let catalog = catalog_json(tables);
    let artifact_json = run_bin(
        "OO_A233_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).expect("catalog"),
    );
    assert!(!artifact_json.ends_with('\n'));
    let artifact: Value = serde_json::from_str(&artifact_json).expect("artifact json");
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .expect("schema hash")
        .to_string();
    assert_eq!(schema_hash, catalog["schema_hash"].as_str().unwrap());
    let decisions_json = serde_json::to_string(&decisions).expect("decisions");
    let command_in = json!({
        "protocol": "oo.builder.reviewed-effective-oos-command/v1",
        "artifact": artifact,
        "settings": settings(),
        "decisions": decisions,
    });
    let command_json = run_bin(
        "OO_A233_REVIEWED_EFFECTIVE",
        &[],
        &serde_json::to_string(&command_in).expect("command in"),
    );
    assert!(!command_json.ends_with('\n'));
    assert!(
        !command_json.contains(OWNER),
        "effective command copied the catalog owner"
    );
    let command: Value = serde_json::from_str(&command_json).expect("command json");
    let artifact_sha256 = command["provenance"]["sealed_source_sha256"]
        .as_str()
        .expect("sealed hash")
        .to_string();
    Chain {
        artifact_json,
        command_json,
        decisions_json,
        schema_hash,
        artifact_sha256,
    }
}

fn compile_ledger() -> Chain {
    let tables = ledger_tables();
    let catalog = catalog_json(&tables);
    let artifact_json = run_bin(
        "OO_A233_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).expect("catalog"),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).expect("artifact");
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .expect("schema hash")
        .to_string();
    let prints = fingerprints_of(&artifact);
    compile_tables(&tables, ledger_decisions(&prints, &schema_hash))
}

fn allow(dataset: &str, column: &str, explicit_allow: bool) -> ReviewedAllow {
    ReviewedAllow {
        dataset: dataset.to_string(),
        column: column.to_string(),
        explicit_allow,
    }
}

fn teller_allow() -> Vec<ReviewedAllow> {
    vec![
        allow("public.account", "id", false),
        allow("public.account", "email", true),
        allow("public.account", "open", false),
        allow("public.entry", "id", false),
        allow("public.entry", "account_id", false),
        allow("public.entry", "memo", true),
    ]
}

fn audit_allow() -> Vec<ReviewedAllow> {
    vec![
        allow("public.account", "id", false),
        allow("public.account", "open", false),
        allow("public.entry", "id", false),
        allow("public.entry", "account_id", false),
    ]
}

fn binding(chain: &Chain, role: &str, login: &str, allow_list: Vec<ReviewedAllow>) -> ReviewedBinding {
    ReviewedBinding {
        identity: ReviewedIdentity {
            project: "ledger".to_string(),
            ontology_version: "ont-ledger".to_string(),
            catalog_version: "cat-ledger".to_string(),
            policy_id: "pol-ledger".to_string(),
            policy_version: "1".to_string(),
            knowledge_id: "know-ledger".to_string(),
            knowledge_version: "1".to_string(),
        },
        seal: ReviewedSourceSeal {
            source_id: "ledger_src".to_string(),
            schema: "public".to_string(),
            schema_hash: chain.schema_hash.clone(),
            artifact_sha256: chain.artifact_sha256.clone(),
        },
        config_digest: DIGEST.to_string(),
        generation: 7,
        role: role.to_string(),
        login: login.to_string(),
        allow: allow_list,
    }
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

fn decision_reason(err: ReviewedPolicyError) -> &'static str {
    match err {
        ReviewedPolicyError::Decision { reason } => reason,
        other => panic!("expected a decision refusal, got {other} {}", other.code()),
    }
}

fn spec<'a>(policy: &'a ColumnAclPolicy, name: &str) -> &'a ColumnSpec {
    policy
        .table
        .columns
        .iter()
        .find(|column| column.name == name)
        .unwrap_or_else(|| panic!("missing column {name}"))
}

fn names(policy: &ColumnAclPolicy) -> Vec<&str> {
    policy.table.columns.iter().map(|column| column.name.as_str()).collect()
}

fn assert_producer_draft_hash(chain: &Chain) {
    assert!(chain.artifact_json.starts_with("{\"draft\":"));
    let draft_bytes = raw_value(&chain.artifact_json["{\"draft\":".len()..]);
    let recomputed = sha256_hex(draft_bytes.as_bytes());
    let artifact: Value = serde_json::from_str(&chain.artifact_json).expect("artifact");
    let command: Value = serde_json::from_str(&chain.command_json).expect("command");
    let wrapper_hash = artifact["wrapper"]["draft_hash"].as_str().expect("wrapper draft_hash");
    let command_hash = command["provenance"]["draft_hash"].as_str().expect("command draft_hash");
    assert_eq!(wrapper_hash, recomputed);
    assert_eq!(command_hash, recomputed);
    let provenance = artifact["draft"]["provenance"].as_object().expect("draft provenance");
    assert!(provenance.get("draft_hash").is_none());
    assert_eq!(
        provenance.get("manifest_hash").and_then(Value::as_str).expect("manifest").len(),
        64
    );
    assert_eq!(provenance["schema_hash"].as_str().expect("schema"), chain.schema_hash);
    let artifact_hash = sha256_hex(chain.artifact_json.as_bytes());
    assert_eq!(
        command["provenance"]["sealed_source_sha256"].as_str().unwrap(),
        artifact_hash
    );
    assert_eq!(chain.artifact_sha256, artifact_hash);
    for key in ["authentication", "authorization", "grants", "publication", "semantic_publication"] {
        assert_eq!(command["provenance"][key].as_str(), Some("not_claimed"), "{key}");
    }
    assert_eq!(artifact["wrapper"]["publication"], "unavailable");
    assert_eq!(artifact["wrapper"]["grants"], "not_authorized");
    assert_eq!(artifact["draft"]["provenance"]["publication"], "unavailable");
    assert_eq!(command["protocol"], "oo.builder.reviewed-effective-oos-command/v1");
    assert_eq!(command["compiler"], "oo-builder/reviewed-effective-oos/v1");
}

fn flip_last_hex(json: &str, key: &str) -> String {
    let marker = format!("\"{key}\":\"");
    let start = json.find(&marker).unwrap_or_else(|| panic!("missing {key}")) + marker.len();
    let mut bytes = json.as_bytes().to_vec();
    let pos = start + 63;
    assert!(bytes[pos].is_ascii_hexdigit());
    bytes[pos] = if bytes[pos] == b'0' { b'1' } else { b'0' };
    String::from_utf8(bytes).expect("utf-8")
}

fn effective_column<'a>(command: &'a Value, dataset: &str, column: &str) -> &'a Value {
    command["effective_oos"]["datasets"]
        .as_array()
        .expect("datasets")
        .iter()
        .find(|item| item["dataset"] == dataset)
        .unwrap_or_else(|| panic!("missing dataset {dataset}"))["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|item| item["name"] == column)
        .unwrap_or_else(|| panic!("missing column {column}"))
}

#[test]
fn two_roles_compile_from_the_producer_output() {
    let chain = compile_ledger();
    assert_producer_draft_hash(&chain);
    let command: Value = serde_json::from_str(&chain.command_json).expect("command");
    let email = effective_column(&command, "public.account", "email");
    assert_eq!(email["data_type"], "text");
    assert_eq!(email["semantic_type"], "email");
    assert_eq!(email["pii"], "personal");
    assert_eq!(email["classification"]["state"], "confirmed");
    assert_eq!(email["classification"]["by"], REVIEWER);
    assert_eq!(email["classification"]["at"], REVIEWED_ON);
    assert!(email["review"].is_null());
    assert_eq!(effective_column(&command, "public.account", "id")["data_type"], "int8");
    assert_eq!(effective_column(&command, "public.account", "open")["data_type"], "bool");
    assert_eq!(effective_column(&command, "public.entry", "account_id")["data_type"], "int4");
    assert_ne!(REVIEWER, "teller_desk");

    let teller = compile_with(&chain, "teller", "teller_desk", teller_allow()).expect("teller");
    assert_eq!(teller.compiled.role(), "oo_r_teller");
    assert_eq!(teller.compiled.len(), 2);
    assert_eq!(teller.policies.len(), 2);
    assert_eq!(teller.policies[0].table.namespace, "public");
    assert_eq!(teller.policies[0].table.name, "account");
    assert_eq!(teller.policies[1].table.name, "entry");
    assert_eq!(names(&teller.policies[0]), ["id", "email", "open"]);
    assert_eq!(names(&teller.policies[1]), ["id", "account_id", "memo"]);
    assert_eq!(spec(&teller.policies[0], "id").status, ClassStatus::Confirmed);
    assert_eq!(spec(&teller.policies[0], "id").semantic_type, "identifier");
    assert_eq!(spec(&teller.policies[0], "id").pii, PiiClass::None);
    assert_eq!(spec(&teller.policies[0], "email").semantic_type, "email");
    assert_eq!(
        spec(&teller.policies[0], "email").pii,
        PiiClass::Declared { explicit_allow: true }
    );
    assert_eq!(spec(&teller.policies[0], "open").semantic_type, "boolean_flag");
    assert_eq!(spec(&teller.policies[1], "account_id").semantic_type, "foreign_key");
    assert_eq!(spec(&teller.policies[1], "memo").semantic_type, "free_text");
    assert_eq!(
        spec(&teller.policies[1], "memo").pii,
        PiiClass::Declared { explicit_allow: true }
    );
    for policy in &teller.policies {
        assert_eq!(policy.project, "ledger");
        assert_eq!(policy.config_digest, DIGEST);
        assert_eq!(policy.generation, 7);
        assert_eq!(policy.role, "teller");
        assert_eq!(policy.login, "teller_desk");
        assert!(policy.row_filter.is_none());
        assert!(!format!("{policy:?}").contains(OWNER));
        assert!(!format!("{policy:?}").contains(REVIEWER));
    }

    let audit = compile_with(&chain, "audit", "audit_desk", audit_allow()).expect("audit");
    assert_eq!(audit.compiled.role(), "oo_r_audit");
    assert_eq!(audit.compiled.len(), 2);
    assert_eq!(names(&audit.policies[0]), ["id", "open"]);
    assert_eq!(names(&audit.policies[1]), ["id", "account_id"]);
    for policy in &audit.policies {
        assert_eq!(policy.role, "audit");
        assert_eq!(policy.login, "audit_desk");
        assert_eq!(policy.generation, 7);
        assert!(policy.row_filter.is_none());
        assert!(policy.table.columns.iter().all(|column| column.pii == PiiClass::None));
    }
}

#[test]
fn unknown_column_refuses() {
    let chain = compile_ledger();
    let err = compile_with(
        &chain,
        "teller",
        "teller_desk",
        vec![allow("public.account", "ghost", false)],
    )
    .expect_err("absent column");
    assert_eq!(decision_reason(err), "unknown");
}

#[test]
fn stale_schema_and_stale_fact_refuse() {
    let tables = ledger_tables();
    let catalog = catalog_json(&tables);
    let artifact_json = run_bin(
        "OO_A233_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).expect("catalog"),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).expect("artifact");
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .expect("schema")
        .to_string();
    let prints = fingerprints_of(&artifact);

    let mut stale_schema = ledger_decisions(&prints, &schema_hash);
    stale_schema
        .iter_mut()
        .find(|item| item["column"] == "email")
        .expect("email")["schema_hash"] = json!("ef".repeat(32));
    let chain = compile_tables(&tables, stale_schema);
    let command: Value = serde_json::from_str(&chain.command_json).expect("command");
    assert!(command["hidden"].as_array().expect("hidden").iter().any(|item| {
        item["column"] == "email" && item["reason"] == "decision.stale_schema"
    }));
    let err = compile_with(
        &chain,
        "teller",
        "teller_desk",
        vec![allow("public.account", "email", true)],
    )
    .expect_err("stale schema");
    assert_eq!(decision_reason(err), "stale_schema");

    let mut stale_fact = ledger_decisions(&prints, &schema_hash);
    stale_fact
        .iter_mut()
        .find(|item| item["column"] == "memo")
        .expect("memo")["fact_fingerprint"] = json!("ab".repeat(32));
    let chain = compile_tables(&tables, stale_fact);
    let command: Value = serde_json::from_str(&chain.command_json).expect("command");
    assert!(command["hidden"].as_array().expect("hidden").iter().any(|item| {
        item["column"] == "memo" && item["reason"] == "decision.stale_fact"
    }));
    let err = compile_with(
        &chain,
        "teller",
        "teller_desk",
        vec![
            allow("public.entry", "id", false),
            allow("public.entry", "memo", true),
        ],
    )
    .expect_err("stale memo refuses the entry set");
    assert_eq!(decision_reason(err), "stale_fact");
}

#[test]
fn missing_decision_refuses_the_set() {
    let tables = ledger_tables();
    let catalog = catalog_json(&tables);
    let artifact_json = run_bin(
        "OO_A233_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).expect("catalog"),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).expect("artifact");
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .expect("schema")
        .to_string();
    let prints = fingerprints_of(&artifact);
    let mut decisions = ledger_decisions(&prints, &schema_hash);
    decisions.retain(|item| item["column"] != "open");
    let chain = compile_tables(&tables, decisions);
    let command: Value = serde_json::from_str(&chain.command_json).expect("command");
    assert_eq!(
        command["effective_oos"]["ontology"]["classification"]["auto_accept"],
        "rule_high_confidence"
    );
    assert!(command["hidden"].as_array().expect("hidden").iter().any(|item| {
        item["column"] == "open" && item["reason"] == "classification.absent"
    }));
    let err = compile_with(
        &chain,
        "audit",
        "audit_desk",
        vec![
            allow("public.account", "id", false),
            allow("public.account", "open", false),
        ],
    )
    .expect_err("missing open refuses id");
    assert_eq!(decision_reason(err), "missing");
}

#[test]
fn tampered_hashes_and_an_invented_draft_hash_refuse() {
    let chain = compile_ledger();
    let bound = binding(&chain, "teller", "teller_desk", teller_allow());
    let flipped_source = flip_last_hex(&chain.command_json, "sealed_source_sha256");
    let err = compile_reviewed_policies(
        &flipped_source,
        &chain.artifact_json,
        &chain.decisions_json,
        &bound,
    )
    .expect_err("sealed source");
    assert!(matches!(err, ReviewedPolicyError::Seal));
    assert!(err.code().ends_with("/seal"));

    let flipped_draft = flip_last_hex(&chain.command_json, "draft_hash");
    let err = compile_reviewed_policies(
        &flipped_draft,
        &chain.artifact_json,
        &chain.decisions_json,
        &bound,
    )
    .expect_err("command draft_hash");
    assert!(matches!(err, ReviewedPolicyError::Seal));

    let mut artifact: Value = serde_json::from_str(&chain.artifact_json).expect("artifact");
    artifact["evidence_envelope"]["catalog"]["tables"][0]["owner"] = json!("changed_owner_role");
    let err = compile_reviewed_policies(
        &chain.command_json,
        &serde_json::to_string(&artifact).expect("tampered artifact"),
        &chain.decisions_json,
        &bound,
    )
    .expect_err("artifact byte");
    assert!(matches!(err, ReviewedPolicyError::Seal));

    artifact = serde_json::from_str(&chain.artifact_json).expect("artifact");
    let placed = artifact["wrapper"]["draft_hash"].clone();
    artifact["draft"]["provenance"]["draft_hash"] = placed;
    let err = compile_reviewed_policies(
        &chain.command_json,
        &serde_json::to_string(&artifact).expect("invented field"),
        &chain.decisions_json,
        &bound,
    )
    .expect_err("draft provenance draft_hash is not a repair");
    assert!(matches!(err, ReviewedPolicyError::Seal));
}

#[test]
fn pii_allow_token_is_required() {
    let chain = compile_ledger();
    let err = compile_with(
        &chain,
        "teller",
        "teller_desk",
        vec![allow("public.account", "email", false)],
    )
    .expect_err("personal without the token");
    assert!(matches!(err, ReviewedPolicyError::PiiAllow));
    assert!(err.code().ends_with("/pii_allow"));
    let err = compile_with(
        &chain,
        "audit",
        "audit_desk",
        vec![allow("public.account", "id", true)],
    )
    .expect_err("none with the token");
    assert!(matches!(err, ReviewedPolicyError::PiiAllow));
}

#[test]
fn float4_and_json_semantic_refuse() {
    let tables = ledger_tables();
    let catalog = catalog_json(&tables);
    let artifact_json = run_bin(
        "OO_A233_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).expect("catalog"),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).expect("artifact");
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .expect("schema")
        .to_string();
    let prints = fingerprints_of(&artifact);
    let mut decisions = ledger_decisions(&prints, &schema_hash);
    decisions
        .iter_mut()
        .find(|item| item["column"] == "memo")
        .expect("memo")["semantic_type"] = json!("json");
    let chain = compile_tables(&tables, decisions);
    let err = compile_with(
        &chain,
        "teller",
        "teller_desk",
        vec![allow("public.entry", "memo", true)],
    )
    .expect_err("json semantic");
    assert!(matches!(err, ReviewedPolicyError::Semantic));
    assert!(err.code().ends_with("/semantic"));

    let gauge = [TableSpec {
        name: "gauge",
        rls: false,
        columns: vec![Col { attnum: 1, name: "reading", oid: 700, type_name: "float4" }],
        pk: vec![(1, "reading")],
    }];
    let catalog = catalog_json(&gauge);
    let artifact_json = run_bin(
        "OO_A233_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).expect("gauge"),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).expect("artifact");
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .expect("schema")
        .to_string();
    let prints = fingerprints_of(&artifact);
    let decisions = vec![decision(
        &prints,
        &schema_hash,
        "public.gauge",
        "reading",
        "none",
        "identifier",
    )];
    let chain = compile_tables(&gauge, decisions);
    let err = compile_with(
        &chain,
        "teller",
        "teller_desk",
        vec![allow("public.gauge", "reading", false)],
    )
    .expect_err("float4");
    assert!(matches!(err, ReviewedPolicyError::Semantic));
}

#[test]
fn identity_mismatch_uses_the_contract_sentence() {
    let chain = compile_ledger();
    let mut bound = binding(&chain, "teller", "teller_desk", teller_allow());
    bound.identity.project = "other".to_string();
    let err = compile_reviewed_policies(
        &chain.command_json,
        &chain.artifact_json,
        &chain.decisions_json,
        &bound,
    )
    .expect_err("project");
    assert!(matches!(err, ReviewedPolicyError::Identity));
    assert!(err.code().ends_with("/identity"));
    assert_eq!(
        err.to_string(),
        "reviewed document identity does not match the administrative binding"
    );
    let command: Value = serde_json::from_str(&chain.command_json).expect("command");
    assert_eq!(command["provenance"]["authentication"], "not_claimed");
    assert_eq!(command["provenance"]["semantic_publication"], "not_claimed");
}

#[test]
fn adapter_source_does_not_publish_or_connect() {
    let source = include_str!("../src/reviewed_policy.rs");
    assert!(!source.contains("tokio_postgres"));
    assert!(!source.contains("install_column"));
    assert!(!source.contains("Client"));
    assert!(!source.contains("compile_structure_draft"));
}
