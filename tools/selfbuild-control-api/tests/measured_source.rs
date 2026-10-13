use std::path::PathBuf;

use serde_json::{json, Value};

use selfbuild_control_api::adapt::{self, ProtocolPin};
use selfbuild_control_api::builder_inputs::{self, CARDS_PROTOCOL, PGSQL_PROTOCOL, SEAL_PROTOCOL};
use selfbuild_control_api::oosyaml::{generated_from_inputs, render_pack};

const INPUTS: &str = "evidence/ad13411e-d04e-4c91-8a20-49caf867aa23/DL01/postgres-product/linux/runner/out/capture-g2/builder-inputs";
const SOURCE_COMMIT: &str = "11f1132d9dff8dc3e543ee7576d073723a2cbd0c";

fn repo_file(rel: &str) -> Vec<u8> {
    let root = std::env::var("OO_SELFBUILD_REPO_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let path = root.join(rel);
    std::fs::read(&path).unwrap_or_else(|_| panic!("missing {}", path.display()))
}

fn parse(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("json")
}

fn pin(catalog: &str, profile: &str, semantics: &str) -> ProtocolPin {
    ProtocolPin {
        catalog: catalog.to_owned(),
        profile: profile.to_owned(),
        semantics: semantics.to_owned(),
        source_commit: SOURCE_COMMIT.to_owned(),
        semantics_commit: SOURCE_COMMIT.to_owned(),
    }
}

#[test]
fn measured_pgsql_dictionary_seals_and_withholds_bytea() {
    let catalog = repo_file(&format!("{INPUTS}/dictionary.json"));
    let profile = repo_file(&format!("{INPUTS}/profile.json"));
    let audit = repo_file(&format!("{INPUTS}/builder-bind.json"));
    let comments = repo_file(&format!("{INPUTS}/comments.json"));
    let projection = adapt::project_measured(
        &catalog,
        &profile,
        &audit,
        &pin(PGSQL_PROTOCOL, "oo.pg-builder.profile/v1", "oo.postgres-preview.builder-bind/v1"),
    )
    .expect("measured dictionary");
    let receipt = &projection.receipt;
    assert_eq!(receipt["catalog_protocol"], PGSQL_PROTOCOL);
    assert_eq!(receipt["source_commit"], SOURCE_COMMIT);
    assert_eq!(receipt["source_rows"], 100051);
    assert_eq!(receipt["rows_compared"], Value::Null);
    assert_eq!(receipt["delete_status"], "unknown");
    assert_eq!(receipt["grade_status"], "unknown");
    assert_eq!(receipt["auto_approved"], false);
    assert_eq!(receipt["unknown_auto_approved"], false);
    assert_eq!(receipt["derive_source_bound_called"], false);
    assert_eq!(receipt["engine"], "postgres");
    assert_eq!(receipt["schema_hash"], "652359ac4582d23b338a5494c8a217b82dab120ae442d24ad49907f36ccefc47");
    let columns = receipt["columns"].as_array().expect("columns");
    let bytea: Vec<_> = columns
        .iter()
        .filter(|item| item["target_type"] == "bytea")
        .map(|item| format!("{}.{}", item["dataset"].as_str().unwrap_or(""), item["column"].as_str().unwrap_or("")))
        .collect();
    assert_eq!(
        bytea,
        vec![
            "customer_east.delivery_notes.receipt".to_owned(),
            "public.hubs.badge".to_owned(),
            "public.shipments.seal".to_owned(),
            "public.tracking_events.payload".to_owned(),
        ]
    );
    for item in columns.iter().filter(|item| item["target_type"] == "bytea") {
        assert!(item["data_type"].is_null());
        assert_eq!(item["oos_support"], "oos_unsupported");
        assert_eq!(item["auto_approved"], false);
    }
    let amount = columns
        .iter()
        .find(|item| item["dataset"] == "customer_east.delivery_notes" && item["column"] == "amount")
        .expect("amount");
    assert_eq!(amount["data_type"], "numeric");
    let token = columns
        .iter()
        .find(|item| item["dataset"] == "public.hubs" && item["column"] == "hub_token")
        .expect("hub_token");
    assert_eq!(token["data_type"], "uuid");

    let prepared = builder_inputs::prepare(&catalog, &profile, Some(&comments), 16);
    let document = parse(&prepared.document);
    assert_eq!(document["outcome"], "sealed", "{}", document["message"]);
    assert_eq!(document["catalog_protocol"], PGSQL_PROTOCOL);
    assert_eq!(document["seal_written"], true);
    assert_eq!(document["cards_written"], true);
    assert_eq!(document["model"], "not_called");
    assert_eq!(document["pack"], "not_produced");
    assert_eq!(document["ready"], false);
    assert_eq!(document["published"], false);
    assert_eq!(document["bytea_remapped"], false);
    assert_eq!(document["structural_draft"], false);
    assert_eq!(document["llm_calls"], 0);
    let seal = parse(prepared.seal.as_ref().expect("seal"));
    let cards = parse(prepared.cards.as_ref().expect("cards"));
    assert_eq!(seal["protocol"], SEAL_PROTOCOL);
    assert_eq!(cards["protocol"], CARDS_PROTOCOL);
    assert_eq!(cards["sample_policy"], "none");
    assert_eq!(seal["schema_hash"], receipt["schema_hash"]);
    assert_ne!(seal.get("ready"), Some(&Value::Bool(true)));
    assert_ne!(cards.get("published"), Some(&Value::Bool(true)));
    assert_eq!(cards["tables"].as_array().map(Vec::len), Some(4));

    let (model, yaml) = generated_from_inputs(&projection.typed_catalog, &json!({"columns": []}), "none").expect("yaml");
    let hubs = model.datasets.get("public.hubs").expect("hubs");
    assert!(hubs.columns.contains_key("hub_id"));
    assert!(!hubs.columns.contains_key("badge"));
    assert!(hubs.columns.values().all(|column| column.pii == "unknown" && column.classification_state != "auto_accepted"));
    assert!(yaml.contains("hub_id"));
    assert!(!yaml.contains("auto_accepted"));
    assert!(render_pack(&model, "analyst", &["public.hubs.hub_id".to_owned()], "unreviewed").is_err());
    publish_operator_bundle(&catalog, &profile, &audit, &comments, &prepared, &yaml);
}

#[test]
fn mysql_and_oracle_dictionaries_are_not_retagged_or_sealed() {
    for (protocol, profile_protocol, audit_protocol) in [
        ("oo.source.mysql/v1", "oo.source.mysql.profile/v1", "oo.source.mysql.audit/v1"),
        ("oo.source.oracle/v1", "oo.source.oracle.profile/v1", "oo.source.oracle.audit/v1"),
    ] {
        let catalog = json!({
            "protocol": protocol,
            "tables": [{
                "schema": "public",
                "name": "orders",
                "columns": [
                    {"name": "_id", "type_name": "varchar"},
                    {"name": "order_id", "type_name": "int8"},
                    {"name": "payload", "type_name": "bytea"}
                ]
            }]
        });
        let profile = json!({"protocol": profile_protocol, "tables": []});
        let audit = json!({"protocol": audit_protocol, "delete_grade": "unknown"});
        let catalog_bytes = serde_json::to_vec(&catalog).unwrap();
        let profile_bytes = serde_json::to_vec(&profile).unwrap();
        let audit_bytes = serde_json::to_vec(&audit).unwrap();
        let projection = adapt::project_measured(&catalog_bytes, &profile_bytes, &audit_bytes, &pin(protocol, profile_protocol, audit_protocol))
            .unwrap_or_else(|_| panic!("{protocol} measured"));
        assert_eq!(projection.receipt["catalog_protocol"], protocol);
        assert_ne!(projection.receipt["catalog_protocol"], PGSQL_PROTOCOL);
        assert_eq!(projection.receipt["delete_status"], "unknown");
        let names: Vec<_> = projection.receipt["columns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["column"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(names, vec!["_id".to_owned(), "order_id".to_owned(), "payload".to_owned()]);
        let id = &projection.receipt["columns"][0];
        assert_eq!(id["column"], "_id");
        assert_eq!(id["type_status"], "unknown");
        assert!(id["data_type"].is_null());
        assert_eq!(projection.receipt["columns"][1]["data_type"], "bigint");
        assert!(projection.receipt["columns"][2]["data_type"].is_null());
        assert_eq!(projection.receipt["columns"][2]["oos_support"], "oos_unsupported");
        let typed = &projection.typed_catalog["tables"][0]["columns"];
        assert_eq!(typed.as_array().map(Vec::len), Some(1));
        assert_eq!(typed[0]["name"], "order_id");
        let prepared = builder_inputs::prepare_observed(&catalog_bytes, &profile_bytes, None, &[], &[], 16);
        assert!(prepared.seal.is_none());
        assert!(prepared.cards.is_none());
        let document = parse(&prepared.document);
        assert_eq!(document["outcome"], "producer_refused");
        assert_eq!(document["catalog_protocol"], protocol);
        assert_eq!(document["seal_written"], false);
        assert_eq!(document["ready"], false);
        assert_eq!(document["published"], false);
        assert_eq!(document["model"], "not_called");
        assert_eq!(document["pack"], "not_produced");
    }
}

fn publish_operator_bundle(
    catalog: &[u8],
    profile: &[u8],
    audit: &[u8],
    comments: &[u8],
    prepared: &selfbuild_control_api::builder_inputs::Prepared,
    yaml: &str,
) {
    let Ok(root) = std::env::var("OO_MEASURED_BUNDLE") else {
        return;
    };
    let dir = PathBuf::from(root);
    std::fs::create_dir_all(&dir).expect("bundle dir");
    std::fs::write(dir.join("catalog.json"), catalog).unwrap();
    std::fs::write(dir.join("profile.json"), profile).unwrap();
    std::fs::write(dir.join("source_semantics.json"), audit).unwrap();
    std::fs::write(
        dir.join("read_logic.json"),
        br#"{"contract":"oo.selfbuild-control-api/read-logic/v1","executed":false,"source":"control_envelope"}"#,
    )
    .unwrap();
    std::fs::write(dir.join("rules.json"), br#"{"columns":[],"relationships":[]}"#).unwrap();
    std::fs::write(
        dir.join("suggestions.json"),
        br#"{"certified":false,"contract":"oo.selfbuild-control-api/suggestions/v1","merged":false,"ready":false}"#,
    )
    .unwrap();
    std::fs::write(dir.join("generation.json"), br#"{"llm_calls":0,"outcome":"structural_draft","proxy_call":false}"#).unwrap();
    std::fs::write(
        dir.join("roles.json"),
        br#"{"contract":"oo.selfbuild-control-api/sealed-roles/v1","roles":{"oo_r_reader":["public.hubs.hub_id"]}}"#,
    )
    .unwrap();
    std::fs::write(dir.join("comments.json"), comments).unwrap();
    selfbuild_control_api::sealed::write_manifest(&dir).expect("manifest");
    std::fs::write(dir.join("seal.json"), prepared.seal.as_ref().expect("seal bytes")).unwrap();
    std::fs::write(dir.join("cards.json"), prepared.cards.as_ref().expect("cards bytes")).unwrap();
    std::fs::write(dir.join("builder-prepare.json"), &prepared.document).unwrap();
    std::fs::write(dir.join("generated.yaml"), yaml).unwrap();
    let protocol = selfbuild_control_api::config::SourceProtocol {
        catalog: PGSQL_PROTOCOL.to_owned(),
        profile: "oo.pg-builder.profile/v1".to_owned(),
        semantics: "oo.postgres-preview.builder-bind/v1".to_owned(),
        source_commit: SOURCE_COMMIT.to_owned(),
        semantics_commit: SOURCE_COMMIT.to_owned(),
        type_basis: Vec::new(),
        schema_audit: Vec::new(),
    };
    let loaded = selfbuild_control_api::sealed::all_roles(dir.to_str().expect("utf8 bundle path"), Some(&protocol)).expect("service loader");
    assert_eq!(loaded.get("oo_r_reader").map(Vec::as_slice), Some(["public.hubs.hub_id".to_owned()].as_slice()));
    let pins = json!({
        "catalog": PGSQL_PROTOCOL,
        "profile": "oo.pg-builder.profile/v1",
        "semantics": "oo.postgres-preview.builder-bind/v1",
        "source_commit": SOURCE_COMMIT,
        "semantics_commit": SOURCE_COMMIT,
        "type_basis": [],
        "schema_audit": [],
        "comments": "oo.pg-builder.comments/v1",
        "source_rows": 100051,
        "input_directory": INPUTS
    });
    std::fs::write(dir.join("source-protocol.json"), serde_json::to_vec_pretty(&pins).unwrap()).unwrap();
}
