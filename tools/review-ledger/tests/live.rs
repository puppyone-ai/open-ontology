//! Private PostgreSQL proof. Refuses to pass when REVIEW_LEDGER_HOST is unset.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};
use description_pack::apply_description_pack;
use review_ledger::{
    batch, decide, enqueue, history, install, receipt, sha256_hex, BatchItem, BatchRequest,
    DecideRequest, Endpoint, EnqueueRequest, HistoryRequest, InstallRequest, LedgerError, Output,
    ReceiptRequest,
};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use tokio_postgres::config::SslMode;
use tokio_postgres::Config;
use tokio_postgres_rustls::MakeRustlsConnect;

#[path = "support/world.rs"]
mod world;

use world::review_fixture;

const PROJECT: &str = "shop";
const PROTOCOL_CANDIDATE: &str = "oo.builder.description-candidate/v1";
const DATE: &str = "2026-10-08";
const REVIEWER: &str = "local-operator";
const OWNER: &str = "ada.owner";
const DATABASE: &str = "oo_a358_review";
const ADMIN_USER: &str = "oo_review_ledger_admin";
const RUNTIME_USER: &str = "oo_a358_runtime";

struct Paths {
    control: String,
    reviewer: String,
    owner: String,
    auditor: String,
    review_command: String,
    pack_command: String,
    curated_dir: PathBuf,
}

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
async fn private_postgres_matrix() {
    let host = env::var("REVIEW_LEDGER_HOST").unwrap_or_else(|_| {
        panic!("REVIEW_LEDGER_HOST is required; this test does not skip a missing database");
    });
    let paths = write_material();
    if env::var("REVIEW_LEDGER_PHASE").ok().as_deref() == Some("restart") {
        assert_same_history(&paths).await;
        return;
    }
    let admin = endpoint(&host, ADMIN_USER, &env_var("REVIEW_LEDGER_ADMIN_PASSWORD_FILE"));
    let runtime = endpoint(&host, RUNTIME_USER, &env_var("REVIEW_LEDGER_RUNTIME_PASSWORD_FILE"));
    let installed = must(
        install(&InstallRequest {
            protocol: "oo.review-ledger.install/v1".to_owned(),
            project_id: PROJECT.to_owned(),
            control_file: paths.control.clone(),
            postgres: clone_endpoint(&admin),
        })
        .await,
        "installed",
    );
    assert_eq!(installed.kind, "persisted");
    let replay = must(
        install(&InstallRequest {
            protocol: "oo.review-ledger.install/v1".to_owned(),
            project_id: PROJECT.to_owned(),
            control_file: paths.control.clone(),
            postgres: clone_endpoint(&admin),
        })
        .await,
        "replay",
    );
    assert_eq!(replay.kind, "replay");

    let fixture = review_fixture();
    fs::write(&paths.review_command, fixture.command_text.as_bytes()).unwrap();
    let source = sha256_hex(fixture.command_text.as_bytes()).unwrap();
    let review = Digests {
        candidate: fixture.candidate_sha256.clone(),
        input: fixture.input_sha256.clone(),
        evidence: fixture.evidence_sha256.clone(),
        schema: fixture.schema_sha256.clone(),
        curated: fixture.curated_sha256.clone(),
        source,
    };
    let queued = must(put(&runtime, &paths, "dataset", "public.b", &review, None).await, "persisted");
    assert_eq!(queued.revision, Some(1));
    refuse(
        put(&runtime, &paths, "dataset", "public.b", &review, Some(9)).await,
        "revision.conflict",
    );
    let approved = must(
        choose(
            &runtime,
            &paths,
            &paths.reviewer,
            REVIEWER,
            "public.b",
            "label.b.1",
            "label_description",
            "approve",
            1,
            &review,
            Some(fixture.label_fields.clone()),
        )
        .await,
        "persisted",
    );
    let decision_body = approved.body_sha256.clone().unwrap();
    let again = must(
        choose(
            &runtime,
            &paths,
            &paths.reviewer,
            REVIEWER,
            "public.b",
            "label.b.1",
            "label_description",
            "approve",
            1,
            &review,
            Some(fixture.label_fields.clone()),
        )
        .await,
        "replay",
    );
    assert_eq!(again.body_sha256.as_deref(), Some(decision_body.as_str()));
    refuse(
        choose(
            &runtime,
            &paths,
            &paths.reviewer,
            REVIEWER,
            "public.b",
            "label.b.1",
            "label_description",
            "reject",
            1,
            &review,
            None,
        )
        .await,
        "idempotency.conflict",
    );
    must(put(&runtime, &paths, "dataset", "public.b", &review, Some(1)).await, "replay");
    assert_eq!(count("SELECT count(*)::bigint FROM oo_review_ledger.audit_event WHERE project_id = $1 AND object_id = $2 AND action = 'enqueue'", "public.b").await, 1);
    let mut drifted = review.clone();
    drifted.source = fill(0xee);
    refuse(
        put(&runtime, &paths, "dataset", "public.b", &drifted, Some(1)).await,
        "idempotency.conflict",
    );
    refuse(
        put_as(&runtime, &paths, &paths.auditor, "dataset", "public.b", &review, Some(1)).await,
        "role.forbidden",
    );
    refuse(
        choose(
            &runtime,
            &paths,
            &paths.reviewer,
            REVIEWER,
            "public.secret",
            "scope.secret",
            "label_description",
            "reject",
            1,
            &review,
            None,
        )
        .await,
        "scope.unauthorized",
    );

    let other = synthetic(0x21);
    must(put(&runtime, &paths, "dataset", "public.other", &other, None).await, "persisted");
    let mut changed = other.clone();
    changed.candidate = fill(0x22);
    refuse(
        choose(
            &runtime,
            &paths,
            &paths.reviewer,
            REVIEWER,
            "public.other",
            "other.changed",
            "label_description",
            "reject",
            1,
            &changed,
            None,
        )
        .await,
        "candidate.changed",
    );
    let mut stale = other.clone();
    stale.evidence = fill(0x99);
    refuse(
        choose(
            &runtime,
            &paths,
            &paths.reviewer,
            REVIEWER,
            "public.other",
            "other.stale",
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
            &paths,
            &paths.reviewer,
            REVIEWER,
            "public.b",
            "owner.try",
            "owner_certification",
            "record",
            1,
            &review,
            None,
        )
        .await,
        "reviewer_not_owner",
    );
    assert_eq!(count_key("decision", "owner.try").await, 0);
    let recorded = must(
        choose(
            &runtime,
            &paths,
            &paths.owner,
            OWNER,
            "public.b",
            "owner.record",
            "owner_certification",
            "record",
            1,
            &review,
            None,
        )
        .await,
        "persisted",
    );
    assert_eq!(recorded.grants, "not_authorized");
    assert!(!recorded.curated_applied);
    assert!(!recorded.published);

    let pack_bytes = b"{";
    fs::write(&paths.pack_command, pack_bytes).unwrap();
    let pack_sha = sha256_hex(pack_bytes).unwrap();
    let mut column = synthetic(0x51);
    column.source = pack_sha;
    refuse(pack_receipt(&runtime, &paths, "class.b.id").await, "classification.missing");
    must(put(&runtime, &paths, "column", "public.b.id", &column, None).await, "persisted");
    refuse(
        choose(
            &runtime,
            &paths,
            &paths.reviewer,
            REVIEWER,
            "public.b.id",
            "class.unknown",
            "classification",
            "confirm",
            1,
            &column,
            Some(json!({"pii": "unknown", "semantic_type": "identifier", "state": "confirmed"})),
        )
        .await,
        "pii.unknown",
    );
    assert_eq!(count_key("decision", "class.unknown").await, 0);
    must(
        choose(
            &runtime,
            &paths,
            &paths.reviewer,
            REVIEWER,
            "public.b.id",
            "class.b.id",
            "classification",
            "confirm",
            1,
            &column,
            Some(json!({"pii": "none", "semantic_type": "identifier", "state": "confirmed"})),
        )
        .await,
        "persisted",
    );
    let pack_error = apply_description_pack("{").unwrap_err();
    assert!(
        pack_error.code.starts_with("oo.builder.description-pack-command/v1"),
        "{}",
        pack_error.code
    );
    refuse(pack_receipt(&runtime, &paths, "class.b.id").await, "compiler.refused");
    assert_eq!(count_key("compiler_receipt", "class.b.id").await, 0);

    fs::create_dir_all(&paths.curated_dir).unwrap();
    let bound = must(review_receipt(&runtime, &paths).await, "bound");
    assert_eq!(bound.kind, "bound");
    assert!(!bound.git_applied);
    assert!(fs::read_dir(&paths.curated_dir).unwrap().next().is_none());
    let row = decision_row("label.b.1").await;
    assert_eq!(row.body, decision_body);
    assert!(!row.curated_applied);
    assert!(!row.published);
    assert_eq!(row.grants, "not_authorized");
    assert!(!row.authenticated);
    must(review_receipt(&runtime, &paths).await, "replay");
    assert_eq!(count_key("compiler_receipt", "label.b.1").await, 1);

    let left = synthetic(0x31);
    must(put(&runtime, &paths, "dataset", "public.left", &left, None).await, "persisted");
    let partial = must(
        batch(&BatchRequest {
            protocol: "oo.review-ledger.batch/v1".to_owned(),
            project_id: PROJECT.to_owned(),
            control_file: paths.control.clone(),
            credential_file: paths.reviewer.clone(),
            postgres: clone_endpoint(&runtime),
            batch_id: "bpartial".to_owned(),
            accept_all: false,
            items: vec![
                item("public.left", "left.approve", "label_description", "approve", &left, Some(dataset_fields())),
                item(
                    "public.right",
                    "right.unknown",
                    "classification",
                    "confirm",
                    &synthetic(0x41),
                    Some(json!({"pii": "unknown", "semantic_type": "identifier", "state": "confirmed"})),
                ),
            ],
        })
        .await,
        "partial",
    );
    assert_eq!(partial.items.len(), 2);
    assert_eq!(partial.items[0].code, "persisted");
    assert_eq!(partial.items[1].code, "pii.unknown");
    let batch_row = batch_row("bpartial").await;
    assert_eq!(batch_row.status, "partial");
    assert_eq!(batch_row.refused, 1);
    assert!(!batch_row.applied);
    let before_accept = count_project("SELECT count(*)::bigint FROM oo_review_ledger.batch WHERE project_id = $1").await;
    refuse(
        batch(&BatchRequest {
            protocol: "oo.review-ledger.batch/v1".to_owned(),
            project_id: PROJECT.to_owned(),
            control_file: paths.control.clone(),
            credential_file: paths.reviewer.clone(),
            postgres: clone_endpoint(&runtime),
            batch_id: "baccept".to_owned(),
            accept_all: true,
            items: vec![item("public.left", "left.again", "label_description", "approve", &left, Some(dataset_fields()))],
        })
        .await,
        "batch.accept_all",
    );
    assert_eq!(
        count_project("SELECT count(*)::bigint FROM oo_review_ledger.batch WHERE project_id = $1").await,
        before_accept
    );
    let right = synthetic(0x41);
    must(put(&runtime, &paths, "dataset", "public.right", &right, None).await, "persisted");
    let complete = must(
        batch(&BatchRequest {
            protocol: "oo.review-ledger.batch/v1".to_owned(),
            project_id: PROJECT.to_owned(),
            control_file: paths.control.clone(),
            credential_file: paths.reviewer.clone(),
            postgres: clone_endpoint(&runtime),
            batch_id: "bcomplete".to_owned(),
            accept_all: false,
            items: vec![item(
                "public.right",
                "right.approve",
                "label_description",
                "approve",
                &right,
                Some(dataset_fields()),
            )],
        })
        .await,
        "complete",
    );
    assert_eq!(complete.kind, "complete");
    assert!(!batch_row_applied("bcomplete").await);

    let poison_body = fill(0xab);
    admin_insert_poison(&poison_body).await;
    refuse(
        choose(
            &runtime,
            &paths,
            &paths.reviewer,
            REVIEWER,
            "public.b",
            "poison.1",
            "label_description",
            "approve",
            1,
            &review,
            Some(dataset_fields()),
        )
        .await,
        "audit.rollback",
    );
    assert_eq!(count_key("decision", "poison.1").await, 0);
    assert_eq!(
        count(
            "SELECT count(*)::bigint FROM oo_review_ledger.audit_event WHERE project_id = $1 AND idempotency_key = $2",
            "poison.1",
        )
        .await,
        1
    );

    let mut next = review.clone();
    next.candidate = fill(0xcd);
    must(put(&runtime, &paths, "dataset", "public.b", &next, Some(1)).await, "persisted");
    refuse(review_receipt(&runtime, &paths).await, "candidate.changed");
    assert_eq!(count_key("compiler_receipt", "label.b.1").await, 1);

    let decisions_before = count_project("SELECT count(*)::bigint FROM oo_review_ledger.decision WHERE project_id = $1").await;
    let denied = cli(&paths, "decide", &not_grant_request(&paths, &runtime));
    assert_eq!(denied.status, 2);
    assert_eq!(denied.value["code"], "not_grant");
    assert_eq!(
        count_project("SELECT count(*)::bigint FROM oo_review_ledger.decision WHERE project_id = $1").await,
        decisions_before
    );
    let library = must(history(&history_request(&paths, &runtime)).await, "history");
    let cli_history = cli(&paths, "history", &history_json(&paths, &runtime));
    assert_eq!(cli_history.status, 0);
    assert_eq!(cli_history.value["code"], "history");
    assert_eq!(cli_history.value["authenticated"], false);
    assert_eq!(cli_history.value["management_identity"], "not_claimed");
    assert_eq!(cli_history.value["curated_applied"], false);
    assert_eq!(cli_history.value["published"], false);
    assert_eq!(
        cli_history.value["history_sha256"],
        library.history_sha256.as_deref().unwrap()
    );
    fs::write(env_var("REVIEW_LEDGER_MARKER"), library.history_sha256.unwrap()).unwrap();
}

async fn assert_same_history(paths: &Paths) {
    let host = env_var("REVIEW_LEDGER_HOST");
    let runtime = endpoint(&host, RUNTIME_USER, &env_var("REVIEW_LEDGER_RUNTIME_PASSWORD_FILE"));
    let output = must(history(&history_request(paths, &runtime)).await, "history");
    let marker = fs::read_to_string(env_var("REVIEW_LEDGER_MARKER")).unwrap();
    assert_eq!(output.history_sha256.unwrap(), marker.trim());
    assert!(count_project("SELECT count(*)::bigint FROM oo_review_ledger.decision WHERE project_id = $1").await >= 1);
    assert_eq!(count_key("compiler_receipt", "label.b.1").await, 1);
    let row = decision_row("label.b.1").await;
    assert!(!row.curated_applied);
    assert!(!row.published);
    assert_eq!(row.grants, "not_authorized");
}

fn write_material() -> Paths {
    let root = PathBuf::from(env_var("REVIEW_LEDGER_STATE"));
    fs::create_dir_all(&root).unwrap();
    let reviewer = root.join("reviewer.cred");
    let owner = root.join("owner.cred");
    let auditor = root.join("auditor.cred");
    fs::write(&reviewer, b"local-operator-credential").unwrap();
    fs::write(&owner, b"ada-owner-credential").unwrap();
    fs::write(&auditor, b"auditor-credential").unwrap();
    let control = root.join("control.json");
    let value = json!({
        "authority": "not_verified",
        "management_identity": "not_claimed",
        "principals": [
            principal(REVIEWER, "reviewer", REVIEWER, "local-operator-credential", &[
                "public.b", "public.b.id", "public.other", "public.left", "public.right",
            ]),
            principal(OWNER, "owner", OWNER, "ada-owner-credential", &["public.b", "public.b.id"]),
            principal("auditor", "auditor", "auditor", "auditor-credential", &["public.b"]),
        ],
        "project_id": PROJECT,
        "protocol": "oo.review-ledger.control/v1"
    });
    fs::write(&control, serde_json::to_vec(&value).unwrap()).unwrap();
    let curated_dir = root.join("curated");
    Paths {
        control: control.display().to_string(),
        reviewer: reviewer.display().to_string(),
        owner: owner.display().to_string(),
        auditor: auditor.display().to_string(),
        review_command: root.join("review-command.json").display().to_string(),
        pack_command: root.join("pack-command.json").display().to_string(),
        curated_dir,
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

fn endpoint(host: &str, user: &str, password_file: &str) -> Endpoint {
    Endpoint {
        host: host.to_owned(),
        port: env::var("REVIEW_LEDGER_PORT").ok().and_then(|value| value.parse().ok()).unwrap_or(5432),
        database: DATABASE.to_owned(),
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

fn history_request(paths: &Paths, postgres: &Endpoint) -> HistoryRequest {
    HistoryRequest {
        protocol: "oo.review-ledger.history/v1".to_owned(),
        project_id: PROJECT.to_owned(),
        control_file: paths.control.clone(),
        credential_file: paths.reviewer.clone(),
        postgres: clone_endpoint(postgres),
    }
}

fn history_json(paths: &Paths, postgres: &Endpoint) -> Value {
    json!({
        "control_file": paths.control,
        "credential_file": paths.reviewer,
        "postgres": postgres_json(postgres),
        "project_id": PROJECT,
        "protocol": "oo.review-ledger.history/v1"
    })
}

fn not_grant_request(paths: &Paths, postgres: &Endpoint) -> Value {
    let mut value = json!({
        "candidate_sha256": fill(0x21),
        "choice": "reject",
        "control_file": paths.control,
        "credential_file": paths.reviewer,
        "curated_sha256": fill(0x21),
        "decision_kind": "label_description",
        "evidence_sha256": fill(0x21),
        "expected_revision": 1,
        "idempotency_key": "not.grant",
        "input_sha256": fill(0x21),
        "object_id": "public.b",
        "postgres": postgres_json(postgres),
        "project_id": PROJECT,
        "protocol": "oo.review-ledger.decide/v1",
        "reviewer_at": DATE,
        "reviewer_label": REVIEWER,
        "schema_sha256": fill(0x21),
        "source_sha256": fill(0x21)
    });
    value["grant"] = json!(true);
    value
}

fn postgres_json(endpoint: &Endpoint) -> Value {
    json!({
        "ca_file": endpoint.ca_file,
        "database": endpoint.database,
        "host": endpoint.host,
        "password_file": endpoint.password_file,
        "port": endpoint.port,
        "server_name": endpoint.server_name,
        "user": endpoint.user
    })
}

async fn put(
    postgres: &Endpoint,
    paths: &Paths,
    object_kind: &str,
    object_id: &str,
    digests: &Digests,
    expected_revision: Option<i64>,
) -> Result<Output, LedgerError> {
    put_as(postgres, paths, &paths.reviewer, object_kind, object_id, digests, expected_revision).await
}

#[allow(clippy::too_many_arguments)]
async fn put_as(
    postgres: &Endpoint,
    paths: &Paths,
    credential: &str,
    object_kind: &str,
    object_id: &str,
    digests: &Digests,
    expected_revision: Option<i64>,
) -> Result<Output, LedgerError> {
    enqueue(&EnqueueRequest {
        protocol: "oo.review-ledger.enqueue/v1".to_owned(),
        project_id: PROJECT.to_owned(),
        control_file: paths.control.clone(),
        credential_file: credential.to_owned(),
        postgres: clone_endpoint(postgres),
        object_kind: object_kind.to_owned(),
        object_id: object_id.to_owned(),
        expected_revision,
        candidate_sha256: digests.candidate.clone(),
        input_sha256: digests.input.clone(),
        evidence_sha256: digests.evidence.clone(),
        schema_sha256: digests.schema.clone(),
        curated_sha256: digests.curated.clone(),
        source_sha256: digests.source.clone(),
        candidate_protocol: PROTOCOL_CANDIDATE.to_owned(),
        owner_label: OWNER.to_owned(),
    })
    .await
}

#[allow(clippy::too_many_arguments)]
async fn choose(
    postgres: &Endpoint,
    paths: &Paths,
    credential: &str,
    reviewer_label: &str,
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
        control_file: paths.control.clone(),
        credential_file: credential.to_owned(),
        postgres: clone_endpoint(postgres),
        object_id: object_id.to_owned(),
        expected_revision: revision,
        idempotency_key: key.to_owned(),
        decision_kind: kind.to_owned(),
        choice: choice.to_owned(),
        reviewer_label: reviewer_label.to_owned(),
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

fn item(object_id: &str, key: &str, kind: &str, choice: &str, digests: &Digests, fields: Option<Value>) -> BatchItem {
    BatchItem {
        object_id: object_id.to_owned(),
        expected_revision: 1,
        idempotency_key: key.to_owned(),
        decision_kind: kind.to_owned(),
        choice: choice.to_owned(),
        reviewer_label: REVIEWER.to_owned(),
        reviewer_at: DATE.to_owned(),
        candidate_sha256: digests.candidate.clone(),
        input_sha256: digests.input.clone(),
        evidence_sha256: digests.evidence.clone(),
        schema_sha256: digests.schema.clone(),
        curated_sha256: digests.curated.clone(),
        source_sha256: digests.source.clone(),
        fields,
    }
}

async fn review_receipt(postgres: &Endpoint, paths: &Paths) -> Result<Output, LedgerError> {
    receipt(
        &ReceiptRequest {
            protocol: "oo.review-ledger.receipt/v1".to_owned(),
            project_id: PROJECT.to_owned(),
            control_file: paths.control.clone(),
            credential_file: paths.reviewer.clone(),
            postgres: clone_endpoint(postgres),
            idempotency_key: "label.b.1".to_owned(),
            compiler: "description-review".to_owned(),
            command_file: paths.review_command.clone(),
            required_columns: Vec::new(),
        },
    )
    .await
}

async fn pack_receipt(postgres: &Endpoint, paths: &Paths, key: &str) -> Result<Output, LedgerError> {
    receipt(
        &ReceiptRequest {
            protocol: "oo.review-ledger.receipt/v1".to_owned(),
            project_id: PROJECT.to_owned(),
            control_file: paths.control.clone(),
            credential_file: paths.reviewer.clone(),
            postgres: clone_endpoint(postgres),
            idempotency_key: key.to_owned(),
            compiler: "description-pack".to_owned(),
            command_file: paths.pack_command.clone(),
            required_columns: vec!["public.b.id".to_owned()],
        },
    )
    .await
}

fn dataset_fields() -> Value {
    json!({
        "description": {"overwrite": false},
        "grain": {"overwrite": false},
        "label": {"overwrite": false}
    })
}

fn synthetic(byte: u8) -> Digests {
    Digests {
        candidate: fill(byte),
        input: fill(byte.wrapping_add(1)),
        evidence: fill(byte.wrapping_add(2)),
        schema: fill(byte.wrapping_add(3)),
        curated: fill(byte.wrapping_add(4)),
        source: fill(byte.wrapping_add(5)),
    }
}

fn fill(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

impl Clone for Digests {
    fn clone(&self) -> Self {
        Self {
            candidate: self.candidate.clone(),
            input: self.input.clone(),
            evidence: self.evidence.clone(),
            schema: self.schema.clone(),
            curated: self.curated.clone(),
            source: self.source.clone(),
        }
    }
}

fn must(result: Result<Output, LedgerError>, code: &str) -> Output {
    match result {
        Ok(output) => {
            assert_eq!(output.code, code, "kind {}", output.kind);
            assert!(!output.authenticated);
            assert_eq!(output.authority, "not_verified");
            assert_eq!(output.management_identity, "not_claimed");
            assert!(!output.curated_applied);
            assert!(!output.published);
            assert_eq!(output.grants, "not_authorized");
            assert!(!output.git_applied);
            assert_eq!(output.protocol, review_ledger::RESULT_PROTOCOL);
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

struct CliOut {
    status: i32,
    value: Value,
}

fn cli(paths: &Paths, command: &str, body: &Value) -> CliOut {
    let _ = paths;
    let path = PathBuf::from(env_var("REVIEW_LEDGER_STATE")).join(format!("{command}-request.json"));
    fs::write(&path, serde_json::to_vec(body).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_review-ledger"))
        .arg(command)
        .arg(&path)
        .output()
        .unwrap_or_else(|error| panic!("cli failed to start: {error}"));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!("cli stdout was not JSON, status {:?}", output.status.code());
    });
    CliOut {
        status: output.status.code().unwrap_or(1),
        value,
    }
}

struct DecisionRow {
    body: String,
    curated_applied: bool,
    published: bool,
    grants: String,
    authenticated: bool,
}

async fn decision_row(key: &str) -> DecisionRow {
    let row = admin_query_one(
        "SELECT body_sha256, curated_applied, published, grants, authenticated \
         FROM oo_review_ledger.decision WHERE project_id = $1 AND idempotency_key = $2",
        key,
    )
    .await;
    DecisionRow {
        body: row.get(0),
        curated_applied: row.get(1),
        published: row.get(2),
        grants: row.get(3),
        authenticated: row.get(4),
    }
}

struct BatchRow {
    status: String,
    refused: i32,
    applied: bool,
}

async fn batch_row(batch_id: &str) -> BatchRow {
    let row = admin_query_one(
        "SELECT status, refused_count, applied FROM oo_review_ledger.batch \
         WHERE project_id = $1 AND batch_id = $2",
        batch_id,
    )
    .await;
    BatchRow {
        status: row.get(0),
        refused: row.get(1),
        applied: row.get(2),
    }
}

async fn batch_row_applied(batch_id: &str) -> bool {
    batch_row(batch_id).await.applied
}

async fn count(sql: &str, key: &str) -> i64 {
    let row = admin_query_one(sql, key).await;
    row.get(0)
}

async fn count_key(table: &str, key: &str) -> i64 {
    let sql = format!(
        "SELECT count(*)::bigint FROM oo_review_ledger.{table} WHERE project_id = $1 AND idempotency_key = $2"
    );
    count(&sql, key).await
}

async fn count_project(sql: &str) -> i64 {
    let client = admin_client().await;
    let row = client.query_one(sql, &[&PROJECT]).await.unwrap_or_else(|error| {
        panic!("count failed: {error}");
    });
    row.get(0)
}

async fn admin_insert_poison(body: &str) {
    let client = admin_client().await;
    client
        .execute(
            "INSERT INTO oo_review_ledger.audit_event ( \
                project_id, idempotency_key, action, detail, object_id, revision, body_sha256, principal_id \
             ) VALUES ($1, 'poison.1', 'decide', '', 'public.b', 1, $2, 'poison')",
            &[&PROJECT, &body],
        )
        .await
        .unwrap_or_else(|error| panic!("poison insert failed: {error}"));
}

async fn admin_query_one(sql: &str, key: &str) -> tokio_postgres::Row {
    let client = admin_client().await;
    client
        .query_one(sql, &[&PROJECT, &key])
        .await
        .unwrap_or_else(|error| panic!("query failed: {error}"))
}

async fn admin_client() -> tokio_postgres::Client {
    let host = env_var("REVIEW_LEDGER_HOST");
    let endpoint = endpoint(&host, ADMIN_USER, &env_var("REVIEW_LEDGER_ADMIN_PASSWORD_FILE"));
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

fn env_var(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("missing {name}"))
}
