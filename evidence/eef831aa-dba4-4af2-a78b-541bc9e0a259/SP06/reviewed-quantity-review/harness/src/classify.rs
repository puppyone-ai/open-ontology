//! Independent producer and reviewed-policy checks.
//! This file does not open a database.

use std::fs;

use serde_json::{json, Value};

use crate::catalog::{
    self, column, decision, fingerprints_of, parse_policies, run_policy, Col, Identity, TableSpec,
    AMOUNT_I2, AMOUNT_I4, AMOUNT_I8,
};
use crate::proc::{self, Stop};

const DIGEST: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
const GENERATION: i64 = 13;

fn identity() -> Identity {
    Identity {
        project: "qtylot",
        source_id: "qty_src",
        database: "qtydb",
        digest: DIGEST.to_string(),
        generation: GENERATION,
        role: "clerk",
        login: "lot_login",
    }
}

fn measure_table() -> TableSpec {
    TableSpec {
        schema: "public",
        name: "lot",
        columns: vec![
            Col { attnum: 1, name: "batch_i8", oid: 20, type_name: "int8" },
            Col { attnum: 2, name: "amount_i2", oid: 21, type_name: "int2" },
            Col { attnum: 3, name: "amount_i4", oid: 23, type_name: "int4" },
            Col { attnum: 4, name: "amount_i8", oid: 20, type_name: "int8" },
        ],
        pk: vec![(1, "batch_i8")],
    }
}

fn one_column(type_name: &'static str, oid: i64) -> TableSpec {
    TableSpec {
        schema: "public",
        name: "lot",
        columns: vec![Col { attnum: 1, name: "amount", oid, type_name }],
        pk: vec![(1, "amount")],
    }
}

fn dataset(table: &TableSpec) -> String {
    format!("{}.{}", table.schema, table.name)
}

fn measure_decisions(
    prints: &std::collections::BTreeMap<(String, String), String>,
    schema_hash: &str,
    dataset: &str,
) -> Result<Vec<Value>, Stop> {
    Ok(vec![
        decision(prints, dataset, schema_hash, "batch_i8", "none", "identifier")?,
        decision(prints, dataset, schema_hash, "amount_i2", "none", "quantity")?,
        decision(prints, dataset, schema_hash, "amount_i4", "none", "quantity")?,
        decision(prints, dataset, schema_hash, "amount_i8", "personal", "quantity")?,
    ])
}

fn assert_line(output: &proc::Spawn, code: i32, line: &str) -> Result<(), Stop> {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.code != code || !output.stdout.is_empty() || stderr != line {
        return Err(Stop::new(
            "reviewed-policy",
            format!("exit {} stderr {stderr}", output.code),
        ));
    }
    if stderr.contains("amount") {
        return Err(Stop::new("reviewed-policy", "stderr named a column"));
    }
    Ok(())
}

fn extremes() -> Result<(), Stop> {
    if AMOUNT_I2[0].parse::<i16>().ok() != Some(i16::MIN)
        || AMOUNT_I2[1].parse::<i16>().ok() != Some(i16::MAX)
        || AMOUNT_I2[2].parse::<i16>().ok() != Some(0)
        || AMOUNT_I4[0].parse::<i32>().ok() != Some(i32::MIN)
        || AMOUNT_I4[1].parse::<i32>().ok() != Some(i32::MAX)
        || AMOUNT_I4[2].parse::<i32>().ok() != Some(0)
        || AMOUNT_I8[0].parse::<i64>().ok() != Some(i64::MIN)
        || AMOUNT_I8[1].parse::<i64>().ok() != Some(i64::MAX)
        || AMOUNT_I8[2].parse::<i64>().ok() != Some(0)
    {
        return Err(Stop::new("extremes", "hand decimals are not the integer bounds"));
    }
    Ok(())
}

pub fn run() -> Result<(), Stop> {
    extremes()?;
    let root = proc::evidence()?;
    let dir = root.join("classify");
    fs::create_dir_all(&dir).map_err(|_| Stop::new("classify", "mkdir"))?;
    let id = identity();
    let table = measure_table();
    let data = dataset(&table);
    let catalog = catalog::catalog_json(&table, &id);
    let artifact_json = proc::run_ok(
        "OO_A259_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog).map_err(|_| Stop::new("catalog", "encode"))?,
    )?;
    let artifact: Value = serde_json::from_str(&artifact_json).map_err(|_| Stop::new("captured-pg-draft", "json"))?;
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .ok_or_else(|| Stop::new("captured-pg-draft", "schema hash"))?
        .to_string();
    let prints = fingerprints_of(&artifact)?;
    let chain = catalog::compile_table(&table, &id, measure_decisions(&prints, &schema_hash, &data)?)?;
    let command: Value = serde_json::from_str(&chain.command_json).map_err(|_| Stop::new("reviewed-effective", "json"))?;
    for name in ["amount_i2", "amount_i4", "amount_i8"] {
        let found = command["effective_oos"]["datasets"][0]["columns"]
            .as_array()
            .ok_or_else(|| Stop::new("reviewed-effective", "columns"))?
            .iter()
            .find(|item| item["name"] == name)
            .ok_or_else(|| Stop::new("reviewed-effective", format!("omitted {name}")))?;
        if found["semantic_type"] != "quantity" {
            return Err(Stop::new("reviewed-effective", format!("{name} was reclassified")));
        }
    }
    let allow = [
        ("amount_i2", false),
        ("amount_i4", false),
        ("amount_i8", true),
        ("batch_i8", false),
    ];
    let text = catalog::operator_for(&chain, &id, &data, &allow)?;
    let out = run_policy(&text)?;
    if out.code != 0 || !out.stderr.is_empty() {
        return Err(Stop::new(
            "reviewed-policy",
            format!("exit {} {}", out.code, String::from_utf8_lossy(&out.stderr)),
        ));
    }
    let again = run_policy(&text)?;
    if again.stdout != out.stdout {
        return Err(Stop::new("reviewed-policy", "stdout was not repeatable"));
    }
    let stdout = proc::utf8("reviewed-policy", &out.stdout)?;
    fs::write(dir.join("policy.stdout"), &stdout).map_err(|_| Stop::new("classify", "policy stdout"))?;
    let policies = parse_policies(&stdout)?;
    let compiled = oo_gateway::policy::compile_column_acl_set(&policies)
        .map_err(|_| Stop::new("compile_column_acl_set", "refused the quantity set"))?;
    if compiled.len() != 1 || policies.len() != 1 {
        return Err(Stop::new("reviewed-policy", "policy count"));
    }
    let policy = &policies[0];
    if policy.generation != GENERATION || policy.generation == 48 || policy.generation == 1 {
        return Err(Stop::new("reviewed-policy", "library generation was not the binding"));
    }
    if policy.project != "qtylot"
        || policy.config_digest != DIGEST
        || policy.role != "clerk"
        || policy.login != "lot_login"
        || policy.row_filter.is_some()
        || policy.table.namespace != "public"
        || policy.table.name != "lot"
    {
        return Err(Stop::new("reviewed-policy", "policy identity"));
    }
    expect_column(policy, "amount_i2", "quantity", oo_gateway::PiiClass::None)?;
    expect_column(policy, "amount_i4", "quantity", oo_gateway::PiiClass::None)?;
    expect_column(
        policy,
        "amount_i8",
        "quantity",
        oo_gateway::PiiClass::Declared { explicit_allow: true },
    )?;
    expect_column(policy, "batch_i8", "identifier", oo_gateway::PiiClass::None)?;
    refusals(&id, &table, &prints, &schema_hash, &data)?;
    wrong_physical(&id)?;
    money_and_other(&id)?;
    fs::write(dir.join("classify.txt"), "classify=ok\n").map_err(|_| Stop::new("classify", "note"))?;
    println!("classify=ok");
    Ok(())
}

fn expect_column(
    policy: &oo_gateway::ColumnAclPolicy,
    name: &str,
    semantic: &str,
    pii: oo_gateway::PiiClass,
) -> Result<(), Stop> {
    let spec = column(policy, name)?;
    if spec.status != oo_gateway::ClassStatus::Confirmed || spec.semantic_type != semantic || spec.pii != pii {
        return Err(Stop::new("reviewed-policy", format!("{name} classification")));
    }
    Ok(())
}

fn refusals(
    id: &Identity,
    table: &TableSpec,
    prints: &std::collections::BTreeMap<(String, String), String>,
    schema_hash: &str,
    data: &str,
) -> Result<(), Stop> {
    let mut missing = measure_decisions(prints, schema_hash, data)?;
    missing.retain(|item| item["column"] != "amount_i8");
    let missing_chain = catalog::compile_table(table, id, missing)?;
    let missing_out = run_policy(&catalog::operator_for(
        &missing_chain,
        id,
        data,
        &[("amount_i8", true)],
    )?)?;
    assert_line(&missing_out, 10, "reviewed-policy: decision missing\n")?;

    let mut stale_schema = measure_decisions(prints, schema_hash, data)?;
    stale_schema
        .iter_mut()
        .find(|item| item["column"] == "amount_i8")
        .ok_or_else(|| Stop::new("decision", "amount_i8"))?["schema_hash"] = json!("ef".repeat(32));
    let schema_chain = catalog::compile_table(table, id, stale_schema)?;
    let schema_out = run_policy(&catalog::operator_for(
        &schema_chain,
        id,
        data,
        &[("amount_i8", true)],
    )?)?;
    assert_line(&schema_out, 10, "reviewed-policy: decision stale_schema\n")?;

    let mut stale_fact = measure_decisions(prints, schema_hash, data)?;
    stale_fact
        .iter_mut()
        .find(|item| item["column"] == "amount_i8")
        .ok_or_else(|| Stop::new("decision", "amount_i8"))?["fact_fingerprint"] = json!("ab".repeat(32));
    let fact_chain = catalog::compile_table(table, id, stale_fact)?;
    let fact_out = run_policy(&catalog::operator_for(
        &fact_chain,
        id,
        data,
        &[("amount_i8", true)],
    )?)?;
    assert_line(&fact_out, 10, "reviewed-policy: decision stale_fact\n")?;

    let chain = catalog::compile_table(table, id, measure_decisions(prints, schema_hash, data)?)?;
    let personal = run_policy(&catalog::operator_for(&chain, id, data, &[("amount_i8", false)])?)?;
    assert_line(&personal, 12, "reviewed-policy: pii_allow\n")?;
    let extra = run_policy(&catalog::operator_for(&chain, id, data, &[("amount_i2", true)])?)?;
    assert_line(&extra, 12, "reviewed-policy: pii_allow\n")?;
    Ok(())
}

fn wrong_physical(id: &Identity) -> Result<(), Stop> {
    let cases = [
        ("float4", 700_i64),
        ("float8", 701),
        ("numeric", 1700),
        ("date", 1082),
        ("bool", 16),
        ("text", 25),
    ];
    for (type_name, oid) in cases {
        let table = one_column(type_name, oid);
        let data = dataset(&table);
        let chain = single(id, &table, "quantity", "none")?;
        let out = run_policy(&catalog::operator_for(&chain, id, &data, &[("amount", false)])?)?;
        assert_line(&out, 11, "reviewed-policy: semantic\n")?;
    }
    Ok(())
}

fn single(id: &Identity, table: &TableSpec, semantic: &str, pii: &str) -> Result<catalog::Chain, Stop> {
    let data = dataset(table);
    let catalog_doc = catalog::catalog_json(table, id);
    let artifact_json = proc::run_ok(
        "OO_A259_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog_doc).map_err(|_| Stop::new("catalog", "encode"))?,
    )?;
    let artifact: Value = serde_json::from_str(&artifact_json).map_err(|_| Stop::new("captured-pg-draft", "json"))?;
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .ok_or_else(|| Stop::new("captured-pg-draft", "schema hash"))?
        .to_string();
    let prints = fingerprints_of(&artifact)?;
    let decisions = vec![decision(&prints, &data, &schema_hash, "amount", pii, semantic)?];
    catalog::compile_table(table, id, decisions)
}

fn money_and_other(id: &Identity) -> Result<(), Stop> {
    let table = one_column("int8", 20);
    let data = dataset(&table);
    let catalog_doc = catalog::catalog_json(&table, id);
    let artifact_json = proc::run_ok(
        "OO_A259_OO_BUILDER",
        &["captured-pg-draft"],
        &serde_json::to_string(&catalog_doc).map_err(|_| Stop::new("catalog", "encode"))?,
    )?;
    let artifact: Value = serde_json::from_str(&artifact_json).map_err(|_| Stop::new("captured-pg-draft", "json"))?;
    let schema_hash = artifact["draft"]["provenance"]["schema_hash"]
        .as_str()
        .ok_or_else(|| Stop::new("captured-pg-draft", "schema hash"))?
        .to_string();
    let prints = fingerprints_of(&artifact)?;
    let money = vec![decision(&prints, &data, &schema_hash, "amount", "none", "money")?];
    let command_in = json!({
        "protocol": "oo.builder.reviewed-effective-oos-command/v1",
        "artifact": artifact,
        "settings": catalog::settings(id, table.schema),
        "decisions": money,
    });
    let program = proc::bin("OO_A259_REVIEWED_EFFECTIVE")?;
    let refused = proc::spawn(
        &program,
        &[],
        serde_json::to_string(&command_in)
            .map_err(|_| Stop::new("reviewed-effective", "encode"))?
            .as_bytes(),
    )?;
    let stderr = String::from_utf8_lossy(&refused.stderr);
    if refused.code == 0
        || !refused.stdout.is_empty()
        || !stderr.contains("oos.invalid")
        || !stderr.contains("effective OOS validation failed")
    {
        return Err(Stop::new("money", format!("exit {} {stderr}", refused.code)));
    }
    for semantic in ["ratio", "event_time", "date", "audit_time", "geo", "json", "system"] {
        let chain = single(id, &table, semantic, "none")?;
        let out = run_policy(&catalog::operator_for(&chain, id, &data, &[("amount", false)])?)?;
        assert_line(&out, 11, "reviewed-policy: semantic\n")?;
    }
    Ok(())
}
