//! Private PostgreSQL proof. The pack comes from `oo_builder`, not a second compiler.

use std::env;
use std::fs;
use std::time::Duration;

use oo_builder::{
    REVIEWED_EFFECTIVE_COMMAND_PROTOCOL, compile_captured_pg_catalog, compile_reviewed_effective,
};
use serde_json::{Value, json};
use tokio_postgres::config::SslMode;

use crate::seal::Expectation;
use crate::sha256_hex;
use crate::{
    CheckpointRequest, Endpoint, GATE_IDS, GateRequest, InstallRequest, PlanRequest, Receipt,
    RecoverRequest, StageRequest, StoreError, activate, checkpoint, commit_activation,
    compile_seal, dispatch, enter_applying, gate_record, install, plan, recover, stage, status,
};

const PROJECT: &str = "shop";
const POLICY_ID: &str = "pol-shop";
const POLICY_VERSION: &str = "1";
const KNOWLEDGE_ID: &str = "know-shop";
const KNOWLEDGE_VERSION: &str = "1";
const CATALOG_VERSION: &str = "cat-1";

struct Pg {
    endpoint: Endpoint,
    password: String,
    reader_password: String,
    ca: Vec<u8>,
    config_path: String,
}

struct Sealed {
    id: &'static str,
    version: &'static str,
    source: String,
    pack: String,
    manifest: String,
}

fn env_var(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{name} is required for the private postgres test"))
}

fn secret(path: &str) -> String {
    let mut text = fs::read_to_string(path).unwrap_or_else(|_| panic!("secret file {path}"));
    if text.ends_with('\n') {
        text.pop();
    }
    assert!(!text.is_empty());
    text
}

fn load_pg() -> Pg {
    let port = env_var("OO_A359_PG_PORT").parse().expect("port");
    let password_file = env_var("OO_A359_PASSWORD_FILE");
    let ca_file = env_var("OO_A359_CA_FILE");
    let endpoint = Endpoint {
        host: env_var("OO_A359_PG_HOST"),
        port,
        database: env_var("OO_A359_PG_DATABASE"),
    };
    let config_path = env::temp_dir().join("oo-a359-release-config.json");
    let config = json!({
        "host": endpoint.host,
        "port": port,
        "database": endpoint.database,
        "user": "oo_release_admin",
        "password_file": password_file,
        "ca_file": ca_file,
    });
    fs::write(&config_path, serde_json::to_vec(&config).expect("config")).expect("config write");
    Pg {
        endpoint,
        password: secret(&password_file),
        reader_password: secret(&env_var("OO_A359_READER_PASSWORD_FILE")),
        ca: fs::read(&ca_file).expect("ca"),
        config_path: config_path.display().to_string(),
    }
}

fn column_preimage(attnum: i32, name: &str, type_oid: i64) -> String {
    format!(
        r#"{{"attnum":{attnum},"name":"{name}","type_oid":{type_oid},"typmod":-1,"not_null":true,"collation_oid":0,"typtype":"b","base_type_oid":null,"identity":"","generated":"","default_sha256":null,"enum_labels":[],"domain_checks":[]}}"#
    )
}

fn catalog_text_for(tables: &[&str]) -> String {
    let columns = format!(
        "{},{}",
        column_preimage(1, "id", 23),
        column_preimage(2, "contact", 25)
    );
    let preimage = format!(
        r#"{{"tables":[{{"schema":"public","name":"customer","relkind":"r","replica_identity":"d","rls":false,"force_rls":false,"columns":[{columns}],"primary_key":[{{"attnum":1,"name":"id"}}],"primary_key_nulls_not_distinct":false,"unique_keys":[],"foreign_keys":[],"policies":[]}}]}}"#
    );
    let table_preimage = preimage
        .strip_prefix("{\"tables\":[")
        .and_then(|s| s.strip_suffix("]}"))
        .expect("preimage table");
    let table_preimages = tables
        .iter()
        .map(|name| {
            table_preimage.replace("\"name\":\"customer\"", &format!("\"name\":\"{name}\""))
        })
        .collect::<Vec<_>>()
        .join(",");
    let preimage = format!("{{\"tables\":[{table_preimages}]}}");
    let schema_hash = sha256_hex(preimage.as_bytes());
    let mut captured = json!({
        "protocol": "oo.source.pgsql/v1",
        "core_contract": "oo.config/v1",
        "source_id": "shop_src",
        "database": "shop",
        "captured_at": "2026-10-04T12:00:00.000000Z",
        "schema_hash": schema_hash,
        "hash_algorithm": "pg-catalog-hash/v1",
        "ident_algorithm": "pgident/v1",
        "tables": [{
            "schema": "public",
            "schema_ident": "public",
            "name": "customer",
            "name_ident": "customer",
            "relkind": "r",
            "replica_identity": "d",
            "rls_enabled": false,
            "rls_forced": false,
            "owner": "owner_role_do_not_leak",
            "comment": Value::Null,
            "columns": [column_json(1, "id", 23, "int4"), column_json(2, "contact", 25, "text")],
            "primary_key": [{"attnum": 1, "name": "id"}],
            "primary_key_nulls_not_distinct": false,
            "unique_keys": [],
            "foreign_keys": [],
            "policies": [],
            "privileges": [{"grantee": "owner_role_do_not_leak", "privilege": "SELECT", "grantable": false}]
        }],
        "type_conversion": "unsupported"
    });
    let table = captured["tables"][0].clone();
    captured["tables"] = json!(
        tables
            .iter()
            .map(|name| {
                let mut t = table.clone();
                t["name"] = json!(name);
                t["name_ident"] = json!(name);
                t
            })
            .collect::<Vec<_>>()
    );
    serde_json::to_string(&captured).expect("catalog")
}

fn column_json(attnum: i32, name: &str, type_oid: i64, type_name: &str) -> Value {
    json!({
        "attnum": attnum,
        "name": name,
        "name_ident": name,
        "type_oid": type_oid,
        "type_schema": "pg_catalog",
        "type_name": type_name,
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
        "type_support": {"converted": false, "reason": "known_builtin_not_converted"},
        "current_privileges": {"select": true, "insert": false, "update": false}
    })
}

pub(crate) fn shop(columns: &[&str]) -> (Value, Value) {
    shop_tables(columns, &["customer"])
}
pub(crate) fn shop_tables(columns: &[&str], tables: &[&str]) -> (Value, Value) {
    let catalog = catalog_text_for(tables);
    let compiled = compile_captured_pg_catalog(&catalog, None, None).expect("captured catalog");
    let draft: Value = serde_json::from_str(&compiled.draft).expect("draft");
    let schema = draft["provenance"]["schema_hash"]
        .as_str()
        .expect("schema")
        .to_owned();
    let mut decisions = Vec::new();
    for table in tables {
        for column in ["id", "contact"] {
            let item_id = format!("column/public.{table}/{column}");
            let fingerprint = draft["review_queue"]
                .as_array()
                .expect("queue")
                .iter()
                .find(|item| item["item_id"] == item_id && item["kind"] == "structural_column")
                .expect("column fact")["fact_fingerprint"]
                .as_str()
                .expect("fingerprint")
                .to_owned();
            decisions.push(json!({
                "dataset": format!("public.{table}"),
                "column": column,
                "schema_hash": schema,
                "fact_fingerprint": fingerprint,
                "pii": "none",
                "semantic_type": "identifier",
                "reviewer": {"by": "ada.owner", "at": "2026-10-06"}
            }));
        }
    }
    let artifact = json!({
        "draft": serde_json::from_str::<Value>(&compiled.draft).unwrap(),
        "evidence_envelope": serde_json::from_str::<Value>(&compiled.evidence_envelope).unwrap(),
        "protocol": "oo.builder.captured-pg-draft-command/v1",
        "wrapper": serde_json::from_str::<Value>(&compiled.wrapper).unwrap(),
    });
    let command = json!({
        "protocol": REVIEWED_EFFECTIVE_COMMAND_PROTOCOL,
        "artifact": artifact,
        "settings": {
            "project": PROJECT,
            "ontology_version": "ont-1",
            "catalog_version": CATALOG_VERSION,
            "policy": {"id": POLICY_ID, "version": POLICY_VERSION},
            "knowledge": {"id": KNOWLEDGE_ID, "version": KNOWLEDGE_VERSION},
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
        },
        "decisions": decisions,
    });
    let reviewed = compile_reviewed_effective(&serde_json::to_string(&command).expect("command"))
        .expect("reviewed effective");
    let reviewed: Value = serde_json::from_str(&reviewed).expect("reviewed json");
    let visibility = json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": PROJECT,
        "principal": "ada",
        "role": "oo_r_clerk",
        "ontology_version": "ont-1",
        "catalog_version": CATALOG_VERSION,
        "policy": {"id": POLICY_ID, "version": POLICY_VERSION},
        "knowledge": {"id": KNOWLEDGE_ID, "version": KNOWLEDGE_VERSION},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": tables.iter().map(|table|json!({"dataset":format!("public.{table}"),"row_security":"none","global_stats":"denied","columns":columns})).collect::<Vec<_>>(),
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    });
    (reviewed["effective_oos"].clone(), visibility)
}

#[test]
fn compiler_seal_is_the_role_pack_and_not_a_production_certificate() {
    let (effective, visibility) = shop(&["contact", "id"]);
    let (same, narrow) = shop(&["contact"]);
    assert_eq!(effective, same);
    assert_ne!(visibility, narrow);
    let seal = compile_seal(
        &effective,
        &visibility,
        &Expectation {
            project_id: PROJECT,
            candidate_id: "cand-unit",
            version: "ver-unit",
            base_version: None,
            policy_id: POLICY_ID,
            policy_version: POLICY_VERSION,
            knowledge_id: KNOWLEDGE_ID,
            knowledge_version: KNOWLEDGE_VERSION,
            catalog_version: CATALOG_VERSION,
        },
    )
    .expect("seal");
    assert_eq!(seal.compiler, "oo-builder/role-pack/v1");
    assert_eq!(seal.role, "oo_r_clerk");
    assert!(seal.files.iter().any(|file| file.path == "manifest.json"));
    assert!(
        seal.files
            .iter()
            .all(|file| !file.path.starts_with('/') && !file.path.contains(".."))
    );
    compile_seal(
        &effective,
        &narrow,
        &Expectation {
            project_id: PROJECT,
            candidate_id: "cand-unit",
            version: "ver-unit",
            base_version: None,
            policy_id: POLICY_ID,
            policy_version: POLICY_VERSION,
            knowledge_id: KNOWLEDGE_ID,
            knowledge_version: KNOWLEDGE_VERSION,
            catalog_version: CATALOG_VERSION,
        },
    )
    .expect("narrow seal");
}

fn flags(receipt: &Receipt) {
    let body = &receipt.body;
    for key in [
        "full_sp08_accepted",
        "public_deployment_claimed",
        "customer_ownership_claimed",
        "standby_configured",
        "production_ready_certificate",
        "owner_authenticated",
        "credential_verified",
        "integrity_verified",
        "customer_gate_verified",
        "runsc_verified",
    ] {
        assert_eq!(body[key], false, "{key} in {body}");
    }
    assert_eq!(
        body["test_mode_cannot_produce_production_certificate"],
        true
    );
    assert_eq!(body["contract"], "oo.release-coordinator/v1");
}

fn ok(receipt: Receipt, outcome: &str) -> Receipt {
    flags(&receipt);
    assert_eq!(receipt.code, 0, "{:?}", receipt.body);
    assert_eq!(receipt.body["outcome"], outcome, "{:?}", receipt.body);
    receipt
}

fn refused(receipt: Receipt, reason: &str) -> Receipt {
    flags(&receipt);
    assert_eq!(receipt.code, 2, "{:?}", receipt.body);
    assert_eq!(receipt.body["reason"], reason, "{:?}", receipt.body);
    assert_eq!(receipt.body["certificate_class"], "not_issued");
    receipt
}

fn covers() -> Vec<String> {
    GATE_IDS.iter().map(|gate| (*gate).to_owned()).collect()
}

#[tokio::test]
async fn private_postgres_activates_only_a_test_scope_candidate() {
    let pg = load_pg();
    let (wide, wide_visibility) = shop(&["contact", "id"]);
    let (_same, narrow_visibility) = shop(&["contact"]);
    let names = covers();
    let installed = ok(
        install(&install_request(&pg, "fix-shop", &names))
            .await
            .expect("install"),
        "installed",
    );
    let authority = installed.body["authority_statement_sha256"]
        .as_str()
        .expect("authority")
        .to_owned();
    let again = ok(
        install(&install_request(&pg, "fix-shop", &names))
            .await
            .expect("replay install"),
        "replay",
    );
    assert_eq!(again.body["authority_statement_sha256"], authority);
    let drift = refused(
        install(&install_request(&pg, "other-auth", &names))
            .await
            .expect("drift"),
        "install_drift",
    );
    assert_eq!(drift.body["database_used"], true);

    refused(
        stage_at(
            &pg,
            "cand-x",
            "ver-x",
            Some("nope"),
            &wide,
            &wide_visibility,
        )
        .await
        .expect("base"),
        "base_mismatch",
    );
    refused(
        status_of(&pg, "cand-x", &"ab".repeat(32))
            .await
            .expect("absent"),
        "activation",
    );

    let sealed_a = stage_new(&pg, "cand-a", "ver-a", None, &wide, &wide_visibility).await;
    let replay = ok(
        stage_at(&pg, "cand-a", "ver-a", None, &wide, &wide_visibility)
            .await
            .expect("replay stage"),
        "replay",
    );
    assert_eq!(replay.body["source_hash"], sealed_a.source);
    let sealed_b = stage_new(&pg, "cand-b", "ver-b", None, &wide, &wide_visibility).await;
    let sealed_c = stage_new(&pg, "cand-c", "ver-c", None, &wide, &wide_visibility).await;
    let sealed_d = stage_new(&pg, "cand-d", "ver-d", None, &wide, &wide_visibility).await;
    let sealed_f = stage_new(&pg, "cand-f", "ver-f", None, &wide, &wide_visibility).await;
    assert_ne!(sealed_a.source, sealed_d.source);

    arm(&pg, &sealed_a, &authority, true).await;
    let produced = production_blocked(&pg, &sealed_a).await;
    assert!(
        produced.body["reasons"]
            .as_array()
            .expect("reasons")
            .iter()
            .any(|reason| reason == "credential_missing")
    );
    assert!(
        produced.body["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "fence_not_production_admissible")
    );
    assert_eq!(produced.body["production_outcome"], "production_blocked");
    let blocked_activate = activate(&plan_request(&pg, &sealed_a, "production", None))
        .await
        .expect("production activate");
    refused(blocked_activate, "production_blocked");
    let before = status_of(&pg, sealed_a.id, &sealed_a.source)
        .await
        .expect("status");
    ok(before, "status");
    let before = status_of(&pg, sealed_a.id, &sealed_a.source)
        .await
        .expect("status 2");
    assert_eq!(before.body["state"], "candidate_built");
    assert!(before.body["pointer_version"].is_null());

    let planned_ready = ok(
        plan(&plan_request(&pg, &sealed_a, "test_scope", None))
            .await
            .expect("plan a"),
        "planned",
    );
    assert_eq!(planned_ready.body["state"], "gates_passed");
    let applying = ok(
        enter_applying(&plan_request(&pg, &sealed_a, "test_scope", None))
            .await
            .expect("enter"),
        "applying",
    );
    assert_eq!(applying.body["half_published"], false);
    assert!(applying.body["pointer_version"].is_null());
    let mid = ok(
        status_of(&pg, sealed_a.id, &sealed_a.source)
            .await
            .expect("mid"),
        "status",
    );
    assert_eq!(mid.body["state"], "applying");
    assert_eq!(mid.body["half_published"], false);
    assert!(mid.body["pointer_version"].is_null());
    let recovered = ok(
        recover(&recover_request(&pg, "recover", sealed_a.id, None))
            .await
            .expect("recover"),
        "recovered",
    );
    assert_eq!(recovered.body["state"], "failed");
    assert!(recovered.body["pointer_version"].is_null());
    let recover_replay = ok(
        recover(&recover_request(&pg, "recover", sealed_a.id, None))
            .await
            .expect("recover replay"),
        "replay",
    );
    assert_eq!(recover_replay.body["state"], "failed");
    let planned_a = ok(
        plan(&plan_request(&pg, &sealed_a, "test_scope", None))
            .await
            .expect("replan"),
        "planned",
    );
    assert_eq!(planned_a.body["state"], "gates_passed");

    prepare_b(&pg, &sealed_b, &authority).await;
    prepare_c(&pg, &sealed_c, &authority).await;
    let g8 = ok(
        record_gate(
            &pg,
            &sealed_f,
            "G8",
            "component_authority",
            "fail",
            true,
            "ignored",
            None,
        )
        .await
        .expect("g8"),
        "recorded",
    );
    assert_eq!(g8.body["disposition"], "admissible");
    assert_eq!(g8.body["detail"], "role_pack_samples_omitted");
    assert_eq!(g8.body["pii_database_scan"], "not_performed");
    assert_eq!(g8.body["production_admissible"], false);
    let invalidated = ok(
        stage_at(&pg, "cand-f", "ver-f", None, &wide, &narrow_visibility)
            .await
            .expect("invalidate"),
        "invalidated",
    );
    assert_ne!(invalidated.body["source_hash"], sealed_f.source);
    assert_ne!(invalidated.body["pack_digest"], sealed_f.pack);
    let sealed_f = Sealed {
        id: "cand-f",
        version: "ver-f",
        source: invalidated.body["source_hash"]
            .as_str()
            .expect("source")
            .to_owned(),
        pack: invalidated.body["pack_digest"]
            .as_str()
            .expect("pack")
            .to_owned(),
        manifest: invalidated.body["manifest_sha256"]
            .as_str()
            .expect("manifest")
            .to_owned(),
    };
    let after_f = ok(
        status_of(&pg, sealed_f.id, &sealed_f.source)
            .await
            .expect("f status"),
        "status",
    );
    assert_eq!(after_f.body["state"], "candidate_built");
    assert!(after_f.body["pointer_version"].is_null());
    assert!(
        after_f.body["test_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "G8_missing")
    );
    let credential = ok(
        record_gate(
            &pg,
            &sealed_f,
            "credential",
            "test_issuer",
            "pass",
            false,
            "test_issuer",
            None,
        )
        .await
        .expect("credential"),
        "recorded",
    );
    assert_eq!(credential.body["credential_verified"], false);
    assert_eq!(credential.body["production_admissible"], false);

    arm(&pg, &sealed_d, &authority, false).await;
    let planned_d = ok(
        plan(&plan_request(&pg, &sealed_d, "test_scope", None))
            .await
            .expect("plan d"),
        "planned",
    );
    assert_eq!(planned_d.body["certificate_class"], "not_issued");
    ok(
        enter_applying(&plan_request(&pg, &sealed_a, "test_scope", None))
            .await
            .expect("enter a"),
        "applying",
    );
    ok(
        enter_applying(&plan_request(&pg, &sealed_d, "test_scope", None))
            .await
            .expect("enter d"),
        "applying",
    );
    let activated = ok(
        commit_activation(&plan_request(&pg, &sealed_d, "test_scope", None))
            .await
            .expect("commit d"),
        "activated",
    );
    assert_eq!(
        activated.body["certificate_class"],
        "test_scope_not_production"
    );
    assert_eq!(activated.body["state"], "active");
    assert!(activated.body["previous_version"].is_null());
    assert_eq!(activated.body["half_published"], false);
    let audit = activated.body["audit_id"]
        .as_str()
        .expect("audit")
        .to_owned();
    let loser = refused(
        commit_activation(&plan_request(&pg, &sealed_a, "test_scope", None))
            .await
            .expect("commit a"),
        "cas_conflict",
    );
    assert_eq!(loser.body["state"], "failed");
    assert_eq!(loser.body["half_published"], false);
    assert_eq!(loser.body["pointer_version"], "ver-d");
    let replay_d = ok(
        activate(&plan_request(&pg, &sealed_d, "test_scope", None))
            .await
            .expect("replay d"),
        "replay",
    );
    assert_eq!(replay_d.body["audit_id"], audit);
    assert_eq!(
        replay_d.body["certificate_class"],
        "test_scope_not_production"
    );
    let still_d = ok(
        status_of(&pg, "cand-d", &sealed_d.source)
            .await
            .expect("d status"),
        "status",
    );
    assert_eq!(still_d.body["state"], "active");
    assert_eq!(still_d.body["pointer_is_candidate"], true);
    assert!(
        !still_d.body["test_reasons"]
            .as_array()
            .expect("reasons")
            .iter()
            .any(|reason| reason == "version_mismatch")
    );
    assert!(!still_d.body["files"].as_array().expect("files").is_empty());

    let sealed_g = stage_new(
        &pg,
        "cand-g",
        "ver-g",
        Some(sealed_d.version),
        &wide,
        &wide_visibility,
    )
    .await;
    arm(&pg, &sealed_g, &authority, false).await;
    ok(
        plan(&plan_request(&pg, &sealed_g, "test_scope", Some("ver-d")))
            .await
            .expect("plan g"),
        "planned",
    );
    let g_live = ok(
        activate(&plan_request(&pg, &sealed_g, "test_scope", Some("ver-d")))
            .await
            .expect("activate g"),
        "activated",
    );
    assert_eq!(g_live.body["previous_version"], "ver-d");
    assert_eq!(
        g_live.body["certificate_class"],
        "test_scope_not_production"
    );
    let d_inactive = ok(
        status_of(&pg, "cand-d", &sealed_d.source)
            .await
            .expect("d inactive"),
        "status",
    );
    assert_eq!(d_inactive.body["state"], "inactive");
    assert_eq!(d_inactive.body["pack_digest"], sealed_d.pack);
    let rolled = ok(
        recover(&recover_request(&pg, "rollback", "cand-g", Some("ver-g")))
            .await
            .expect("rollback"),
        "rolled_back",
    );
    assert_eq!(rolled.body["pointer_version"], "ver-d");
    assert_eq!(rolled.body["pack_digest"], sealed_g.pack);
    assert_eq!(rolled.body["manifest_sha256"], sealed_g.manifest);
    assert_eq!(rolled.body["state"], "inactive");
    let d_restored = ok(
        status_of(&pg, "cand-d", &sealed_d.source)
            .await
            .expect("d restored"),
        "status",
    );
    assert_eq!(d_restored.body["state"], "active");
    assert_eq!(d_restored.body["pointer_version"], "ver-d");
    assert_eq!(d_restored.body["pack_digest"], sealed_d.pack);
    let g_inactive = ok(
        status_of(&pg, "cand-g", &sealed_g.source)
            .await
            .expect("g inactive"),
        "status",
    );
    assert_eq!(g_inactive.body["state"], "inactive");
    assert_eq!(g_inactive.body["pack_digest"], sealed_g.pack);
    let rolled_again = ok(
        recover(&recover_request(&pg, "rollback", "cand-g", Some("ver-g")))
            .await
            .expect("rollback replay"),
        "replay",
    );
    assert_eq!(rolled_again.body["pointer_version"], "ver-d");

    ok(
        recover(&recover_request(&pg, "abort", "cand-b", None))
            .await
            .expect("abort"),
        "aborted",
    );
    let abort_replay = ok(
        recover(&recover_request(&pg, "abort", "cand-b", None))
            .await
            .expect("abort replay"),
        "replay",
    );
    assert_eq!(abort_replay.body["state"], "aborted");
    ok(
        recover(&recover_request(&pg, "retire", "cand-c", None))
            .await
            .expect("retire"),
        "retired",
    );
    refused(
        recover(&recover_request(&pg, "abort", "cand-d", None))
            .await
            .expect("abort active"),
        "seal_conflict",
    );
    refused(
        stage_at(
            &pg,
            "cand-d",
            "ver-d",
            Some("ver-d"),
            &wide,
            &narrow_visibility,
        )
        .await
        .expect("restage active"),
        "seal_conflict",
    );
    let d_final = ok(
        status_of(&pg, "cand-d", &sealed_d.source)
            .await
            .expect("d final"),
        "status",
    );
    assert_eq!(d_final.body["state"], "active");
    assert_eq!(d_final.body["pack_digest"], sealed_d.pack);

    reader_cannot_write(&pg).await;
    let fresh = ok(
        status_of(&pg, "cand-d", &sealed_d.source)
            .await
            .expect("fresh connection"),
        "status",
    );
    assert_eq!(fresh.body["pointer_version"], "ver-d");
    let cli = cli_status(&pg, "cand-d", &sealed_d.source).await;
    assert_eq!(cli["state"], "active");
    assert_eq!(cli["pointer_version"], "ver-d");
    assert_eq!(cli["production_ready_certificate"], false);
    if let Ok(path) = env::var("OO_A359_OBSERVATION") {
        let note = json!({
            "candidate_id": "cand-d",
            "version": "ver-d",
            "source_hash": sealed_d.source,
            "pack_digest": sealed_d.pack,
            "manifest_sha256": sealed_d.manifest,
            "pointer_version": "ver-d",
            "certificate_class": "test_scope_not_production",
            "production_ready_certificate": false,
        });
        fs::write(path, serde_json::to_vec(&note).expect("observation"))
            .expect("observation write");
    }
    let wrong = status(&plan_request_ca(&pg, &sealed_d, b"not-a-certificate")).await;
    assert!(matches!(wrong, Err(StoreError::Tls)));
}

fn install_request<'a>(pg: &'a Pg, authority: &'a str, covers: &'a [String]) -> InstallRequest<'a> {
    InstallRequest {
        endpoint: &pg.endpoint,
        user: "oo_release_admin",
        password: &pg.password,
        ca_pem: &pg.ca,
        project_id: PROJECT,
        authority_id: authority,
        covers,
    }
}

async fn stage_at<'a>(
    pg: &'a Pg,
    id: &'a str,
    version: &'a str,
    base: Option<&'a str>,
    effective: &'a Value,
    visibility: &'a Value,
) -> Result<Receipt, StoreError> {
    stage(&StageRequest {
        endpoint: &pg.endpoint,
        user: "oo_release_admin",
        password: &pg.password,
        ca_pem: &pg.ca,
        project_id: PROJECT,
        candidate_id: id,
        version,
        base_version: base,
        effective,
        visibility,
        policy_id: POLICY_ID,
        policy_version: POLICY_VERSION,
        knowledge_id: KNOWLEDGE_ID,
        knowledge_version: KNOWLEDGE_VERSION,
        catalog_version: CATALOG_VERSION,
    })
    .await
}

async fn stage_new(
    pg: &Pg,
    id: &'static str,
    version: &'static str,
    base: Option<&str>,
    effective: &Value,
    visibility: &Value,
) -> Sealed {
    let receipt = ok(
        stage_at(pg, id, version, base, effective, visibility)
            .await
            .expect("stage"),
        "staged",
    );
    assert_eq!(receipt.body["compiler"], "oo-builder/role-pack/v1");
    assert_eq!(receipt.body["role"], "oo_r_clerk");
    assert_eq!(receipt.body["paths_normalized"], true);
    Sealed {
        id,
        version,
        source: receipt.body["source_hash"]
            .as_str()
            .expect("source")
            .to_owned(),
        pack: receipt.body["pack_digest"]
            .as_str()
            .expect("pack")
            .to_owned(),
        manifest: receipt.body["manifest_sha256"]
            .as_str()
            .expect("manifest")
            .to_owned(),
    }
}

fn plan_request<'a>(
    pg: &'a Pg,
    sealed: &'a Sealed,
    scope: &'a str,
    expected: Option<&'a str>,
) -> PlanRequest<'a> {
    PlanRequest {
        endpoint: &pg.endpoint,
        user: "oo_release_admin",
        password: &pg.password,
        ca_pem: &pg.ca,
        project_id: PROJECT,
        candidate_id: sealed.id,
        source_hash: &sealed.source,
        scope,
        expected_version: expected,
    }
}

fn plan_request_ca<'a>(pg: &'a Pg, sealed: &'a Sealed, ca: &'a [u8]) -> PlanRequest<'a> {
    let mut request = plan_request(pg, sealed, "test_scope", None);
    request.ca_pem = ca;
    request
}

fn recover_request<'a>(
    pg: &'a Pg,
    action: &'a str,
    candidate: &'a str,
    expected: Option<&'a str>,
) -> RecoverRequest<'a> {
    RecoverRequest {
        endpoint: &pg.endpoint,
        user: "oo_release_admin",
        password: &pg.password,
        ca_pem: &pg.ca,
        action,
        project_id: PROJECT,
        candidate_id: candidate,
        expected_version: expected,
    }
}

async fn status_of(pg: &Pg, id: &str, source: &str) -> Result<Receipt, StoreError> {
    status(&PlanRequest {
        endpoint: &pg.endpoint,
        user: "oo_release_admin",
        password: &pg.password,
        ca_pem: &pg.ca,
        project_id: PROJECT,
        candidate_id: id,
        source_hash: source,
        scope: "test_scope",
        expected_version: None,
    })
    .await
}

#[allow(clippy::too_many_arguments)]
async fn record_gate(
    pg: &Pg,
    sealed: &Sealed,
    gate: &str,
    issuer: &str,
    status_text: &str,
    blocking: bool,
    report_type: &str,
    authority: Option<&str>,
) -> Result<Receipt, StoreError> {
    gate_record(&GateRequest {
        endpoint: &pg.endpoint,
        user: "oo_release_admin",
        password: &pg.password,
        ca_pem: &pg.ca,
        project_id: PROJECT,
        candidate_id: sealed.id,
        source_hash: &sealed.source,
        gate_id: gate,
        issuer_class: issuer,
        status: status_text,
        blocking,
        report_type,
        authority_statement_sha256: authority,
    })
    .await
}

async fn record_checkpoint(
    pg: &Pg,
    sealed: &Sealed,
    kind: &str,
    epoch: &str,
    barrier: &str,
    sessions: bool,
) -> Receipt {
    ok(
        checkpoint(&CheckpointRequest {
            endpoint: &pg.endpoint,
            user: "oo_release_admin",
            password: &pg.password,
            ca_pem: &pg.ca,
            project_id: PROJECT,
            candidate_id: sealed.id,
            kind,
            source_hash: &sealed.source,
            epoch,
            barrier,
            sessions_terminated: sessions,
            issuer_class: "test_issuer",
            authority_statement_sha256: None,
        })
        .await
        .expect("checkpoint"),
        "recorded",
    )
}

async fn arm(pg: &Pg, sealed: &Sealed, authority: &str, fabricated: bool) {
    for gate in GATE_IDS {
        let receipt = if *gate == "G8" {
            record_gate(
                pg,
                sealed,
                gate,
                "component_authority",
                "not_run",
                true,
                "component_recompile",
                None,
            )
            .await
        } else if *gate == "G9" {
            record_gate(
                pg,
                sealed,
                gate,
                "fixture_authority",
                "pass",
                false,
                "fixture_certificate",
                Some(authority),
            )
            .await
        } else {
            record_gate(
                pg,
                sealed,
                gate,
                "test_issuer",
                "pass",
                false,
                "test_issuer",
                None,
            )
            .await
        }
        .expect("gate");
        let recorded = ok(receipt, "recorded");
        assert_eq!(recorded.body["disposition"], "admissible", "{gate}");
        if *gate == "G1" {
            let replay = ok(
                record_gate(
                    pg,
                    sealed,
                    gate,
                    "test_issuer",
                    "pass",
                    false,
                    "test_issuer",
                    None,
                )
                .await
                .expect("gate replay"),
                "replay",
            );
            assert_eq!(
                replay.body["certificate_sha256"],
                recorded.body["certificate_sha256"]
            );
        }
    }
    record_checkpoint(pg, sealed, "fence", "1", "closed", false).await;
    record_checkpoint(pg, sealed, "replica", "1", "replayed", false).await;
    if fabricated {
        let fake = record_checkpoint(pg, sealed, "fence", "term", "closed", true).await;
        assert_eq!(fake.body["disposition"], "fabricated_session");
        assert_eq!(fake.body["sessions_terminated_by_tool"], false);
    }
}

async fn production_blocked(pg: &Pg, sealed: &Sealed) -> Receipt {
    refused(
        plan(&plan_request(pg, sealed, "production", None))
            .await
            .expect("production plan"),
        "production_blocked",
    )
}

async fn prepare_b(pg: &Pg, sealed: &Sealed, authority: &str) {
    for gate in GATE_IDS {
        if *gate == "G4" {
            continue;
        }
        let receipt = if *gate == "G8" {
            record_gate(
                pg,
                sealed,
                gate,
                "component_authority",
                "pass",
                false,
                "ignored",
                None,
            )
            .await
        } else if *gate == "G9" {
            record_gate(
                pg,
                sealed,
                gate,
                "fixture_authority",
                "pass",
                false,
                "fixture_certificate",
                Some(authority),
            )
            .await
        } else {
            record_gate(
                pg,
                sealed,
                gate,
                "test_issuer",
                "pass",
                false,
                "test_issuer",
                None,
            )
            .await
        }
        .expect("b gate");
        ok(receipt, "recorded");
    }
    let missing = refused(
        plan(&plan_request(pg, sealed, "test_scope", None))
            .await
            .expect("b missing"),
        "not_ready",
    );
    assert!(
        missing.body["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "G4_missing")
    );
    let skipped = ok(
        record_gate(
            pg,
            sealed,
            "G4",
            "test_issuer",
            "not_run",
            false,
            "test_issuer",
            None,
        )
        .await
        .expect("not run"),
        "recorded",
    );
    assert_eq!(skipped.body["disposition"], "not_run");
    let not_run = refused(
        plan(&plan_request(pg, sealed, "test_scope", None))
            .await
            .expect("b not run"),
        "not_ready",
    );
    assert!(
        not_run.body["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "G4_not_run")
    );
    let second = ok(
        record_gate(
            pg,
            sealed,
            "G4",
            "test_issuer",
            "pass",
            false,
            "test_issuer",
            None,
        )
        .await
        .expect("second"),
        "recorded",
    );
    assert_eq!(second.body["disposition"], "admissible");
    let conflict = refused(
        plan(&plan_request(pg, sealed, "test_scope", None))
            .await
            .expect("b conflict"),
        "not_ready",
    );
    assert!(
        conflict.body["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "G4_conflict")
    );
    record_checkpoint(pg, sealed, "fence", "1", "closed", false).await;
    record_checkpoint(pg, sealed, "fence", "2", "closed", false).await;
    let fences = refused(
        plan(&plan_request(pg, sealed, "test_scope", None))
            .await
            .expect("b fences"),
        "not_ready",
    );
    assert!(
        fences.body["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "conflict")
    );
}

async fn prepare_c(pg: &Pg, sealed: &Sealed, _authority: &str) {
    let foreign = ok(
        record_gate(
            pg,
            sealed,
            "G3",
            "fixture_authority",
            "pass",
            false,
            "fixture_certificate",
            Some(&"ab".repeat(32)),
        )
        .await
        .expect("foreign"),
        "recorded",
    );
    assert_eq!(foreign.body["disposition"], "foreign_receipt");
    let stale = record_checkpoint(pg, sealed, "fence", "1", "stale", false).await;
    assert_eq!(stale.body["disposition"], "not_fresh");
    let plan = refused(
        plan(&plan_request(pg, sealed, "test_scope", None))
            .await
            .expect("c plan"),
        "not_ready",
    );
    let reasons = plan.body["reasons"].as_array().unwrap();
    assert!(reasons.iter().any(|reason| reason == "G3_foreign_receipt"));
    assert!(reasons.iter().any(|reason| reason == "stale_fence"));
}

async fn reader_cannot_write(pg: &Pg) {
    let tls = crate::rustls_connector(&pg.ca).expect("reader tls");
    let mut config = tokio_postgres::Config::new();
    config.user("oo_release_reader_login");
    config.password(&pg.reader_password);
    config.host(&pg.endpoint.host);
    config.port(pg.endpoint.port);
    config.dbname(&pg.endpoint.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("release-coordinator-reader");
    config.connect_timeout(Duration::from_secs(10));
    let (client, connection) = config.connect(tls).await.expect("reader connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let row = client
        .query_one(
            "SELECT production_ready_certificate, full_sp08_accepted, public_deployment_claimed, customer_ownership_claimed, standby_configured, version FROM oo_release_control.publication_status",
            &[],
        )
        .await
        .expect("view");
    let production: bool = row.get(0);
    let accepted: bool = row.get(1);
    let public_claim: bool = row.get(2);
    let customer: bool = row.get(3);
    let standby: bool = row.get(4);
    let version: Option<String> = row.get(5);
    assert!(!production && !accepted && !public_claim && !customer && !standby);
    assert_eq!(version.as_deref(), Some("ver-d"));
    let denied = client
        .batch_execute("UPDATE oo_release_control.active_pointer SET version = version")
        .await
        .expect_err("update");
    assert_eq!(denied.code().map(|code| code.code()), Some("42501"));
    let hidden = client
        .query("SELECT candidate_id FROM oo_release_control.candidate", &[])
        .await
        .expect_err("table");
    assert_eq!(hidden.code().map(|code| code.code()), Some("42501"));
}

async fn cli_status(pg: &Pg, id: &str, source: &str) -> Value {
    let body = json!({
        "protocol": "oo.release-coordinator/status/v1",
        "project_id": PROJECT,
        "candidate_id": id,
        "source_hash": source,
    });
    let args = vec![
        "status".to_owned(),
        "--config".to_owned(),
        pg.config_path.clone(),
    ];
    let exit = dispatch(&args, &serde_json::to_vec(&body).expect("stdin")).await;
    assert_eq!(exit.code, 0, "{}", String::from_utf8_lossy(&exit.stderr));
    serde_json::from_slice(&exit.stdout).expect("cli json")
}
