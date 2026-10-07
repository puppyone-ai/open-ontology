//! Independent E53 checks. Expected words come from the hand file.
//! Pack file hashes are computed with sha2, not the coordinator hasher.

use std::env;
use std::fs;
use std::time::Duration;

use oo_builder::{
    compile_captured_pg_catalog, compile_reviewed_effective, compile_role_pack, REVIEWED_EFFECTIVE_COMMAND_PROTOCOL,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_postgres::config::SslMode;

use crate::seal::Expectation;
use crate::{
    activate, checkpoint, commit_activation, compile_seal, dispatch, enter_applying, gate_record, install, plan,
    recover, stage, status, CheckpointRequest, Endpoint, GateRequest, InstallRequest, PlanRequest, Receipt,
    RecoverRequest, StageRequest, StoreError, GATE_IDS,
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
}

struct Made {
    id: &'static str,
    version: &'static str,
    source: String,
    pack: String,
    manifest: String,
}

fn hand(key: &str) -> String {
    let path = env::var("OO_A366_HAND").expect("OO_A366_HAND");
    let text = fs::read_to_string(path).expect("hand file");
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((name, value)) = line.split_once('=')
            && name == key
        {
            return value.to_owned();
        }
    }
    panic!("hand file missing {key}");
}

fn oracle_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn env_var(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn secret(path: &str) -> String {
    let mut text = fs::read_to_string(path).unwrap_or_else(|_| panic!("secret {path}"));
    if text.ends_with('\n') {
        text.pop();
    }
    assert!(!text.is_empty());
    text
}

fn load_pg() -> Pg {
    let port = env_var("OO_A366_PG_PORT").parse().expect("port");
    let password_file = env_var("OO_A366_PASSWORD_FILE");
    let ca_file = env_var("OO_A366_CA_FILE");
    Pg {
        endpoint: Endpoint { host: env_var("OO_A366_PG_HOST"), port, database: env_var("OO_A366_PG_DATABASE") },
        password: secret(&password_file),
        reader_password: secret(&env_var("OO_A366_READER_PASSWORD_FILE")),
        ca: fs::read(ca_file).expect("ca"),
    }
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
        assert_eq!(body[key], false, "{key}");
        assert_eq!(hand(key), "false", "{key}");
    }
    assert_eq!(body["test_mode_cannot_produce_production_certificate"], true);
    assert_eq!(hand("test_mode_cannot"), "true");
    assert_eq!(body["contract"], hand("contract"));
    assert_eq!(body["sessions_terminated_by_tool"], false);
    assert_eq!(hand("sessions_terminated_by_tool"), "false");
}

fn ok(receipt: Receipt, outcome_key: &str) -> Receipt {
    flags(&receipt);
    assert_eq!(receipt.code, 0, "{}", receipt.body);
    assert_eq!(receipt.body["outcome"], hand(outcome_key), "{}", receipt.body);
    receipt
}

fn refused(receipt: Receipt, reason_key: &str) -> Receipt {
    flags(&receipt);
    assert_eq!(receipt.code, 2, "{}", receipt.body);
    assert_eq!(receipt.body["reason"], hand(reason_key), "{}", receipt.body);
    assert_eq!(receipt.body["certificate_class"], hand("certificate_class_other"));
    receipt
}

fn has_reason(receipt: &Receipt, key: &str) {
    let want = hand(key);
    let reasons = receipt.body["reasons"].as_array().unwrap_or_else(|| panic!("{}", receipt.body));
    assert!(reasons.iter().any(|reason| reason == &want), "{want} in {reasons:?}");
}

fn column_preimage(attnum: i32, name: &str, type_oid: i64) -> String {
    format!(
        r#"{{"attnum":{attnum},"name":"{name}","type_oid":{type_oid},"typmod":-1,"not_null":true,"collation_oid":0,"typtype":"b","base_type_oid":null,"identity":"","generated":"","default_sha256":null,"enum_labels":[],"domain_checks":[]}}"#
    )
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

fn catalog_text() -> String {
    let columns = format!("{},{}", column_preimage(1, "id", 23), column_preimage(2, "contact", 25));
    let preimage = format!(
        r#"{{"tables":[{{"schema":"public","name":"customer","relkind":"r","replica_identity":"d","rls":false,"force_rls":false,"columns":[{columns}],"primary_key":[{{"attnum":1,"name":"id"}}],"primary_key_nulls_not_distinct":false,"unique_keys":[],"foreign_keys":[],"policies":[]}}]}}"#
    );
    let schema_hash = oracle_sha256(preimage.as_bytes());
    serde_json::to_string(&json!({
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
    }))
    .expect("catalog")
}

fn shop(columns: &[&str]) -> (Value, Value) {
    let catalog = catalog_text();
    let compiled = compile_captured_pg_catalog(&catalog, None, None).expect("captured catalog");
    let draft: Value = serde_json::from_str(&compiled.draft).expect("draft");
    let schema = draft["provenance"]["schema_hash"].as_str().expect("schema").to_owned();
    let mut decisions = Vec::new();
    for column in ["id", "contact"] {
        let item_id = format!("column/public.customer/{column}");
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
            "dataset": "public.customer",
            "column": column,
            "schema_hash": schema,
            "fact_fingerprint": fingerprint,
            "pii": "none",
            "semantic_type": "identifier",
            "reviewer": {"by": "ada.owner", "at": "2026-10-06"}
        }));
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
    let reviewed = compile_reviewed_effective(&serde_json::to_string(&command).expect("command")).expect("reviewed");
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
        "datasets": [{
            "dataset": "public.customer",
            "row_security": "none",
            "global_stats": "denied",
            "columns": columns
        }],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    });
    (reviewed["effective_oos"].clone(), visibility)
}

fn fixture_path() -> String {
    env_var("OO_A366_FIXTURE")
}

fn write_fixture(effective: &Value, wide: &Value, narrow: &Value) {
    let body = json!({"effective": effective, "wide": wide, "narrow": narrow});
    fs::write(fixture_path(), serde_json::to_vec(&body).expect("fixture")).expect("fixture write");
}

fn load_docs() -> (Value, Value, Value) {
    let body: Value = serde_json::from_slice(&fs::read(fixture_path()).expect("fixture")).expect("fixture json");
    (body["effective"].clone(), body["wide"].clone(), body["narrow"].clone())
}

fn expect<'a>(id: &'a str, version: &'a str, base: Option<&'a str>) -> Expectation<'a> {
    Expectation {
        project_id: PROJECT,
        candidate_id: id,
        version,
        base_version: base,
        policy_id: POLICY_ID,
        policy_version: POLICY_VERSION,
        knowledge_id: KNOWLEDGE_ID,
        knowledge_version: KNOWLEDGE_VERSION,
        catalog_version: CATALOG_VERSION,
    }
}

fn hash_pack(effective: &Value, visibility: &Value) -> crate::Seal {
    let seal = compile_seal(effective, visibility, &expect("rev-oracle", "rv-oracle", None)).expect("seal");
    let pack = compile_role_pack(&seal.effective, &seal.visibility).expect("role pack");
    let manifest = pack.file("manifest.json").expect("manifest");
    assert_eq!(oracle_sha256(manifest.as_bytes()), pack.manifest_sha256);
    assert_eq!(oracle_sha256(manifest.as_bytes()), seal.manifest_sha256);
    let manifest_json: Value = serde_json::from_str(manifest).expect("manifest json");
    assert_eq!(manifest_json["compiler"], hand("compiler"));
    assert_eq!(manifest_json["publication"], hand("publication"));
    assert_eq!(manifest_json["authentication"], hand("authentication"));
    assert_eq!(manifest_json["authorization"], hand("authorization"));
    assert_eq!(manifest_json["samples"], hand("samples"));
    assert_eq!(manifest_json["mount"], hand("mount"));
    assert_eq!(manifest_json["row_security_reproven"], false);
    assert_eq!(hand("row_security_reproven"), "false");
    for file in &seal.files {
        assert!(!file.path.starts_with('/'));
        assert!(!file.path.contains(".."));
        let text = pack.file(&file.path).expect("pack file");
        assert_eq!(oracle_sha256(text.as_bytes()), file.sha256);
        assert_eq!(text.len() as i32, file.byte_len);
    }
    assert!(seal.files.iter().any(|file| file.path == "manifest.json"));
    seal
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

fn covers() -> Vec<String> {
    GATE_IDS.iter().map(|gate| (*gate).to_owned()).collect()
}

async fn stage_at(pg: &Pg, id: &'static str, version: &'static str, base: Option<&str>, effective: &Value, visibility: &Value) -> Receipt {
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
    .expect("stage")
}

async fn stage_new(pg: &Pg, id: &'static str, version: &'static str, base: Option<&str>, effective: &Value, visibility: &Value) -> Made {
    let receipt = ok(stage_at(pg, id, version, base, effective, visibility).await, "outcome_staged");
    assert_eq!(receipt.body["compiler"], hand("compiler"));
    assert_eq!(receipt.body["state"], hand("state_built"));
    Made {
        id,
        version,
        source: receipt.body["source_hash"].as_str().expect("source").to_owned(),
        pack: receipt.body["pack_digest"].as_str().expect("pack").to_owned(),
        manifest: receipt.body["manifest_sha256"].as_str().expect("manifest").to_owned(),
    }
}

struct GateIn<'a> {
    gate: &'a str,
    issuer: &'a str,
    status: &'a str,
    blocking: bool,
    report: &'a str,
    authority: Option<&'a str>,
}

fn gate_in<'a>(
    gate: &'a str,
    issuer: &'a str,
    status: &'a str,
    blocking: bool,
    report: &'a str,
    authority: Option<&'a str>,
) -> GateIn<'a> {
    GateIn { gate, issuer, status, blocking, report, authority }
}

async fn record_gate(pg: &Pg, made: &Made, input: GateIn<'_>) -> Receipt {
    gate_record(&GateRequest {
        endpoint: &pg.endpoint,
        user: "oo_release_admin",
        password: &pg.password,
        ca_pem: &pg.ca,
        project_id: PROJECT,
        candidate_id: made.id,
        source_hash: &made.source,
        gate_id: input.gate,
        issuer_class: input.issuer,
        status: input.status,
        blocking: input.blocking,
        report_type: input.report,
        authority_statement_sha256: input.authority,
    })
    .await
    .expect("gate")
}

async fn record_checkpoint(pg: &Pg, made: &Made, kind: &str, epoch: &str, barrier: &str, sessions: bool) -> Receipt {
    checkpoint(&CheckpointRequest {
        endpoint: &pg.endpoint,
        user: "oo_release_admin",
        password: &pg.password,
        ca_pem: &pg.ca,
        project_id: PROJECT,
        candidate_id: made.id,
        kind,
        source_hash: &made.source,
        epoch,
        barrier,
        sessions_terminated: sessions,
        issuer_class: "test_issuer",
        authority_statement_sha256: None,
    })
    .await
    .expect("checkpoint")
}

fn plan_request<'a>(pg: &'a Pg, made: &'a Made, scope: &'a str, expected: Option<&'a str>) -> PlanRequest<'a> {
    PlanRequest {
        endpoint: &pg.endpoint,
        user: "oo_release_admin",
        password: &pg.password,
        ca_pem: &pg.ca,
        project_id: PROJECT,
        candidate_id: made.id,
        source_hash: &made.source,
        scope,
        expected_version: expected,
    }
}

fn recover_request<'a>(pg: &'a Pg, action: &'a str, id: &'a str, expected: Option<&'a str>) -> RecoverRequest<'a> {
    RecoverRequest {
        endpoint: &pg.endpoint,
        user: "oo_release_admin",
        password: &pg.password,
        ca_pem: &pg.ca,
        action,
        project_id: PROJECT,
        candidate_id: id,
        expected_version: expected,
    }
}

async fn arm(pg: &Pg, made: &Made, authority: &str) {
    for gate in GATE_IDS {
        let receipt = if *gate == "G8" {
            record_gate(pg, made, gate_in(gate, "component_authority", "not_run", true, "component_recompile", None)).await
        } else if *gate == "G9" {
            record_gate(pg, made, gate_in(gate, "fixture_authority", "pass", false, "fixture_certificate", Some(authority))).await
        } else {
            record_gate(pg, made, gate_in(gate, "test_issuer", "pass", false, "test_issuer", None)).await
        };
        let recorded = ok(receipt, "outcome_recorded");
        assert_eq!(recorded.body["disposition"], hand("disposition_admissible"));
        assert_eq!(recorded.body["production_admissible"], false);
        assert_eq!(recorded.body["pii_database_scan"], hand("pii_database_scan"));
        if *gate == "G8" {
            assert_eq!(recorded.body["detail"], hand("g8_detail"));
        }
    }
    let fence = ok(record_checkpoint(pg, made, "fence", "e1", "closed", false).await, "outcome_recorded");
    assert_eq!(fence.body["disposition"], hand("disposition_fresh"));
    let replica = ok(record_checkpoint(pg, made, "replica", "e1", "replayed", false).await, "outcome_recorded");
    assert_eq!(replica.body["disposition"], hand("disposition_fresh"));
}

fn reseal(id: &'static str, version: &'static str, base: Option<&str>, effective: &Value, visibility: &Value) -> Made {
    let seal = compile_seal(effective, visibility, &expect(id, version, base)).expect("reseal");
    Made { id, version, source: seal.source_hash, pack: seal.pack_digest, manifest: seal.manifest_sha256 }
}

async fn deployment_flags_false(pg: &Pg) {
    let client = sql(pg).await;
    let row = client
        .query_one(
            "SELECT production_ready_certificate, credential_verified, integrity_verified, customer_gate_verified, runsc_verified, owner_authenticated, full_sp08_accepted, public_deployment_claimed, customer_ownership_claimed, standby_configured FROM oo_release_control.deployment",
            &[],
        )
        .await
        .expect("deployment");
    for index in 0..10 {
        let value: bool = row.get(index);
        assert!(!value, "deployment column {index}");
    }
}

async fn assert_events(pg: &Pg, id: &str) {
    let client = sql(pg).await;
    let rows = client
        .query("SELECT event FROM oo_release_control.audit_event WHERE candidate_id = $1", &[&id])
        .await
        .expect("audit");
    let events: Vec<String> = rows.iter().map(|row| row.get(0)).collect();
    assert!(events.iter().any(|event| event == &hand("audit_replica")), "{events:?}");
    assert!(events.iter().any(|event| event == &hand("audit_activated")), "{events:?}");
}

async fn sql(pg: &Pg) -> tokio_postgres::Client {
    let tls = crate::rustls_connector(&pg.ca).expect("tls");
    let mut config = tokio_postgres::Config::new();
    config.user("oo_release_admin");
    config.password(&pg.password);
    config.host(&pg.endpoint.host);
    config.port(pg.endpoint.port);
    config.dbname(&pg.endpoint.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("e53-review-oracle");
    config.connect_timeout(Duration::from_secs(10));
    let (client, connection) = config.connect(tls).await.expect("admin connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn pointer(pg: &Pg) -> Option<String> {
    let client = sql(pg).await;
    let row = client.query_one("SELECT version FROM oo_release_control.active_pointer", &[]).await.expect("pointer");
    row.get(0)
}

async fn candidate_row(pg: &Pg, id: &str) -> (String, String, String) {
    let client = sql(pg).await;
    let row = client
        .query_one(
            "SELECT state, pack_digest, manifest_sha256 FROM oo_release_control.candidate WHERE candidate_id = $1",
            &[&id],
        )
        .await
        .expect("candidate");
    (row.get(0), row.get(1), row.get(2))
}

async fn stored_files_match(pg: &Pg, made: &Made, effective: &Value, visibility: &Value) {
    let seal = compile_seal(effective, visibility, &expect(made.id, made.version, None)).expect("seal");
    let pack = compile_role_pack(&seal.effective, &seal.visibility).expect("pack");
    let client = sql(pg).await;
    let rows = client
        .query(
            "SELECT path, sha256, byte_len FROM oo_release_control.candidate_file WHERE candidate_id = $1",
            &[&made.id],
        )
        .await
        .expect("files");
    assert_eq!(rows.len(), seal.files.len());
    for row in rows {
        let path: String = row.get(0);
        let sha: String = row.get(1);
        let len: i32 = row.get(2);
        let text = pack.file(&path).expect("stored path");
        assert_eq!(sha, oracle_sha256(text.as_bytes()));
        assert_eq!(len, text.len() as i32);
    }
}

#[tokio::test]
async fn review_01_core_sealed_role_pack_hand_oracle() {
    let (effective, wide) = shop(&["contact", "id"]);
    let (same, narrow) = shop(&["contact"]);
    assert_eq!(effective, same);
    let wide_seal = hash_pack(&effective, &wide);
    let narrow_seal = hash_pack(&effective, &narrow);
    assert_eq!(wide_seal.compiler, hand("compiler"));
    assert_eq!(wide_seal.role, "oo_r_clerk");
    assert_ne!(wide_seal.source_hash, narrow_seal.source_hash);
    assert_ne!(wide_seal.pack_digest, narrow_seal.pack_digest);
    assert_ne!(wide_seal.manifest_sha256, narrow_seal.manifest_sha256);
    write_fixture(&effective, &wide, &narrow);
    let product = br#"{"protocol":"oo.release-coordinator/plan/v1","project_id":"shop","candidate_id":"rev-x","source_hash":"abcd","scope":"production","expected_version":null,"product_mode_can_activate":true}"#;
    let exit = dispatch(&["plan".to_owned(), "--config".to_owned(), "/tmp/oo-a366-absent.json".to_owned()], product).await;
    assert_eq!(exit.code, 1);
    assert!(exit.stdout.is_empty());
    let waiver = br#"{"protocol":"oo.release-coordinator/plan/v1","all_accepted":true}"#;
    let exit = dispatch(&["plan".to_owned(), "--config".to_owned(), "/tmp/oo-a366-absent.json".to_owned()], waiver).await;
    assert_eq!(exit.code, 1);
    assert!(exit.stdout.is_empty());
    let alias = br#"{"allaccepted":true}"#;
    let exit = dispatch(&["activate".to_owned(), "--config".to_owned(), "/tmp/oo-a366-absent.json".to_owned()], alias).await;
    assert_eq!(exit.code, 1);
    assert!(exit.stdout.is_empty());
}

#[tokio::test]
async fn review_02_install_replay_drift_and_base_mismatch() {
    let pg = load_pg();
    let names = covers();
    let (effective, wide, _) = load_docs();
    let installed = ok(install(&install_request(&pg, "fix-shop", &names)).await.expect("install"), "outcome_installed");
    let authority = installed.body["authority_statement_sha256"].as_str().expect("authority").to_owned();
    fs::write(env_var("OO_A366_AUTHORITY"), &authority).expect("authority file");
    let again = ok(install(&install_request(&pg, "fix-shop", &names)).await.expect("replay"), "outcome_replay");
    assert_eq!(again.body["authority_statement_sha256"], authority);
    let drift = refused(install(&install_request(&pg, "other-auth", &names)).await.expect("drift"), "reason_drift");
    assert_eq!(drift.body["database_used"], true);
    refused(stage_at(&pg, "rev-x", "rv-x", Some("nope"), &effective, &wide).await, "reason_base");
    let client = sql(&pg).await;
    let count: i64 = client.query_one("SELECT count(*) FROM oo_release_control.candidate", &[]).await.expect("count").get(0);
    assert_eq!(count, 0);
    assert!(pointer(&pg).await.is_none());
}

#[tokio::test]
async fn review_03_missing_gate_refused() {
    let pg = load_pg();
    let (effective, wide, _) = load_docs();
    let authority = fs::read_to_string(env_var("OO_A366_AUTHORITY")).expect("authority");
    let made = stage_new(&pg, "rev-m", "rv-m", None, &effective, &wide).await;
    stored_files_match(&pg, &made, &effective, &wide).await;
    for gate in GATE_IDS {
        if *gate == "G4" {
            continue;
        }
        let receipt = if *gate == "G8" {
            record_gate(&pg, &made, gate_in(gate, "component_authority", "fail", true, "component_recompile", None)).await
        } else if *gate == "G9" {
            record_gate(&pg, &made, gate_in(gate, "fixture_authority", "pass", false, "fixture_certificate", Some(authority.trim()))).await
        } else {
            record_gate(&pg, &made, gate_in(gate, "test_issuer", "pass", false, "test_issuer", None)).await
        };
        let recorded = ok(receipt, "outcome_recorded");
        assert_eq!(recorded.body["disposition"], hand("disposition_admissible"));
        if *gate == "G8" {
            assert_eq!(recorded.body["detail"], hand("g8_detail"));
            assert_eq!(recorded.body["production_admissible"], false);
        }
    }
    ok(record_checkpoint(&pg, &made, "fence", "e1", "closed", false).await, "outcome_recorded");
    ok(record_checkpoint(&pg, &made, "replica", "e1", "replayed", false).await, "outcome_recorded");
    let missing = refused(plan(&plan_request(&pg, &made, "test_scope", None)).await.expect("plan"), "reason_not_ready");
    has_reason(&missing, "missing");
    assert_eq!(missing.body["state"], hand("state_built"));
    let (state, _, _) = candidate_row(&pg, made.id).await;
    assert_eq!(state, hand("state_built"));
    assert!(pointer(&pg).await.is_none());
}

#[tokio::test]
async fn review_04_not_run_refused() {
    let pg = load_pg();
    let (effective, wide, _) = load_docs();
    let authority = fs::read_to_string(env_var("OO_A366_AUTHORITY")).expect("authority");
    let made = stage_new(&pg, "rev-n", "rv-n", None, &effective, &wide).await;
    for gate in GATE_IDS {
        if *gate == "G4" {
            let skipped = ok(
                record_gate(&pg, &made, gate_in(gate, "test_issuer", "not_run", false, "test_issuer", None)).await,
                "outcome_recorded",
            );
            assert_eq!(skipped.body["disposition"], hand("disposition_not_run"));
            continue;
        }
        let receipt = if *gate == "G8" {
            record_gate(&pg, &made, gate_in(gate, "component_authority", "not_run", false, "component_recompile", None)).await
        } else if *gate == "G9" {
            record_gate(&pg, &made, gate_in(gate, "fixture_authority", "pass", false, "fixture_certificate", Some(authority.trim()))).await
        } else {
            record_gate(&pg, &made, gate_in(gate, "test_issuer", "pass", false, "test_issuer", None)).await
        };
        ok(receipt, "outcome_recorded");
    }
    ok(record_checkpoint(&pg, &made, "fence", "e1", "closed", false).await, "outcome_recorded");
    ok(record_checkpoint(&pg, &made, "replica", "e1", "replayed", false).await, "outcome_recorded");
    let not_run = refused(plan(&plan_request(&pg, &made, "test_scope", None)).await.expect("plan"), "reason_not_ready");
    has_reason(&not_run, "not_run");
    assert_eq!(not_run.body["state"], hand("state_built"));
    assert!(pointer(&pg).await.is_none());
}

#[tokio::test]
async fn review_05_stale_and_foreign_refused() {
    let pg = load_pg();
    let (effective, wide, _) = load_docs();
    let made = stage_new(&pg, "rev-s", "rv-s", None, &effective, &wide).await;
    let stale_made = Made {
        id: made.id,
        version: made.version,
        source: "cd".repeat(32),
        pack: made.pack.clone(),
        manifest: made.manifest.clone(),
    };
    let stale = ok(record_gate(&pg, &stale_made, gate_in("G1", "test_issuer", "pass", false, "test_issuer", None)).await, "outcome_recorded");
    assert_eq!(stale.body["disposition"], hand("disposition_stale"));
    let foreign_hash = "ab".repeat(32);
    let foreign = ok(
        record_gate(&pg, &made, gate_in("G3", "fixture_authority", "pass", false, "fixture_certificate", Some(&foreign_hash))).await,
        "outcome_recorded",
    );
    assert_eq!(foreign.body["disposition"], hand("disposition_foreign"));
    let fence = ok(record_checkpoint(&pg, &made, "fence", "e1", "stale", false).await, "outcome_recorded");
    assert_eq!(fence.body["disposition"], hand("disposition_not_fresh"));
    let refused_plan = refused(plan(&plan_request(&pg, &made, "test_scope", None)).await.expect("plan"), "reason_not_ready");
    has_reason(&refused_plan, "foreign");
    has_reason(&refused_plan, "stale_fence");
    assert_eq!(refused_plan.body["state"], hand("state_built"));
    assert!(pointer(&pg).await.is_none());
}

#[tokio::test]
async fn review_06_conflict_gate_and_fence() {
    let pg = load_pg();
    let (effective, wide, _) = load_docs();
    let authority = fs::read_to_string(env_var("OO_A366_AUTHORITY")).expect("authority");
    let made = stage_new(&pg, "rev-k", "rv-k", None, &effective, &wide).await;
    for gate in GATE_IDS {
        if *gate == "G4" {
            let skipped = ok(
                record_gate(&pg, &made, gate_in(gate, "test_issuer", "not_run", false, "test_issuer", None)).await,
                "outcome_recorded",
            );
            assert_eq!(skipped.body["disposition"], hand("disposition_not_run"));
            let second = ok(
                record_gate(&pg, &made, gate_in(gate, "test_issuer", "pass", false, "test_issuer", None)).await,
                "outcome_recorded",
            );
            assert_eq!(second.body["disposition"], hand("disposition_admissible"));
            continue;
        }
        let receipt = if *gate == "G8" {
            record_gate(&pg, &made, gate_in(gate, "component_authority", "not_run", false, "component_recompile", None)).await
        } else if *gate == "G9" {
            record_gate(&pg, &made, gate_in(gate, "fixture_authority", "pass", false, "fixture_certificate", Some(authority.trim()))).await
        } else {
            record_gate(&pg, &made, gate_in(gate, "test_issuer", "pass", false, "test_issuer", None)).await
        };
        ok(receipt, "outcome_recorded");
    }
    ok(record_checkpoint(&pg, &made, "fence", "e1", "closed", false).await, "outcome_recorded");
    ok(record_checkpoint(&pg, &made, "fence", "e2", "closed", false).await, "outcome_recorded");
    ok(record_checkpoint(&pg, &made, "replica", "e1", "replayed", false).await, "outcome_recorded");
    let conflict = refused(plan(&plan_request(&pg, &made, "test_scope", None)).await.expect("plan"), "reason_not_ready");
    has_reason(&conflict, "conflict_gate");
    has_reason(&conflict, "conflict_fence");
    assert_eq!(conflict.body["state"], hand("state_built"));
    assert!(pointer(&pg).await.is_none());
}

#[tokio::test]
async fn review_07_applying_pointer_unchanged() {
    let pg = load_pg();
    let (effective, wide, _) = load_docs();
    let authority = fs::read_to_string(env_var("OO_A366_AUTHORITY")).expect("authority");
    let made = stage_new(&pg, "rev-a", "rv-a", None, &effective, &wide).await;
    arm(&pg, &made, authority.trim()).await;
    let fake = ok(record_checkpoint(&pg, &made, "fence", "term", "closed", true).await, "outcome_recorded");
    assert_eq!(fake.body["disposition"], hand("disposition_fabricated"));
    assert_eq!(fake.body["sessions_terminated_by_tool"], false);
    let blocked = refused(plan(&plan_request(&pg, &made, "production", None)).await.expect("production plan"), "production_blocked");
    has_reason(&blocked, "credential_missing");
    has_reason(&blocked, "fence_not_production");
    assert_eq!(blocked.body["production_outcome"], hand("production_blocked"));
    assert_eq!(blocked.body["state"], hand("state_built"));
    let blocked_activate = refused(activate(&plan_request(&pg, &made, "production", None)).await.expect("production activate"), "production_blocked");
    assert_eq!(blocked_activate.body["state"], hand("state_built"));
    assert!(pointer(&pg).await.is_none());
    let planned = ok(plan(&plan_request(&pg, &made, "test_scope", None)).await.expect("plan"), "outcome_planned");
    assert_eq!(planned.body["state"], hand("state_passed"));
    let applying = ok(enter_applying(&plan_request(&pg, &made, "test_scope", None)).await.expect("enter"), "outcome_applying");
    assert_eq!(applying.body["state"], hand("state_applying"));
    assert_eq!(applying.body["half_published"], false);
    assert!(applying.body["pointer_version"].is_null());
    assert!(pointer(&pg).await.is_none());
    let (state, _, _) = candidate_row(&pg, made.id).await;
    assert_eq!(state, hand("state_applying"));
}

#[tokio::test]
async fn review_08_recover_leaves_pointer() {
    let pg = load_pg();
    let (effective, wide, _) = load_docs();
    let recovered = ok(recover(&recover_request(&pg, "recover", "rev-a", None)).await.expect("recover"), "outcome_recovered");
    assert_eq!(recovered.body["state"], hand("state_failed"));
    assert_eq!(recovered.body["half_published"], false);
    assert!(pointer(&pg).await.is_none());
    let replay = ok(recover(&recover_request(&pg, "recover", "rev-a", None)).await.expect("replay"), "outcome_replay");
    assert_eq!(replay.body["state"], hand("state_failed"));
    let made = reseal("rev-a", "rv-a", None, &effective, &wide);
    let planned = ok(plan(&plan_request(&pg, &made, "test_scope", None)).await.expect("replan"), "outcome_planned");
    assert_eq!(planned.body["state"], hand("state_passed"));
    assert!(pointer(&pg).await.is_none());
    let (state, _, _) = candidate_row(&pg, "rev-a").await;
    assert_eq!(state, hand("state_passed"));
}

#[tokio::test]
async fn review_09_activation_audit_and_cas() {
    let pg = load_pg();
    let (effective, wide, _) = load_docs();
    let authority = fs::read_to_string(env_var("OO_A366_AUTHORITY")).expect("authority");
    let winner = stage_new(&pg, "rev-d", "rv-d", None, &effective, &wide).await;
    let loser = stage_new(&pg, "rev-w", "rv-w", None, &effective, &wide).await;
    assert_ne!(winner.source, loser.source);
    assert_eq!(winner.pack, loser.pack);
    arm(&pg, &winner, authority.trim()).await;
    arm(&pg, &loser, authority.trim()).await;
    assert_eq!(ok(plan(&plan_request(&pg, &winner, "test_scope", None)).await.expect("plan d"), "outcome_planned").body["state"], hand("state_passed"));
    assert_eq!(ok(plan(&plan_request(&pg, &loser, "test_scope", None)).await.expect("plan w"), "outcome_planned").body["state"], hand("state_passed"));
    let enter_d = ok(enter_applying(&plan_request(&pg, &winner, "test_scope", None)).await.expect("enter d"), "outcome_applying");
    let enter_w = ok(enter_applying(&plan_request(&pg, &loser, "test_scope", None)).await.expect("enter w"), "outcome_applying");
    assert!(enter_d.body["pointer_version"].is_null());
    assert!(enter_w.body["pointer_version"].is_null());
    assert_eq!(enter_d.body["half_published"], false);
    assert!(pointer(&pg).await.is_none());
    let activated = ok(commit_activation(&plan_request(&pg, &winner, "test_scope", None)).await.expect("commit d"), "outcome_activated");
    assert_eq!(activated.body["certificate_class"], hand("certificate_class_activation"));
    assert_eq!(activated.body["state"], hand("state_active"));
    assert_eq!(activated.body["pointer_version"], "rv-d");
    assert!(activated.body["previous_version"].is_null());
    assert_eq!(activated.body["half_published"], false);
    assert_eq!(activated.body["production_ready_certificate"], false);
    let audit_id = activated.body["audit_id"].as_str().expect("audit").to_owned();
    fs::write(env_var("OO_A366_AUDIT"), &audit_id).expect("audit file");
    assert_events(&pg, "rev-d").await;
    assert_eq!(pointer(&pg).await.as_deref(), Some("rv-d"));
    let lost = refused(commit_activation(&plan_request(&pg, &loser, "test_scope", None)).await.expect("commit w"), "reason_cas");
    assert_eq!(lost.body["state"], hand("state_failed"));
    assert_eq!(lost.body["pointer_version"], "rv-d");
    assert_eq!(lost.body["half_published"], false);
    assert_eq!(lost.body["certificate_class"], hand("certificate_class_other"));
    let (winner_state, winner_pack, _) = candidate_row(&pg, "rev-d").await;
    let (loser_state, loser_pack, _) = candidate_row(&pg, "rev-w").await;
    assert_eq!(winner_state, hand("state_active"));
    assert_eq!(loser_state, hand("state_failed"));
    assert_eq!(winner_pack, winner.pack);
    assert_eq!(loser_pack, loser.pack);
    assert_eq!(pointer(&pg).await.as_deref(), Some("rv-d"));
}

#[tokio::test]
async fn review_10_replay_same_audit() {
    let pg = load_pg();
    let (effective, wide, _) = load_docs();
    let made = reseal("rev-d", "rv-d", None, &effective, &wide);
    let replay = ok(activate(&plan_request(&pg, &made, "test_scope", None)).await.expect("replay"), "outcome_replay");
    assert_eq!(replay.body["audit_id"], fs::read_to_string(env_var("OO_A366_AUDIT")).expect("audit").trim());
    assert_eq!(replay.body["certificate_class"], hand("certificate_class_activation"));
    assert_eq!(replay.body["state"], hand("state_active"));
    assert_eq!(replay.body["pointer_version"], "rv-d");
    assert_eq!(pointer(&pg).await.as_deref(), Some("rv-d"));
}

#[tokio::test]
async fn review_11_rollback_restores_pointer_and_pack() {
    let pg = load_pg();
    let (effective, wide, _) = load_docs();
    let authority = fs::read_to_string(env_var("OO_A366_AUTHORITY")).expect("authority");
    let (_, d_pack, d_manifest) = candidate_row(&pg, "rev-d").await;
    let made = stage_new(&pg, "rev-g", "rv-g", Some("rv-d"), &effective, &wide).await;
    arm(&pg, &made, authority.trim()).await;
    assert_eq!(ok(plan(&plan_request(&pg, &made, "test_scope", Some("rv-d"))).await.expect("plan g"), "outcome_planned").body["state"], hand("state_passed"));
    let activated = ok(activate(&plan_request(&pg, &made, "test_scope", Some("rv-d"))).await.expect("activate g"), "outcome_activated");
    assert_eq!(activated.body["pointer_version"], "rv-g");
    assert_eq!(activated.body["previous_version"], "rv-d");
    assert_eq!(activated.body["certificate_class"], hand("certificate_class_activation"));
    assert_eq!(activated.body["half_published"], false);
    let (d_state, d_pack_now, d_manifest_now) = candidate_row(&pg, "rev-d").await;
    let (g_state, g_pack, g_manifest) = candidate_row(&pg, "rev-g").await;
    assert_eq!(d_state, hand("state_inactive"));
    assert_eq!(g_state, hand("state_active"));
    assert_eq!(d_pack_now, d_pack);
    assert_eq!(d_manifest_now, d_manifest);
    assert_eq!(g_pack, made.pack);
    assert_eq!(g_manifest, made.manifest);
    let rolled = ok(recover(&recover_request(&pg, "rollback", "rev-g", Some("rv-g"))).await.expect("rollback"), "outcome_rolled");
    assert_eq!(rolled.body["state"], hand("state_inactive"));
    assert_eq!(rolled.body["pointer_version"], "rv-d");
    assert_eq!(rolled.body["half_published"], false);
    assert_eq!(rolled.body["certificate_class"], hand("certificate_class_other"));
    let (d_state, d_pack_now, _) = candidate_row(&pg, "rev-d").await;
    let (g_state, g_pack_now, _) = candidate_row(&pg, "rev-g").await;
    assert_eq!(d_state, hand("state_active"));
    assert_eq!(g_state, hand("state_inactive"));
    assert_eq!(d_pack_now, d_pack);
    assert_eq!(g_pack_now, made.pack);
    assert_eq!(pointer(&pg).await.as_deref(), Some("rv-d"));
    let replay = ok(recover(&recover_request(&pg, "rollback", "rev-g", Some("rv-g"))).await.expect("rollback replay"), "outcome_replay");
    assert_eq!(replay.body["state"], hand("state_inactive"));
    assert_eq!(pointer(&pg).await.as_deref(), Some("rv-d"));
}

#[tokio::test]
async fn review_12_test_issuer_never_production_certificate() {
    let pg = load_pg();
    let (effective, wide, narrow) = load_docs();
    let made_n = reseal("rev-n", "rv-n", None, &effective, &wide);
    for gate in ["credential", "integrity", "authenticated_owner", "customer", "runsc"] {
        let recorded = ok(
            record_gate(&pg, &made_n, gate_in(gate, "test_issuer", "pass", false, "test_issuer", None)).await,
            "outcome_recorded",
        );
        assert_eq!(recorded.body["credential_verified"], false);
        assert_eq!(recorded.body["integrity_verified"], false);
        assert_eq!(recorded.body["owner_authenticated"], false);
        assert_eq!(recorded.body["customer_gate_verified"], false);
        assert_eq!(recorded.body["runsc_verified"], false);
        assert_eq!(recorded.body["production_ready_certificate"], false);
        assert_eq!(recorded.body["certificate_class"], hand("certificate_class_other"));
    }
    deployment_flags_false(&pg).await;
    let aborted = ok(recover(&recover_request(&pg, "abort", "rev-k", None)).await.expect("abort"), "outcome_aborted");
    assert_eq!(aborted.body["state"], hand("state_aborted"));
    let abort_replay = ok(recover(&recover_request(&pg, "abort", "rev-k", None)).await.expect("abort replay"), "outcome_replay");
    assert_eq!(abort_replay.body["state"], hand("state_aborted"));
    let retired = ok(recover(&recover_request(&pg, "retire", "rev-s", None)).await.expect("retire"), "outcome_retired");
    assert_eq!(retired.body["state"], hand("state_retired"));
    let (_, pack_before, _) = candidate_row(&pg, "rev-d").await;
    let active = refused(recover(&recover_request(&pg, "abort", "rev-d", None)).await.expect("abort active"), "reason_seal");
    assert_eq!(active.body["state"], hand("state_active"));
    refused(stage_at(&pg, "rev-d", "rv-d", Some("rv-d"), &effective, &narrow).await, "reason_seal");
    let (state, pack_after, _) = candidate_row(&pg, "rev-d").await;
    assert_eq!(state, hand("state_active"));
    assert_eq!(pack_after, pack_before);
    let made_d = reseal("rev-d", "rv-d", None, &effective, &wide);
    let current = ok(status(&plan_request(&pg, &made_d, "test_scope", None)).await.expect("status"), "outcome_status");
    assert_eq!(current.body["state"], hand("state_active"));
    assert_eq!(current.body["pointer_version"], "rv-d");
    assert_eq!(current.body["pointer_is_candidate"], true);
    assert_eq!(current.body["certificate_class"], hand("certificate_class_other"));
    assert_eq!(current.body["production_ready_certificate"], false);
    assert_eq!(current.body["production_blocked"], true);
    assert_eq!(current.body["half_published"], false);
    assert!(current.body["test_reasons"].as_array().expect("reasons").is_empty(), "{}", current.body);
}

#[tokio::test]
async fn review_13_reader_privileges() {
    let pg = load_pg();
    let (effective, wide, _) = load_docs();
    let tls = crate::rustls_connector(&pg.ca).expect("reader tls");
    let mut config = tokio_postgres::Config::new();
    config.user("oo_release_reader_login");
    config.password(&pg.reader_password);
    config.host(&pg.endpoint.host);
    config.port(pg.endpoint.port);
    config.dbname(&pg.endpoint.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("e53-review-reader");
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
    for index in 0..5 {
        let value: bool = row.get(index);
        assert!(!value, "view column {index}");
    }
    let version: Option<String> = row.get(5);
    assert_eq!(version.as_deref(), Some("rv-d"));
    let denied = client.batch_execute("UPDATE oo_release_control.active_pointer SET version = version").await.expect_err("update");
    assert_eq!(denied.code().map(|code| code.code()), Some(hand("reader_sqlstate").as_str()));
    let hidden = client.query("SELECT candidate_id FROM oo_release_control.candidate", &[]).await.expect_err("table");
    assert_eq!(hidden.code().map(|code| code.code()), Some(hand("reader_sqlstate").as_str()));
    let made = reseal("rev-d", "rv-d", None, &effective, &wide);
    let wrong = status(&PlanRequest {
        endpoint: &pg.endpoint,
        user: "oo_release_admin",
        password: &pg.password,
        ca_pem: b"not-a-certificate",
        project_id: PROJECT,
        candidate_id: made.id,
        source_hash: &made.source,
        scope: "test_scope",
        expected_version: None,
    })
    .await;
    assert!(matches!(wrong, Err(StoreError::Tls)));
    let note = json!({
        "candidate_id": "rev-d",
        "version": "rv-d",
        "source_hash": made.source,
        "pack_digest": made.pack,
        "manifest_sha256": made.manifest,
        "pointer_version": "rv-d",
        "activation_certificate_class": hand("certificate_class_activation"),
        "status_certificate_class": hand("certificate_class_other"),
        "production_ready_certificate": false,
        "full_sp08_accepted": false
    });
    fs::write(env_var("OO_A366_OBSERVATION"), serde_json::to_vec_pretty(&note).expect("note")).expect("observation");
}
