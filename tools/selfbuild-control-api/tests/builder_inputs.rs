use std::path::PathBuf;

use serde_json::Value;

use selfbuild_control_api::builder_inputs::{self, CARDS_PROTOCOL, PGSQL_PROTOCOL, SEAL_PROTOCOL};

const FIXTURE: &str = "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/live/pass1";

fn repo_file(rel: &str) -> Vec<u8> {
    let root = std::env::var("OO_SELFBUILD_REPO_ROOT").expect("OO_SELFBUILD_REPO_ROOT");
    let path = PathBuf::from(root).join(rel);
    std::fs::read(&path).unwrap_or_else(|_| panic!("missing {}", path.display()))
}

fn parse(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("json")
}

#[test]
fn legal_pgsql_catalog_prepares_seal_and_cards() {
    let catalog = repo_file(&format!("{FIXTURE}/catalog.json"));
    let profile = repo_file(&format!("{FIXTURE}/profile.json"));
    let comments = repo_file(&format!("{FIXTURE}/comments.json"));
    let catalog_value = parse(&catalog);
    assert_eq!(catalog_value["protocol"], PGSQL_PROTOCOL);
    let prepared = builder_inputs::prepare(&catalog, &profile, Some(&comments), 16);
    let document = parse(&prepared.document);
    assert_eq!(document["outcome"], "sealed");
    assert_eq!(document["seal_written"], true);
    assert_eq!(document["cards_written"], true);
    assert_eq!(document["model"], "not_called");
    assert_eq!(document["pack"], "not_produced");
    assert_eq!(document["ready"], false);
    assert_eq!(document["published"], false);
    assert_eq!(document["structural_draft"], false);
    assert_eq!(document["bytea_remapped"], false);
    assert_eq!(document["native_catalog_called"], false);
    assert_eq!(document["llm_calls"], 0);
    let seal = parse(prepared.seal.as_ref().expect("seal"));
    let cards = parse(prepared.cards.as_ref().expect("cards"));
    assert_eq!(seal["protocol"], SEAL_PROTOCOL);
    assert_eq!(cards["protocol"], CARDS_PROTOCOL);
    assert_eq!(cards["sample_policy"], "none");
    assert_eq!(seal["schema_hash"], catalog_value["schema_hash"]);
    assert!(seal["command"].as_str().is_some_and(|text| !text.is_empty()));
    assert_ne!(seal.get("ready"), Some(&Value::Bool(true)));
    assert_ne!(cards.get("ready"), Some(&Value::Bool(true)));
    let tables = cards["tables"].as_array().expect("tables");
    let catalog_tables = catalog_value["tables"].as_array().expect("catalog tables");
    assert_eq!(tables.len(), catalog_tables.len());
    for table in tables {
        let schema = table["schema"].as_str().unwrap_or("");
        let name = table["name"].as_str().unwrap_or("");
        let source = catalog_tables.iter().find(|item| {
            item["schema"].as_str() == Some(schema) && item["name"].as_str() == Some(name)
        });
        let source = source.expect("card table");
        for column in table["columns"].as_array().expect("columns") {
            assert_eq!(column["most_common_vals"], "withheld");
            assert_eq!(column["histogram_bounds"], "withheld");
            let column_name = column["name"].as_str().unwrap_or("");
            let source_column = source["columns"]
                .as_array()
                .expect("source columns")
                .iter()
                .find(|item| item["name"].as_str() == Some(column_name))
                .expect("source column");
            assert_eq!(column["type_name"], source_column["type_name"]);
        }
    }
}

#[test]
fn capture_protocol_does_not_write_seal() {
    let catalog = br#"{"protocol":"oo.postgres-preview.capture-check/v1","tables":[]}"#;
    let profile = br#"{"sample_policy":"none","tables":[]}"#;
    let prepared = builder_inputs::prepare(catalog, profile, None, 16);
    assert!(prepared.seal.is_none());
    assert!(prepared.cards.is_none());
    let document = parse(&prepared.document);
    assert_eq!(document["outcome"], "compiler_refused");
    assert_eq!(document["seal_written"], false);
    assert_eq!(document["cards_written"], false);
    assert_eq!(document["catalog_protocol"], "oo.postgres-preview.capture-check/v1");
    assert_eq!(document["model"], "not_called");
    assert_eq!(document["ready"], false);
    assert_eq!(document["published"], false);
    assert_eq!(document["structural_draft"], false);
    assert_eq!(document["bytea_remapped"], false);
    assert_eq!(document["native_catalog_called"], false);
    assert_eq!(document["native_request_protocol"], "oo.pipeline.native-sealed-catalog/v1");
    let message = document["message"].as_str().unwrap_or("");
    assert!(message.contains("oo.source.pgsql/v1"), "{message}");
    let text = String::from_utf8(prepared.document).expect("utf8");
    assert!(!text.contains("password"));
    assert!(!text.contains("dba_checklist"));
}

#[test]
fn capture_without_audits_does_not_project() {
    let catalog = br#"{"protocol":"oo.postgres-preview.capture-check/v1","tables":[]}"#;
    let profile = br#"{"protocol":"oo.postgres-preview.capture-compare/v1","rows_compared":0,"sampled":false,"samples":[],"tables":[]}"#;
    let prepared = builder_inputs::prepare_observed(catalog, profile, None, &[], &[], 16);
    assert!(prepared.seal.is_none());
    assert!(prepared.cards.is_none());
    let document = parse(&prepared.document);
    assert_eq!(document["outcome"], "compiler_refused");
    assert_eq!(document["seal_written"], false);
    assert_eq!(document["cards_written"], false);
    assert!(document.get("projection").is_none());
    let message = document["message"].as_str().unwrap_or("");
    assert!(message.contains("oo.source.pgsql/v1"), "{message}");
}

#[test]
fn synthetic_capture_projects_seal_and_cards() {
    let audit = observed_csv(
        "2026-10-11 14:19:33.100 UTC",
        "SET default_transaction_read_only = on",
    ) + &observed_csv(
        "2026-10-11 14:19:33.582 UTC",
        "CREATE TABLE public.item (id bigint PRIMARY KEY); GRANT SELECT ON TABLE public.item TO oo_reader",
    );
    let basis = br#"{"protocol":"oo.pipeline.catalog-import-pg/v1","source_id":"oo_dl01w1_46","manifest":{"protocol":"oo.pipeline.pg-sealed-manifest/v1","source_id":"oo_dl01w1_46","tables":[{"logical_schema":"public","logical_table":"item","columns":[{"logical_name":"id","oid":20,"typmod":-1,"key":false}]}]}}"#;
    let catalog = br#"{"protocol":"oo.postgres-preview.capture-check/v1","tables":[{"schema":"public","table":"item","columns":["id"],"excluded":[],"pk":"id","replica":"d"}]}"#;
    let profile = br#"{"protocol":"oo.postgres-preview.capture-compare/v1","rows_compared":1,"sampled":false,"full_scan":true,"samples":[],"tables":[]}"#;
    let audit_bytes = audit.into_bytes();
    let audit_ref: &[u8] = &audit_bytes;
    let prepared = builder_inputs::prepare_observed(catalog, profile, None, &[basis], &[audit_ref], 16);
    let document = parse(&prepared.document);
    assert_eq!(document["outcome"], "sealed", "{}", document["message"]);
    assert_eq!(document["projection"]["captured_at"], "2026-10-11T14:19:33.582000Z");
    assert_eq!(document["projection"]["parsed_create_count"], 1);
    assert_eq!(document["model"], "not_called");
    assert_eq!(document["ready"], false);
    assert_eq!(document["bytea_remapped"], false);
    let seal = parse(prepared.seal.as_ref().expect("seal"));
    let cards = parse(prepared.cards.as_ref().expect("cards"));
    assert_eq!(seal["protocol"], SEAL_PROTOCOL);
    assert_eq!(cards["protocol"], CARDS_PROTOCOL);
    assert_eq!(cards["sample_policy"], "none");
    assert_ne!(seal.get("ready"), Some(&Value::Bool(true)));
    assert_eq!(seal["schema_hash"], document["projection"]["schema_hash"]);
}

#[test]
fn observed_capture_projects_seal_and_cards() {
    let pg = "evidence/ad13411e-d04e-4c91-8a20-49caf867aa23/DL01/postgres-product";
    let catalog = repo_file(&format!("{pg}/linux/runner/out/capture/phases/catalog-capture-check.json"));
    let profile = repo_file(&format!("{pg}/linux/runner/out/capture/phases/compare-capture-compare.json"));
    let run_basis = repo_file(&format!("{pg}/linux/runner/out/run/stages/catalog-import.request.json"));
    let east_basis = repo_file(&format!("{pg}/linux/runner/out/customer-east/stages/catalog-import.request.json"));
    let public_audit = repo_file(&format!("{pg}/linux/failed-attempt-admission-denied/source.csv"));
    let east_audit = repo_file(&format!("{pg}/linux/runner/out/customer-east/source-window.csv"));
    let bases: [&[u8]; 2] = [&run_basis, &east_basis];
    let audits: [&[u8]; 2] = [&public_audit, &east_audit];
    let prepared = builder_inputs::prepare_observed(&catalog, &profile, None, &bases, &audits, 16);
    let document = parse(&prepared.document);
    assert_eq!(document["outcome"], "sealed", "{}", document["message"]);
    assert_eq!(document["catalog_protocol"], PGSQL_PROTOCOL);
    assert_eq!(document["model"], "not_called");
    assert_eq!(document["ready"], false);
    assert_eq!(document["published"], false);
    assert_eq!(document["structural_draft"], false);
    assert_eq!(document["bytea_remapped"], false);
    assert_eq!(document["native_catalog_called"], false);
    assert_eq!(document["llm_calls"], 0);
    let projection = &document["projection"];
    assert_eq!(projection["captured_at"], "2026-10-11T14:19:33.582000Z");
    assert_eq!(projection["captured_at_rule"], "latest_ddl_statement");
    assert_eq!(projection["rows_compared"], 100050);
    assert_eq!(projection["table_checks_omitted"], true);
    assert_eq!(projection["input_protocol"], "oo.postgres-preview.capture-check/v1");
    assert_eq!(projection["parsed_create_count"], 6);
    assert_eq!(projection["emitted_tables"], 4);
    assert_eq!(
        projection["select_grants"],
        serde_json::json!([
            "customer_east.delivery_notes",
            "public.hubs",
            "public.shipments",
            "public.tracking_events"
        ])
    );
    let seal = parse(prepared.seal.as_ref().expect("seal"));
    let cards = parse(prepared.cards.as_ref().expect("cards"));
    assert_eq!(seal["protocol"], SEAL_PROTOCOL);
    assert_eq!(cards["protocol"], CARDS_PROTOCOL);
    assert_eq!(cards["sample_policy"], "none");
    assert_ne!(seal.get("ready"), Some(&Value::Bool(true)));
    assert_ne!(cards.get("ready"), Some(&Value::Bool(true)));
    assert_eq!(seal["schema_hash"], projection["schema_hash"]);
    let tables = cards["tables"].as_array().expect("tables");
    assert_eq!(tables.len(), 4);
    assert_eq!(card_column(&cards, "public", "hubs", "hub_id")["type_name"], "int8");
    assert_eq!(card_column(&cards, "public", "hubs", "hub_id")["not_null"], true);
    assert_eq!(card_column(&cards, "public", "hubs", "region_code")["not_null"], false);
    assert_eq!(card_column(&cards, "public", "hubs", "latitude")["type_name"], "numeric");
    let mut bytea = Vec::new();
    for table in tables {
        for column in table["columns"].as_array().expect("columns") {
            if column["type_name"] == "bytea" {
                bytea.push(format!(
                    "{}.{}.{}",
                    table["schema"].as_str().unwrap_or(""),
                    table["name"].as_str().unwrap_or(""),
                    column["name"].as_str().unwrap_or("")
                ));
            }
        }
    }
    bytea.sort();
    assert_eq!(
        bytea,
        vec![
            "customer_east.delivery_notes.receipt".to_owned(),
            "public.hubs.badge".to_owned(),
            "public.shipments.seal".to_owned(),
            "public.tracking_events.payload".to_owned(),
        ]
    );
    let text = String::from_utf8(prepared.document).expect("utf8");
    assert!(!text.contains("password"));
    assert!(!text.contains("dba_checklist"));
}

fn card_column<'a>(cards: &'a Value, schema: &str, table: &str, name: &str) -> &'a Value {
    cards["tables"]
        .as_array()
        .expect("tables")
        .iter()
        .find(|item| item["schema"].as_str() == Some(schema) && item["name"].as_str() == Some(table))
        .expect("table")["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|item| item["name"].as_str() == Some(name))
        .expect("column")
}

fn observed_csv(stamp: &str, statement: &str) -> String {
    let mut fields = vec![stamp.to_owned(), "oo_dl01w1_46_dba".to_owned(), "src".to_owned()];
    while fields.len() < 13 {
        fields.push("x".to_owned());
    }
    fields.push(format!("statement: {statement}"));
    fields.join(",") + "\n"
}

#[test]
fn extra_pgsql_field_does_not_write_seal() {
    let mut catalog: Value = parse(&repo_file(&format!("{FIXTURE}/catalog.json")));
    catalog["forged"] = Value::Bool(true);
    let bytes = serde_json::to_vec(&catalog).expect("catalog");
    let prepared = builder_inputs::prepare(&bytes, br#"{"sample_policy":"none","tables":[]}"#, None, 16);
    assert!(prepared.seal.is_none());
    assert!(prepared.cards.is_none());
    let document = parse(&prepared.document);
    assert_eq!(document["outcome"], "compiler_refused");
    assert_eq!(document["seal_written"], false);
    assert_eq!(document["model"], "not_called");
}
