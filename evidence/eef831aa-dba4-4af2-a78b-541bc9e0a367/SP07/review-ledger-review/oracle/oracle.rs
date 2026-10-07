//! Independent E52 oracle. Copied into the mutable archive only for this run.
//! It does not edit the product. A green run is not publication or management RBAC.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use description_pack::apply_description_pack;
use review_ledger::{
    batch, decide, enqueue, history, install, parse_control, receipt, screen_request, sha256_hex,
    BatchItem, BatchRequest, DecideRequest, Endpoint, EnqueueRequest, HistoryRequest,
    InstallRequest, LedgerError, Output, ReceiptRequest,
};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use serde_json::{json, Value};
use tokio_postgres::config::SslMode;
use tokio_postgres::Config;
use tokio_postgres_rustls::MakeRustlsConnect;

#[allow(dead_code)]
#[path = "support/world.rs"]
mod fixture;

const PROJECT: &str = "ora367";
const DATE: &str = "2026-10-08";
const REVIEWER_ID: &str = "rev.a367";
const REVIEWER_LABEL: &str = "local.reviewer";
const OWNER_ID: &str = "ada.owner";
const DATASET: &str = "public.shop";
const COLUMN: &str = "public.shop.note";
const OUTSIDER: &str = "public.outsider";
const PROTOCOL: &str = "oo.builder.description-candidate/v1";

struct Material {
    control: String,
    reviewer: String,
    auditor: String,
    bad_command: String,
    pack_command: String,
}

#[derive(Clone)]
struct Digests {
    candidate: String,
    input: String,
    evidence: String,
    schema: String,
    curated: String,
    source: String,
}

#[tokio::test]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn independent_boundary() {
    let host = env::var("REVIEW_LEDGER_HOST").expect("REVIEW_LEDGER_HOST");
    let database = env::var("REVIEW_LEDGER_DATABASE").expect("REVIEW_LEDGER_DATABASE");
    assert_eq!(database, "oo_a367_oracle");
    let material = write_material();
    let admin = endpoint(
        &host,
        &database,
        "oo_review_ledger_admin",
        &env_var("REVIEW_LEDGER_ADMIN_PASSWORD_FILE"),
    );
    let runtime = endpoint(
        &host,
        &database,
        "oo_a367_runtime",
        &env_var("REVIEW_LEDGER_RUNTIME_PASSWORD_FILE"),
    );
    let superuser = endpoint(
        &host,
        &database,
        "oo_a367_admin",
        &env_var("REVIEW_LEDGER_SUPER_PASSWORD_FILE"),
    );

    declared_authority_refuses_before_database();
    let installed = must(
        install(&InstallRequest {
            protocol: "oo.review-ledger.install/v1".to_owned(),
            project_id: PROJECT.to_owned(),
            control_file: material.control.clone(),
            postgres: clone_endpoint(&admin),
        })
        .await,
        "installed",
    );
    assert_eq!(installed.kind, "persisted");
    assert_flags(&installed);
    let client = connect(&superuser).await;
    let track: String = client
        .query_one("SELECT current_setting('track_counts')", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(track, "on");

    let dataset = fill_set(0x11);
    must(
        put(&runtime, &material, "dataset", DATASET, &dataset, None).await,
        "persisted",
    );
    refuse(
        put(&runtime, &material, "dataset", DATASET, &dataset, Some(9)).await,
        "revision.conflict",
    );

    let before_rows = counts(&client).await;
    let before_tuples = tuples(&client).await;
    refuse(auditor_batch(&runtime, &material, &dataset).await, "role.forbidden");
    let after_rows = counts(&client).await;
    let after_tuples = tuples(&client).await;
    assert_eq!(before_rows, after_rows, "auditor batch changed ledger rows");
    assert_eq!(before_tuples, after_tuples, "auditor batch changed tuple counters");
    assert_eq!(count_key(&client, "decision", "auditor.batch").await, 0);
    assert_eq!(count_batch(&client, "baudit").await, 0);

    let audits_before_history = count_table(&client, "audit_event").await;
    let tuples_before_history = tuples(&client).await;
    let seen = must(auditor_history(&runtime, &material).await, "history");
    assert_eq!(seen.kind, "history");
    assert!(seen.history_sha256.is_some());
    assert_flags(&seen);
    assert_eq!(count_table(&client, "audit_event").await, audits_before_history);
    assert_eq!(tuples(&client).await, tuples_before_history);

    let rows_before_refuse = counts(&client).await;
    refuse(
        choose(
            &runtime,
            &material,
            &material.reviewer,
            "other.reviewer",
            DATASET,
            "label.mismatch",
            "label_description",
            "reject",
            1,
            &dataset,
            None,
        )
        .await,
        "reviewer.mismatch",
    );
    refuse(
        choose(
            &runtime,
            &material,
            &material.reviewer,
            REVIEWER_LABEL,
            OUTSIDER,
            "scope.out",
            "label_description",
            "reject",
            1,
            &dataset,
            None,
        )
        .await,
        "scope.unauthorized",
    );
    refuse(
        choose(
            &runtime,
            &material,
            &material.reviewer,
            REVIEWER_LABEL,
            DATASET,
            "revision.late",
            "label_description",
            "reject",
            9,
            &dataset,
            None,
        )
        .await,
        "revision.conflict",
    );
    let mut stale = dataset.clone();
    stale.input = fill(0x5a);
    refuse(
        choose(
            &runtime,
            &material,
            &material.reviewer,
            REVIEWER_LABEL,
            DATASET,
            "stale.hash",
            "label_description",
            "reject",
            1,
            &stale,
            None,
        )
        .await,
        "stale",
    );
    refuse(
        choose(
            &runtime,
            &material,
            &material.reviewer,
            REVIEWER_LABEL,
            DATASET,
            "owner.try",
            "owner_certification",
            "record",
            1,
            &dataset,
            None,
        )
        .await,
        "reviewer_not_owner",
    );
    assert_eq!(counts(&client).await, rows_before_refuse);
    assert_eq!(count_key(&client, "decision", "owner.try").await, 0);
    assert_eq!(count_key(&client, "decision", "stale.hash").await, 0);
    assert_eq!(count_key(&client, "decision", "revision.late").await, 0);
    assert_eq!(count_key(&client, "decision", "scope.out").await, 0);

    let decisions_before = count_table(&client, "decision").await;
    let audits_before = count_table(&client, "audit_event").await;
    let approved = must(
        choose(
            &runtime,
            &material,
            &material.reviewer,
            REVIEWER_LABEL,
            DATASET,
            "label.shop",
            "label_description",
            "approve",
            1,
            &dataset,
            Some(approve_fields()),
        )
        .await,
        "persisted",
    );
    assert_flags(&approved);
    assert_eq!(count_table(&client, "decision").await, decisions_before + 1);
    assert_eq!(count_table(&client, "audit_event").await, audits_before + 1);
    let stored = decision_flags(&client, "label.shop").await;
    assert!(!stored.0 && !stored.1 && !stored.2);
    assert_eq!(stored.3, "not_authorized");
    let replay = must(
        choose(
            &runtime,
            &material,
            &material.reviewer,
            REVIEWER_LABEL,
            DATASET,
            "label.shop",
            "label_description",
            "approve",
            1,
            &dataset,
            Some(approve_fields()),
        )
        .await,
        "replay",
    );
    assert_flags(&replay);
    assert_eq!(count_table(&client, "decision").await, decisions_before + 1);
    assert_eq!(count_table(&client, "audit_event").await, audits_before + 1);
    refuse(
        choose(
            &runtime,
            &material,
            &material.reviewer,
            REVIEWER_LABEL,
            DATASET,
            "label.shop",
            "label_description",
            "reject",
            1,
            &dataset,
            None,
        )
        .await,
        "idempotency.conflict",
    );
    assert_eq!(count_key(&client, "decision", "label.shop").await, 1);
    assert_eq!(count_table(&client, "audit_event").await, audits_before + 1);

    let poison_body = fill(0xab);
    client
        .execute(
            "INSERT INTO oo_review_ledger.audit_event ( \
                project_id, idempotency_key, action, detail, object_id, revision, body_sha256, principal_id \
             ) VALUES ($1, 'poison.a367', 'decide', '', $2, 1, $3, 'poison')",
            &[&PROJECT, &DATASET, &poison_body],
        )
        .await
        .expect("poison audit");
    let decisions_at_poison = count_table(&client, "decision").await;
    refuse(
        choose(
            &runtime,
            &material,
            &material.reviewer,
            REVIEWER_LABEL,
            DATASET,
            "poison.a367",
            "label_description",
            "reject",
            1,
            &dataset,
            None,
        )
        .await,
        "audit.rollback",
    );
    assert_eq!(count_key(&client, "decision", "poison.a367").await, 0);
    assert_eq!(count_table(&client, "decision").await, decisions_at_poison);

    let packed = fixture::pack_fixture();
    assert!(
        packed.outcome_name == "packed" || packed.outcome_name == "reviewed_unchanged",
        "{}",
        packed.outcome_name
    );
    let again = apply_description_pack(&packed.command_text).expect("pack compiler");
    assert_eq!(again, packed.outcome_text);
    let document: Value = serde_json::from_str(&again).expect("pack json");
    assert_eq!(document["outcome"], packed.outcome_name);
    assert_eq!(document["protocol"], description_pack::PACK_PROTOCOL);
    assert_eq!(document["compiler"], description_pack::PACK_COMPILER);
    assert_eq!(document["statements"]["authentication"], "not_claimed");
    assert_eq!(document["statements"]["publication"], "unavailable");
    assert_eq!(document["statements"]["grants"], "not_authorized");
    let pack_source = sha256_hex(packed.command_text.as_bytes()).unwrap();
    let pack_outcome = sha256_hex(again.as_bytes()).unwrap();
    fs::write(&material.pack_command, packed.command_text.as_bytes()).unwrap();

    let malformed = apply_description_pack("{").expect_err("malformed pack");
    assert!(
        malformed.code.starts_with("oo.builder.description-pack-command/v1"),
        "{}",
        malformed.code
    );
    fs::write(&material.bad_command, b"{").unwrap();
    let bad_source = sha256_hex(b"{").unwrap();
    let mut bad = fill_set(0x51);
    bad.source = bad_source;
    must(
        put(&runtime, &material, "column", COLUMN, &bad, None).await,
        "persisted",
    );
    must(
        choose(
            &runtime,
            &material,
            &material.reviewer,
            REVIEWER_LABEL,
            COLUMN,
            "class.bad",
            "classification",
            "confirm",
            1,
            &bad,
            Some(classification_fields()),
        )
        .await,
        "persisted",
    );
    let receipts_before_bad = count_table(&client, "compiler_receipt").await;
    refuse(
        pack_receipt(&runtime, &material, "class.bad", &material.bad_command).await,
        "compiler.refused",
    );
    assert_eq!(count_table(&client, "compiler_receipt").await, receipts_before_bad);
    assert_eq!(count_key(&client, "compiler_receipt", "class.bad").await, 0);
    let bad_decision = decision_flags(&client, "class.bad").await;
    assert!(!bad_decision.0 && !bad_decision.1 && !bad_decision.2);
    assert_eq!(bad_decision.3, "not_authorized");

    let mut note = fill_set(0x61);
    note.source = pack_source.clone();
    must(
        put(&runtime, &material, "column", "public.shop.pack", &note, None).await,
        "persisted",
    );
    let classified = must(
        choose(
            &runtime,
            &material,
            &material.reviewer,
            REVIEWER_LABEL,
            "public.shop.pack",
            "class.pack",
            "classification",
            "confirm",
            1,
            &note,
            Some(classification_fields()),
        )
        .await,
        "persisted",
    );
    assert_flags(&classified);
    let body_before = decision_body(&client, "class.pack").await;
    let bound = must(
        pack_receipt(&runtime, &material, "class.pack", &material.pack_command).await,
        "bound",
    );
    assert_eq!(bound.kind, "bound");
    assert_flags(&bound);
    let row = receipt_row(&client, "class.pack").await;
    assert_eq!(row.0, pack_source);
    assert_eq!(row.1, note.candidate);
    assert_eq!(row.2, pack_outcome);
    assert_eq!(row.3, description_pack::PACK_PROTOCOL);
    assert!(!row.4 && !row.5 && !row.6);
    assert_eq!(decision_body(&client, "class.pack").await, body_before);
    let after_decision = decision_flags(&client, "class.pack").await;
    assert!(!after_decision.0 && !after_decision.1 && !after_decision.2);
    assert_eq!(after_decision.3, "not_authorized");
    must(
        pack_receipt(&runtime, &material, "class.pack", &material.pack_command).await,
        "replay",
    );
    assert_eq!(count_key(&client, "compiler_receipt", "class.pack").await, 1);

    let summary = json!({
        "project": PROJECT,
        "database": database,
        "track_counts": track,
        "auditor_batch": "role.forbidden",
        "auditor_tuples_unchanged": true,
        "pack_outcome": packed.outcome_name,
        "pack_command_sha256": pack_source,
        "pack_candidate_sha256": note.candidate,
        "pack_outcome_sha256": pack_outcome,
        "git_applied": false,
        "published": false,
        "curated_files_written": false,
        "grants": "not_authorized",
        "authenticated": false
    });
    let out = env_var("REVIEW_LEDGER_ORACLE_OUT");
    fs::write(&out, serde_json::to_vec_pretty(&summary).unwrap()).unwrap();
    println!("{summary}");
}

fn declared_authority_refuses_before_database() {
    let mut claimed = control_value();
    claimed["authority"] = json!("verified");
    let error = parse_control(&serde_json::to_vec(&claimed).unwrap()).expect_err("authority");
    assert_eq!(error.code, "authority.not_verified");
    let mut identity = control_value();
    identity["management_identity"] = json!("claimed");
    let error = parse_control(&serde_json::to_vec(&identity).unwrap()).expect_err("identity");
    assert_eq!(error.code, "authority.not_verified");
    let mut role = control_value();
    role["principals"][0]["role"] = json!("admin");
    let error = parse_control(&serde_json::to_vec(&role).unwrap()).expect_err("role");
    assert_eq!(error.code, "control.role");
    for key in ["grant", "role", "publish", "authenticated", "auto_accept"] {
        let mut request = json!({
            "protocol": "oo.review-ledger.decide/v1",
            "project_id": PROJECT
        });
        request[key] = json!(true);
        let error = screen_request(&serde_json::to_vec(&request).unwrap()).expect_err(key);
        assert_eq!(error.code, "not_grant", "{key}");
    }
}

fn control_value() -> Value {
    json!({
        "protocol": "oo.review-ledger.control/v1",
        "project_id": PROJECT,
        "authority": "not_verified",
        "management_identity": "not_claimed",
        "principals": [
            principal(REVIEWER_ID, "reviewer", REVIEWER_LABEL, "a367-reviewer-credential", &[DATASET, COLUMN, "public.shop.pack"]),
            principal(OWNER_ID, "owner", OWNER_ID, "a367-owner-credential", &[DATASET]),
            principal("aud.a367", "auditor", "aud.a367", "a367-auditor-credential", &[DATASET])
        ]
    })
}

fn write_material() -> Material {
    let root = PathBuf::from(env_var("REVIEW_LEDGER_STATE")).join("oracle");
    fs::create_dir_all(&root).unwrap();
    let control = root.join("control.json");
    let reviewer = root.join("reviewer.cred");
    let auditor = root.join("auditor.cred");
    fs::write(&reviewer, b"a367-reviewer-credential").unwrap();
    fs::write(root.join("owner.cred"), b"a367-owner-credential").unwrap();
    fs::write(&auditor, b"a367-auditor-credential").unwrap();
    fs::write(&control, serde_json::to_vec(&control_value()).unwrap()).unwrap();
    Material {
        control: control.display().to_string(),
        reviewer: reviewer.display().to_string(),
        auditor: auditor.display().to_string(),
        bad_command: root.join("bad.json").display().to_string(),
        pack_command: root.join("pack.json").display().to_string(),
    }
}

fn principal(id: &str, role: &str, label: &str, credential: &str, scope: &[&str]) -> Value {
    json!({
        "credential_sha256": sha256_hex(credential.as_bytes()).unwrap(),
        "principal_id": id,
        "reviewer_label": label,
        "role": role,
        "scope": scope
    })
}

fn endpoint(host: &str, database: &str, user: &str, password_file: &str) -> Endpoint {
    Endpoint {
        host: host.to_owned(),
        port: env::var("REVIEW_LEDGER_PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(5432),
        database: database.to_owned(),
        user: user.to_owned(),
        password_file: password_file.to_owned(),
        ca_file: env_var("REVIEW_LEDGER_CA"),
        server_name: host.to_owned(),
    }
}

fn clone_endpoint(endpoint: &Endpoint) -> Endpoint {
    Endpoint {
        host: endpoint.host.clone(),
        port: endpoint.port,
        database: endpoint.database.clone(),
        user: endpoint.user.clone(),
        password_file: endpoint.password_file.clone(),
        ca_file: endpoint.ca_file.clone(),
        server_name: endpoint.server_name.clone(),
    }
}

fn approve_fields() -> Value {
    json!({
        "label": {"overwrite": false},
        "description": {"overwrite": false},
        "grain": {"overwrite": false}
    })
}

fn classification_fields() -> Value {
    json!({"pii": "none", "semantic_type": "identifier", "state": "confirmed"})
}

fn fill(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn fill_set(byte: u8) -> Digests {
    Digests {
        candidate: fill(byte),
        input: fill(byte.wrapping_add(1)),
        evidence: fill(byte.wrapping_add(2)),
        schema: fill(byte.wrapping_add(3)),
        curated: fill(byte.wrapping_add(4)),
        source: fill(byte.wrapping_add(5)),
    }
}

async fn put(
    postgres: &Endpoint,
    material: &Material,
    kind: &str,
    object_id: &str,
    digests: &Digests,
    expected: Option<i64>,
) -> Result<Output, LedgerError> {
    enqueue(&EnqueueRequest {
        protocol: "oo.review-ledger.enqueue/v1".to_owned(),
        project_id: PROJECT.to_owned(),
        control_file: material.control.clone(),
        credential_file: material.reviewer.clone(),
        postgres: clone_endpoint(postgres),
        object_kind: kind.to_owned(),
        object_id: object_id.to_owned(),
        expected_revision: expected,
        candidate_sha256: digests.candidate.clone(),
        input_sha256: digests.input.clone(),
        evidence_sha256: digests.evidence.clone(),
        schema_sha256: digests.schema.clone(),
        curated_sha256: digests.curated.clone(),
        source_sha256: digests.source.clone(),
        candidate_protocol: PROTOCOL.to_owned(),
        owner_label: OWNER_ID.to_owned(),
    })
    .await
}

#[allow(clippy::too_many_arguments)]
async fn choose(
    postgres: &Endpoint,
    material: &Material,
    credential: &str,
    label: &str,
    object_id: &str,
    key: &str,
    kind: &str,
    choice: &str,
    revision: i64,
    digests: &Digests,
    fields: Option<Value>,
) -> Result<Output, LedgerError> {
    decide(&DecideRequest {
        protocol: "oo.review-ledger.decide/v1".to_owned(),
        project_id: PROJECT.to_owned(),
        control_file: material.control.clone(),
        credential_file: credential.to_owned(),
        postgres: clone_endpoint(postgres),
        object_id: object_id.to_owned(),
        expected_revision: revision,
        idempotency_key: key.to_owned(),
        decision_kind: kind.to_owned(),
        choice: choice.to_owned(),
        reviewer_label: label.to_owned(),
        reviewer_at: DATE.to_owned(),
        candidate_sha256: digests.candidate.clone(),
        input_sha256: digests.input.clone(),
        evidence_sha256: digests.evidence.clone(),
        schema_sha256: digests.schema.clone(),
        curated_sha256: digests.curated.clone(),
        source_sha256: digests.source.clone(),
        fields,
    })
    .await
}

async fn auditor_batch(
    postgres: &Endpoint,
    material: &Material,
    digests: &Digests,
) -> Result<Output, LedgerError> {
    batch(&BatchRequest {
        protocol: "oo.review-ledger.batch/v1".to_owned(),
        project_id: PROJECT.to_owned(),
        control_file: material.control.clone(),
        credential_file: material.auditor.clone(),
        postgres: clone_endpoint(postgres),
        batch_id: "baudit".to_owned(),
        accept_all: false,
        items: vec![item(DATASET, "auditor.batch", digests)],
    })
    .await
}

fn item(object_id: &str, key: &str, digests: &Digests) -> BatchItem {
    BatchItem {
        object_id: object_id.to_owned(),
        expected_revision: 1,
        idempotency_key: key.to_owned(),
        decision_kind: "label_description".to_owned(),
        choice: "reject".to_owned(),
        reviewer_label: "aud.a367".to_owned(),
        reviewer_at: DATE.to_owned(),
        candidate_sha256: digests.candidate.clone(),
        input_sha256: digests.input.clone(),
        evidence_sha256: digests.evidence.clone(),
        schema_sha256: digests.schema.clone(),
        curated_sha256: digests.curated.clone(),
        source_sha256: digests.source.clone(),
        fields: None,
    }
}

async fn auditor_history(postgres: &Endpoint, material: &Material) -> Result<Output, LedgerError> {
    history(&HistoryRequest {
        protocol: "oo.review-ledger.history/v1".to_owned(),
        project_id: PROJECT.to_owned(),
        control_file: material.control.clone(),
        credential_file: material.auditor.clone(),
        postgres: clone_endpoint(postgres),
    })
    .await
}

async fn pack_receipt(
    postgres: &Endpoint,
    material: &Material,
    key: &str,
    command: &str,
) -> Result<Output, LedgerError> {
    let column = if key == "class.bad" {
        COLUMN
    } else {
        "public.shop.pack"
    };
    receipt(&ReceiptRequest {
        protocol: "oo.review-ledger.receipt/v1".to_owned(),
        project_id: PROJECT.to_owned(),
        control_file: material.control.clone(),
        credential_file: material.reviewer.clone(),
        postgres: clone_endpoint(postgres),
        idempotency_key: key.to_owned(),
        compiler: "description-pack".to_owned(),
        command_file: command.to_owned(),
        required_columns: vec![column.to_owned()],
    })
    .await
}

fn must(result: Result<Output, LedgerError>, code: &str) -> Output {
    match result {
        Ok(output) => {
            assert_eq!(output.code, code, "kind {}", output.kind);
            assert_flags(&output);
            output
        }
        Err(error) => panic!("expected {code}, got {} {}", error.code, error.message),
    }
}

fn refuse(result: Result<Output, LedgerError>, code: &str) {
    match result {
        Ok(output) => panic!("expected {code}, got {}", output.code),
        Err(error) => assert_eq!(error.code, code, "{}", error.message),
    }
}

fn assert_flags(output: &Output) {
    assert!(!output.authenticated);
    assert_eq!(output.authority, "not_verified");
    assert_eq!(output.management_identity, "not_claimed");
    assert!(!output.curated_applied);
    assert!(!output.published);
    assert_eq!(output.grants, "not_authorized");
    assert!(!output.git_applied);
}

fn env_var(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("missing {name}"))
}

async fn connect(endpoint: &Endpoint) -> tokio_postgres::Client {
    let mut password = fs::read(&endpoint.password_file).unwrap();
    if password.last() == Some(&b'\n') {
        password.pop();
    }
    let password = String::from_utf8(password).unwrap();
    let ca = fs::read(&endpoint.ca_file).unwrap();
    let certificate = CertificateDer::from_pem_slice(&ca).unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate).unwrap();
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let config = rustls::ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let mut builder = Config::new();
    builder
        .host(&endpoint.host)
        .port(endpoint.port)
        .dbname(&endpoint.database)
        .user(&endpoint.user)
        .password(password)
        .ssl_mode(SslMode::Require)
        .connect_timeout(Duration::from_secs(20));
    let (client, task) = builder.connect(MakeRustlsConnect::new(config)).await.unwrap();
    tokio::spawn(async move {
        let _ = task.await;
    });
    client
}

async fn counts(client: &tokio_postgres::Client) -> (i64, i64, i64, i64) {
    let row = client
        .query_one(
            "SELECT \
                (SELECT count(*)::bigint FROM oo_review_ledger.batch), \
                (SELECT count(*)::bigint FROM oo_review_ledger.batch_item), \
                (SELECT count(*)::bigint FROM oo_review_ledger.decision), \
                (SELECT count(*)::bigint FROM oo_review_ledger.audit_event)",
            &[],
        )
        .await
        .unwrap();
    (row.get(0), row.get(1), row.get(2), row.get(3))
}

async fn tuples(client: &tokio_postgres::Client) -> (i64, i64, i64) {
    let row = client
        .query_one(
            "SELECT COALESCE(sum(n_tup_ins), 0)::bigint, \
                    COALESCE(sum(n_tup_upd), 0)::bigint, \
                    COALESCE(sum(n_tup_del), 0)::bigint \
             FROM pg_stat_user_tables WHERE schemaname = 'oo_review_ledger'",
            &[],
        )
        .await
        .unwrap();
    (row.get(0), row.get(1), row.get(2))
}

async fn count_table(client: &tokio_postgres::Client, table: &str) -> i64 {
    let sql = format!("SELECT count(*)::bigint FROM oo_review_ledger.{table}");
    client.query_one(&sql, &[]).await.unwrap().get(0)
}

async fn count_key(client: &tokio_postgres::Client, table: &str, key: &str) -> i64 {
    let sql = format!(
        "SELECT count(*)::bigint FROM oo_review_ledger.{table} WHERE project_id = $1 AND idempotency_key = $2"
    );
    client.query_one(&sql, &[&PROJECT, &key]).await.unwrap().get(0)
}

async fn count_batch(client: &tokio_postgres::Client, batch_id: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::bigint FROM oo_review_ledger.batch WHERE project_id = $1 AND batch_id = $2",
            &[&PROJECT, &batch_id],
        )
        .await
        .unwrap()
        .get(0)
}

async fn decision_flags(client: &tokio_postgres::Client, key: &str) -> (bool, bool, bool, String) {
    let row = client
        .query_one(
            "SELECT curated_applied, published, authenticated, grants \
             FROM oo_review_ledger.decision WHERE project_id = $1 AND idempotency_key = $2",
            &[&PROJECT, &key],
        )
        .await
        .unwrap();
    (row.get(0), row.get(1), row.get(2), row.get(3))
}

async fn decision_body(client: &tokio_postgres::Client, key: &str) -> String {
    client
        .query_one(
            "SELECT body_sha256 FROM oo_review_ledger.decision WHERE project_id = $1 AND idempotency_key = $2",
            &[&PROJECT, &key],
        )
        .await
        .unwrap()
        .get(0)
}

#[allow(clippy::type_complexity)]
async fn receipt_row(client: &tokio_postgres::Client, key: &str) -> (String, String, String, String, bool, bool, bool) {
    let row = client
        .query_one(
            "SELECT command_sha256, candidate_sha256, outcome_sha256, outcome_protocol, \
                    git_applied, published, curated_files_written \
             FROM oo_review_ledger.compiler_receipt \
             WHERE project_id = $1 AND idempotency_key = $2 AND compiler = 'description-pack'",
            &[&PROJECT, &key],
        )
        .await
        .unwrap();
    (
        row.get(0),
        row.get(1),
        row.get(2),
        row.get(3),
        row.get(4),
        row.get(5),
        row.get(6),
    )
}
