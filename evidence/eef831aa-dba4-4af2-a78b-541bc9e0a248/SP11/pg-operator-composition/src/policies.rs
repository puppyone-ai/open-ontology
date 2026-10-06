//! `reviewed-policy` for two roles. The policy array is the compiler DTO.
//!
//! Install keeps that array, including `"row_filter": null`. Query drops the
//! key when the value is null. This file does not invent a claims filter.

use std::fs;

use oo_gateway::{ClassStatus, ColumnAclPolicy, ColumnSpec, LogicalTable, PiiClass};
use serde_json::{json, Value};

use crate::bridge::find_column;
use crate::docs::Docs;
use crate::fixture;
use crate::live::{Live, LOGIN, SCHEMA, SOURCE_ID};
use crate::pg::Imported;
use crate::proc::{parse_json, require_exit, run_cmd, save_cmd};
use crate::stop::{disk, Stop};

const POLICY_PREFIX: &[u8] = b"{\"protocol\":\"oo.gateway.reviewed-policy-cli/v1\",\"policies\":";

pub struct Emitted {
    pub clerk_install: Vec<u8>,
    pub peek_install: Vec<u8>,
    pub clerk_query: Vec<Value>,
    pub peek_query: Vec<Value>,
    pub clerk_acl: Vec<ColumnAclPolicy>,
    pub peek_acl: Vec<ColumnAclPolicy>,
}

struct RoleSpec<'a> {
    slug: &'a str,
    customer: &'a [&'a str],
    order: &'a [&'a str],
}

pub fn emit(live: &Live, docs: &Docs, imported: &Imported) -> Result<Emitted, Stop> {
    let artifact = parse_json(&docs.artifact_stdout, "review-pg-draft")?;
    if artifact["evidence_envelope"]["source"]["source_id"] != SOURCE_ID {
        return Err(Stop::new("reviewed-policy", "source_id"));
    }
    if artifact["draft"]["provenance"]["schema_hash"] != docs.schema_hash {
        return Err(Stop::new("reviewed-policy", "schema_hash"));
    }
    let clerk = one_role(
        live,
        docs,
        imported,
        RoleSpec {
            slug: "clerk",
            customer: &fixture::CLERK_CUSTOMER,
            order: &fixture::CLERK_ORDER,
        },
    )?;
    let peek = one_role(
        live,
        docs,
        imported,
        RoleSpec {
            slug: "peek",
            customer: &fixture::PEEK_CUSTOMER,
            order: &fixture::PEEK_ORDER,
        },
    )?;
    write_seam(live)?;
    Ok(Emitted {
        clerk_install: clerk.install,
        peek_install: peek.install,
        clerk_query: clerk.query,
        peek_query: peek.query,
        clerk_acl: clerk.acl,
        peek_acl: peek.acl,
    })
}

struct RoleOut {
    install: Vec<u8>,
    query: Vec<Value>,
    acl: Vec<ColumnAclPolicy>,
}

fn one_role(live: &Live, docs: &Docs, imported: &Imported, role: RoleSpec<'_>) -> Result<RoleOut, Stop> {
    let step = format!("reviewed-policy-{}", role.slug);
    let request = operator_request(docs, imported, &role)?;
    let bytes = serde_json::to_vec(&request).map_err(|_| Stop::new(&step, "encode"))?;
    reject_password(live, &bytes, &step)?;
    fs::write(live.evidence.join(format!("{step}.request.json")), &bytes).map_err(|err| disk(&step, &err))?;
    let output = run_cmd(&live.reviewed_policy, &[], &bytes, 60)?;
    save_cmd(live, &step, &output)?;
    require_exit(&step, &output, 0)?;
    if !output.stderr.is_empty() || output.stdout.ends_with(b"\n") {
        return Err(Stop::new(&step, "stdout or stderr shape"));
    }
    let array = policies_bytes(&step, &output.stdout)?;
    if count_slice(array, b"\"row_filter\":null") != 2 || count_slice(array, b"\"kind\":\"claims\"") != 0 {
        return Err(Stop::new(&step, "row_filter was not null"));
    }
    let parsed: Vec<Value> = serde_json::from_slice(array).map_err(|_| Stop::new(&step, "policies"))?;
    if parsed.len() != 2 {
        return Err(Stop::new(&step, "table count"));
    }
    check_policy(&parsed[0], imported, role.slug, "customer", role.customer, &docs.effective_oos)?;
    check_policy(&parsed[1], imported, role.slug, "order_doc", role.order, &docs.effective_oos)?;
    let mut query = Vec::with_capacity(parsed.len());
    let mut acl = Vec::with_capacity(parsed.len());
    for policy in &parsed {
        query.push(omit_null_filter(policy)?);
        acl.push(to_acl(policy)?);
    }
    Ok(RoleOut {
        install: array.to_vec(),
        query,
        acl,
    })
}

fn operator_request(docs: &Docs, imported: &Imported, role: &RoleSpec<'_>) -> Result<Value, Stop> {
    let command = std::str::from_utf8(&docs.command_stdout).map_err(|_| Stop::new("reviewed-policy", "command utf8"))?;
    let artifact = std::str::from_utf8(&docs.artifact_stdout).map_err(|_| Stop::new("reviewed-policy", "artifact utf8"))?;
    let decisions = std::str::from_utf8(&docs.decisions_json).map_err(|_| Stop::new("reviewed-policy", "decisions utf8"))?;
    if command.is_empty() || artifact.is_empty() || decisions.is_empty() {
        return Err(Stop::new("reviewed-policy", "empty producer bytes"));
    }
    Ok(json!({
        "protocol": "oo.gateway.reviewed-policy-cli/v1",
        "command": command,
        "artifact": artifact,
        "decisions": decisions,
        "binding": {
            "identity": identity(&docs.effective_oos)?,
            "seal": {
                "source_id": SOURCE_ID,
                "schema": SCHEMA,
                "schema_hash": docs.schema_hash,
                "artifact_sha256": docs.sealed_source_sha256
            },
            "config_digest": imported.digest,
            "generation": imported.generation,
            "role": role.slug,
            "login": LOGIN,
            "allow": allow_list(&docs.effective_oos, role.customer, role.order)?
        }
    }))
}

fn identity(effective: &Value) -> Result<Value, Stop> {
    Ok(json!({
        "project": text(effective, "project")?,
        "ontology_version": text(effective, "ontology_version")?,
        "catalog_version": text(effective, "catalog_version")?,
        "policy_id": text(&effective["policy"], "id")?,
        "policy_version": text(&effective["policy"], "version")?,
        "knowledge_id": text(&effective["knowledge"], "id")?,
        "knowledge_version": text(&effective["knowledge"], "version")?
    }))
}

fn allow_list(effective: &Value, customer: &[&str], order: &[&str]) -> Result<Vec<Value>, Stop> {
    let mut allow = Vec::with_capacity(customer.len() + order.len());
    for column in customer {
        allow.push(allow_entry(effective, "shop.customer", column)?);
    }
    for column in order {
        allow.push(allow_entry(effective, "shop.order_doc", column)?);
    }
    Ok(allow)
}

fn allow_entry(effective: &Value, dataset: &str, column: &str) -> Result<Value, Stop> {
    let found = find_column(effective, dataset, column)?;
    let pii = found["pii"].as_str().ok_or_else(|| Stop::new("reviewed-policy", "binding pii"))?;
    let explicit = match pii {
        "none" => false,
        "personal" | "sensitive" | "restricted" => true,
        _ => return Err(Stop::new("reviewed-policy", "binding pii token")),
    };
    Ok(json!({
        "dataset": dataset,
        "column": column,
        "explicit_allow": explicit
    }))
}

fn policies_bytes<'a>(step: &str, stdout: &'a [u8]) -> Result<&'a [u8], Stop> {
    if !stdout.starts_with(POLICY_PREFIX) || !stdout.ends_with(b"}") {
        return Err(Stop::new(step, "stdout shape"));
    }
    let array = &stdout[POLICY_PREFIX.len()..stdout.len() - 1];
    if !array.starts_with(b"[") || !array.ends_with(b"]") {
        return Err(Stop::new(step, "policies array"));
    }
    Ok(array)
}

fn check_policy(
    policy: &Value,
    imported: &Imported,
    role: &str,
    table: &str,
    columns: &[&str],
    effective: &Value,
) -> Result<(), Stop> {
    if policy["project"] != SOURCE_ID
        || policy["config_digest"] != imported.digest
        || policy["generation"] != imported.generation
        || policy["role"] != role
        || policy["login"] != LOGIN
        || policy.get("row_filter") != Some(&Value::Null)
    {
        return Err(Stop::new("reviewed-policy", "policy identity"));
    }
    let body = &policy["table"];
    if body["namespace"] != SCHEMA || body["name"] != table {
        return Err(Stop::new("reviewed-policy", "table"));
    }
    let got = body["columns"].as_array().ok_or_else(|| Stop::new("reviewed-policy", "columns"))?;
    if got.len() != columns.len() {
        return Err(Stop::new("reviewed-policy", "column width"));
    }
    for (column, name) in got.iter().zip(columns.iter()) {
        if column["name"] != *name || column["status"] != "confirmed" {
            return Err(Stop::new("reviewed-policy", "column"));
        }
        let dataset = format!("{SCHEMA}.{table}");
        let source = find_column(effective, &dataset, name)?;
        if column["semantic_type"] != source["semantic_type"] {
            return Err(Stop::new("reviewed-policy", "semantic_type"));
        }
        expect_pii(&column["pii"], source["pii"].as_str().unwrap_or(""))?;
    }
    Ok(())
}

fn expect_pii(pii: &Value, token: &str) -> Result<(), Stop> {
    match token {
        "none" => {
            if pii["class"] == "none" && pii.get("explicit_allow").is_none() {
                Ok(())
            } else {
                Err(Stop::new("reviewed-policy", "none pii"))
            }
        }
        "personal" | "sensitive" | "restricted" => {
            if pii["class"] == "declared" && pii["explicit_allow"] == Value::Bool(true) {
                Ok(())
            } else {
                Err(Stop::new("reviewed-policy", "declared pii"))
            }
        }
        _ => Err(Stop::new("reviewed-policy", "pii token")),
    }
}

fn omit_null_filter(policy: &Value) -> Result<Value, Stop> {
    let mut copy = policy.clone();
    let object = copy.as_object_mut().ok_or_else(|| Stop::new("query-pg", "policy"))?;
    match object.remove("row_filter") {
        Some(Value::Null) => {}
        _ => return Err(Stop::new("query-pg", "row_filter was not JSON null")),
    }
    if has_null(&copy) {
        return Err(Stop::new("query-pg", "JSON null remains after the key is omitted"));
    }
    Ok(copy)
}

fn has_null(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.iter().any(has_null),
        Value::Object(map) => map.values().any(has_null),
        Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

fn to_acl(policy: &Value) -> Result<ColumnAclPolicy, Stop> {
    if policy.get("row_filter") != Some(&Value::Null) {
        return Err(Stop::new("reviewed-policy", "row_filter"));
    }
    let table = &policy["table"];
    let columns = table["columns"].as_array().ok_or_else(|| Stop::new("reviewed-policy", "columns"))?;
    let mut specs = Vec::with_capacity(columns.len());
    for column in columns {
        specs.push(column_spec(column)?);
    }
    Ok(ColumnAclPolicy {
        project: text(policy, "project")?,
        config_digest: text(policy, "config_digest")?,
        generation: policy["generation"].as_i64().ok_or_else(|| Stop::new("reviewed-policy", "generation"))?,
        role: text(policy, "role")?,
        login: text(policy, "login")?,
        table: LogicalTable {
            namespace: text(table, "namespace")?,
            name: text(table, "name")?,
            columns: specs,
        },
        row_filter: None,
    })
}

fn column_spec(column: &Value) -> Result<ColumnSpec, Stop> {
    let status = match column["status"].as_str() {
        Some("confirmed") => ClassStatus::Confirmed,
        Some("auto_accepted") => ClassStatus::AutoAccepted,
        Some("unclassified") => ClassStatus::Unclassified,
        _ => return Err(Stop::new("reviewed-policy", "status")),
    };
    Ok(ColumnSpec {
        name: text(column, "name")?,
        status,
        semantic_type: text(column, "semantic_type")?,
        pii: pii_class(&column["pii"])?,
    })
}

fn pii_class(pii: &Value) -> Result<PiiClass, Stop> {
    match (pii["class"].as_str(), pii.get("explicit_allow")) {
        (Some("none"), None) => Ok(PiiClass::None),
        (Some("unknown"), None) => Ok(PiiClass::Unknown),
        (Some("declared"), Some(Value::Bool(true))) => Ok(PiiClass::Declared { explicit_allow: true }),
        (Some("declared"), Some(Value::Bool(false))) => Ok(PiiClass::Declared { explicit_allow: false }),
        _ => Err(Stop::new("reviewed-policy", "pii dto")),
    }
}

fn text(value: &Value, key: &str) -> Result<String, Stop> {
    value[key]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Stop::new("reviewed-policy", key))
}

fn count_slice(haystack: &[u8], needle: &[u8]) -> usize {
    haystack.windows(needle.len()).filter(|window| *window == needle).count()
}

fn reject_password(live: &Live, bytes: &[u8], step: &str) -> Result<(), Stop> {
    for secret in live.passwords() {
        if !secret.is_empty() && bytes.windows(secret.len()).any(|window| window == secret.as_bytes()) {
            return Err(Stop::new(step, "document contains a password"));
        }
    }
    Ok(())
}

fn write_seam(live: &Live) -> Result<(), Stop> {
    fs::write(
        live.evidence.join("row-filter-seam.txt"),
        "reviewed_policy_row_filter=null\ninstall_column_acl_row_filter=null_preserved\nquery_pg_row_filter=key_omitted\nreason=query-pg reject_null refuses JSON null; PolicyDoc uses serde default so an omitted key is None\nnot_a_product_change=true\n",
    )
    .map_err(|err| disk("row-filter", &err))
}
