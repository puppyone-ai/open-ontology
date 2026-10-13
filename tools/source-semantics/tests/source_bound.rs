//! Real Oracle dictionaries become role-pack flags. Hand booleans are not the contract.

use serde_json::{Value, json};
use source_semantics::compile_source_semantics;

const BATCH_DIGEST: &str = "59a1d30cfedb66d93fa8277124851ed6fcb3504c697688ddcf92dcadfcc289dc";
const WIDE_DIGEST: &str = "71eba817dac14915743a455dc02d55305d1cde2294a0391541bafc1fd347978c";
const TS_DIGEST: &str = "89ab99d0ae23b23f05435bbbd3818f736c8fb6ff30ab0ff16dbc3da59b8a4a04";
const OLD_BATCH_PACK: &str = "fe426fa5d4d510be49f9d7de57113a489b1ba176f0218f4a5723163505eb2d27";
const OLD_WIDE_PACK: &str = "4724a3379df0a409bd70203ba23b69fbb495cfe8567978d65bbc4ccd1c22853c";
const OLD_TS_PACK: &str = "962b09d45a58dd53fd05f7781a710f82f813cd573687119002f674caa0901470";

fn chain(relative: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/live-chain")
        .join(relative)
}

fn read_chain(relative: &str) -> String {
    std::fs::read_to_string(chain(relative)).unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

fn run(command: &Value) -> Result<Value, source_semantics::SemanticsError> {
    let text = compile_source_semantics(&serde_json::to_string(command).unwrap())?;
    Ok(serde_json::from_str(&text).unwrap())
}

fn bound_for(catalog: &Value, audit_relative: &str, bindings: &[(&str, &str)]) -> Value {
    let audit: Value = serde_json::from_str(&read_chain(audit_relative)).unwrap();
    assert!(audit.get("consistency_grade").is_none());
    assert!(audit.get("delete_detection").is_none());
    assert!(!audit.to_string().contains("\"S1\""));
    json!({
        "protocol": "oo.source-semantics.source-bound/v1",
        "engine": catalog["engine"],
        "source_id": catalog["source_id"],
        "source_generation": catalog["source_generation"],
        "target_generation": catalog["target_generation"],
        "run_id": catalog["run_id"],
        "schema_digest": catalog["schema_digest"],
        "type_digest": catalog["type_digest"],
        "config_digest": catalog["config_digest"],
        "input_digest": catalog["input_digest"],
        "banner": audit["banner"],
        "scn": audit["scn"],
        "registry_snapshot": audit["registry_snapshot"],
        "published_scn": audit["published_scn"],
        "dictionary": {
            "protocol": "oo.source-semantics.type-dictionary/v1",
            "columns": audit["dictionary_columns"]
        },
        "bindings": bindings.iter().map(|(sealed, dictionary)| json!({
            "sealed_column": sealed,
            "dictionary_column": dictionary
        })).collect::<Vec<_>>()
    })
}

fn command(catalog: &Value, identity: &str, dataset: &str, columns: &[(&str, &str)], example: &str, bound: Value) -> Value {
    let decisions: Vec<Value> = columns
        .iter()
        .map(|(name, semantic)| {
            json!({
                "dataset": dataset,
                "column": name,
                "pii": "none",
                "semantic_type": semantic,
                "reviewer": {"by": "ada.owner", "at": "2026-10-12"}
            })
        })
        .collect();
    let names: Vec<&str> = columns.iter().map(|(name, _)| *name).collect();
    json!({
        "protocol": "oo.source-semantics.command/v1",
        "catalog": catalog,
        "source_bound": bound,
        "target": {
            "family": "oracle",
            "product": "oracle",
            "version": "23.26.3.0.0",
            "capture": "sealed_import",
            "source_kind": "oracle_table",
            "cursor": null,
            "chunk": {"key_kind": "integer", "binary_order": null, "hash_chunking": false},
            "flags": {
                "synthetic_binlog": false,
                "append_only": false,
                "eventual_consistency": false,
                "default_for_missing": false,
                "accent_insensitive": false,
                "soft_delete_hint": false
            },
            "columns": [],
            "constraints": []
        },
        "binding": {
            "settings": json!({
                "project": "batch",
                "ontology_version": "ont-1",
                "catalog_version": identity,
                "policy": {"id": "pol-batch", "version": "1"},
                "knowledge": {"id": "know-batch", "version": "1"},
                "ontology": {
                    "spec_version": "oos/v1",
                    "name": "batch",
                    "locale": "en-US",
                    "include_schemas": ["batch"],
                    "default_tier_preference": ["raw"],
                    "publish": {"include_unreviewed": []},
                    "classification": {"auto_accept": "rule_high_confidence"},
                    "lifecycle": {"deprecation_min_releases": 2},
                    "compile": {
                        "per_role": true,
                        "budgets": {"readme_tokens": 2000, "table_file_tokens": 4000},
                        "sample_values": {"default": "masked", "max_per_column": 0}
                    }
                }
            }),
            "decisions": decisions,
            "visibility": {
                "protocol": "oo.builder.role-visibility/v1",
                "project": "batch",
                "principal": "ada",
                "role": "oo_r_clerk",
                "ontology_version": "ont-1",
                "catalog_version": identity,
                "policy": {"id": "pol-batch", "version": "1"},
                "knowledge": {"id": "know-batch", "version": "1"},
                "issued_for_compiler": "oo-builder/role-pack/v1",
                "datasets": [{
                    "dataset": dataset,
                    "row_security": "none",
                    "global_stats": "denied",
                    "columns": names
                }],
                "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
            },
            "examples": [
                {
                    "id": "present",
                    "question": "present rows",
                    "sql": format!("select {example} from {dataset} where {example} is not null"),
                    "uses": [format!("dataset:{dataset}")],
                    "verified_by": "pg-gateway",
                    "verified_at": "2026-10-12"
                },
                {
                    "id": "source-nvl",
                    "question": "skipped",
                    "sql": format!("select NVL({example}, {example}) from {dataset}"),
                    "verified_by": "pg-gateway",
                    "verified_at": "2026-10-12"
                }
            ]
        }
    })
}

fn pack_page(result: &Value, dataset: &str) -> String {
    result["pack"]["pages"][&format!("tables/{dataset}.md")]
        .as_str()
        .unwrap()
        .to_string()
}

fn assert_observed_absent(result: &Value) {
    let profile = &result["source_semantics_profile"];
    assert_eq!(profile["protocol"], "oo.builder.source-semantics-profile/v1");
    assert_eq!(profile["compiler"], "derive_source_bound");
    assert_eq!(profile["version"], "23.26.3.0.0");
    assert_eq!(profile["capture"], "sealed_import");
    assert!(profile["consistency_grade"].is_null());
    assert_eq!(profile["consistency_grade_observed"], false);
    assert!(profile["delete_detection"].is_null());
    assert_eq!(profile["delete_detection_observed"], false);
    assert_eq!(profile["scn"], "6395124");
    assert_eq!(profile["registry_snapshot"], "oo-dl01w1-04-scn-6395124");
    assert_eq!(result["freshness_projection"]["gateway_get_freshness"], "not_called");
    assert_eq!(result["freshness_projection"]["consistency_grade_observed"], false);
    assert_eq!(result["freshness_projection"]["consistency_source"], "source_bound_observation");
    let effective: Value = serde_json::from_str(result["role_pack_inputs"]["effective_json"].as_str().unwrap()).unwrap();
    let semantics = &effective["datasets"][0]["source_semantics"];
    assert!(semantics.get("grade").is_none());
    assert!(semantics.get("delete_detection").is_none());
    assert_eq!(semantics["version"], "23.26.3.0.0");
    assert_eq!(semantics["capture"], "sealed_import");
}

fn profile_column<'a>(result: &'a Value, name: &str) -> &'a Value {
    result["source_semantics_profile"]["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|column| column["column"] == name)
        .unwrap_or_else(|| panic!("missing profile column {name}"))
}

#[test]
fn historical_role_packs_stay_byte_identical_and_do_not_gain_flags() {
    for (relative, digest, len) in [
        ("builder/out-fresh/role-pack/pack.json", OLD_BATCH_PACK, 5142),
        ("builder-wide/out-fresh/role-pack/pack.json", OLD_WIDE_PACK, 4964),
        ("builder-ts/out-fresh/role-pack/pack.json", OLD_TS_PACK, 4996),
    ] {
        let bytes = std::fs::read(chain(relative)).unwrap();
        assert_eq!(bytes.len(), len, "{relative}");
        assert_eq!(oo_core::api::payload_sha256(&bytes), digest, "{relative}");
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains("empty_string_is_null"), "{relative}");
        assert!(!text.contains("unconstrained_number"), "{relative}");
        assert!(!text.contains("precision_loss"), "{relative}");
    }
}

#[test]
fn oracle_dictionaries_compile_three_flags_into_role_pack_pages() {
    let batch: Value = serde_json::from_str(&read_chain("catalog-batch/catalog.json")).unwrap();
    assert_eq!(batch["schema_digest"], BATCH_DIGEST);
    let draft = oo_builder::compile_sealed_import_catalog(&batch.to_string()).unwrap();
    assert_eq!(draft.schema_digest, BATCH_DIGEST);
    for name in ["c6", "v2", "d", "n16", "id"] {
        let column = draft.columns.iter().find(|column| column.name == name).unwrap();
        assert!(column.caveat.is_none(), "{name}");
        assert!(!column.precision_loss, "{name}");
    }
    let packed = command(
        &batch,
        &draft.identity_hash,
        "batch.t_batch",
        &[
            ("id", "identifier"),
            ("n16", "quantity"),
            ("n1", "quantity"),
            ("d", "event_time"),
            ("c6", "code"),
            ("v2", "free_text"),
            ("r", "code"),
        ],
        "c6",
        bound_for(
            &batch,
            "identity-67/batch/audit/readonly-audit.json",
            &[
                ("id", "ID"),
                ("n16", "N16"),
                ("n1", "N1"),
                ("d", "D"),
                ("c6", "C6"),
                ("v2", "V2"),
                ("r", "R"),
            ],
        ),
    );
    let result = run(&packed).expect("batch pack");
    assert_eq!(result["status"], "packed");
    assert_eq!(result["catalog_proof"]["schema_digest"], BATCH_DIGEST);
    assert_eq!(
        result["builder_calls"],
        json!([
            "compile_sealed_import_catalog",
            "derive_source_bound",
            "compile_reviewed_sealed_import",
            "compile_role_pack"
        ])
    );
    assert_observed_absent(&result);
    assert_eq!(profile_column(&result, "c6")["caveats"], json!(["empty_string_is_null"]));
    assert!(profile_column(&result, "c6")["native_note"].is_null());
    assert_eq!(profile_column(&result, "v2")["caveats"], json!(["empty_string_is_null"]));
    assert!(profile_column(&result, "d")["caveats"].as_array().unwrap().is_empty());
    assert!(profile_column(&result, "d")["native_note"].is_null());
    assert!(profile_column(&result, "id")["caveats"].as_array().unwrap().is_empty());
    let page = pack_page(&result, "batch.t_batch");
    assert!(page.contains("| `c6` |  | bpchar | code | none |  |"), "{page}");
    assert!(page.contains("caveats: empty_string_is_null, postgresql_target_storage"), "{page}");
    assert!(page.contains("version: 23.26.3.0.0"), "{page}");
    assert!(page.contains("capture: sealed_import"), "{page}");
    assert!(!page.contains("consistency_grade:"));
    assert!(!page.contains("delete_detection:"));
    assert!(!page.contains("oracle_date_includes_time"));
    assert!(!page.contains("offset_lost"));
    assert!(!page.contains("time_ambiguous"));
    assert!(!page.contains("unconstrained_number"));
    assert!(!page.contains("S1"));
    let effective: Value = serde_json::from_str(result["role_pack_inputs"]["effective_json"].as_str().unwrap()).unwrap();
    let c6 = effective["datasets"][0]["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|column| column["name"] == "c6")
        .unwrap();
    assert_eq!(c6["data_type"], "bpchar");
    assert!(c6.get("caveats").is_none() || c6["caveats"].as_array().unwrap().is_empty());

    let wide: Value = serde_json::from_str(&read_chain("catalog-wide/catalog.json")).unwrap();
    let wide_draft = oo_builder::compile_sealed_import_catalog(&wide.to_string()).unwrap();
    assert_eq!(wide_draft.schema_digest, WIDE_DIGEST);
    let number = wide_draft.columns.iter().find(|column| column.name == "n_free_text").unwrap();
    assert!(!number.precision_loss);
    assert!(number.caveat.is_none());
    let wide_result = run(&command(
        &wide,
        &wide_draft.identity_hash,
        "batch.t_wide_text",
        &[("id", "identifier"), ("n_free_text", "free_text")],
        "n_free_text",
        bound_for(&wide, "identity-67/wide/audit/readonly-audit.json", &[("id", "ID"), ("n_free_text", "N_FREE")]),
    ))
    .expect("wide pack");
    assert_eq!(wide_result["status"], "packed");
    assert_eq!(wide_result["catalog_proof"]["schema_digest"], WIDE_DIGEST);
    assert_observed_absent(&wide_result);
    assert!(profile_column(&wide_result, "n_free_text")["caveats"].as_array().unwrap().is_empty());
    assert_eq!(profile_column(&wide_result, "n_free_text")["native_note"], "unconstrained_number");
    let wide_page = pack_page(&wide_result, "batch.t_wide_text");
    assert!(wide_page.contains("native: n_free_text unconstrained_number"), "{wide_page}");
    assert!(!wide_page.contains("precision_loss"));
    assert!(!wide_page.contains("empty_string_is_null"));
    assert!(!wide_page.contains("consistency_grade:"));

    let ts: Value = serde_json::from_str(&read_chain("catalog-ts/catalog.json")).unwrap();
    let ts_draft = oo_builder::compile_sealed_import_catalog(&ts.to_string()).unwrap();
    assert_eq!(ts_draft.schema_digest, TS_DIGEST);
    for name in ["ts_text", "tstz_text"] {
        let column = ts_draft.columns.iter().find(|column| column.name == name).unwrap();
        assert!(!column.precision_loss, "{name}");
        assert!(column.caveat.is_none(), "{name}");
    }
    let ts_result = run(&command(
        &ts,
        &ts_draft.identity_hash,
        "batch.t_ts_text",
        &[("id", "identifier"), ("ts_text", "event_time"), ("tstz_text", "event_time")],
        "ts_text",
        bound_for(
            &ts,
            "identity-67/ts/audit/readonly-audit.json",
            &[("id", "ID"), ("ts_text", "TS"), ("tstz_text", "TSTZ")],
        ),
    ))
    .expect("ts pack");
    assert_eq!(ts_result["status"], "packed");
    assert_eq!(ts_result["catalog_proof"]["schema_digest"], TS_DIGEST);
    assert_observed_absent(&ts_result);
    assert_eq!(profile_column(&ts_result, "ts_text")["caveats"], json!(["precision_loss"]));
    assert_eq!(profile_column(&ts_result, "tstz_text")["caveats"], json!(["precision_loss"]));
    assert!(profile_column(&ts_result, "tstz_text")["native_note"].is_null());
    let ts_page = pack_page(&ts_result, "batch.t_ts_text");
    assert!(ts_page.contains("caveats: postgresql_target_storage, precision_loss"), "{ts_page}");
    assert!(!ts_page.contains("offset_lost"));
    assert!(!ts_page.contains("time_ambiguous"));
    assert!(!ts_page.contains("empty_string_is_null"));
    assert!(!ts_page.contains("oracle_date_includes_time"));
}

#[test]
fn synthetic_observation_passes_s1_and_reconcile_and_is_not_the_audit() {
    let batch: Value = serde_json::from_str(&read_chain("catalog-batch/catalog.json")).unwrap();
    let draft = oo_builder::compile_sealed_import_catalog(&batch.to_string()).unwrap();
    let mut bound = bound_for(
        &batch,
        "identity-67/batch/audit/readonly-audit.json",
        &[
            ("id", "ID"),
            ("n16", "N16"),
            ("n1", "N1"),
            ("d", "D"),
            ("c6", "C6"),
            ("v2", "V2"),
            ("r", "R"),
        ],
    );
    bound["consistency_grade"] = json!("S1");
    bound["delete_detection"] = json!("reconcile");
    let result = run(&command(
        &batch,
        &draft.identity_hash,
        "batch.t_batch",
        &[("c6", "code"), ("v2", "free_text")],
        "c6",
        bound,
    ))
    .expect("synthetic");
    assert_eq!(result["status"], "packed");
    let page = pack_page(&result, "batch.t_batch");
    assert!(page.contains("consistency_grade: S1"), "{page}");
    assert!(page.contains("delete_detection: reconcile"), "{page}");
    assert_eq!(result["source_semantics_profile"]["consistency_grade"], "S1");
    assert_eq!(result["source_semantics_profile"]["consistency_grade_observed"], true);
    assert_eq!(result["source_semantics_profile"]["delete_detection"], "reconcile");
    assert_eq!(result["source_semantics_profile"]["delete_detection_observed"], true);
    let audit = read_chain("identity-67/batch/audit/readonly-audit.json");
    assert!(!audit.contains("\"S1\""));
    assert!(!audit.contains("reconcile"));
}

#[test]
fn cross_source_digest_column_token_and_fidelity_are_refused() {
    let batch_text = read_chain("catalog-batch/catalog.json");
    let batch: Value = serde_json::from_str(&batch_text).unwrap();
    let draft = oo_builder::compile_sealed_import_catalog(&batch_text).unwrap();
    let bindings = [
        ("id", "ID"),
        ("n16", "N16"),
        ("n1", "N1"),
        ("d", "D"),
        ("c6", "C6"),
        ("v2", "V2"),
        ("r", "R"),
    ];
    let base = command(
        &batch,
        &draft.identity_hash,
        "batch.t_batch",
        &[("c6", "code")],
        "c6",
        bound_for(&batch, "identity-67/batch/audit/readonly-audit.json", &bindings),
    );

    let mut cross = base.clone();
    cross["source_bound"]["source_id"] = json!("oo04_t_wide_text");
    let err = run(&cross).expect_err("cross source");
    assert!(err.code.ends_with("/cross_source"), "{err}");

    let mut run_mismatch = base.clone();
    run_mismatch["source_bound"]["run_id"] = json!("7ac85454-5082-429a-a94c-b3cca4833113");
    let err = run(&run_mismatch).expect_err("cross run");
    assert!(err.code.ends_with("/cross_run"), "{err}");

    let mut digest = base.clone();
    digest["source_bound"]["schema_digest"] = json!(WIDE_DIGEST);
    let err = run(&digest).expect_err("digest");
    assert!(err.code.ends_with("/digest.mismatch"), "{err}");
    assert!(err.code.starts_with("oo.source-semantics.derived/v1/"), "{err}");

    let mut columns = base.clone();
    columns["source_bound"]["bindings"][0]["sealed_column"] = json!("c6");
    columns["source_bound"]["bindings"][4]["sealed_column"] = json!("id");
    let err = run(&columns).expect_err("column");
    assert!(err.code.ends_with("/column.mismatch"), "{err}");

    let mut injected = base.clone();
    injected["source_bound"]["dictionary"]["columns"][4] = json!("C6|CHAR empty_string_is_null|-|-|6");
    let err = run(&injected).expect_err("injected");
    assert!(err.code.ends_with("/token.rejected"), "{err}");

    let mut unknown = base.clone();
    unknown["source_bound"]["caveats"] = json!(["empty_string_is_null"]);
    let err = run(&unknown).expect_err("caveats field");
    assert!(err.code.ends_with("/field.unknown"), "{err}");

    let mut asserted = base.clone();
    asserted["asserted_semantics"] = json!({
        "protocol": "oo.source-semantics.asserted/v1",
        "columns": [{"column": "c6", "caveats": ["precision_loss"]}]
    });
    let err = run(&asserted).expect_err("asserted c6");
    assert!(err.code.ends_with("/token.mismatch"), "{err}");

    let ts: Value = serde_json::from_str(&read_chain("catalog-ts/catalog.json")).unwrap();
    let ts_draft = oo_builder::compile_sealed_import_catalog(&ts.to_string()).unwrap();
    let mut offset = command(
        &ts,
        &ts_draft.identity_hash,
        "batch.t_ts_text",
        &[("ts_text", "event_time"), ("tstz_text", "event_time")],
        "tstz_text",
        bound_for(
            &ts,
            "identity-67/ts/audit/readonly-audit.json",
            &[("id", "ID"), ("ts_text", "TS"), ("tstz_text", "TSTZ")],
        ),
    );
    offset["asserted_semantics"] = json!({
        "protocol": "oo.source-semantics.asserted/v1",
        "columns": [{"column": "tstz_text", "caveats": ["offset_lost"]}]
    });
    let err = run(&offset).expect_err("offset");
    assert!(err.code.ends_with("/token.mismatch"), "{err}");

    let mut foreign = base.clone();
    foreign["asserted_semantics"] = json!({
        "protocol": "oo.source-semantics.asserted/v1",
        "columns": [{"column": "c6", "caveats": ["not_a_real_token"]}]
    });
    let err = run(&foreign).expect_err("foreign token");
    assert!(err.code.ends_with("/token.rejected"), "{err}");

    let mut stamped = base.clone();
    stamped["target"]["source_consistency"] = json!("S1");
    let err = run(&stamped).expect_err("stamped grade");
    assert!(err.code.ends_with("/fidelity.contradiction"), "{err}");

    let mut dated = base.clone();
    dated["target"]["columns"] = json!([{
        "schema": "batch",
        "table": "t_batch",
        "name": "d",
        "native_type": "date"
    }]);
    let err = run(&dated).expect_err("date native");
    assert!(err.code.ends_with("/type.ambiguous"), "{err}");
    assert!(!err.message.contains("oracle_date_includes_time"));

    let mut stored = batch.clone();
    stored["tables"][0]["columns"][0]["caveat"] = json!("tinyint1_as_int2");
    stored["schema_digest"] = json!(schema_digest(&stored["tables"]));
    assert_ne!(stored["schema_digest"], BATCH_DIGEST);
    let stored_draft = oo_builder::compile_sealed_import_catalog(&stored.to_string()).expect("storage caveat compiles");
    let mut stored_bound = bound_for(&stored, "identity-67/batch/audit/readonly-audit.json", &bindings);
    stored_bound["schema_digest"] = stored["schema_digest"].clone();
    let stored_command = command(
        &stored,
        &stored_draft.identity_hash,
        "batch.t_batch",
        &[("id", "identifier")],
        "id",
        stored_bound,
    );
    let err = run(&stored_command).expect_err("storage caveat");
    assert!(err.code.ends_with("/fidelity.contradiction"), "{err}");

    let mut bare = base.clone();
    bare.as_object_mut().unwrap().remove("source_bound");
    bare["target"].as_object_mut().unwrap().remove("source_consistency");
    let err = run(&bare).expect_err("missing grade");
    assert!(err.code.ends_with("/field.missing"), "{err}");
}

fn schema_digest(tables: &Value) -> String {
    let mut out = String::new();
    for table in tables.as_array().unwrap() {
        oo_core::sealed_import::push_schema_table(
            &mut out,
            table["logical_schema"].as_str().unwrap(),
            table["logical_table"].as_str().unwrap(),
        );
        for column in table["columns"].as_array().unwrap() {
            oo_core::sealed_import::push_schema_column(
                &mut out,
                column["logical_name"].as_str().unwrap(),
                column["oid"].as_i64().unwrap(),
                i32::try_from(column["typmod"].as_i64().unwrap()).unwrap(),
                column["caveat"].as_str(),
            );
        }
    }
    oo_core::api::payload_sha256(out.as_bytes())
}
