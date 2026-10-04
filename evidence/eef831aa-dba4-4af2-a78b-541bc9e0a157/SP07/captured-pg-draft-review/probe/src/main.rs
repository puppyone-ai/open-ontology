//! Independent review of `compile_captured_pg_catalog`.
//!
//! Expected columns, keys, and the comment hash come from this fixture's DDL.
//! The compiler output is checked against those expectations.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use oo_builder::{compile_captured_pg_catalog, compile_structure_draft, CapturedPgDraft};
use oo_connectors::{
    PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef,
    SelectedTable, SourceId, TableSelection,
};
use oo_core::oos::{validate_dataset, validate_relationships, DatasetFile, RelationshipsFile};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const COMMENT_BODY: &str = "a157-review-comment-body-do-not-carry";
const OWNER: &str = "a157_owner";
const READER: &str = "oo_a157_reader";
const PARTY: &str = "ledger.party";
const INVOICE: &str = "ledger.invoice";
const EMAIL_ITEM: &str = "column/ledger.party/email";
const ID_ITEM: &str = "column/ledger.party/id";
const REL_ITEM: &str = "relationship/ledger.invoice/invoice_party_fk";
const HISTORICAL: &str = "b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74";
const FK_NAME: &str = "invoice_party_fk";

#[tokio::main]
async fn main() -> ExitCode {
    let phase = env::args().nth(1).unwrap_or_default();
    let result = match phase.as_str() {
        "capture" => capture_phase().await,
        "stale" => stale_phase().await,
        _ => Err("usage: capture|stale".to_string()),
    };
    match result {
        Ok(()) => {
            println!("PROBE_OK phase={phase}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("probe_error={}", scrub(&err));
            ExitCode::from(1)
        }
    }
}

async fn capture_phase() -> Result<(), String> {
    let out = out_dir()?;
    let catalog_json = fetch_catalog().await?;
    reject_comment("catalog", &catalog_json)?;
    let catalog: Value = serde_json::from_str(&catalog_json).map_err(|err| err.to_string())?;
    assert_catalog(&catalog, &["tenant_id", "id", "email"])?;
    let compiled = compile(&catalog_json, None, None)?;
    let repeated = compile(&catalog_json, None, Some("[]"))?;
    if repeated.evidence_envelope != compiled.evidence_envelope
        || repeated.draft != compiled.draft
        || repeated.wrapper != compiled.wrapper
    {
        return Err("empty reviews changed the sealed bytes".to_string());
    }
    let round = compile_structure_draft(&compiled.evidence_envelope)
        .map_err(|err| format!("round trip {} {}", err.code, err.path))?;
    if round != compiled.draft {
        return Err("evidence envelope does not compile to the sealed draft".to_string());
    }
    let draft: Value = serde_json::from_str(&compiled.draft).map_err(|err| err.to_string())?;
    let wrapper: Value = serde_json::from_str(&compiled.wrapper).map_err(|err| err.to_string())?;
    let envelope: Value =
        serde_json::from_str(&compiled.evidence_envelope).map_err(|err| err.to_string())?;
    assert_closed(&draft, &wrapper, &compiled)?;
    assert_seals(&compiled, &draft, &wrapper, &envelope)?;
    assert_selection(&envelope, false)?;
    strict_core(&draft)?;
    assert_structure(&draft)?;
    let reversed = reorder(&catalog_json)?;
    let again = compile(&reversed, None, None)?;
    if again.draft != compiled.draft
        || again.evidence_envelope != compiled.evidence_envelope
        || again.wrapper != compiled.wrapper
        || again.schema_hash != compiled.schema_hash
        || again.captured_catalog_hash != compiled.captured_catalog_hash
    {
        return Err("equivalent catalog reorder changed the sealed bytes".to_string());
    }
    expect_code(&swap_primary_key(&catalog_json)?, None, None, "hash.schema")?;
    expect_code(&flip_converted(&catalog_json)?, None, None, "type.converted")?;
    expect_code(&comment_body_field(&catalog_json)?, None, None, "field.unknown")?;
    expect_code(
        r#"{"protocol":"oo.source.pgsql/v1","protocol":"again"}"#,
        None,
        None,
        "json.duplicate_key",
    )?;
    let email_fp = fingerprint(&draft, EMAIL_ITEM, "structural_column")?;
    let id_fp = fingerprint(&draft, ID_ITEM, "structural_column")?;
    assert_accept(&catalog_json, &compiled.schema_hash, &email_fp)?;
    assert_closure(&catalog_json, &compiled.schema_hash, &id_fp)?;
    let oracle = json!({
        "phase": "capture",
        "schema_hash": compiled.schema_hash,
        "draft_hash": compiled.draft_hash,
        "wrapper_hash": compiled.wrapper_hash,
        "reorder_stable": true,
        "empty_reviews_stable": true,
        "pk_order_changes_schema_hash": true,
        "converted_true_rejected": "type.converted",
        "comment_body_rejected": "field.unknown",
        "duplicate_key_rejected": "json.duplicate_key",
        "comment_byte_len": COMMENT_BODY.len(),
        "comment_sha256": sha256_hex(COMMENT_BODY.as_bytes()),
        "party_columns": ["tenant_id", "id", "email"],
        "invoice_columns": ["tenant_id", "id", "party_id"],
        "primary_key": ["tenant_id", "id"],
        "fk": FK_NAME,
        "cardinality": "many_to_one",
        "pii": "pending",
        "semantic": "pending",
        "live_catalog_review": "pending",
        "historical_integration": "not_accepted",
        "row_import": "not_proven",
        "grants": "not_authorized",
        "accept_does_not_approve_capture": true,
        "fresh_reject_closes_fk": "to_column_not_effective",
        "strict_core_oos": true,
    });
    fs::write(out.join("capture-oracle.json"), oracle.to_string() + "\n").map_err(|err| err.to_string())?;
    fs::write(
        out.join("first-facts.json"),
        json!({
            "schema_hash": compiled.schema_hash,
            "email_fingerprint": email_fp,
        })
        .to_string()
            + "\n",
    )
    .map_err(|err| err.to_string())?;
    write_clean(&out.join("catalog-before.json"), &catalog_json)?;
    write_clean(&out.join("draft-before.json"), &compiled.draft)?;
    write_clean(&out.join("wrapper-before.json"), &compiled.wrapper)?;
    Ok(())
}

async fn stale_phase() -> Result<(), String> {
    let out = out_dir()?;
    let facts: Value = serde_json::from_str(
        &fs::read_to_string(out.join("first-facts.json")).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    let prior_hash = facts["schema_hash"].as_str().ok_or("prior schema hash")?.to_string();
    let fingerprint = facts["email_fingerprint"]
        .as_str()
        .ok_or("prior fingerprint")?
        .to_string();
    let catalog_json = fetch_catalog().await?;
    reject_comment("catalog", &catalog_json)?;
    let catalog: Value = serde_json::from_str(&catalog_json).map_err(|err| err.to_string())?;
    assert_catalog(&catalog, &["tenant_id", "id", "email", "note"])?;
    let curated = r#"{"datasets":[{"dataset":"ledger.party","columns":[{"name":"email","label":"Correspondence","pii":"sensitive"}]}],"relationships":[]}"#;
    let reviews = json!([{
        "item_id": EMAIL_ITEM,
        "action": "accept",
        "schema_hash": prior_hash,
        "fact_fingerprint": fingerprint,
    }])
    .to_string();
    let compiled = compile(&catalog_json, Some(curated), Some(&reviews))?;
    if compiled.schema_hash == prior_hash {
        return Err("schema change did not change schema_hash".to_string());
    }
    let draft: Value = serde_json::from_str(&compiled.draft).map_err(|err| err.to_string())?;
    let wrapper: Value = serde_json::from_str(&compiled.wrapper).map_err(|err| err.to_string())?;
    let envelope: Value =
        serde_json::from_str(&compiled.evidence_envelope).map_err(|err| err.to_string())?;
    assert_closed(&draft, &wrapper, &compiled)?;
    assert_seals(&compiled, &draft, &wrapper, &envelope)?;
    assert_selection(&envelope, true)?;
    strict_core(&draft)?;
    let effective = column_of(&draft, "effective", PARTY, "email")?;
    if effective["label"] != "email" {
        return Err("stale decision changed the effective label".to_string());
    }
    if !effective["pii"].is_null() {
        return Err("stale curated pii was applied".to_string());
    }
    let curated_email = column_of(&draft, "curated", PARTY, "email")?;
    if curated_email["label"] != "Correspondence" || curated_email["pii"] != "sensitive" {
        return Err("curated echo was not preserved".to_string());
    }
    let stale_item = queue_item(&draft, EMAIL_ITEM, "stale_decision")?;
    if stale_item["detail"] != "stale_schema_hash" || stale_item["state"] != "pending" || stale_item["publishable"] != false {
        return Err("prior decision was not a pending stale_schema_hash".to_string());
    }
    if queue_item(&draft, EMAIL_ITEM, "curated_unbound")?["state"] != "pending" {
        return Err("curated label was applied or not queued unbound".to_string());
    }
    if queue_item(&draft, "pii/ledger.party/email", "semantic_field_not_applied")?["detail"] != "pii" {
        return Err("curated pii was applied".to_string());
    }
    let names = column_names(&draft, "generated", PARTY)?;
    if names != ["tenant_id", "id", "email", "note"] {
        return Err(format!("party columns after alter: {names:?}"));
    }
    if dataset(&draft, "generated", PARTY)?["primary_key"] != json!(["tenant_id", "id"]) {
        return Err("composite primary key changed".to_string());
    }
    let oracle = json!({
        "phase": "stale",
        "prior_schema_hash": prior_hash,
        "schema_hash": compiled.schema_hash,
        "schema_hash_changed": true,
        "stale_detail": "stale_schema_hash",
        "stale_state": "pending",
        "curated_label": "Correspondence",
        "effective_label": "email",
        "pii_applied": false,
        "live_catalog_review": wrapper["live_catalog_review"],
        "historical_integration": draft["provenance"]["pg_catalog_integration"],
        "party_columns": names,
        "strict_core_oos": true,
    });
    fs::write(out.join("stale-oracle.json"), oracle.to_string() + "\n").map_err(|err| err.to_string())?;
    write_clean(&out.join("catalog-after.json"), &catalog_json)?;
    write_clean(&out.join("draft-after.json"), &compiled.draft)?;
    write_clean(&out.join("wrapper-after.json"), &compiled.wrapper)?;
    Ok(())
}

async fn fetch_catalog() -> Result<String, String> {
    let host = env::var("PGHOST").map_err(|_| "PGHOST is unset")?;
    let port: u16 = env::var("PGPORT").map_err(|_| "PGPORT")?.parse().map_err(|_| "PGPORT")?;
    let database = env::var("PGDATABASE").map_err(|_| "PGDATABASE")?;
    let user = env::var("PGUSER").map_err(|_| "PGUSER")?;
    let password = fs::read_to_string(env::var("PASSWORD_FILE").map_err(|_| "PASSWORD_FILE")?)
        .map_err(|err| format!("password file: {err}"))?;
    let password = password.trim_end_matches(['\r', '\n']).to_string();
    let ca = fs::read(env::var("CA_FILE").map_err(|_| "CA_FILE")?).map_err(|err| format!("ca: {err}"))?;
    let config = PgSourceConfig::try_new(
        SourceId::new("ledger_src").map_err(|err| err.to_string())?,
        PgEndpoint::new(host, port, database).map_err(|err| err.to_string())?,
        PgCredentials::new(user, password).map_err(|err| err.to_string())?,
        SecretRef::new("secret://a157/reader").map_err(|err| err.to_string())?,
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
            SelectedTable::new("ledger", "party").map_err(|err| err.to_string())?,
            SelectedTable::new("ledger", "invoice").map_err(|err| err.to_string())?,
        ])
        .map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    let connector = PgConnector::new(config);
    let mut session = connector.connect().await.map_err(|err| format!("connect: {err}"))?;
    let catalog = session.catalog().await.map_err(|err| format!("catalog: {err}"))?;
    serde_json::to_string(&catalog).map_err(|err| err.to_string())
}

fn compile(catalog: &str, curated: Option<&str>, reviews: Option<&str>) -> Result<CapturedPgDraft, String> {
    compile_captured_pg_catalog(catalog, curated, reviews).map_err(|err| format!("{} {}", err.code, err.path))
}

fn expect_code(catalog: &str, curated: Option<&str>, reviews: Option<&str>, needle: &str) -> Result<(), String> {
    match compile_captured_pg_catalog(catalog, curated, reviews) {
        Ok(_) => Err(format!("expected {needle}")),
        Err(err) if err.code.contains(needle) => Ok(()),
        Err(err) => Err(format!("got {} {} wanted {needle}", err.code, err.path)),
    }
}

fn assert_catalog(catalog: &Value, party_columns: &[&str]) -> Result<(), String> {
    if catalog["protocol"] != "oo.source.pgsql/v1" || catalog["type_conversion"] != "unsupported" {
        return Err("catalog protocol or type conversion is not the current source document".to_string());
    }
    if catalog["source_id"] != "ledger_src" || catalog["database"] != "oo_a157" {
        return Err("catalog source or database is unexpected".to_string());
    }
    let party = table(catalog, "party")?;
    let invoice = table(catalog, "invoice")?;
    if party["owner"] != OWNER || invoice["owner"] != OWNER {
        return Err("table owner is not the fixture role".to_string());
    }
    if names_of(&party["columns"])? != party_columns {
        return Err(format!("catalog party columns: {:?}", names_of(&party["columns"])?));
    }
    if names_of(&invoice["columns"])? != ["tenant_id", "id", "party_id"] {
        return Err("catalog invoice columns differ from the DDL".to_string());
    }
    if party["primary_key"] != json!([{"attnum": 1, "name": "tenant_id"}, {"attnum": 2, "name": "id"}])
        || invoice["primary_key"] != json!([{"attnum": 1, "name": "tenant_id"}, {"attnum": 2, "name": "id"}])
    {
        return Err("catalog primary key is not the composite DDL order".to_string());
    }
    let fk = invoice["foreign_keys"]
        .as_array()
        .ok_or("foreign keys")?
        .iter()
        .find(|item| item["name"] == FK_NAME)
        .ok_or("invoice_party_fk missing")?;
    if fk["columns"]
        != json!([
            {"attnum": 1, "name": "tenant_id", "ref_schema": "ledger", "ref_table": "party", "ref_attnum": 1, "ref_name": "tenant_id"},
            {"attnum": 3, "name": "party_id", "ref_schema": "ledger", "ref_table": "party", "ref_attnum": 2, "ref_name": "id"}
        ])
    {
        return Err("catalog foreign key pairs differ from the DDL".to_string());
    }
    for table_value in [party, invoice] {
        for column in table_value["columns"].as_array().ok_or("columns")? {
            if column["type_support"]["converted"].as_bool() != Some(false) {
                return Err("production catalog claims type conversion".to_string());
            }
        }
    }
    let email = column_in(party, "email")?;
    let tenant = column_in(party, "tenant_id")?;
    if email["collation_oid"].as_i64().unwrap_or(0) == 0 || !email["collation_name"].is_string() {
        return Err("live text collation was zero or unnamed".to_string());
    }
    if tenant["collation_oid"].as_i64() != Some(0) || !tenant["collation_name"].is_null() {
        return Err("int4 collation was not absent".to_string());
    }
    let comment_sha = sha256_hex(COMMENT_BODY.as_bytes());
    if email["comment"]["byte_len"].as_u64() != Some(COMMENT_BODY.len() as u64)
        || email["comment"]["sha256"].as_str() != Some(comment_sha.as_str())
        || email["comment"].get("body").is_some()
    {
        return Err("email comment hash, length, or body does not match the fixture".to_string());
    }
    let privileges = party["privileges"].as_array().ok_or("privileges")?;
    if !privileges.iter().any(|item| item["grantee"] == READER && item["privilege"] == "SELECT") {
        return Err("reader SELECT grant is missing".to_string());
    }
    if privileges.iter().any(|item| item["grantee"] == READER && item["privilege"] == "INSERT") {
        return Err("reader INSERT grant is present".to_string());
    }
    Ok(())
}

fn assert_structure(draft: &Value) -> Result<(), String> {
    if dataset(draft, "generated", PARTY)?["primary_key"] != json!(["tenant_id", "id"])
        || dataset(draft, "generated", INVOICE)?["primary_key"] != json!(["tenant_id", "id"])
    {
        return Err("draft primary key is not the composite DDL order".to_string());
    }
    if column_names(draft, "generated", PARTY)? != ["tenant_id", "id", "email"] {
        return Err("draft party columns differ".to_string());
    }
    if column_names(draft, "generated", INVOICE)? != ["tenant_id", "id", "party_id"] {
        return Err("draft invoice columns differ".to_string());
    }
    let email = column_of(draft, "effective", PARTY, "email")?;
    if email["data_type"] != "text" || !email["pii"].is_null() || !email["semantic_type"].is_null() {
        return Err("email gained a type, pii, or semantic classification".to_string());
    }
    let relationship = draft["generated"]["relationships"]
        .as_array()
        .ok_or("relationships")?
        .iter()
        .find(|item| item["name"] == FK_NAME)
        .ok_or("relationship missing")?;
    if relationship["provenance"] != "declared_fk"
        || relationship["status"] != "draft"
        || !relationship["confidence"].is_null()
        || relationship["join_hint"] != "left"
        || relationship["cardinality"] != "many_to_one"
        || relationship["from"]["dataset"] != INVOICE
        || relationship["to"]["dataset"] != PARTY
        || relationship["from"]["columns"] != json!(["tenant_id", "party_id"])
        || relationship["to"]["columns"] != json!(["tenant_id", "id"])
    {
        return Err("foreign key draft is not the tentative composite fixture".to_string());
    }
    if queue_item(draft, "pii/ledger.party/email", "pii_unknown")?["state"] != "pending" {
        return Err("pii is not pending".to_string());
    }
    if queue_item(draft, "semantic/ledger.party/email", "semantic_unknown")?["state"] != "pending" {
        return Err("semantic type is not pending".to_string());
    }
    if queue_item(draft, REL_ITEM, "candidate_relationship")?["publishable"] != false {
        return Err("candidate relationship is publishable".to_string());
    }
    Ok(())
}

fn assert_accept(catalog_json: &str, schema_hash: &str, fingerprint: &str) -> Result<(), String> {
    let reviews = json!([{
        "item_id": EMAIL_ITEM,
        "action": "accept",
        "schema_hash": schema_hash,
        "fact_fingerprint": fingerprint,
    }])
    .to_string();
    let compiled = compile(catalog_json, None, Some(&reviews))?;
    let draft: Value = serde_json::from_str(&compiled.draft).map_err(|err| err.to_string())?;
    let wrapper: Value = serde_json::from_str(&compiled.wrapper).map_err(|err| err.to_string())?;
    assert_closed(&draft, &wrapper, &compiled)?;
    strict_core(&draft)?;
    if queue_item(&draft, EMAIL_ITEM, "structural_column")?["state"] != "accepted" {
        return Err("fresh accept did not mark the structural column accepted".to_string());
    }
    if queue_item(&draft, EMAIL_ITEM, "structural_column")?["publishable"] != false {
        return Err("accepted column became publishable".to_string());
    }
    let email = column_of(&draft, "effective", PARTY, "email")?;
    if !email["pii"].is_null() || !email["semantic_type"].is_null() {
        return Err("accept classified pii or semantics".to_string());
    }
    Ok(())
}

fn assert_closure(catalog_json: &str, schema_hash: &str, fingerprint: &str) -> Result<(), String> {
    let curated = r#"{"datasets":[{"dataset":"ledger.party","columns":[{"name":"id","status":"rejected","label":"Kept label"}]}],"relationships":[]}"#;
    let reviews = json!([{
        "item_id": ID_ITEM,
        "action": "reject",
        "schema_hash": schema_hash,
        "fact_fingerprint": fingerprint,
    }])
    .to_string();
    let compiled = compile(catalog_json, Some(curated), Some(&reviews))?;
    let draft: Value = serde_json::from_str(&compiled.draft).map_err(|err| err.to_string())?;
    let wrapper: Value = serde_json::from_str(&compiled.wrapper).map_err(|err| err.to_string())?;
    assert_closed(&draft, &wrapper, &compiled)?;
    strict_core(&draft)?;
    if column_names(&draft, "generated", PARTY)?.contains(&"id".to_string()) == false {
        return Err("generated party lost id".to_string());
    }
    if column_names(&draft, "effective", PARTY)?.contains(&"id".to_string()) {
        return Err("rejected id stayed in effective".to_string());
    }
    let curated_id = column_of(&draft, "curated", PARTY, "id")?;
    if curated_id["status"] != "rejected" || curated_id["label"] != "Kept label" {
        return Err("curated rejection echo was not kept".to_string());
    }
    if relationship_named(&draft, "generated", FK_NAME).is_none() {
        return Err("generated relationship was removed".to_string());
    }
    if relationship_named(&draft, "effective", FK_NAME).is_some() {
        return Err("effective relationship survived a rejected referenced column".to_string());
    }
    let item = queue_item(&draft, REL_ITEM, "relationship_endpoint_not_effective")?;
    if item["detail"] != "to_column_not_effective" || item["state"] != "pending" || item["publishable"] != false {
        return Err(format!("endpoint closure detail {}", item["detail"]));
    }
    Ok(())
}

fn assert_closed(draft: &Value, wrapper: &Value, compiled: &CapturedPgDraft) -> Result<(), String> {
    reject_comment("draft", &compiled.draft)?;
    reject_comment("wrapper", &compiled.wrapper)?;
    reject_comment("envelope", &compiled.evidence_envelope)?;
    if compiled.draft.contains(OWNER) || compiled.wrapper.contains(OWNER) {
        return Err("draft or wrapper carries the owner role".to_string());
    }
    if !compiled.evidence_envelope.contains(OWNER) {
        return Err("envelope dropped the owner that stays on the catalog document".to_string());
    }
    if draft["protocol"] != "oo.builder.structure-draft/v1"
        || draft["compiler"] != "oo-builder/structure-draft/v1"
        || draft["provenance"]["pg_catalog_integration"] != "not_accepted"
        || draft["provenance"]["pg_catalog_contract_commit"] != HISTORICAL
        || draft["provenance"]["pg_catalog_contract"] != "oo.source.pgsql/v1"
        || draft["provenance"]["publication"] != "unavailable"
        || draft["provenance"]["certification"] != "none"
        || draft["provenance"]["llm"] != "not_used"
        || draft["provenance"]["source_connection"] != "not_used"
    {
        return Err("historical draft provenance changed".to_string());
    }
    if wrapper["protocol"] != "oo.builder.captured-pg-draft/v1"
        || wrapper["compiler"] != "oo-builder/captured-pg-draft/v1"
        || wrapper["live_catalog_review"] != "pending"
        || wrapper["row_import"] != "not_proven"
        || wrapper["grants"] != "not_authorized"
        || wrapper["publication"] != "unavailable"
        || wrapper["certification"] != "none"
        || wrapper["llm"] != "not_used"
        || wrapper["source_connection"] != "not_used"
        || wrapper["connector_protocol"] != "oo.source.pgsql/v1"
        || wrapper["historical"]["pg_catalog_integration"] != "not_accepted"
        || wrapper["historical"]["pg_catalog_contract_commit"] != HISTORICAL
        || wrapper["historical"]["protocol"] != "oo.builder.structure-draft/v1"
    {
        return Err("wrapper claims an approval the capture does not have".to_string());
    }
    Ok(())
}

fn assert_seals(compiled: &CapturedPgDraft, draft: &Value, wrapper: &Value, envelope: &Value) -> Result<(), String> {
    if sha256_hex(compiled.draft.as_bytes()) != compiled.draft_hash {
        return Err("draft_hash is not the draft bytes".to_string());
    }
    if sha256_hex(compiled.evidence_envelope.as_bytes()) != compiled.evidence_envelope_hash {
        return Err("evidence_envelope_hash is not the envelope bytes".to_string());
    }
    if sha256_hex(canonical_json(&envelope["catalog"]).as_bytes()) != compiled.captured_catalog_hash {
        return Err("captured_catalog_hash is not the envelope catalog".to_string());
    }
    let mut draft_preimage = draft.clone();
    draft_preimage
        .get_mut("provenance")
        .and_then(Value::as_object_mut)
        .ok_or("provenance")?
        .remove("manifest_hash");
    if sha256_hex(canonical_json(&draft_preimage).as_bytes()) != compiled.manifest_hash {
        return Err("manifest_hash does not match the draft with the field omitted".to_string());
    }
    let mut wrapper_preimage = wrapper.clone();
    wrapper_preimage.as_object_mut().ok_or("wrapper")?.remove("wrapper_hash");
    if sha256_hex(canonical_json(&wrapper_preimage).as_bytes()) != compiled.wrapper_hash {
        return Err("wrapper_hash does not match the wrapper with the field omitted".to_string());
    }
    if canonical_json(&json!({"b": 1, "a": 2})) != r#"{"a":2,"b":1}"# {
        return Err("review canonicalizer drifted".to_string());
    }
    Ok(())
}

fn assert_selection(envelope: &Value, with_note: bool) -> Result<(), String> {
    let mut party_columns = BTreeSet::from(["tenant_id".to_string(), "id".to_string(), "email".to_string()]);
    if with_note {
        party_columns.insert("note".to_string());
    }
    let invoice_columns = BTreeSet::from([
        "tenant_id".to_string(),
        "id".to_string(),
        "party_id".to_string(),
    ]);
    let party_keys = BTreeSet::from(["primary_key".to_string()]);
    let invoice_keys = BTreeSet::from(["primary_key".to_string(), FK_NAME.to_string()]);
    let tables = envelope["selection"]["tables"].as_array().ok_or("selection")?;
    if tables.len() != 2 {
        return Err("selection table count is not the DDL".to_string());
    }
    for table in tables {
        let schema = table["schema"].as_str().unwrap_or("");
        let name = table["name"].as_str().unwrap_or("");
        let columns = string_set(&table["columns"])?;
        let keys = string_set(&table["keys"])?;
        let expected = match (schema, name) {
            ("ledger", "party") => (&party_columns, &party_keys),
            ("ledger", "invoice") => (&invoice_columns, &invoice_keys),
            _ => return Err(format!("unexpected selected table {schema}.{name}")),
        };
        if &columns != expected.0 || &keys != expected.1 {
            return Err(format!("selection for {schema}.{name} is not the exact DDL set"));
        }
    }
    Ok(())
}

fn strict_core(draft: &Value) -> Result<(), String> {
    for layer in ["generated", "effective"] {
        let datasets = draft[layer]["datasets"].as_array().ok_or("datasets")?;
        if datasets.is_empty() {
            return Err(format!("{layer} has no dataset"));
        }
        for dataset in datasets {
            let parsed: DatasetFile = serde_json::from_value(dataset.clone())
                .map_err(|err| format!("{layer} dataset {err}"))?;
            if !validate_dataset(&parsed).is_empty() {
                return Err(format!("{layer} dataset failed strict core validation"));
            }
        }
        let file = json!({
            "schema_version": datasets[0]["schema_version"],
            "relationships": draft[layer]["relationships"],
        });
        let parsed: RelationshipsFile =
            serde_json::from_value(file).map_err(|err| format!("{layer} relationships {err}"))?;
        if !validate_relationships(&parsed).is_empty() {
            return Err(format!("{layer} relationships failed strict core validation"));
        }
    }
    Ok(())
}

fn reorder(catalog_json: &str) -> Result<String, String> {
    let mut value: Value = serde_json::from_str(catalog_json).map_err(|err| err.to_string())?;
    let tables = value.get_mut("tables").and_then(Value::as_array_mut).ok_or("tables")?;
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

fn swap_primary_key(catalog_json: &str) -> Result<String, String> {
    let mut value: Value = serde_json::from_str(catalog_json).map_err(|err| err.to_string())?;
    let key = value
        .get_mut("tables")
        .and_then(Value::as_array_mut)
        .ok_or("tables")?
        .iter_mut()
        .find(|item| item["name"] == "party")
        .ok_or("party")?
        .get_mut("primary_key")
        .and_then(Value::as_array_mut)
        .ok_or("primary key")?;
    key.swap(0, 1);
    serde_json::to_string(&value).map_err(|err| err.to_string())
}

fn flip_converted(catalog_json: &str) -> Result<String, String> {
    let mut value: Value = serde_json::from_str(catalog_json).map_err(|err| err.to_string())?;
    column_in_mut(&mut value, "party", "email")?["type_support"]["converted"] = json!(true);
    serde_json::to_string(&value).map_err(|err| err.to_string())
}

fn comment_body_field(catalog_json: &str) -> Result<String, String> {
    let mut value: Value = serde_json::from_str(catalog_json).map_err(|err| err.to_string())?;
    column_in_mut(&mut value, "party", "email")?["comment"]["body"] = json!(COMMENT_BODY);
    serde_json::to_string(&value).map_err(|err| err.to_string())
}

fn table<'a>(catalog: &'a Value, name: &str) -> Result<&'a Value, String> {
    catalog["tables"]
        .as_array()
        .ok_or("tables")?
        .iter()
        .find(|item| item["schema"] == "ledger" && item["name"] == name)
        .ok_or_else(|| format!("missing {name}"))
}

fn column_in<'a>(table_value: &'a Value, name: &str) -> Result<&'a Value, String> {
    table_value["columns"]
        .as_array()
        .ok_or("columns")?
        .iter()
        .find(|item| item["name"] == name)
        .ok_or_else(|| format!("missing {name}"))
}

fn column_in_mut<'a>(catalog: &'a mut Value, table_name: &str, column_name: &str) -> Result<&'a mut Value, String> {
    let table = catalog
        .get_mut("tables")
        .and_then(Value::as_array_mut)
        .ok_or("tables")?
        .iter_mut()
        .find(|item| item["name"] == table_name)
        .ok_or("table")?;
    table
        .get_mut("columns")
        .and_then(Value::as_array_mut)
        .ok_or("columns")?
        .iter_mut()
        .find(|item| item["name"] == column_name)
        .ok_or_else(|| format!("missing {column_name}"))
}

fn names_of(columns: &Value) -> Result<Vec<String>, String> {
    columns
        .as_array()
        .ok_or("columns")?
        .iter()
        .map(|item| item["name"].as_str().map(str::to_string).ok_or_else(|| "column name".to_string()))
        .collect()
}

fn string_set(value: &Value) -> Result<BTreeSet<String>, String> {
    value
        .as_array()
        .ok_or("array")?
        .iter()
        .map(|item| item.as_str().map(str::to_string).ok_or_else(|| "string".to_string()))
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

fn relationship_named<'a>(draft: &'a Value, layer: &str, name: &str) -> Option<&'a Value> {
    draft[layer]["relationships"]
        .as_array()?
        .iter()
        .find(|item| item["name"] == name)
}

fn queue_item<'a>(draft: &'a Value, item_id: &str, kind: &str) -> Result<&'a Value, String> {
    draft["review_queue"]
        .as_array()
        .ok_or("review queue")?
        .iter()
        .find(|item| item["item_id"] == item_id && item["kind"] == kind)
        .ok_or_else(|| format!("missing queue item {item_id} {kind}"))
}

fn fingerprint(draft: &Value, item_id: &str, kind: &str) -> Result<String, String> {
    queue_item(draft, item_id, kind)?["fact_fingerprint"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("missing fingerprint {item_id}"))
}

fn reject_comment(label: &str, text: &str) -> Result<(), String> {
    if text.contains(COMMENT_BODY) {
        Err(format!("{label} contains the comment body"))
    } else {
        Ok(())
    }
}

fn write_clean(path: &PathBuf, text: &str) -> Result<(), String> {
    reject_comment("output", text)?;
    fs::write(path, text).map_err(|err| err.to_string())?;
    if !text.ends_with('\n') {
        fs::write(path, format!("{text}\n")).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn out_dir() -> Result<PathBuf, String> {
    let out = PathBuf::from(env::var("OUT_DIR").map_err(|_| "OUT_DIR")?);
    fs::create_dir_all(&out).map_err(|err| err.to_string())?;
    Ok(out)
}

fn scrub(text: &str) -> String {
    let mut out = text.replace(COMMENT_BODY, "[redacted-comment]");
    if let Ok(path) = env::var("PASSWORD_FILE") {
        if let Ok(raw) = fs::read_to_string(path) {
            let password = raw.trim();
            if !password.is_empty() {
                out = out.replace(password, "[redacted]");
            }
        }
    }
    out
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
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
            ch if u32::from(ch) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(ch)));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}
