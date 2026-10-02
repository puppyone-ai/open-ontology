//! Checked-in schemas, documented examples, and the rejection cases.

use std::fs;
use std::path::PathBuf;

use oo_core::config::{
    parse_agent_yaml, parse_eval_suite_yaml, parse_files_yaml, parse_hooks_yaml, parse_oo_yaml,
    parse_roles_yaml, parse_sources_yaml,
};
use oo_core::oos::{
    FactPin, merge_effective, parse_calendar_yaml, parse_dataset_yaml, parse_decisions_yaml,
    parse_entity_yaml, parse_enum_yaml, parse_examples_yaml, parse_glossary_yaml,
    parse_metrics_yaml, parse_ontology_yaml, parse_relationships_yaml, parse_resolution_yaml,
};
use oo_core::text::HexDigest;
use oo_core::yaml::parse_yaml_value;
use serde_json::json;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    fs::read_to_string(repo().join(path)).unwrap_or_else(|error| panic!("read {path}: {error}"))
}

#[test]
fn schemas_match_the_rust_types_and_the_meta_schema() {
    let docs = oo_core::schema_export::contract_documents().expect("schema export");
    let dir = repo().join("contracts/schema");
    if std::env::var("OO_WRITE_SCHEMAS").ok().as_deref() == Some("1") {
        fs::create_dir_all(&dir).unwrap();
        for (name, value) in &docs {
            let pretty = oo_core::schema_export::to_pretty(value);
            let tmp = dir.join(format!(".{name}.tmp"));
            fs::write(&tmp, &pretty).unwrap();
            fs::rename(&tmp, dir.join(name)).unwrap();
        }
    }
    for (name, value) in &docs {
        let checked = fs::read_to_string(dir.join(name))
            .unwrap_or_else(|error| panic!("missing {name}: {error}"));
        assert_eq!(checked, oo_core::schema_export::to_pretty(value), "{name}");
        if *name != "openapi-v1.json" {
            assert!(
                jsonschema::draft202012::meta::is_valid(value),
                "{name} is not a valid draft 2020-12 schema"
            );
        }
    }
    let openapi = &docs
        .iter()
        .find(|item| item.0 == "openapi-v1.json")
        .unwrap()
        .1;
    assert_eq!(openapi["openapi"], "3.1.0");
    assert!(
        openapi
            .get("paths")
            .map(|paths| paths.as_object().is_none_or(|map| map.is_empty()))
            .unwrap_or(true)
    );
    assert!(
        !openapi["components"]["schemas"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert!(
        openapi["info"]["description"]
            .as_str()
            .unwrap()
            .contains("not a running server")
    );
}

#[test]
fn batch_arrow_schema_states_physical_bounds() {
    let docs = oo_core::schema_export::contract_documents().expect("schemas");
    let batch = &docs
        .iter()
        .find(|item| item.0 == "batch-v1.json")
        .expect("batch")
        .1;
    let one_of = batch["$defs"]["ArrowTypeName"]["oneOf"]
        .as_array()
        .expect("oneOf");
    let variant = |kind: &str| {
        one_of
            .iter()
            .find(|item| item["properties"]["kind"]["const"] == kind)
            .unwrap_or_else(|| panic!("missing {kind}"))
    };
    let decimal128 = variant("decimal128");
    let decimal256 = variant("decimal256");
    assert_eq!(decimal128["properties"]["precision"]["minimum"], 1);
    assert_eq!(decimal128["properties"]["precision"]["maximum"], 38);
    assert_eq!(decimal128["properties"]["scale"]["minimum"], -128);
    assert_eq!(decimal256["properties"]["precision"]["minimum"], 1);
    assert_eq!(decimal256["properties"]["precision"]["maximum"], 76);
    assert_eq!(decimal256["properties"]["scale"]["minimum"], -128);
    let description = variant("list")["description"].as_str().unwrap();
    assert!(description.contains("32"), "{description}");
    assert!(description.contains("2020-12"), "{description}");
    let validator = oo_core::schema_util::compile_schema(decimal128).expect("decimal128");
    assert!(!validator.is_valid(&json!({"kind":"decimal128","precision":0,"scale":0})));
    assert!(validator.is_valid(&json!({"kind":"decimal128","precision":1,"scale":-128})));
    assert!(validator.is_valid(&json!({"kind":"decimal128","precision":38,"scale":-100})));
    assert!(validator.is_valid(&json!({"kind":"decimal128","precision":38,"scale":38})));
    assert!(!validator.is_valid(&json!({"kind":"decimal128","precision":5,"scale":6})));
    assert!(!validator.is_valid(&json!({"kind":"decimal128","precision":38,"scale":127})));
    let wide = oo_core::schema_util::compile_schema(decimal256).expect("decimal256");
    assert!(wide.is_valid(&json!({"kind":"decimal256","precision":76,"scale":-128})));
    assert!(wide.is_valid(&json!({"kind":"decimal256","precision":76,"scale":76})));
    assert!(!wide.is_valid(&json!({"kind":"decimal256","precision":10,"scale":12})));
    assert!(!wide.is_valid(&json!({"kind":"decimal256","precision":77,"scale":0})));
}

#[test]
fn documented_config_examples_parse() {
    parse_oo_yaml(&read("examples/config/oo.yaml")).expect("oo");
    parse_sources_yaml(&read("examples/config/sources.yaml")).expect("sources");
    parse_files_yaml(&read("examples/config/files.yaml")).expect("files");
    parse_roles_yaml(&read("examples/config/roles.yaml")).expect("roles");
    parse_agent_yaml(&read("examples/config/agent.yaml")).expect("agent");
    parse_hooks_yaml(&read("examples/config/hooks.yaml")).expect("hooks");
    parse_eval_suite_yaml(&read("examples/config/eval-customer-core.yaml")).expect("eval");
}

#[test]
fn documented_oos_examples_parse() {
    parse_ontology_yaml(&read("examples/schema/ontology.yaml")).expect("ontology");
    parse_dataset_yaml(&read("examples/schema/dataset-kh.yaml")).expect("dataset");
    parse_entity_yaml(&read("examples/schema/entity-customer.yaml")).expect("entity");
    parse_relationships_yaml(&read("examples/schema/relationships.yaml")).expect("relationships");
    parse_enum_yaml(&read("examples/schema/enum-ysk-zt.yaml")).expect("enum");
    parse_metrics_yaml(&read("examples/schema/metrics-finance.yaml")).expect("metrics");
    parse_glossary_yaml(&read("examples/schema/glossary.yaml")).expect("glossary");
    parse_examples_yaml(&read("examples/schema/examples-finance.yaml")).expect("examples");
    parse_calendar_yaml(&read("examples/schema/calendar.yaml")).expect("calendar");
    parse_resolution_yaml(&read("examples/schema/resolution-customer.yaml")).expect("resolution");
    parse_decisions_yaml(&read("examples/schema/decisions.yaml")).expect("decisions");
}

#[test]
fn checked_in_schemas_accept_the_same_examples() {
    let config = fs::read_to_string(repo().join("contracts/schema/config-v1.json")).unwrap();
    let config: serde_json::Value = serde_json::from_str(&config).unwrap();
    let validator = jsonschema::draft202012::options()
        .offline()
        .build(&config)
        .unwrap();
    for path in [
        "examples/config/oo.yaml",
        "examples/config/sources.yaml",
        "examples/config/files.yaml",
        "examples/config/roles.yaml",
        "examples/config/agent.yaml",
        "examples/config/hooks.yaml",
        "examples/config/eval-customer-core.yaml",
    ] {
        let value = parse_yaml_value(&read(path), "oo.config/v1").unwrap();
        let errors: Vec<_> = validator
            .iter_errors(&value)
            .map(|error| error.to_string())
            .collect();
        assert!(errors.is_empty(), "{path}: {errors:?}");
    }
}

#[test]
fn unknown_duplicate_depth_tag_and_version_fail() {
    let oo = read("examples/config/oo.yaml");
    let unknown = oo.replace("deployment:", "extra: 1\ndeployment:");
    assert!(
        parse_oo_yaml(&unknown)
            .unwrap_err()
            .items
            .iter()
            .any(|item| item.code.ends_with("/schema"))
    );

    let version = oo.replace("schema_version: config/v1", "schema_version: config/v0");
    let err = parse_oo_yaml(&version).unwrap_err();
    assert_eq!(err.items.len(), 1);
    assert!(err.items[0].code.ends_with("version.unsupported"));
    assert_eq!(err.items[0].path, "/schema_version");
    assert_eq!(err.items[0].message, "`schema_version` must be `config/v1`");
    assert!(!err.items[0].message.contains("config/v0"));

    let missing = oo.replacen("schema_version: config/v1\n", "", 1);
    let err = parse_oo_yaml(&missing).unwrap_err();
    assert_eq!(err.items.len(), 1);
    assert!(err.items[0].code.ends_with("version.missing"));

    let dup = parse_yaml_value("a: 1\na: 2\n", "oo.config/v1").unwrap_err();
    assert!(dup.items[0].code.ends_with("yaml.duplicate_key"));

    let tag = parse_yaml_value("note: !!str hello\n", "oo.config/v1").unwrap_err();
    assert!(tag.items[0].code.ends_with("yaml.tag"));
    let include = parse_yaml_value("note: !include secrets.yaml\n", "oo.config/v1").unwrap_err();
    assert!(include.items[0].code.ends_with("yaml.tag"));
    parse_yaml_value("note: \"hello!\"\n", "oo.config/v1").expect("quoted bang is data");

    let deep = format!("{}1{}", "{a:".repeat(40), "}".repeat(40));
    let err = parse_yaml_value(&deep, "oo.config/v1").unwrap_err();
    assert!(
        err.items[0].code.contains("yaml.budget.depth")
            || err.items[0].code.contains("yaml.syntax"),
        "{}",
        err.items[0].code
    );

    let tls = read("examples/config/sources.yaml").replace("tls: verify_full", "tls: disable");
    let err = parse_sources_yaml(&tls).unwrap_err();
    assert!(
        err.items
            .iter()
            .any(|item| item.code.ends_with("tls.insecure"))
    );

    let plain = read("examples/config/sources.yaml").replace("secret://erp_mysql", "hunter2");
    let err = parse_sources_yaml(&plain).unwrap_err();
    assert!(
        err.items
            .iter()
            .any(|item| item.code.ends_with("secret.plaintext"))
    );
}

#[test]
fn api_fixtures_match_their_schema_and_reject_numbers() {
    let error = read("contracts/fixtures/api/error-rate-limited.json");
    let envelope = oo_core::api::parse_api_json(&error, |item: &oo_core::api::ErrorEnvelope| {
        if item.consistent() {
            Vec::new()
        } else {
            vec![oo_core::diagnostic::Diagnostic::error(
                "oo.api/v1",
                "cross_field",
                "/error/retryable",
                "retryable flag does not match the code",
            )]
        }
    })
    .expect("error");
    assert!(envelope.consistent());
    assert_eq!(envelope.error.code().http_status(), 429);

    oo_core::api::parse_api_json(
        &read("contracts/fixtures/api/query-request.json"),
        |_: &oo_core::api::QueryRequest| Vec::new(),
    )
    .expect("request");
    oo_core::api::parse_api_json(
        &read("contracts/fixtures/api/query-response.json"),
        |_: &oo_core::api::QueryResponse| Vec::new(),
    )
    .expect("response");
    oo_core::api::parse_api_json(
        &read("contracts/fixtures/api/accepted.json"),
        |_: &oo_core::api::AcceptedOperation| Vec::new(),
    )
    .expect("accepted");
    oo_core::api::parse_api_json(
        &read("contracts/fixtures/api/page.json"),
        |_: &oo_core::api::PageRequest| Vec::new(),
    )
    .expect("page");
    oo_core::api::parse_api_json(
        &read("contracts/fixtures/api/export.json"),
        |_: &oo_core::api::ExportCompletion| Vec::new(),
    )
    .expect("export");

    let number = r#"{"sql":"select 1","format":"json","max_rows":1.5}"#;
    assert!(
        oo_core::api::parse_api_json(number, |_: &oo_core::api::QueryRequest| Vec::new()).is_err()
    );
    let cell = read("contracts/fixtures/api/query-response.json").replace("[true]", "[1]");
    assert!(
        oo_core::api::parse_api_json(&cell, |_: &oo_core::api::QueryResponse| Vec::new()).is_err()
    );

    assert_eq!(
        oo_core::api::idempotency_decision(None, "abc"),
        oo_core::api::IdempotencyDecision::FirstAccept
    );
    assert_eq!(
        oo_core::api::idempotency_decision(Some("abc"), "abc"),
        oo_core::api::IdempotencyDecision::Replay
    );
    assert_eq!(
        oo_core::api::idempotency_decision(Some("abc"), "def"),
        oo_core::api::IdempotencyDecision::Conflict
    );
}

#[test]
fn overlay_merges_by_key_and_keeps_stale_curated_values() {
    let generated = json!({
        "columns": [
            {"name": "a", "label": "generated-a"},
            {"name": "b", "label": "generated-b"}
        ],
        "caveats": ["old"],
        "description": "from generated"
    });
    let curated = json!({
        "columns": [
            {"name": "b", "label": "curated-b"},
            {"name": "c", "label": "curated-c", "status": "rejected"}
        ],
        "caveats": ["new"]
    });
    let pin = FactPin {
        path: "/columns/b/label".to_owned(),
        generated_fingerprint: HexDigest::new(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap(),
        recorded_fingerprint: HexDigest::new(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )
        .unwrap(),
    };
    let merged = merge_effective(&generated, &curated, &[pin]).unwrap();
    assert_eq!(
        merged.effective["columns"],
        json!([
            {"name": "a", "label": "generated-a"},
            {"name": "b", "label": "curated-b"}
        ])
    );
    assert_eq!(merged.removed.len(), 1);
    assert_eq!(merged.removed[0].path, "/columns/c");
    assert_eq!(merged.effective["caveats"], json!(["new"]));
    assert_eq!(merged.effective["description"], "from generated");
    assert_eq!(merged.stale.len(), 1);
    assert_eq!(merged.stale[0].path, "/columns/b/label");
    assert!(
        !merged
            .missing_pins
            .iter()
            .any(|path| path == "/columns/b/label")
    );
    assert!(merged.missing_pins.iter().any(|path| path == "/columns/c"));
    assert!(merged.missing_pins.iter().any(|path| path == "/caveats/0"));

    let empty_columns = json!({"columns": []});
    let kept = merge_effective(&generated, &empty_columns, &[]).unwrap();
    assert_eq!(kept.effective["columns"], generated["columns"]);
    assert!(
        merge_effective(
            &json!({"columns": [{"name": "a"}, {"name": "a"}]}),
            &json!({"columns": []}),
            &[]
        )
        .is_err()
    );
}
