//! Integer quantity from a compiled reviewed-effective command through the CLI.
//!
//! `OO_A254_OO_BUILDER`, `OO_A254_REVIEWED_EFFECTIVE`, and
//! `OO_A254_REVIEWED_POLICY` are those binaries. Hand amounts are the integer
//! extremes below. This file does not open a database and does not install.

use std::collections::BTreeMap;
use std::env;
use std::io::Write;
use std::process::{Command, Stdio};

use oo_gateway::policy::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, LogicalTable, PiiClass, RowFilter,
    compile_column_acl_set,
};
use oo_gateway::reviewed_policy::{
    ReviewedAllow, ReviewedBinding, ReviewedIdentity, ReviewedSourceSeal,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const OWNER: &str = "owner_role_do_not_leak";
const CAPTURED_AT: &str = "2026-10-04T12:00:00.000000Z";
const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PROTOCOL: &str = "oo.gateway.reviewed-policy-cli/v1";
const BINDING_GENERATION: i64 = 7;

/// Handwritten decimal text. Not a series from a generator.
const AMOUNT_I2: [&str; 3] = ["-32768", "32767", "0"];
const AMOUNT_I4: [&str; 3] = ["-2147483648", "2147483647", "0"];
const AMOUNT_I8: [&str; 3] = ["-9223372036854775808", "9223372036854775807", "0"];
const AMOUNT_COLUMNS: [&str; 3] = ["amount_i2", "amount_i4", "amount_i8"];

struct Col {
    attnum: i32,
    name: &'static str,
    oid: i64,
    type_name: &'static str,
}

struct TableSpec {
    name: &'static str,
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
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(ch)));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn schema_preimage(table: &TableSpec) -> String {
    let mut buf = String::from("{\"tables\":[{");
    write_pair(&mut buf, "schema", "public");
    buf.push(',');
    write_pair(&mut buf, "name", table.name);
    buf.push(',');
    write_pair(&mut buf, "relkind", "r");
    buf.push(',');
    write_pair(&mut buf, "replica_identity", "d");
    buf.push(',');
    write_bool(&mut buf, "rls", false);
    buf.push(',');
    write_bool(&mut buf, "force_rls", false);
    buf.push_str(",\"columns\":[");
    for (index, column) in table.columns.iter().enumerate() {
        if index > 0 {
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
    for (index, (attnum, name)) in table.pk.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push_str("{\"attnum\":");
        buf.push_str(&attnum.to_string());
        buf.push_str(",\"name\":");
        push_string(&mut buf, name);
        buf.push('}');
    }
    buf.push_str("],\"primary_key_nulls_not_distinct\":false,\"unique_keys\":[],\"foreign_keys\":[],\"policies\":[]}]}");
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
    // typmod -1 numeric is unbounded. The draft classifier requires that reason.
    let reason = if column.type_name == "numeric" {
        "unbounded_or_wide_numeric"
    } else {
        "known_builtin_not_converted"
    };
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
        "type_support": {"converted": false, "reason": reason},
        "current_privileges": {"select": true, "insert": false, "update": false}
    })
}

fn catalog_json(table: &TableSpec) -> Value {
    let hash = sha256_hex(schema_preimage(table).as_bytes());
    json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": "shop_src",
        "database": "shop",
        "captured_at": CAPTURED_AT,
        "schema_hash": hash,
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": [json!({
            "schema": "public",
            "schema_ident": "public",
            "name": table.name,
            "name_ident": table.name,
            "relkind": "r",
            "replica_identity": "d",
            "rls_enabled": false,
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
        })],
        "type_conversion": "unsupported"
    })
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

fn measure_table() -> TableSpec {
    TableSpec {
        name: "measure",
        columns: vec![
            Col {
                attnum: 1,
                name: "batch_i8",
                oid: 20,
                type_name: "int8",
            },
            Col {
                attnum: 2,
                name: "amount_i2",
                oid: 21,
                type_name: "int2",
            },
            Col {
                attnum: 3,
                name: "amount_i4",
                oid: 23,
                type_name: "int4",
            },
            Col {
                attnum: 4,
                name: "amount_i8",
                oid: 20,
                type_name: "int8",
            },
        ],
        pk: vec![(1, "batch_i8")],
    }
}

fn one_column(type_name: &'static str, oid: i64) -> TableSpec {
    TableSpec {
        name: "measure",
        columns: vec![Col {
            attnum: 1,
            name: "amount",
            oid,
            type_name,
        }],
        pk: vec![(1, "amount")],
    }
}

struct Spawn {
    code: i32,
    stdout: String,
    stderr: String,
}

fn spawn(env_name: &str, args: &[&str], stdin_text: &str) -> Spawn {
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
    Spawn {
        code: output.status.code().unwrap_or(127),
        stdout: String::from_utf8(output.stdout).expect("stdout utf-8"),
        stderr: String::from_utf8(output.stderr).expect("stderr utf-8"),
    }
}

fn run_bin(env_name: &str, args: &[&str], stdin_text: &str) -> String {
    let output = spawn(env_name, args, stdin_text);
    assert!(
        output.code == 0,
        "{env_name} {args:?} exited {}\n{}\n{}",
        output.code,
        output.stderr,
        output.stdout
    );
    output.stdout
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
    column: &str,
    pii: &str,
    semantic: &str,
) -> Value {
    let dataset = "public.measure";
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

fn compile_table(table: &TableSpec, decisions: Vec<Value>) -> Chain {
    let catalog = catalog_json(table);
    let artifact_json = run_bin(
        "OO_A254_OO_BUILDER",
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
        "OO_A254_REVIEWED_EFFECTIVE",
        &[],
        &serde_json::to_string(&command_in).unwrap(),
    );
    assert!(!command_json.ends_with('\n'));
    assert!(!command_json.contains(OWNER));
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

fn allow(column: &str, explicit_allow: bool) -> ReviewedAllow {
    ReviewedAllow {
        dataset: "public.measure".to_string(),
        column: column.to_string(),
        explicit_allow,
    }
}

fn binding(chain: &Chain, allow_list: Vec<ReviewedAllow>) -> ReviewedBinding {
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
        generation: BINDING_GENERATION,
        role: "clerk".to_string(),
        login: "clerk_login".to_string(),
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

fn operator(chain: &Chain, allow_list: Vec<ReviewedAllow>) -> String {
    let bound = binding(chain, allow_list);
    serde_json::to_string(&json!({
        "protocol": PROTOCOL,
        "command": chain.command_json,
        "artifact": chain.artifact_json,
        "decisions": chain.decisions_json,
        "binding": binding_json(&bound)
    }))
    .unwrap()
}

fn run_cli(stdin_text: &str) -> CliOut {
    let output = spawn("OO_A254_REVIEWED_POLICY", &[], stdin_text);
    CliOut {
        code: output.code,
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

fn assert_line(out: &CliOut, code: i32, line: &str) {
    assert_eq!(out.code, code, "exit {}", out.stderr);
    assert_eq!(out.stdout, "");
    assert_eq!(out.stderr, line);
}

fn measure_decisions(
    fingerprints: &BTreeMap<(String, String), String>,
    schema_hash: &str,
) -> Vec<Value> {
    vec![
        decision(fingerprints, schema_hash, "batch_i8", "none", "identifier"),
        decision(fingerprints, schema_hash, "amount_i2", "none", "quantity"),
        decision(fingerprints, schema_hash, "amount_i4", "none", "quantity"),
        decision(
            fingerprints,
            schema_hash,
            "amount_i8",
            "personal",
            "quantity",
        ),
    ]
}

fn measure_chain() -> Chain {
    let table = measure_table();
    let catalog = catalog_json(&table);
    let artifact_json = run_bin(
        "OO_A254_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).unwrap(),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).unwrap();
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let prints = fingerprints_of(&artifact);
    compile_table(&table, measure_decisions(&prints, &schema_hash))
}

fn quantity_allow() -> Vec<ReviewedAllow> {
    vec![
        allow("amount_i2", false),
        allow("amount_i4", false),
        allow("amount_i8", true),
        allow("batch_i8", false),
    ]
}

fn policies_of(stdout: &str) -> Vec<ColumnAclPolicy> {
    assert!(!stdout.is_empty());
    assert!(!stdout.ends_with('\n'));
    assert!(!stdout.contains(OWNER));
    let doc: SetDoc = serde_json::from_str(stdout).expect("policy set");
    assert_eq!(doc.protocol, PROTOCOL);
    doc.policies
        .into_iter()
        .map(|item| item.into_policy().expect("column policy"))
        .collect()
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

impl PolicyDoc {
    fn into_policy(self) -> Result<ColumnAclPolicy, &'static str> {
        let row_filter = match self.row_filter {
            None => None,
            Some(filter) => Some(filter.into_filter()?),
        };
        let mut columns = Vec::with_capacity(self.table.columns.len());
        for column in self.table.columns {
            columns.push(column.into_spec()?);
        }
        Ok(ColumnAclPolicy {
            project: self.project,
            config_digest: self.config_digest,
            generation: self.generation,
            role: self.role,
            login: self.login,
            table: LogicalTable {
                namespace: self.table.namespace,
                name: self.table.name,
                columns,
            },
            row_filter,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RowFilterDoc {
    kind: String,
    column: String,
    claim: String,
}

impl RowFilterDoc {
    fn into_filter(self) -> Result<RowFilter, &'static str> {
        if self.kind != "claims" {
            return Err("row filter");
        }
        Ok(RowFilter::Claims {
            column: self.column,
            claim: self.claim,
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
    fn into_spec(self) -> Result<ColumnSpec, &'static str> {
        let status = match self.status.as_str() {
            "confirmed" => ClassStatus::Confirmed,
            "auto_accepted" => ClassStatus::AutoAccepted,
            "unclassified" => ClassStatus::Unclassified,
            _ => return Err("status"),
        };
        Ok(ColumnSpec {
            name: self.name,
            status,
            semantic_type: self.semantic_type,
            pii: self.pii.into_class()?,
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
    fn into_class(self) -> Result<PiiClass, &'static str> {
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

fn single_decision(
    type_name: &'static str,
    oid: i64,
    semantic: &'static str,
    pii: &'static str,
) -> Chain {
    let table = one_column(type_name, oid);
    let catalog = catalog_json(&table);
    let artifact_json = run_bin(
        "OO_A254_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).unwrap(),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).unwrap();
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let prints = fingerprints_of(&artifact);
    let decisions = vec![decision(&prints, &schema_hash, "amount", pii, semantic)];
    compile_table(&table, decisions)
}

#[test]
fn hand_amount_bounds_are_the_integer_extremes() {
    assert_eq!(AMOUNT_I2[0].parse::<i16>().unwrap(), i16::MIN);
    assert_eq!(AMOUNT_I2[1].parse::<i16>().unwrap(), i16::MAX);
    assert_eq!(AMOUNT_I2[2].parse::<i16>().unwrap(), 0);
    assert_eq!(AMOUNT_I4[0].parse::<i32>().unwrap(), i32::MIN);
    assert_eq!(AMOUNT_I4[1].parse::<i32>().unwrap(), i32::MAX);
    assert_eq!(AMOUNT_I4[2].parse::<i32>().unwrap(), 0);
    assert_eq!(AMOUNT_I8[0].parse::<i64>().unwrap(), i64::MIN);
    assert_eq!(AMOUNT_I8[1].parse::<i64>().unwrap(), i64::MAX);
    assert_eq!(AMOUNT_I8[2].parse::<i64>().unwrap(), 0);
    assert_eq!(AMOUNT_COLUMNS.len(), 3);
}

#[test]
fn integer_quantity_reaches_the_reviewed_policy_cli() {
    let chain = measure_chain();
    let command: Value = serde_json::from_str(&chain.command_json).unwrap();
    for name in AMOUNT_COLUMNS {
        let column = command["effective_oos"]["datasets"][0]["columns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["name"] == name)
            .unwrap_or_else(|| panic!("producer omitted {name}"));
        assert_eq!(column["semantic_type"], "quantity", "{name}");
    }
    let text = operator(&chain, quantity_allow());
    let out = run_cli(&text);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stderr, "");
    let again = run_cli(&text);
    assert_eq!(again.stdout, out.stdout);
    let policies = policies_of(&out.stdout);
    let compiled = compile_column_acl_set(&policies).expect("existing set compiler");
    assert_eq!(compiled.len(), 1);
    assert_eq!(policies.len(), 1);
    let policy = &policies[0];
    assert_eq!(policy.generation, BINDING_GENERATION);
    assert_ne!(policy.generation, 48);
    assert_ne!(policy.generation, 1);
    assert_eq!(policy.project, "shop");
    assert_eq!(policy.config_digest, DIGEST);
    assert_eq!(policy.role, "clerk");
    assert_eq!(policy.login, "clerk_login");
    assert!(policy.row_filter.is_none());
    assert_eq!(policy.table.namespace, "public");
    assert_eq!(policy.table.name, "measure");
    let columns = &policy.table.columns;
    assert_eq!(columns.len(), 4);
    let expect = [
        ("amount_i2", "quantity", PiiClass::None),
        ("amount_i4", "quantity", PiiClass::None),
        (
            "amount_i8",
            "quantity",
            PiiClass::Declared {
                explicit_allow: true,
            },
        ),
        ("batch_i8", "identifier", PiiClass::None),
    ];
    for (column, (name, semantic, pii)) in columns.iter().zip(expect) {
        assert_eq!(column.name, name);
        assert_eq!(column.status, ClassStatus::Confirmed);
        assert_eq!(column.semantic_type, semantic);
        assert_eq!(column.pii, pii);
    }
    for name in AMOUNT_COLUMNS {
        assert!(out.stdout.contains(name), "dto omitted {name}");
    }
    assert!(out.stdout.contains("\"semantic_type\":\"quantity\""));
}

#[test]
fn quantity_pii_token_must_match_the_current_allow_rule() {
    let chain = measure_chain();
    let missing = run_cli(&operator(&chain, vec![allow("amount_i8", false)]));
    assert_line(&missing, 12, "reviewed-policy: pii_allow\n");
    let extra = run_cli(&operator(&chain, vec![allow("amount_i2", true)]));
    assert_line(&extra, 12, "reviewed-policy: pii_allow\n");
    let declared = run_cli(&operator(&chain, vec![allow("amount_i8", true)]));
    assert_eq!(declared.code, 0, "{}", declared.stderr);
    let policies = policies_of(&declared.stdout);
    assert_eq!(
        policies[0].table.columns[0].pii,
        PiiClass::Declared {
            explicit_allow: true
        }
    );
    assert_eq!(policies[0].table.columns[0].semantic_type, "quantity");
}

#[test]
fn wrong_physical_quantity_is_semantic() {
    let cases = [
        ("float4", 700_i64),
        ("float8", 701),
        ("numeric", 1700),
        ("date", 1082),
        ("bool", 16),
        ("text", 25),
    ];
    for (type_name, oid) in cases {
        let chain = single_decision(type_name, oid, "quantity", "none");
        let out = run_cli(&operator(&chain, vec![allow("amount", false)]));
        assert_line(&out, 11, "reviewed-policy: semantic\n");
        assert!(!out.stderr.contains("amount"));
    }
}

#[test]
fn missing_and_stale_quantity_decisions_refuse() {
    let table = measure_table();
    let catalog = catalog_json(&table);
    let artifact_json = run_bin(
        "OO_A254_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).unwrap(),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).unwrap();
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let prints = fingerprints_of(&artifact);
    let mut decisions = measure_decisions(&prints, &schema_hash);
    decisions.retain(|item| item["column"] != "amount_i8");
    let missing_chain = compile_table(&table, decisions);
    let missing = run_cli(&operator(&missing_chain, vec![allow("amount_i8", true)]));
    assert_line(&missing, 10, "reviewed-policy: decision missing\n");

    let mut stale_schema = measure_decisions(&prints, &schema_hash);
    stale_schema
        .iter_mut()
        .find(|item| item["column"] == "amount_i8")
        .unwrap()["schema_hash"] = json!("ef".repeat(32));
    let schema_chain = compile_table(&table, stale_schema);
    let schema = run_cli(&operator(&schema_chain, vec![allow("amount_i8", true)]));
    assert_line(&schema, 10, "reviewed-policy: decision stale_schema\n");

    let mut stale_fact = measure_decisions(&prints, &schema_hash);
    stale_fact
        .iter_mut()
        .find(|item| item["column"] == "amount_i8")
        .unwrap()["fact_fingerprint"] = json!("ab".repeat(32));
    let fact_chain = compile_table(&table, stale_fact);
    let fact = run_cli(&operator(&fact_chain, vec![allow("amount_i8", true)]));
    assert_line(&fact, 10, "reviewed-policy: decision stale_fact\n");
}

#[test]
fn money_needs_a_unit_and_other_semantics_stay_refused() {
    let table = one_column("int8", 20);
    let catalog = catalog_json(&table);
    let artifact_json = run_bin(
        "OO_A254_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).unwrap(),
    );
    let artifact: Value = serde_json::from_str(&artifact_json).unwrap();
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let prints = fingerprints_of(&artifact);
    let money = vec![decision(&prints, &schema_hash, "amount", "none", "money")];
    let command_in = json!({
        "protocol": "oo.builder.reviewed-effective-oos-command/v1",
        "artifact": artifact,
        "settings": settings(),
        "decisions": money,
    });
    let refused = spawn(
        "OO_A254_REVIEWED_EFFECTIVE",
        &[],
        &serde_json::to_string(&command_in).unwrap(),
    );
    assert_ne!(refused.code, 0, "{}", refused.stdout);
    assert!(refused.stderr.contains("oos.invalid"), "{}", refused.stderr);
    assert!(
        refused.stderr.contains("effective OOS validation failed"),
        "{}",
        refused.stderr
    );
    assert!(refused.stdout.is_empty());

    for semantic in [
        "ratio",
        "event_time",
        "date",
        "audit_time",
        "geo",
        "json",
        "system",
    ] {
        let chain = single_decision("int8", 20, semantic, "none");
        let out = run_cli(&operator(&chain, vec![allow("amount", false)]));
        assert_line(&out, 11, "reviewed-policy: semantic\n");
    }
}
