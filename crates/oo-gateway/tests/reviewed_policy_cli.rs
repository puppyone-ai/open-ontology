//! `reviewed-policy` reads one operator document and writes a policy-set DTO.
//!
//! `OO_A234_OO_BUILDER`, `OO_A234_REVIEWED_EFFECTIVE`, and
//! `OO_A234_REVIEWED_POLICY` are the compiled binaries. This file does not
//! open a database and does not install a grant.

use std::collections::BTreeMap;
use std::env;
use std::io::Write;
use std::process::{Command, Stdio};

use oo_gateway::policy::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, LogicalTable, PiiClass, PolicyError, RowFilter,
    compile_column_acl_set,
};
use oo_gateway::reviewed_policy::{
    ReviewedAllow, ReviewedBinding, ReviewedIdentity, ReviewedSourceSeal, compile_reviewed_policies,
};
use serde::Deserialize;
use serde_json::{Value, json};

const OWNER: &str = "owner_role_do_not_leak";
const SAMPLE: &str = "do-not-echo-sample";
const CAPTURED_AT: &str = "2026-10-04T12:00:00.000000Z";
const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PROTOCOL: &str = "oo.gateway.reviewed-policy-cli/v1";
const USAGE: &str = "reviewed-policy: usage: reviewed-policy reads one JSON document from stdin and takes no arguments\n";

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

struct CliOut {
    code: i32,
    stdout: String,
    stderr: String,
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

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
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

fn shop_tables() -> Vec<TableSpec> {
    vec![
        TableSpec {
            name: "customer",
            rls: false,
            columns: vec![
                Col {
                    attnum: 1,
                    name: "id",
                    oid: 23,
                    type_name: "int4",
                },
                Col {
                    attnum: 2,
                    name: "email",
                    oid: 25,
                    type_name: "text",
                },
                Col {
                    attnum: 3,
                    name: "active",
                    oid: 16,
                    type_name: "bool",
                },
            ],
            pk: vec![(1, "id")],
        },
        TableSpec {
            name: "purchase",
            rls: false,
            columns: vec![
                Col {
                    attnum: 1,
                    name: "id",
                    oid: 23,
                    type_name: "int4",
                },
                Col {
                    attnum: 2,
                    name: "customer_id",
                    oid: 23,
                    type_name: "int4",
                },
                Col {
                    attnum: 3,
                    name: "note",
                    oid: 25,
                    type_name: "text",
                },
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

fn run_cli(stdin_text: &str, args: &[&str]) -> CliOut {
    let program = env::var("OO_A234_REVIEWED_POLICY").expect("missing OO_A234_REVIEWED_POLICY");
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
    CliOut {
        code: output.status.code().unwrap_or(127),
        stdout: String::from_utf8(output.stdout).expect("stdout utf-8"),
        stderr: String::from_utf8(output.stderr).expect("stderr utf-8"),
    }
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
) -> Vec<Value> {
    vec![
        decision(
            fingerprints,
            schema_hash,
            "public.customer",
            "id",
            "none",
            "identifier",
        ),
        decision(
            fingerprints,
            schema_hash,
            "public.customer",
            "email",
            "personal",
            "email",
        ),
        decision(
            fingerprints,
            schema_hash,
            "public.customer",
            "active",
            "none",
            "boolean_flag",
        ),
        decision(
            fingerprints,
            schema_hash,
            "public.purchase",
            "id",
            "none",
            "identifier",
        ),
        decision(
            fingerprints,
            schema_hash,
            "public.purchase",
            "customer_id",
            "none",
            "foreign_key",
        ),
        decision(
            fingerprints,
            schema_hash,
            "public.purchase",
            "note",
            "sensitive",
            "free_text",
        ),
    ]
}

fn compile_tables(tables: &[TableSpec], decisions: Vec<Value>) -> Chain {
    let catalog = catalog_json(tables);
    let artifact_json = run_bin(
        "OO_A234_OO_BUILDER",
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
    let decisions_json = serde_json::to_string(&decisions).unwrap();
    let command_in = json!({
        "protocol": "oo.builder.reviewed-effective-oos-command/v1",
        "artifact": artifact,
        "settings": settings(),
        "decisions": decisions,
    });
    let command_json = run_bin(
        "OO_A234_REVIEWED_EFFECTIVE",
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
    Chain {
        artifact_json,
        command_json,
        decisions_json,
        schema_hash,
        artifact_sha256,
    }
}

fn compile_shop() -> Chain {
    let tables = shop_tables();
    let catalog = catalog_json(&tables);
    let artifact_json = run_bin(
        "OO_A234_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).unwrap(),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).unwrap();
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let prints = fingerprints_of(&artifact);
    compile_tables(&tables, shop_decisions(&prints, &schema_hash))
}

fn allow(dataset: &str, column: &str, explicit_allow: bool) -> ReviewedAllow {
    ReviewedAllow {
        dataset: dataset.to_string(),
        column: column.to_string(),
        explicit_allow,
    }
}

fn binding(
    chain: &Chain,
    role: &str,
    login: &str,
    allow_list: Vec<ReviewedAllow>,
) -> ReviewedBinding {
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
        binding_vector: None,
        allow: allow_list,
    }
}

fn binding_json(bound: &ReviewedBinding) -> Value {
    json!({
        "identity": {
            "project": bound.identity.project,
            "ontology_version": bound.identity.ontology_version,
            "catalog_version": bound.identity.catalog_version,
            "policy_id": bound.identity.policy_id,
            "policy_version": bound.identity.policy_version,
            "knowledge_id": bound.identity.knowledge_id,
            "knowledge_version": bound.identity.knowledge_version
        },
        "seal": {
            "source_id": bound.seal.source_id,
            "schema": bound.seal.schema,
            "schema_hash": bound.seal.schema_hash,
            "artifact_sha256": bound.seal.artifact_sha256
        },
        "config_digest": bound.config_digest,
        "generation": bound.generation,
        "role": bound.role,
        "login": bound.login,
        "allow": bound.allow.iter().map(|item| json!({
            "dataset": item.dataset,
            "column": item.column,
            "explicit_allow": item.explicit_allow
        })).collect::<Vec<_>>()
    })
}

fn operator(chain: &Chain, role: &str, login: &str, allow_list: Vec<ReviewedAllow>) -> String {
    let bound = binding(chain, role, login, allow_list);
    serde_json::to_string(&json!({
        "protocol": PROTOCOL,
        "command": chain.command_json,
        "artifact": chain.artifact_json,
        "decisions": chain.decisions_json,
        "binding": binding_json(&bound)
    }))
    .unwrap()
}

fn operator_with_command(chain: &Chain, command: &str, allow_list: Vec<ReviewedAllow>) -> String {
    let bound = binding(chain, "clerk", "clerk_login", allow_list);
    serde_json::to_string(&json!({
        "protocol": PROTOCOL,
        "command": command,
        "artifact": chain.artifact_json,
        "decisions": chain.decisions_json,
        "binding": binding_json(&bound)
    }))
    .unwrap()
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

fn assert_line(out: &CliOut, code: i32, line: &str) {
    assert_eq!(out.code, code, "exit");
    assert_eq!(out.stdout, "", "stdout");
    assert_eq!(out.stderr, line, "stderr");
}

fn assert_silent_about(out: &CliOut, marker: &str) {
    if out.stdout.contains(marker) || out.stderr.contains(marker) {
        panic!("diagnostic echoed a marker");
    }
}

fn policies_of(stdout: &str) -> Vec<ColumnAclPolicy> {
    assert!(!stdout.is_empty());
    assert!(!stdout.ends_with('\n'));
    assert!(!stdout.contains(OWNER));
    assert!(!stdout.contains(SAMPLE));
    let doc: SetDoc = serde_json::from_str(stdout).expect("policy set");
    assert_eq!(doc.protocol, PROTOCOL);
    doc.policies
        .iter()
        .map(|item| to_policy(item).expect("column policy"))
        .collect()
}

fn to_policy(doc: &PolicyDoc) -> Result<ColumnAclPolicy, &'static str> {
    let row_filter = match &doc.row_filter {
        None => None,
        Some(filter) => Some(filter.to_filter()?),
    };
    let mut columns = Vec::with_capacity(doc.table.columns.len());
    for column in &doc.table.columns {
        columns.push(column.to_spec()?);
    }
    Ok(ColumnAclPolicy {
        project: doc.project.clone(),
        config_digest: doc.config_digest.clone(),
        generation: doc.generation,
        role: doc.role.clone(),
        login: doc.login.clone(),
        table: LogicalTable {
            namespace: doc.table.namespace.clone(),
            name: doc.table.name.clone(),
            columns,
        },
        row_filter,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SetDoc {
    protocol: String,
    policies: Vec<PolicyDoc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyDoc {
    project: String,
    config_digest: String,
    generation: i64,
    role: String,
    login: String,
    table: TableDoc,
    row_filter: Option<RowFilterDoc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RowFilterDoc {
    kind: String,
    column: String,
    claim: String,
}

impl RowFilterDoc {
    fn to_filter(&self) -> Result<RowFilter, &'static str> {
        if self.kind != "claims" {
            return Err("row filter");
        }
        Ok(RowFilter::Claims {
            column: self.column.clone(),
            claim: self.claim.clone(),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TableDoc {
    namespace: String,
    name: String,
    columns: Vec<ColumnDoc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnDoc {
    name: String,
    status: String,
    semantic_type: String,
    pii: PiiDoc,
}

impl ColumnDoc {
    fn to_spec(&self) -> Result<ColumnSpec, &'static str> {
        let status = match self.status.as_str() {
            "confirmed" => ClassStatus::Confirmed,
            "auto_accepted" => ClassStatus::AutoAccepted,
            "unclassified" => ClassStatus::Unclassified,
            _ => return Err("status"),
        };
        Ok(ColumnSpec {
            name: self.name.clone(),
            status,
            semantic_type: self.semantic_type.clone(),
            pii: self.pii.to_class()?,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PiiDoc {
    class: String,
    #[serde(default)]
    explicit_allow: Option<bool>,
}

impl PiiDoc {
    fn to_class(&self) -> Result<PiiClass, &'static str> {
        match self.class.as_str() {
            "none" if self.explicit_allow.is_none() => Ok(PiiClass::None),
            "unknown" if self.explicit_allow.is_none() => Ok(PiiClass::Unknown),
            "declared" => Ok(PiiClass::Declared {
                explicit_allow: self.explicit_allow.ok_or("pii")?,
            }),
            _ => Err("pii"),
        }
    }
}

#[test]
fn two_roles_deserialize_into_compile_column_acl_set() {
    let chain = compile_shop();
    let clerk_in = operator(&chain, "clerk", "clerk_login", clerk_allow());
    let parsed: Value = serde_json::from_str(&clerk_in).unwrap();
    assert_eq!(parsed["command"].as_str().unwrap(), chain.command_json);
    let clerk = run_cli(&clerk_in, &[]);
    assert_eq!(clerk.code, 0, "{}", clerk.stderr);
    assert_eq!(clerk.stderr, "");
    let again = run_cli(&clerk_in, &[]);
    assert_eq!(again.stdout, clerk.stdout);
    let policies = policies_of(&clerk.stdout);
    let direct = compile_reviewed_policies(
        &chain.command_json,
        &chain.artifact_json,
        &chain.decisions_json,
        &binding(&chain, "clerk", "clerk_login", clerk_allow()),
    )
    .expect("direct clerk");
    assert_eq!(policies, direct.policies);
    let compiled = compile_column_acl_set(&policies).expect("clerk set");
    assert_eq!(compiled.len(), direct.compiled.len());
    assert_eq!(compiled.role(), direct.compiled.role());
    assert_eq!(compiled.role(), "oo_r_clerk");
    assert_eq!(compiled.len(), 2);
    assert_eq!(policies[0].table.name, "customer");
    assert_eq!(policies[1].table.name, "purchase");
    assert!(policies.iter().all(|policy| policy.row_filter.is_none()));
    assert!(clerk.stdout.contains("\"row_filter\":null"));
    let email = policies[0]
        .table
        .columns
        .iter()
        .find(|column| column.name == "email")
        .expect("email");
    assert_eq!(email.status, ClassStatus::Confirmed);
    assert_eq!(email.semantic_type, "email");
    assert_eq!(
        email.pii,
        PiiClass::Declared {
            explicit_allow: true
        }
    );
    assert!(
        clerk
            .stdout
            .contains("\"class\":\"declared\",\"explicit_allow\":true")
    );
    assert!(clerk.stdout.contains("\"class\":\"none\""));
    assert!(!clerk.stdout.contains("\"explicit_allow\":false"));

    let peek = run_cli(&operator(&chain, "peek", "peek_login", peek_allow()), &[]);
    assert_eq!(peek.code, 0, "{}", peek.stderr);
    let peek_policies = policies_of(&peek.stdout);
    let direct_peek = compile_reviewed_policies(
        &chain.command_json,
        &chain.artifact_json,
        &chain.decisions_json,
        &binding(&chain, "peek", "peek_login", peek_allow()),
    )
    .expect("direct peek");
    assert_eq!(peek_policies, direct_peek.policies);
    let peek_set = compile_column_acl_set(&peek_policies).expect("peek set");
    assert_eq!(peek_set.role(), "oo_r_peek");
    assert_eq!(peek_set.len(), direct_peek.compiled.len());
    assert!(peek_policies.iter().all(|policy| {
        policy
            .table
            .columns
            .iter()
            .all(|column| column.name != "email" && column.name != "note")
    }));
}

#[test]
fn stale_schema_from_the_producer_exits_decision() {
    let tables = shop_tables();
    let catalog = catalog_json(&tables);
    let artifact_json = run_bin(
        "OO_A234_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).unwrap(),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).unwrap();
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let prints = fingerprints_of(&artifact);
    let mut decisions = shop_decisions(&prints, &schema_hash);
    decisions[1]["schema_hash"] = json!("ef".repeat(32));
    let chain = compile_tables(&tables, decisions);
    let out = run_cli(
        &operator(
            &chain,
            "clerk",
            "clerk_login",
            vec![allow("public.customer", "email", true)],
        ),
        &[],
    );
    assert_line(&out, 10, "reviewed-policy: decision stale_schema\n");
}

#[test]
fn pii_token_mismatches_exit_pii_allow() {
    let chain = compile_shop();
    let missing = run_cli(
        &operator(
            &chain,
            "clerk",
            "clerk_login",
            vec![
                allow("public.customer", "id", false),
                allow("public.customer", "email", false),
            ],
        ),
        &[],
    );
    assert_line(&missing, 12, "reviewed-policy: pii_allow\n");
    let extra = run_cli(
        &operator(
            &chain,
            "clerk",
            "clerk_login",
            vec![allow("public.customer", "id", true)],
        ),
        &[],
    );
    assert_line(&extra, 12, "reviewed-policy: pii_allow\n");
}

#[test]
fn forbidden_sample_is_not_echoed() {
    let chain = compile_shop();
    let mut command: Value = serde_json::from_str(&chain.command_json).unwrap();
    command
        .as_object_mut()
        .unwrap()
        .insert(String::from("samples"), json!(SAMPLE));
    let text = canon(&command);
    let out = run_cli(&operator_with_command(&chain, &text, clerk_allow()), &[]);
    assert_silent_about(&out, SAMPLE);
    assert_line(&out, 14, "reviewed-policy: forbidden\n");
}

#[test]
fn malformed_oversize_usage_and_request_are_bounded() {
    assert_line(&run_cli("{", &[]), 4, "reviewed-policy: scan\n");
    let duplicate = r#"{"protocol":"oo.gateway.reviewed-policy-cli/v1","protocol":"x"}"#;
    assert_line(&run_cli(duplicate, &[]), 4, "reviewed-policy: scan\n");

    let mut body = String::from("OVERSIZE-MARKER");
    body.push_str(&"x".repeat(1_048_577 - "OVERSIZE-MARKER".len()));
    assert_eq!(body.len(), 1_048_577);
    let over = run_cli(&body, &[]);
    assert_silent_about(&over, "OVERSIZE-MARKER");
    assert_line(&over, 3, "reviewed-policy: budget\n");

    let usage = run_cli("", &[SAMPLE]);
    assert_silent_about(&usage, SAMPLE);
    assert_line(&usage, 2, USAGE);

    let chain = compile_shop();
    let mut doc: Value =
        serde_json::from_str(&operator(&chain, "clerk", "clerk_login", clerk_allow())).unwrap();
    doc.as_object_mut()
        .unwrap()
        .insert(String::from("extra"), json!(1));
    let extra = run_cli(&serde_json::to_string(&doc).unwrap(), &[]);
    assert_line(&extra, 5, "reviewed-policy: request\n");

    doc = serde_json::from_str(&operator(&chain, "clerk", "clerk_login", clerk_allow())).unwrap();
    doc["command"] = json!(format!("{}\n", chain.command_json));
    let pretty = run_cli(&serde_json::to_string(&doc).unwrap(), &[]);
    assert_line(&pretty, 6, "reviewed-policy: protocol\n");

    let empty = run_cli(&operator(&chain, "clerk", "clerk_login", Vec::new()), &[]);
    assert_line(&empty, 16, "reviewed-policy: policy\n");
}

#[test]
fn claims_row_filter_dto_is_refused_by_compile_column_acl_set() {
    let text = r#"{"protocol":"oo.gateway.reviewed-policy-cli/v1","policies":[{"project":"shop","config_digest":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","generation":4,"role":"clerk","login":"clerk_login","table":{"namespace":"public","name":"customer","columns":[{"name":"id","status":"confirmed","semantic_type":"identifier","pii":{"class":"none"}}]},"row_filter":{"kind":"claims","column":"id","claim":"subject"}}]}"#;
    let policies = policies_of(text);
    assert!(matches!(
        policies[0].row_filter,
        Some(RowFilter::Claims { .. })
    ));
    let err = compile_column_acl_set(&policies).expect_err("claims");
    assert!(matches!(err, PolicyError::RowFilterRefused));
}
