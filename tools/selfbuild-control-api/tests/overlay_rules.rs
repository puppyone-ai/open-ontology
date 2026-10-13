use std::collections::BTreeMap;

use selfbuild_control_api::config::{load_config_bytes, project_input_hash};
use selfbuild_control_api::oosyaml::{
    apply_fact_edit, doc_sha256, gate_report, generated_from_inputs, merge, model_to_y, parse_model, parse_yaml, render_pack, sealed_types, Review,
};
use selfbuild_control_api::sealed::{self, BUNDLE_FILES};
use serde_json::json;

fn catalog() -> serde_json::Value {
    json!({
        "tables": [{
            "schema": "public",
            "table": "entry",
            "columns": [
                {"name": "entry_id", "data_type": "bigint"},
                {"name": "amount", "data_type": "numeric"},
                {"name": "phone", "data_type": "text"}
            ]
        }]
    })
}

fn rules() -> serde_json::Value {
    json!({
        "columns": [
            {"column": "public.entry.entry_id", "semantic_type": "identifier", "pii": "none", "confidence": "high"},
            {"column": "public.entry.amount", "semantic_type": "money", "pii": "none", "confidence": "high", "unit": "USD", "example_value": "5550100"},
            {"column": "public.entry.phone", "semantic_type": "phone", "pii": "personal", "confidence": "high"},
            {"column": "public.entry.note", "semantic_type": "free_text", "pii": "none", "confidence": "low"}
        ],
        "relationships": [{
            "name": "entry_account",
            "from_column": "public.entry.entry_id",
            "to_column": "public.account.account_id",
            "cardinality": "many_to_one",
            "confidence": "high"
        }]
    })
}

fn generated() -> (selfbuild_control_api::oosyaml::Model, String) {
    let mut listed = catalog();
    listed["tables"][0]["columns"].as_array_mut().unwrap().push(json!({"name": "note", "data_type": "text"}));
    generated_from_inputs(&listed, &rules(), "rule_high_confidence").unwrap()
}

#[test]
fn column_order_does_not_change_the_generated_hash() {
    let (model, yaml) = generated();
    let parsed = parse_model(&yaml, "generated").unwrap();
    assert_eq!(parsed.datasets["public.entry"].columns["amount"].classification_state, "auto_accepted");
    assert_eq!(parsed.datasets["public.entry"].columns["phone"].classification_state, "pending");
    assert_eq!(parsed.datasets["public.entry"].columns["phone"].pii, "personal");
    assert_eq!(parsed.datasets["public.entry"].columns["note"].classification_state, "pending");
    assert!(!yaml.contains("5550100"));
    assert!(!yaml.contains("sample"));
    let again = parse_yaml(&yaml).unwrap();
    assert_eq!(doc_sha256(&model_to_y(&model, "generated")).unwrap(), doc_sha256(&again).unwrap());
    let mut reversed = catalog();
    let columns = reversed["tables"][0]["columns"].as_array_mut().unwrap();
    columns.reverse();
    let rules = rules();
    let (_, first) = generated_from_inputs(&catalog(), &rules, "rule_high_confidence").unwrap();
    let (_, second) = generated_from_inputs(&reversed, &rules, "rule_high_confidence").unwrap();
    assert_eq!(first, second);
}

#[test]
fn yaml_rejects_tabs_duplicate_keys_and_curated_types() {
    assert!(parse_yaml("spec_version:\toos/v1\n").is_err());
    assert!(parse_yaml("spec_version: oos/v1\nspec_version: oos/v1\n").is_err());
    let (_, yaml) = generated();
    let fact = generated().0.datasets["public.entry"].columns["amount"].fact_sha256.clone();
    let curated = format!(
        "spec_version: oos/v1\nkind: curated\ndatasets:\n  - name: public.entry\n    columns:\n      - name: amount\n        label: \"Ledger amount\"\n        binds_fact: {fact}\n        data_type: text\nrelationships: []\n"
    );
    assert!(parse_model(&curated, "curated").is_err());
    let _ = yaml;
}

#[test]
fn curated_wins_rejected_stays_out_and_one_fact_goes_stale() {
    let (mut model, _) = generated();
    let relation_fact = model.relationships["entry_account"].fact_sha256.clone();
    let amount_fact = model.datasets["public.entry"].columns["amount"].fact_sha256.clone();
    let curated_yaml = format!(
        "spec_version: oos/v1\nkind: curated\ndatasets:\n  - name: public.entry\n    columns:\n      - name: amount\n        label: \"Ledger amount\"\n        binds_fact: {amount_fact}\nrelationships:\n  - name: entry_account\n    status: rejected\n    fact_sha256: {relation_fact}\n"
    );
    let curated = parse_model(&curated_yaml, "curated").unwrap();
    let merged = merge(&model, &curated, &[]);
    assert_eq!(merged.datasets["public.entry"].columns["amount"].label, "Ledger amount");
    assert_eq!(merged.datasets["public.entry"].columns["amount"].label_from, "curated");
    assert!(!merged.relationships.contains_key("entry_account"));
    assert!(merged.stale.is_empty());
    apply_fact_edit(&mut model, "public.entry", "amount", "text").unwrap();
    assert_eq!(model.datasets["public.entry"].columns["amount"].sealed_data_type, "numeric");
    assert_eq!(model.datasets["public.entry"].columns["amount"].classification_state, "pending");
    assert_eq!(model.datasets["public.entry"].columns["amount"].confidence, "low");
    let stale = merge(&model, &curated, &[]);
    assert_eq!(stale.stale.len(), 1);
    assert_eq!(stale.datasets["public.entry"].columns["amount"].label, "");
    assert!(!stale.relationships.contains_key("entry_account"));
    let review = Review {
        name: "public.entry.amount".to_owned(),
        decision: "confirmed".to_owned(),
        binds_fact: model.datasets["public.entry"].columns["amount"].fact_sha256.clone(),
    };
    let rebound_yaml = format!(
        "spec_version: oos/v1\nkind: curated\ndatasets:\n  - name: public.entry\n    columns:\n      - name: amount\n        label: \"Ledger amount\"\n        binds_fact: {}\nrelationships:\n  - name: entry_account\n    status: rejected\n    fact_sha256: {}\n",
        review.binds_fact, "a".repeat(64)
    );
    let rebound = parse_model(&rebound_yaml, "curated").unwrap();
    let restored = merge(&model, &rebound, &[review]);
    assert!(restored.stale.iter().any(|item| item.name == "entry_account"));
    assert!(!restored.relationships.contains_key("entry_account"));
    assert_eq!(restored.datasets["public.entry"].columns["amount"].label, "Ledger amount");
    assert_eq!(restored.datasets["public.entry"].columns["amount"].classification_state, "confirmed");
}

#[test]
fn packs_hide_samples_pii_and_unapproved_text() {
    let (model, yaml) = generated();
    assert!(yaml.contains("auto_accepted"));
    let analyst = vec!["public.entry.entry_id".to_owned(), "public.entry.amount".to_owned()];
    let clerk = vec!["public.entry.entry_id".to_owned()];
    let phone = vec!["public.entry.phone".to_owned()];
    let left = render_pack(&model, "oo_r_analyst", &analyst, "oos/v1").unwrap();
    let right = render_pack(&model, "oo_r_clerk", &clerk, "oos/v1").unwrap();
    assert_ne!(left.pack_sha256, right.pack_sha256);
    let analyst_body = left.files.iter().map(|file| file.body.as_str()).collect::<String>();
    let clerk_body = right.files.iter().map(|file| file.body.as_str()).collect::<String>();
    assert!(analyst_body.contains("amount"));
    assert!(!clerk_body.contains("amount"));
    assert!(!analyst_body.contains("5550100"));
    assert!(!analyst_body.contains("Ledger"));
    assert!(render_pack(&model, "oo_r_restricted", &phone, "oos/v1").is_err());
    let mut stale = model.clone();
    apply_fact_edit(&mut stale, "public.entry", "amount", "text").unwrap();
    let curated_fact = model.datasets["public.entry"].columns["amount"].fact_sha256.clone();
    let curated = parse_model(
        &format!(
            "spec_version: oos/v1\nkind: curated\ndatasets:\n  - name: public.entry\n    columns:\n      - name: amount\n        label: \"Ledger amount\"\n        binds_fact: {curated_fact}\nrelationships: []\n"
        ),
        "curated",
    )
    .unwrap();
    let merged = merge(&stale, &curated, &[]);
    assert!(render_pack(&merged, "oo_r_analyst", &analyst, "oos/v1").is_err());
    let report = gate_report(&merged, &sealed_types(&stale));
    assert_eq!(report["published"], false);
    assert_eq!(report["publication"], "refused");
    assert_eq!(report["business_owner_signed"], false);
    let gates = report["gates"].as_array().unwrap();
    let g2 = gates.iter().find(|item| item["gate_id"] == "G2").unwrap();
    let g7 = gates.iter().find(|item| item["gate_id"] == "G7").unwrap();
    let g13 = gates.iter().find(|item| item["gate_id"] == "G13").unwrap();
    assert_eq!(g2["status"], "block");
    assert_eq!(g7["status"], "not_run");
    assert_eq!(g13["status"], "block");
}

#[test]
fn unknown_pii_is_not_auto_approved_and_money_needs_a_unit() {
    let mut bad = rules();
    bad["columns"][1]["unit"] = json!("");
    assert!(generated_from_inputs(&catalog(), &bad, "rule_high_confidence").is_err());
    let mut unknown = rules();
    unknown["columns"][0]["pii"] = json!("unknown");
    let (model, _) = generated_from_inputs(&catalog(), &unknown, "rule_high_confidence").unwrap();
    assert_eq!(model.datasets["public.entry"].columns["entry_id"].classification_state, "pending");
    let mut with_sample = rules();
    with_sample["columns"][0]["sample"] = json!("secret");
    assert!(generated_from_inputs(&catalog(), &with_sample, "rule_high_confidence").is_err());
}

#[test]
fn empty_sealed_bundle_does_not_change_the_project_hash() {
    let bare = sample();
    let mut with_empty = sample();
    with_empty["projects"][0]["sealed_bundle"] = json!("");
    let left = load_config_bytes(&serde_json::to_vec(&bare).unwrap()).unwrap();
    let right = load_config_bytes(&serde_json::to_vec(&with_empty).unwrap()).unwrap();
    assert_eq!(project_input_hash(&left.projects[0]), project_input_hash(&right.projects[0]));
    let mut sealed = sample();
    sealed["projects"][0]["sealed_bundle"] = json!("/tmp/oo-sealed-bundle");
    let changed = load_config_bytes(&serde_json::to_vec(&sealed).unwrap()).unwrap();
    assert_ne!(project_input_hash(&left.projects[0]), project_input_hash(&changed.projects[0]));
}

#[test]
fn sealed_bundle_rejects_a_sample_key_and_executed_sql() {
    let dir = std::env::temp_dir().join(format!("oo-sealed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_bundle(&dir, false, "structural_draft");
    assert_eq!(sealed::bundle_outcome(dir.to_str().unwrap()).unwrap(), "structural_draft");
    write_bundle(&dir, true, "model_unauthorized");
    assert!(sealed::bundle_outcome(dir.to_str().unwrap()).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

fn write_bundle(dir: &std::path::Path, sample_key: bool, outcome: &str) {
    let mut rules = rules();
    if sample_key {
        rules["columns"][0]["sample"] = json!("secret");
    }
    let files = BTreeMap::from([
        ("catalog.json", serde_json::to_vec(&catalog()).unwrap()),
        ("profile.json", br#"{"contract":"oo.selfbuild-control-api/sealed-profile/v1"}"#.to_vec()),
        ("read_logic.json", br#"{"contract":"oo.selfbuild-control-api/sealed-read-logic/v1","executed":false}"#.to_vec()),
        ("source_semantics.json", br#"{"contract":"oo.selfbuild-control-api/sealed-source-semantics/v1","executed":false}"#.to_vec()),
        ("rules.json", serde_json::to_vec(&rules).unwrap()),
        ("suggestions.json", br#"{"certified":false,"merged":false,"ready":false}"#.to_vec()),
        ("generation.json", format!(r#"{{"outcome":"{outcome}","llm_calls":0}}"#).into_bytes()),
        (
            "roles.json",
            br#"{"contract":"oo.selfbuild-control-api/sealed-roles/v1","roles":{"oo_r_analyst":["public.entry.entry_id","public.entry.amount"],"oo_r_clerk":["public.entry.entry_id"]}}"#.to_vec(),
        ),
    ]);
    for name in BUNDLE_FILES {
        std::fs::write(dir.join(name), files.get(name).unwrap()).unwrap();
    }
    sealed::write_manifest(dir).unwrap();
}

fn sample() -> serde_json::Value {
    let commit = "a".repeat(64);
    json!({
        "contract": "oo.selfbuild-control-api.config/v1",
        "deployment_origins": ["https://control.example"],
        "listen": {"bind": "127.0.0.1:8443", "cert_file": "/tmp/cert.pem", "key_file": "/tmp/key.pem"},
        "identity": {"origin": "https://127.0.0.1:1", "ca_file": "/tmp/ca.pem"},
        "postgres": {
            "host": "127.0.0.1", "port": 1, "database": "oo_selfbuild",
            "user": "oo_selfbuild_api_admin", "password_file": "/tmp/pw",
            "ca_file": "/tmp/ca.pem", "server_name": "127.0.0.1"
        },
        "work_dir": "/tmp/oo-selfbuild-work",
        "binaries": {
            "catalog": "/tmp/oo-pipeline", "pg_builder": "/tmp/pg-builder",
            "rules": "/tmp/selfbuild-rules", "suggestions": "/tmp/selfbuild-suggestions",
            "review_ledger": "/tmp/review-ledger"
        },
        "projects": [{
            "project_id": "demo",
            "config_commit": commit,
            "source_ref": "src",
            "allowance_id": "allow",
            "llm_secret_ref": "secret://llm/demo",
            "source_secret_ref": "secret://pg/demo",
            "source": {
                "host": "127.0.0.1", "port": 5432, "database": "srcdb", "username": "reader",
                "password_file": "/tmp/srcpw", "ca_file": "/tmp/ca.pem",
                "tables": [{"schema": "app", "table": "customer"}]
            },
            "sample_policy": "none",
            "rules_auto_accept": "none",
            "llm": {"bearer_file": "/tmp/bearer", "origin": "http://127.0.0.1:8317", "model_id": "grok-4.7"},
            "role_policy_file": "/tmp/roles.json",
            "hidden_columns": ["app.customer.email"],
            "visible_columns": ["app.customer.name"]
        }],
        "ledger": {
            "host": "127.0.0.1", "port": 1, "database": "ledgerdb", "user": "ledger_user",
            "password_file": "/tmp/lpw", "ca_file": "/tmp/ca.pem", "server_name": "127.0.0.1"
        },
        "principals": []
    })
}

#[test]
fn original_uppercase_and_document_id_are_kept_in_oos() {
    let catalog = json!({"tables":[{"schema":"public","table":"SHIPMENTS","columns":[
        {"name":"_id","data_type":"text"},{"name":"N_FREE","data_type":"numeric"}
    ]}]});
    let (_, yaml) = generated_from_inputs(&catalog, &json!({"columns":[],"relationships":[]}), "none").unwrap();
    let parsed = parse_model(&yaml, "generated").unwrap();
    assert!(parsed.datasets["public.SHIPMENTS"].columns.contains_key("_id"));
    assert!(parsed.datasets["public.SHIPMENTS"].columns.contains_key("N_FREE"));
    assert!(!selfbuild_control_api::ids::ident_ok("_id"));
}

#[test]
fn original_comments_are_hash_bound_and_tampering_is_refused() {
    let dir = std::env::temp_dir().join(format!("oo-71-comments-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    write_bundle(&dir, false, "structural_draft");
    std::fs::write(dir.join("comments.json"), br#"{"protocol":"oo.pg-builder.comments/v1","tables":[]}"#).unwrap();
    sealed::write_manifest(&dir).unwrap();
    assert!(sealed::all_roles(dir.to_str().unwrap(), None).is_ok());
    std::fs::write(dir.join("comments.json"), br#"{"tables":["tampered"]}"#).unwrap();
    assert!(sealed::all_roles(dir.to_str().unwrap(), None).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_protocol_config_is_accepted_without_retagging() {
    let mut input=sample();
    input["projects"][0]["llm"]["model_id"]=json!("gpt-6.1-sol");
    input["projects"][0]["sealed_bundle"]=json!("/tmp/actual-native-bundle");
    input["projects"][0]["source_protocol"]=json!({"catalog":"oo.pipeline.sealed-import-catalog/v1","profile":"oo.native.profile/v1","semantics":"oo.native.semantics/v1","source_commit":"b593e053e93da2c767a4d0bf19f1fa7d94332fdc","semantics_commit":"b593e053e93da2c767a4d0bf19f1fa7d94332fdc","type_basis":[],"schema_audit":[]});
    let parsed=load_config_bytes(&serde_json::to_vec(&input).unwrap()).unwrap();
    assert_eq!(parsed.projects[0].source_protocol.as_ref().unwrap().catalog,"oo.pipeline.sealed-import-catalog/v1");
}

#[test]
fn role_pack_does_not_disclose_relationship_to_a_hidden_table() {
    let (mut model,_) = generated();
    for dataset in model.datasets.values_mut() { for column in dataset.columns.values_mut() { if column.pii=="none" { column.classification_state="confirmed".into(); } } }
    let pack=render_pack(&model,"oo_r_reader", &["public.entry.entry_id".into(),"public.entry.amount".into()],"oos/v1").unwrap();
    assert!(pack.files.iter().all(|file|!file.body.contains("entry_account")));
}
