//! Live read-only catalog capture into `compile_captured_pg_catalog`.
//!
//! The comment fixture is compared by hash and length. Its text is never written
//! to an output file.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use oo_builder::{compile_captured_pg_catalog, compile_structure_draft, CAPTURED_PROTOCOL};
use oo_connectors::{
    PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef,
    SelectedTable, SourceId, TableSelection,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const COMMENT_BODY: &str = "a156-live-comment-body-do-not-carry";
const OWNER: &str = "a156_owner";
const CUSTOMER: &str = "shop.customer";
const PURCHASE: &str = "shop.purchase";
const EMAIL_ITEM: &str = "column/shop.customer/email";
const HISTORICAL_COMMIT: &str = "b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74";

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => {
            println!("PROBE_OK");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("probe_error={err}");
            ExitCode::from(1)
        }
    }
}

async fn run() -> Result<(), String> {
    let phase = env::args().nth(1).ok_or("usage: capture|stale")?;
    let out = PathBuf::from(env::var("OUT_DIR").map_err(|_| "OUT_DIR is unset")?);
    fs::create_dir_all(&out).map_err(|err| err.to_string())?;
    let catalog = fetch_catalog().await?;
    reject_comment("catalog", &catalog)?;
    match phase.as_str() {
        "capture" => capture(&out, &catalog),
        "stale" => stale(&out, &catalog),
        other => Err(format!("unknown phase {other}")),
    }
}

async fn fetch_catalog() -> Result<String, String> {
    let host = env::var("PGHOST").map_err(|_| "PGHOST is unset")?;
    let port: u16 = env::var("PGPORT")
        .map_err(|_| "PGPORT is unset")?
        .parse()
        .map_err(|_| "PGPORT is not a port")?;
    let database = env::var("PGDATABASE").map_err(|_| "PGDATABASE is unset")?;
    let user = env::var("PGUSER").map_err(|_| "PGUSER is unset")?;
    let password = fs::read_to_string(env::var("PASSWORD_FILE").map_err(|_| "PASSWORD_FILE")?)
        .map_err(|err| format!("password file: {err}"))?;
    let password = password.trim_end_matches(['\r', '\n']).to_string();
    let ca = fs::read(env::var("CA_FILE").map_err(|_| "CA_FILE")?)
        .map_err(|err| format!("ca file: {err}"))?;
    let config = PgSourceConfig::try_new(
        SourceId::new("shop_src").map_err(|err| err.to_string())?,
        PgEndpoint::new(host, port, database).map_err(|err| err.to_string())?,
        PgCredentials::new(user, password).map_err(|err| err.to_string())?,
        SecretRef::new("secret://a156/reader").map_err(|err| err.to_string())?,
        PgTlsConfig::from_ca_pem(ca).map_err(|err| err.to_string())?,
        PgBudget::new(
            Duration::from_secs(30),
            Duration::from_secs(60),
            Duration::from_secs(15),
            1_000,
            1_048_576,
        )
        .map_err(|err| err.to_string())?,
        TableSelection::new(vec![
            SelectedTable::new("shop", "customer").map_err(|err| err.to_string())?,
            SelectedTable::new("shop", "purchase").map_err(|err| err.to_string())?,
        ])
        .map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    let connector = PgConnector::new(config);
    let mut session = connector
        .connect()
        .await
        .map_err(|err| format!("connect: {err}"))?;
    let catalog = session
        .catalog()
        .await
        .map_err(|err| format!("catalog: {err}"))?;
    serde_json::to_string(&catalog).map_err(|err| err.to_string())
}

fn capture(out: &PathBuf, catalog_json: &str) -> Result<(), String> {
    let catalog: Value = serde_json::from_str(catalog_json).map_err(|err| err.to_string())?;
    assert_catalog_shape(&catalog, &["tenant_id", "id", "email"])?;
    let compiled = compile(catalog_json, None, None)?;
    let repeated = compile(catalog_json, None, Some("[]"))?;
    if repeated.evidence_envelope != compiled.evidence_envelope
        || repeated.draft != compiled.draft
        || repeated.wrapper != compiled.wrapper
    {
        return Err("empty reviews changed the sealed bytes".to_string());
    }
    let round = compile_structure_draft(&compiled.evidence_envelope)
        .map_err(|err| format!("envelope round trip {} {}", err.code, err.path))?;
    if round != compiled.draft {
        return Err("evidence envelope does not compile to the sealed draft".to_string());
    }
    let draft: Value = serde_json::from_str(&compiled.draft).map_err(|err| err.to_string())?;
    let wrapper: Value = serde_json::from_str(&compiled.wrapper).map_err(|err| err.to_string())?;
    assert_structure(&draft, &wrapper, &compiled.draft, &compiled.wrapper, &compiled.evidence_envelope)?;
    let reversed = reorder(catalog_json)?;
    let again = compile(&reversed, None, None)?;
    if again.draft != compiled.draft
        || again.evidence_envelope != compiled.evidence_envelope
        || again.wrapper != compiled.wrapper
        || again.captured_catalog_hash != compiled.captured_catalog_hash
    {
        return Err("equivalent catalog reorder changed the sealed bytes".to_string());
    }
    let email = column_of(&draft, "effective", CUSTOMER, "email")?;
    if !email["pii"].is_null() || !email["semantic_type"].is_null() {
        return Err("email gained a semantic or pii classification".to_string());
    }
    if email["data_type"] != "text" {
        return Err("email data_type is not text".to_string());
    }
    let fingerprint = queue_item(&draft, EMAIL_ITEM, "structural_column")?["fact_fingerprint"]
        .as_str()
        .ok_or("email fact fingerprint missing")?
        .to_string();
    let comment = table(&catalog, "customer")?["columns"]
        .as_array()
        .ok_or("columns")?
        .iter()
        .find(|item| item["name"] == "email")
        .ok_or("email column")?["comment"]
        .clone();
    let oracle = json!({
        "phase": "capture",
        "protocol": compiled.protocol,
        "source_id": compiled.source_id,
        "database": compiled.database,
        "schema_hash": compiled.schema_hash,
        "draft_hash": compiled.draft_hash,
        "wrapper_hash": compiled.wrapper_hash,
        "evidence_envelope_hash": compiled.evidence_envelope_hash,
        "captured_catalog_hash": compiled.captured_catalog_hash,
        "reorder_stable": true,
        "empty_reviews_stable": true,
        "comment_byte_len": comment["byte_len"],
        "comment_sha256": comment["sha256"],
        "comment_body_absent": true,
        "owner_absent_from_draft": true,
        "live_catalog_review": wrapper["live_catalog_review"],
        "historical_integration": wrapper["historical"]["pg_catalog_integration"],
        "customer_columns": ["tenant_id", "id", "email"],
        "purchase_columns": ["tenant_id", "id", "customer_id"],
        "primary_key": ["tenant_id", "id"],
        "fk": "purchase_customer_fk",
        "cardinality": "many_to_one",
    });
    fs::write(out.join("capture-oracle.json"), oracle.to_string() + "\n").map_err(|err| err.to_string())?;
    fs::write(
        out.join("first-facts.json"),
        json!({
            "schema_hash": compiled.schema_hash,
            "fact_fingerprint": fingerprint,
        })
        .to_string()
            + "\n",
    )
    .map_err(|err| err.to_string())?;
    write_clean(&out.join("catalog-before.json"), catalog_json)?;
    write_clean(&out.join("draft-before.json"), &compiled.draft)?;
    write_clean(&out.join("wrapper-before.json"), &compiled.wrapper)?;
    Ok(())
}

fn stale(out: &PathBuf, catalog_json: &str) -> Result<(), String> {
    let facts: Value = serde_json::from_str(
        &fs::read_to_string(out.join("first-facts.json")).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    let prior_hash = facts["schema_hash"]
        .as_str()
        .ok_or("prior schema hash missing")?
        .to_string();
    let fingerprint = facts["fact_fingerprint"]
        .as_str()
        .ok_or("prior fingerprint missing")?
        .to_string();
    let catalog: Value = serde_json::from_str(catalog_json).map_err(|err| err.to_string())?;
    assert_catalog_shape(&catalog, &["tenant_id", "id", "email", "note"])?;
    let curated = r#"{"datasets":[{"dataset":"shop.customer","columns":[{"name":"email","label":"Email address","pii":"email"}]}],"relationships":[]}"#;
    let reviews = json!([{
        "item_id": EMAIL_ITEM,
        "action": "override",
        "schema_hash": prior_hash,
        "fact_fingerprint": fingerprint,
    }])
    .to_string();
    let compiled = compile(catalog_json, Some(curated), Some(&reviews))?;
    if compiled.schema_hash == prior_hash {
        return Err("schema change did not change schema_hash".to_string());
    }
    let draft: Value = serde_json::from_str(&compiled.draft).map_err(|err| err.to_string())?;
    let wrapper: Value = serde_json::from_str(&compiled.wrapper).map_err(|err| err.to_string())?;
    assert_closed(&draft, &wrapper, &compiled.draft, &compiled.evidence_envelope)?;
    let effective = column_of(&draft, "effective", CUSTOMER, "email")?;
    if effective["label"] != "email" {
        return Err("stale decision changed the effective label".to_string());
    }
    if !effective["pii"].is_null() {
        return Err("stale curated pii was applied".to_string());
    }
    let curated_email = column_of(&draft, "curated", CUSTOMER, "email")?;
    if curated_email["label"] != "Email address" {
        return Err("curated label was not preserved".to_string());
    }
    let stale_item = queue_item(&draft, EMAIL_ITEM, "stale_decision")?;
    if stale_item["detail"] != "stale_schema_hash" || stale_item["state"] != "pending" {
        return Err("prior decision was not pending with stale_schema_hash".to_string());
    }
    if queue_item(&draft, EMAIL_ITEM, "curated_unbound")?["state"] != "pending" {
        return Err("curated echo is not unbound".to_string());
    }
    if queue_item(&draft, "pii/shop.customer/email", "semantic_field_not_applied")?["detail"] != "pii"
    {
        return Err("curated pii was applied".to_string());
    }
    let names = column_names(&draft, "generated", CUSTOMER)?;
    if names != ["tenant_id", "id", "email", "note"] {
        return Err(format!("customer columns after alter: {names:?}"));
    }
    if dataset(&draft, "generated", CUSTOMER)?["primary_key"] != json!(["tenant_id", "id"]) {
        return Err("composite primary key changed".to_string());
    }
    let oracle = json!({
        "phase": "stale",
        "prior_schema_hash": prior_hash,
        "schema_hash": compiled.schema_hash,
        "schema_hash_changed": true,
        "stale_detail": "stale_schema_hash",
        "stale_state": "pending",
        "curated_label": "Email address",
        "effective_label": "email",
        "live_catalog_review": wrapper["live_catalog_review"],
        "historical_integration": draft["provenance"]["pg_catalog_integration"],
        "customer_columns": names,
        "comment_body_absent": true,
    });
    fs::write(out.join("stale-oracle.json"), oracle.to_string() + "\n").map_err(|err| err.to_string())?;
    write_clean(&out.join("catalog-after.json"), catalog_json)?;
    write_clean(&out.join("draft-after.json"), &compiled.draft)?;
    write_clean(&out.join("wrapper-after.json"), &compiled.wrapper)?;
    Ok(())
}

fn compile(
    catalog_json: &str,
    curated: Option<&str>,
    reviews: Option<&str>,
) -> Result<oo_builder::CapturedPgDraft, String> {
    compile_captured_pg_catalog(catalog_json, curated, reviews)
        .map_err(|err| format!("compile {} {}", err.code, err.path))
}

fn assert_catalog_shape(catalog: &Value, customer_columns: &[&str]) -> Result<(), String> {
    if catalog["protocol"] != "oo.source.pgsql/v1" {
        return Err("catalog protocol is not oo.source.pgsql/v1".to_string());
    }
    if catalog["source_id"] != "shop_src" || catalog["database"] != "oo_a156" {
        return Err("catalog source or database is unexpected".to_string());
    }
    if catalog["type_conversion"] != "unsupported" {
        return Err("catalog claims type conversion".to_string());
    }
    let customer = table(catalog, "customer")?;
    let purchase = table(catalog, "purchase")?;
    if customer["owner"] != OWNER || purchase["owner"] != OWNER {
        return Err("table owner is not the fixture role".to_string());
    }
    let customer_names = names_of(&customer["columns"])?;
    let purchase_names = names_of(&purchase["columns"])?;
    if customer_names != customer_columns {
        return Err(format!("catalog customer columns: {customer_names:?}"));
    }
    if purchase_names != ["tenant_id", "id", "customer_id"] {
        return Err(format!("catalog purchase columns: {purchase_names:?}"));
    }
    if customer["primary_key"] != json!([{"attnum": 1, "name": "tenant_id"}, {"attnum": 2, "name": "id"}])
    {
        return Err("catalog customer primary key is not the composite fixture".to_string());
    }
    let fk = purchase["foreign_keys"]
        .as_array()
        .ok_or("foreign keys")?
        .iter()
        .find(|item| item["name"] == "purchase_customer_fk")
        .ok_or("purchase_customer_fk missing")?;
    if fk["columns"]
        != json!([
            {"attnum": 1, "name": "tenant_id", "ref_schema": "shop", "ref_table": "customer", "ref_attnum": 1, "ref_name": "tenant_id"},
            {"attnum": 3, "name": "customer_id", "ref_schema": "shop", "ref_table": "customer", "ref_attnum": 2, "ref_name": "id"}
        ])
    {
        return Err("catalog foreign key pairs differ from the fixture".to_string());
    }
    let email = customer["columns"]
        .as_array()
        .ok_or("columns")?
        .iter()
        .find(|item| item["name"] == "email")
        .ok_or("email")?;
    let tenant = customer["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "tenant_id")
        .ok_or("tenant_id")?;
    let email_collation = email["collation_oid"].as_i64().ok_or("email collation")?;
    if email_collation == 0 || !email["collation_name"].is_string() {
        return Err("live text collation was zero or unnamed".to_string());
    }
    if tenant["collation_oid"].as_i64() != Some(0) || !tenant["collation_name"].is_null() {
        return Err("int4 collation was not absent".to_string());
    }
    if email["type_support"]["converted"].as_bool() != Some(false) {
        return Err("email type conversion was claimed".to_string());
    }
    let comment_sha = sha256_hex(COMMENT_BODY.as_bytes());
    if email["comment"]["byte_len"].as_u64() != Some(COMMENT_BODY.len() as u64)
        || email["comment"]["sha256"].as_str() != Some(comment_sha.as_str())
    {
        return Err("email comment hash or length does not match the fixture".to_string());
    }
    if email["comment"].get("body").is_some() {
        return Err("catalog comment carries a body".to_string());
    }
    let privileges = customer["privileges"].as_array().ok_or("privileges")?;
    if !privileges.iter().any(|item| item["grantee"] == "oo_a156_reader" && item["privilege"] == "SELECT")
    {
        return Err("reader SELECT grant is missing".to_string());
    }
    if privileges.iter().any(|item| item["grantee"] == "oo_a156_reader" && item["privilege"] == "INSERT")
    {
        return Err("reader INSERT grant is present".to_string());
    }
    Ok(())
}

fn assert_structure(
    draft: &Value,
    wrapper: &Value,
    draft_text: &str,
    wrapper_text: &str,
    envelope_text: &str,
) -> Result<(), String> {
    assert_closed(draft, wrapper, draft_text, envelope_text)?;
    if wrapper_text.contains(OWNER) {
        return Err("wrapper carries the owner role".to_string());
    }
    if dataset(draft, "generated", CUSTOMER)?["primary_key"] != json!(["tenant_id", "id"])
        || dataset(draft, "generated", PURCHASE)?["primary_key"] != json!(["tenant_id", "id"])
    {
        return Err("draft primary key is not the composite fixture".to_string());
    }
    if column_names(draft, "generated", CUSTOMER)? != ["tenant_id", "id", "email"] {
        return Err("draft customer columns differ".to_string());
    }
    if column_names(draft, "generated", PURCHASE)? != ["tenant_id", "id", "customer_id"] {
        return Err("draft purchase columns differ".to_string());
    }
    let relationship = draft["generated"]["relationships"]
        .as_array()
        .ok_or("relationships")?
        .iter()
        .find(|item| item["name"] == "purchase_customer_fk")
        .ok_or("relationship missing")?;
    if relationship["provenance"] != "declared_fk"
        || relationship["status"] != "draft"
        || !relationship["confidence"].is_null()
        || relationship["join_hint"] != "left"
        || relationship["cardinality"] != "many_to_one"
        || relationship["from"]["columns"] != json!(["tenant_id", "customer_id"])
        || relationship["to"]["columns"] != json!(["tenant_id", "id"])
        || relationship["from"]["dataset"] != PURCHASE
        || relationship["to"]["dataset"] != CUSTOMER
    {
        return Err("foreign key draft is not the tentative composite fixture".to_string());
    }
    if queue_item(draft, "pii/shop.customer/email", "pii_unknown")?["state"] != "pending" {
        return Err("pii is not pending".to_string());
    }
    if queue_item(draft, "semantic/shop.customer/email", "semantic_unknown")?["state"] != "pending" {
        return Err("semantic type is not pending".to_string());
    }
    Ok(())
}

fn assert_closed(
    draft: &Value,
    wrapper: &Value,
    draft_text: &str,
    envelope_text: &str,
) -> Result<(), String> {
    reject_comment("draft", draft_text)?;
    reject_comment("envelope", envelope_text)?;
    if draft_text.contains(OWNER) {
        return Err("draft carries the owner role".to_string());
    }
    if !envelope_text.contains(OWNER) {
        return Err("envelope dropped the owner role that stays outside the draft".to_string());
    }
    if draft["protocol"] != "oo.builder.structure-draft/v1" {
        return Err("draft protocol changed".to_string());
    }
    if draft["provenance"]["pg_catalog_integration"] != "not_accepted"
        || draft["provenance"]["pg_catalog_contract_commit"] != HISTORICAL_COMMIT
        || draft["provenance"]["publication"] != "unavailable"
        || draft["provenance"]["certification"] != "none"
        || draft["provenance"]["llm"] != "not_used"
        || draft["provenance"]["source_connection"] != "not_used"
    {
        return Err("historical draft provenance changed".to_string());
    }
    if wrapper["protocol"] != CAPTURED_PROTOCOL
        || wrapper["live_catalog_review"] != "pending"
        || wrapper["row_import"] != "not_proven"
        || wrapper["grants"] != "not_authorized"
        || wrapper["publication"] != "unavailable"
        || wrapper["certification"] != "none"
        || wrapper["llm"] != "not_used"
        || wrapper["source_connection"] != "not_used"
        || wrapper["connector_protocol"] != "oo.source.pgsql/v1"
        || wrapper["historical"]["pg_catalog_integration"] != "not_accepted"
        || wrapper["historical"]["pg_catalog_contract_commit"] != HISTORICAL_COMMIT
    {
        return Err("captured wrapper claims an approval the catalog does not have".to_string());
    }
    Ok(())
}

fn reorder(catalog_json: &str) -> Result<String, String> {
    let mut value: Value = serde_json::from_str(catalog_json).map_err(|err| err.to_string())?;
    let tables = value
        .get_mut("tables")
        .and_then(Value::as_array_mut)
        .ok_or("tables")?;
    tables.reverse();
    for table in tables.iter_mut() {
        if let Some(columns) = table.get_mut("columns").and_then(Value::as_array_mut) {
            columns.reverse();
        }
        if let Some(privileges) = table.get_mut("privileges").and_then(Value::as_array_mut) {
            privileges.reverse();
        }
    }
    let text = serde_json::to_string(&value).map_err(|err| err.to_string())?;
    if text == catalog_json {
        return Err("reorder did not change the catalog json".to_string());
    }
    Ok(text)
}

fn table<'a>(catalog: &'a Value, name: &str) -> Result<&'a Value, String> {
    catalog["tables"]
        .as_array()
        .ok_or("tables")?
        .iter()
        .find(|item| item["schema"] == "shop" && item["name"] == name)
        .ok_or_else(|| format!("missing {name}"))
}

fn names_of(columns: &Value) -> Result<Vec<String>, String> {
    columns
        .as_array()
        .ok_or("columns")?
        .iter()
        .map(|item| {
            item["name"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "column name".to_string())
        })
        .collect()
}

fn dataset<'a>(draft: &'a Value, layer: &str, name: &str) -> Result<&'a Value, String> {
    draft[layer]["datasets"]
        .as_array()
        .ok_or("datasets")?
        .iter()
        .find(|item| item["dataset"] == name)
        .ok_or_else(|| format!("missing dataset {name}"))
}

fn column_names(draft: &Value, layer: &str, dataset_name: &str) -> Result<Vec<String>, String> {
    names_of(&dataset(draft, layer, dataset_name)?["columns"])
}

fn column_of<'a>(draft: &'a Value, layer: &str, dataset_name: &str, name: &str) -> Result<&'a Value, String> {
    dataset(draft, layer, dataset_name)?["columns"]
        .as_array()
        .ok_or("columns")?
        .iter()
        .find(|item| item["name"] == name)
        .ok_or_else(|| format!("missing column {name}"))
}

fn queue_item<'a>(draft: &'a Value, item_id: &str, kind: &str) -> Result<&'a Value, String> {
    draft["review_queue"]
        .as_array()
        .ok_or("review queue")?
        .iter()
        .find(|item| item["item_id"] == item_id && item["kind"] == kind)
        .ok_or_else(|| format!("missing queue item {item_id} {kind}"))
}

fn reject_comment(label: &str, text: &str) -> Result<(), String> {
    if text.contains(COMMENT_BODY) {
        Err(format!("{label} contains the comment body"))
    } else {
        Ok(())
    }
}

fn write_clean(path: &PathBuf, text: &str) -> Result<(), String> {
    reject_comment(path.to_string_lossy().as_ref(), text)?;
    fs::write(path, text).map_err(|err| err.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
